// What WPS's JS add-in runtime offers the Paperloom add-in: loaded by WPS
// for Linux in CI (wps-e2e.sh), it tries each document operation the
// add-in needs and reports every result to the local server (server.py) as
// an image request, which needs no CORS. Nothing here is shipped.
var REPORT = "http://127.0.0.1:8977/report";
var SOURCE = location.protocol === "http:" ? "online" : "offline";
var probed = false;

function report(step, ok, detail) {
  var text = detail === undefined ? "" : String(detail);
  var url = REPORT + "?source=" + SOURCE + "&step=" + encodeURIComponent(step) + "&ok=" + (ok ? 1 : 0) +
    "&detail=" + encodeURIComponent(text.slice(0, 1500)) + "&n=" + Math.random();
  try { new Image().src = url; } catch (e) { /* nothing else to report with */ }
}

function attempt(step, fn) {
  try {
    var value = fn();
    report(step, true, value);
    return value;
  } catch (e) {
    report(step, false, e && (e.message || e.description || e));
    return undefined;
  }
}

function keys(object) {
  var names = [];
  try { for (var name in object) names.push(name); } catch (e) { names.push("(for-in failed)"); }
  try { names = names.concat(Object.getOwnPropertyNames(object)); } catch (e) { /* host object */ }
  return names.filter(function (n, i) { return names.indexOf(n) === i; }).sort().join(",");
}

report("script", true, location.href + " | " + navigator.userAgent);

function application() {
  if (typeof wps !== "undefined" && wps.WpsApplication) return wps.WpsApplication();
  if (window.Application) return window.Application;
  throw new Error("no application object");
}

function OnAddinLoad(ribbonUI) {
  report("OnAddinLoad", true, "wps: " + typeof wps + ", Application: " + typeof window.Application);
  setTimeout(probe, 4000);
  return true;
}

function OnAction(control) {
  report("OnAction", true, control && control.Id);
  return true;
}

function probe() {
  if (probed) return;
  probed = true;
  attempt("wps-keys", function () { return keys(wps); });
  attempt("application", function () { return keys(application()); });
  var app = (function () { try { return application(); } catch (e) { return null; } })();
  if (!app) { report("done", false, "no application"); return; }
  attempt("app-info", function () { return [app.Name, app.Version, app.Build].join(" | "); });
  var doc = (function () {
    try { if (app.ActiveDocument) return app.ActiveDocument; } catch (e) { /* none yet */ }
    try { return app.Documents.Add(); } catch (e) { return null; }
  })();
  attempt("document", function () { return doc ? doc.FullName : "none"; });
  if (!doc) { report("done", false, "no document"); return; }
  var sel = app.Selection;
  var tag = "paperloom:cite:" + SOURCE;

  attempt("type-text", function () {
    sel.EndKey(6);
    sel.TypeParagraph();
    sel.TypeText("Probe " + SOURCE + " ");
    return sel.Range.Start;
  });

  // Content controls, as the Word add-in uses.
  attempt("cc-add", function () {
    var c = doc.ContentControls.Add(0, sel.Range);
    c.Tag = tag;
    c.Title = "Paperloom";
    c.Range.Text = "[1]";
    return c.Tag + " | " + c.Range.Text;
  });
  attempt("cc-superscript", function () {
    var c = doc.ContentControls.Item(doc.ContentControls.Count);
    c.Range.Font.Superscript = true;
    return c.Range.Font.Superscript;
  });
  attempt("cc-find", function () {
    var found = [];
    for (var i = 1; i <= doc.ContentControls.Count; i++) {
      var c = doc.ContentControls.Item(i);
      found.push(c.Tag + "@" + c.Range.Start + "=" + c.Range.Text);
    }
    return found.join("; ");
  });
  attempt("cc-select-tag", function () {
    var list = doc.SelectContentControlsByTag(tag);
    return list.Count;
  });

  // Bookmarks: the fallback if content controls are missing.
  attempt("bookmark", function () {
    sel.EndKey(6);
    sel.TypeText(" ");
    var start = sel.Range.Start;
    sel.TypeText("[2]");
    var range = doc.Range(start, sel.Range.End);
    doc.Bookmarks.Add("paperloom_" + SOURCE + "_2", range);
    var b = doc.Bookmarks.Item("paperloom_" + SOURCE + "_2");
    return doc.Bookmarks.Count + " | " + b.Range.Text + "@" + b.Range.Start;
  });

  // ADDIN fields, as Zotero uses in Word.
  attempt("field-addin", function () {
    sel.EndKey(6);
    sel.TypeText(" ");
    var f = doc.Fields.Add(sel.Range, -1, "ADDIN PAPERLOOM {\"works\":[\"a\"]}", false);
    f.Result.Text = "[3]";
    return doc.Fields.Count + " | " + f.Code.Text + " | " + f.Result.Text;
  });

  // Formatting part of a range (italic journal names in references).
  attempt("format-runs", function () {
    sel.EndKey(6);
    sel.TypeParagraph();
    var start = sel.Range.Start;
    sel.TypeText("Kucsko G. Nature, 2013.");
    var italic = doc.Range(start + 10, start + 16);
    italic.Font.Italic = true;
    return italic.Text + " italic=" + italic.Font.Italic;
  });
  attempt("range-methods", function () {
    var r = doc.Range(0, 0);
    return ["InsertXML", "PasteHtml", "InsertFile", "FormattedText", "InsertAfter", "SetRange"].map(function (m) {
      return m + ":" + typeof r[m];
    }).join(",");
  });

  // Data that travels with the document.
  attempt("variables", function () {
    doc.Variables.Add("paperloom", "{\"style\":\"gb-7714-2015-numeric\"}");
    return doc.Variables.Item("paperloom").Value;
  });
  attempt("plugin-storage", function () {
    wps.PluginStorage.setItem("paperloom", "1");
    return wps.PluginStorage.getItem("paperloom");
  });

  // The connector is on 127.0.0.1: can the add-in reach it?
  try {
    fetch("http://127.0.0.1:8977/ping").then(function (r) { return r.text(); }).then(
      function (t) { report("fetch-get", true, t); },
      function (e) { report("fetch-get", false, e); });
    fetch("http://127.0.0.1:8977/cite/search", {
      method: "POST",
      headers: { "Content-Type": "application/json", Authorization: "Bearer probe" },
      body: "{\"query\":\"\"}",
    }).then(function (r) { return r.text(); }).then(
      function (t) { report("fetch-post", true, t); },
      function (e) { report("fetch-post", false, e); });
  } catch (e) {
    report("fetch", false, e);
  }

  // The task pane, where the add-in's interface lives.
  attempt("taskpane", function () {
    var base = location.href.replace(/[^/]*$/, "");
    var pane = wps.CreateTaskPane(base + "taskpane.html");
    pane.DockPosition = wps.Enum && wps.Enum.JSKsoEnum_msoCTPDockPositionRight !== undefined
      ? wps.Enum.JSKsoEnum_msoCTPDockPositionRight : 2;
    pane.Visible = true;
    return base + "taskpane.html | " + pane.ID;
  });

  setTimeout(function () {
    attempt("save", function () { doc.Save(); return doc.FullName; });
    report("done", true, SOURCE);
  }, 8000);
}

function paneProbe() {
  report("pane-script", true, location.href + " | wps: " + typeof wps + ", Application: " + typeof window.Application);
  attempt("pane-insert", function () {
    var app = application();
    app.Selection.EndKey(6);
    app.Selection.TypeText(" pane");
    return app.ActiveDocument.Name;
  });
  try {
    fetch("http://127.0.0.1:8977/ping").then(function (r) { return r.text(); }).then(
      function (t) { report("pane-fetch", true, t); },
      function (e) { report("pane-fetch", false, e); });
  } catch (e) {
    report("pane-fetch", false, e);
  }
}
