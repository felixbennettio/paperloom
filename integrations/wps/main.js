// WPS calls these globals from ribbon.xml in the hidden index page.
var paperloomRibbon;
var paperloomPane;

function OnAddinLoad(ribbonUI) {
  paperloomRibbon = ribbonUI;
  return true;
}

function OnAction(control) {
  if (!control || control.Id !== "paperloomCite") return true;
  if (!paperloomPane) {
    var url = new URL("pane.html", window.location.href).href;
    paperloomPane = wps.CreateTaskPane(url);
    paperloomPane.DockPosition = wps.Enum && wps.Enum.JSKsoEnum_msoCTPDockPositionRight !== undefined
      ? wps.Enum.JSKsoEnum_msoCTPDockPositionRight : 2;
    paperloomPane.Visible = true;
  } else {
    paperloomPane.Visible = !paperloomPane.Visible;
  }
  return true;
}
