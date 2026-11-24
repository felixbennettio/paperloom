# Installs the NSIS installer from dist/ silently, starts Paperloom, and keeps
# screenshots of the screen and of its window once it has loaded, with a
# summary: whether it installed, started and kept running, and whether the
# installed app reads text from a picture (Windows' own text recognition).
# The last is noted, not fatal: build agents may lack the recognizer.
#
#   pwsh scripts/ci/windows-smoke.ps1 -Dist dist -Out windows-screens
#
# Fails when the installer fails or the app is gone within 30 seconds; a
# black screenshot (no desktop on the build agent) is noted, not fatal.
param(
  [string]$Dist = "dist",
  [string]$Out = "windows-screens"
)
$ErrorActionPreference = "Stop"
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$summary = New-Object System.Collections.Generic.List[string]
function Note([string]$line) { $summary.Add($line); Write-Host $line }

Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class Win {
  [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd, out Rect rect);
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr hwnd, IntPtr hdc, uint flags);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hwnd);
}
"@

function Save-Screen([string]$name) {
  $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
  $bitmap = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
  $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
  try {
    $graphics.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
    $bitmap.Save((Join-Path $Out "$name.png"), [System.Drawing.Imaging.ImageFormat]::Png)
    Note "screen ${name}: $($bounds.Width)x$($bounds.Height)"
  } catch {
    Note "screen ${name}: not captured ($($_.Exception.Message))"
  } finally {
    $graphics.Dispose(); $bitmap.Dispose()
  }
}

# The window alone, drawn by the app itself (works without a visible desktop).
function Save-Window([IntPtr]$hwnd, [string]$name) {
  $rect = New-Object 'Win+Rect'
  [void][Win]::GetWindowRect($hwnd, [ref]$rect)
  $width = $rect.Right - $rect.Left; $height = $rect.Bottom - $rect.Top
  Note "window ${name}: $width x $height at $($rect.Left),$($rect.Top)"
  if ($width -le 0 -or $height -le 0) { return }
  $bitmap = New-Object System.Drawing.Bitmap $width, $height
  $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
  $hdc = $graphics.GetHdc()
  try {
    # 2 = PW_RENDERFULLCONTENT, which includes WebView2's content.
    $ok = [Win]::PrintWindow($hwnd, $hdc, 2)
  } finally {
    $graphics.ReleaseHdc($hdc); $graphics.Dispose()
  }
  $bitmap.Save((Join-Path $Out "$name.png"), [System.Drawing.Imaging.ImageFormat]::Png)
  $bitmap.Dispose()
  if (-not $ok) { Note "window ${name}: PrintWindow failed" }
}

$failed = $false
try {
  $installer = Get-ChildItem $Dist -Filter "*-setup.exe" | Select-Object -First 1
  if (-not $installer) { $installer = Get-ChildItem $Dist -Filter "*.exe" | Select-Object -First 1 }
  if (-not $installer) { throw "no NSIS installer in $Dist" }
  Note "installer: $($installer.Name)"
  $setup = Start-Process -FilePath $installer.FullName -ArgumentList "/S" -PassThru -Wait
  Note "installer exit code: $($setup.ExitCode)"
  if ($setup.ExitCode -ne 0) { throw "the installer failed" }

  # Per-user installs go under LocalAppData, per-machine ones under Program Files.
  $roots = @("$env:LOCALAPPDATA\Paperloom", "$env:LOCALAPPDATA\Programs\Paperloom", "$env:ProgramFiles\Paperloom")
  $exe = $roots | Where-Object { Test-Path $_ } |
    ForEach-Object { Get-ChildItem $_ -Filter "*.exe" -File } |
    Where-Object { $_.Name -notmatch "^uninstall" } | Select-Object -First 1
  if (-not $exe) { throw "Paperloom is not where the installer puts it ($($roots -join ', '))" }
  Note "installed: $($exe.FullName)"

  $app = Start-Process -FilePath $exe.FullName -PassThru
  $hwnd = [IntPtr]::Zero
  for ($i = 0; $i -lt 60 -and $hwnd -eq [IntPtr]::Zero; $i++) {
    Start-Sleep -Seconds 1
    $app.Refresh()
    if ($app.HasExited) { break }
    $hwnd = $app.MainWindowHandle
  }
  if ($app.HasExited) { throw "Paperloom exited at once (exit code $($app.ExitCode))" }
  if ($hwnd -eq [IntPtr]::Zero) { Note "no main window after 60 s" } else { [void][Win]::SetForegroundWindow($hwnd) }

  # Let the WebView load the library.
  Start-Sleep -Seconds 15
  Save-Screen "01-screen"
  if ($hwnd -ne [IntPtr]::Zero) { Save-Window $hwnd "01-window" }
  Start-Sleep -Seconds 15
  $app.Refresh()
  if ($app.HasExited) { throw "Paperloom stopped within 30 s (exit code $($app.ExitCode))" }
  Note "running after 30 s: yes"
  Save-Screen "02-screen"
  if ($hwnd -ne [IntPtr]::Zero) { Save-Window $hwnd "02-window" }
  Stop-Process -Id $app.Id -ErrorAction SilentlyContinue

  # A line of text drawn as a picture, read by the installed app.
  $picture = Join-Path $Out "ocr-sample.png"
  $bitmap = New-Object System.Drawing.Bitmap 1400, 200
  $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
  $graphics.Clear([System.Drawing.Color]::White)
  $graphics.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::AntiAlias
  $font = New-Object System.Drawing.Font("Segoe UI", 44)
  $graphics.DrawString("Thermometry with nanodiamonds", $font, [System.Drawing.Brushes]::Black, 30, 50)
  $graphics.Dispose(); $font.Dispose()
  $bitmap.Save($picture, [System.Drawing.Imaging.ImageFormat]::Png); $bitmap.Dispose()
  $read = Join-Path $Out "ocr-text.txt"
  $ocr = Start-Process -FilePath $exe.FullName -ArgumentList @("--recognize-text", "`"$picture`"", "`"$read`"") -PassThru -Wait
  $meaning = @{ 0 = "read"; 1 = "nothing read"; 2 = "bad arguments"; 3 = "no recognizer on this computer" }[$ocr.ExitCode]
  Note "text recognition: exit code $($ocr.ExitCode) ($meaning)"
  if (Test-Path $read) { Note "text recognition read: $((Get-Content $read -Raw).Trim())" }
} catch {
  $failed = $true
  Note "FAILED: $($_.Exception.Message)"
  Save-Screen "99-failure"
} finally {
  # Crash reports Windows kept for the app, if any.
  Get-ChildItem "$env:LOCALAPPDATA\CrashDumps" -Filter "*aperloom*" -ErrorAction SilentlyContinue |
    ForEach-Object { Note "crash dump: $($_.Name)" }
  $summary | Set-Content -Path (Join-Path $Out "summary.txt") -Encoding utf8
}
if ($failed) { exit 1 }
