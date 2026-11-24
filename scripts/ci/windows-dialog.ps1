# Answers a Windows file dialog the app opened (found through UI Automation,
# driven through its controls' windows, no keyboard focus needed): puts
# -Path into its "File name" box, when given, and presses its Open or Save
# button. Keeps a screenshot of the screen with the dialog in -Shot. Fails when no dialog titled -Title shows within 30
# seconds, its File name box or button is not found (its controls listed)
# or it is still open after the button; a dialog it fails on is cancelled,
# so the app is not left waiting.
#
#   powershell -File scripts/ci/windows-dialog.ps1 -Title Open -Path C:\x\scan.pdf -Shot shot.png
param(
  [Parameter(Mandatory)][string]$Title,
  [string]$Path = "",
  [string]$Shot = ""
)
$ErrorActionPreference = "Stop"
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes, System.Windows.Forms, System.Drawing
$A = [System.Windows.Automation.AutomationElement]
$Scope = [System.Windows.Automation.TreeScope]

function Is($property, $value) { [System.Windows.Automation.PropertyCondition]::new($property, $value) }
function All($first, $second) {
  [System.Windows.Automation.AndCondition]::new([System.Windows.Automation.Condition[]]@($first, $second))
}

function Find-Dialog {
  $isDialog = All (Is $A::ClassNameProperty "#32770") (Is $A::NameProperty $Title)
  # A dialog is a top-level window, or listed under the window that owns it.
  $dialog = $A::RootElement.FindFirst($Scope::Children, $isDialog)
  if ($dialog) { return $dialog }
  foreach ($window in $A::RootElement.FindAll($Scope::Children, [System.Windows.Automation.Condition]::TrueCondition)) {
    $dialog = $window.FindFirst($Scope::Children, $isDialog)
    if ($dialog) { return $dialog }
  }
  return $null
}

function Save-Screen([string]$file) {
  if (-not $file) { return }
  $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
  $bitmap = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height
  $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
  try {
    $graphics.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size)
    $bitmap.Save($file, [System.Drawing.Imaging.ImageFormat]::Png)
  } catch {
    Write-Host "screen not captured: $($_.Exception.Message)"
  } finally {
    $graphics.Dispose(); $bitmap.Dispose()
  }
}

# The common dialogs' own controls, by id and window class: the File name
# box is an Edit with id 1148 in Open dialogs (1001 in Save dialogs), the
# Open or Save button a Button with id 1, Cancel id 2. UI Automation lists
# them as plain panes here, so they are driven through their windows:
# WM_SETTEXT for the name, BM_CLICK for the button (no focus needed).
Add-Type @"
using System;
using System.Runtime.InteropServices;
public static class DialogWindow {
  [DllImport("user32.dll", CharSet = CharSet.Unicode)]
  public static extern IntPtr SendMessage(IntPtr hwnd, uint message, IntPtr wParam, string lParam);
  [DllImport("user32.dll")]
  public static extern bool PostMessage(IntPtr hwnd, uint message, IntPtr wParam, IntPtr lParam);
}
"@
function Find-Control([System.Windows.Automation.AutomationElement]$dialog, [string[]]$ids, [string]$class) {
  foreach ($id in $ids) {
    $found = $dialog.FindFirst($Scope::Descendants, (All (Is $A::AutomationIdProperty $id) (Is $A::ClassNameProperty $class)))
    if ($found -and $found.Current.NativeWindowHandle -ne 0) { return $found }
  }
  return $null
}
function Set-Text([System.Windows.Automation.AutomationElement]$box, [string]$text) {
  [void][DialogWindow]::SendMessage([IntPtr]$box.Current.NativeWindowHandle, 0x000C, [IntPtr]::Zero, $text)  # WM_SETTEXT
}
function Click([System.Windows.Automation.AutomationElement]$button) {
  [void][DialogWindow]::PostMessage([IntPtr]$button.Current.NativeWindowHandle, 0x00F5, [IntPtr]::Zero, [IntPtr]::Zero)  # BM_CLICK
}

# The dialog's controls, for the log when one is not found.
function Write-Controls([System.Windows.Automation.AutomationElement]$dialog) {
  $all = $dialog.FindAll($Scope::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
  foreach ($element in $all | Select-Object -First 120) {
    $c = $element.Current
    Write-Host ("  {0} id={1} name={2} class={3}" -f $c.ControlType.ProgrammaticName, $c.AutomationId, $c.Name, $c.ClassName)
  }
}

function Close-Dialog([System.Windows.Automation.AutomationElement]$dialog) {
  $cancel = Find-Control $dialog @("2") "Button"
  if ($cancel) { Click $cancel }
}

$dialog = $null
for ($i = 0; $i -lt 30 -and -not $dialog; $i++) {
  $dialog = Find-Dialog
  if (-not $dialog) { Start-Sleep -Seconds 1 }
}
if (-not $dialog) {
  Save-Screen $Shot
  throw "no `"$Title`" dialog"
}
Start-Sleep -Seconds 1
if ($Path) {
  $name = Find-Control $dialog @("1148", "1001") "Edit"
  if (-not $name) {
    Save-Screen $Shot
    Write-Controls $dialog
    Close-Dialog $dialog
    throw "no File name box in the `"$Title`" dialog (its controls are listed above)"
  }
  Set-Text $name $Path
  Start-Sleep -Milliseconds 500
}
Save-Screen $Shot
$button = Find-Control $dialog @("1") "Button"
if (-not $button) {
  Write-Controls $dialog
  Close-Dialog $dialog
  throw "no Open or Save button in the `"$Title`" dialog (its controls are listed above)"
}
Click $button
for ($i = 0; $i -lt 10 -and (Find-Dialog); $i++) { Start-Sleep -Seconds 1 }
if (Find-Dialog) {
  Save-Screen $Shot
  Close-Dialog $dialog
  throw "the `"$Title`" dialog stayed open"
}
Write-Host "answered `"$Title`"$(if ($Path) { " with $Path" })"
