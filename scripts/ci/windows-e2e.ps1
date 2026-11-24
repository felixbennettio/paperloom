# The Windows app as a person installs it (the NSIS installer, silently),
# driven by desktop-e2e.mjs through Microsoft Edge WebDriver (msedgedriver of
# the WebView2 runtime's own version), with Windows' own text recognition
# and the real network. The app is started here with WebView2's DevTools
# port open (PAPERLOOM_WEBVIEW2_DEBUG_PORT) and msedgedriver attaches to it:
# launched by msedgedriver (tauri-driver's way) the app never showed it
# ("DevToolsActivePort file doesn't exist", Azure 18). Results go to -Out: screenshots, page text,
# summary.txt, msedgedriver's log and the cited copy. run.sh windows-e2e
# calls it after setting up Node.
#
#   pwsh scripts/ci/windows-e2e.ps1 -Dist <folder with the installer> -Out test-results/desktop-e2e
param(
  [Parameter(Mandatory)][string]$Dist,
  [string]$Out = "test-results/desktop-e2e"
)
$ErrorActionPreference = "Stop"
$root = (Resolve-Path "$PSScriptRoot/../..").Path
New-Item -ItemType Directory -Force -Path $Out | Out-Null
$Out = (Resolve-Path $Out).Path
$work = Join-Path ([System.IO.Path]::GetTempPath()) "paperloom-e2e-$PID"
$files = Join-Path $work "files"
$tools = Join-Path $work "tools"
New-Item -ItemType Directory -Force -Path $files, $tools | Out-Null

# A wider screen than the agent's default, where Windows Server lets us.
try { Set-DisplayResolution -Width 1600 -Height 900 -Force -ErrorAction Stop } catch { Write-Host "screen size kept: $($_.Exception.Message)" }

$installer = Get-ChildItem $Dist -Recurse -Filter "*-setup.exe" | Select-Object -First 1
if (-not $installer) { throw "no NSIS installer in $Dist" }
$setup = Start-Process -FilePath $installer.FullName -ArgumentList "/S" -PassThru -Wait
if ($setup.ExitCode -ne 0) { throw "the installer failed (exit code $($setup.ExitCode))" }
# Per-user installs go under LocalAppData, per-machine ones under Program Files.
$exe = @("$env:LOCALAPPDATA\Paperloom", "$env:LOCALAPPDATA\Programs\Paperloom", "$env:ProgramFiles\Paperloom") |
  Where-Object { Test-Path $_ } | ForEach-Object { Get-ChildItem $_ -Filter "*.exe" -File } |
  Where-Object { $_.Name -notmatch "^uninstall" } | Select-Object -First 1
if (-not $exe) { throw "Paperloom is not where the installer puts it" }
Write-Host "installed $($installer.Name): $($exe.FullName)"

# msedgedriver must match the WebView2 runtime the app draws with.
$client = "Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}"
$version = @("HKLM:\SOFTWARE\WOW6432Node\$client", "HKLM:\SOFTWARE\$client", "HKCU:\Software\$client") |
  ForEach-Object { (Get-ItemProperty $_ -ErrorAction SilentlyContinue).pv } |
  Where-Object { $_ -and $_ -ne "0.0.0.0" } | Select-Object -First 1
if (-not $version) {
  $version = Get-ChildItem "${env:ProgramFiles(x86)}\Microsoft\EdgeWebView\Application" -Directory -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -match '^\d+(\.\d+){3}$' } | Sort-Object { [version]$_.Name } | Select-Object -Last 1 -ExpandProperty Name
}
if (-not $version) { throw "no WebView2 runtime found" }
Write-Host "WebView2 runtime $version"
$zip = Join-Path $tools "edgedriver.zip"
Invoke-WebRequest "https://msedgedriver.microsoft.com/$version/edgedriver_win64.zip" -OutFile $zip -UseBasicParsing
Expand-Archive $zip -DestinationPath (Join-Path $tools "edgedriver") -Force
$edgedriver = Get-ChildItem (Join-Path $tools "edgedriver") -Recurse -Filter "msedgedriver.exe" | Select-Object -First 1
if (-not $edgedriver) { throw "no msedgedriver.exe in the download" }

python -m pip install --quiet --disable-pip-version-check pillow
python "$root/scripts/ci/e2e-files.py" $files
if ($LASTEXITCODE -ne 0) { throw "the test files were not written" }

function Save-Screen([string]$name) {
  Add-Type -AssemblyName System.Windows.Forms, System.Drawing
  $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
  $bitmap = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
  $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
  try {
    $graphics.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
    $bitmap.Save((Join-Path $Out "screen-$name.png"), [System.Drawing.Imaging.ImageFormat]::Png)
  } catch {
    Write-Host "screen not captured: $($_.Exception.Message)"
  } finally {
    $graphics.Dispose(); $bitmap.Dispose()
  }
}

# The DevTools port: the app adds the switch to its own (WebView2's
# environment variable and policy never reached the browser process, Azure
# 19 to 22).
$port = 9222
$env:PAPERLOOM_WEBVIEW2_DEBUG_PORT = "$port"
$app = Start-Process -FilePath $exe.FullName -PassThru
Remove-Item Env:PAPERLOOM_WEBVIEW2_DEBUG_PORT
$devtools = $null
for ($i = 0; $i -lt 60 -and -not $devtools -and -not $app.HasExited; $i++) {
  try { $devtools = Invoke-RestMethod "http://127.0.0.1:$port/json/version" -TimeoutSec 2 } catch { Start-Sleep -Seconds 1 }
}
if (-not $devtools) {
  Save-Screen "app-start"
  # What WebView2 was started with, and where it listens: which of the
  # switch, the port or the data folder went astray.
  $webviews = Get-CimInstance Win32_Process -Filter "Name = 'msedgewebview2.exe'"
  foreach ($process in $webviews | Where-Object { $_.CommandLine -notmatch "--type=" }) {
    Write-Host "WebView2 browser $($process.ProcessId): $($process.CommandLine)"
    if ($process.CommandLine -match '--user-data-dir="?([^"]+?)"?(\s--|$)') {
      $active = Join-Path $Matches[1] "DevToolsActivePort"
      Write-Host "DevToolsActivePort: $(if (Test-Path $active) { (Get-Content $active) -join ' ' } else { 'none' })"
    }
  }
  $ids = @($webviews.ProcessId) + $app.Id
  Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue | Where-Object { $ids -contains $_.OwningProcess } |
    ForEach-Object { Write-Host "listening: $($_.LocalAddress):$($_.LocalPort) (process $($_.OwningProcess))" }
  throw "the app's WebView2 opened no DevTools port (app exited: $($app.HasExited))"
}
Write-Host "WebView2 DevTools on port ${port}: $($devtools.Browser)"

$driver = Start-Process -FilePath $edgedriver.FullName -PassThru `
  -ArgumentList @("--port=4444", "--verbose", "--log-path=`"$(Join-Path $Out 'msedgedriver.log')`"")
for ($i = 0; $i -lt 30; $i++) {
  try { Invoke-WebRequest "http://127.0.0.1:4444/status" -UseBasicParsing -TimeoutSec 2 | Out-Null; break } catch { Start-Sleep -Seconds 1 }
}

$status = 1
try {
  $env:WEBDRIVER_CAPABILITIES = ConvertTo-Json -Compress @{
    browserName = "webview2"
    "ms:edgeOptions" = @{ debuggerAddress = "127.0.0.1:$port" }
  }
  # What the installer registered for reference files; the walk opens one
  # through the shell.
  foreach ($ext in ".ris", ".enw", ".nbib") { Write-Host "association: $(cmd /c assoc $ext 2>&1)" }
  # The app keeps its library and notes in its folder under Roaming.
  $env:APP_HOME = Join-Path $env:APPDATA "io.felixbennett.paperloom"
  node "$root/scripts/ci/desktop-e2e.mjs" $exe.FullName $Out $files
  $status = $LASTEXITCODE
} finally {
  Copy-Item (Join-Path $files "*.docx") $Out -ErrorAction SilentlyContinue
  Stop-Process -Id $driver.Id, $app.Id -ErrorAction SilentlyContinue
  Get-Process msedgedriver, $exe.BaseName -ErrorAction SilentlyContinue | Stop-Process -ErrorAction SilentlyContinue
}
exit $status
