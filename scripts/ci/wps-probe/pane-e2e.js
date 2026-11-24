// Walks the installed WPS add-in's task pane as a person would, for the
// WPS end-to-end test (desktop-e2e.mjs, step "Cite in WPS", which adds this
// script to the pane): connects to Paperloom (the test allows it there),
// turns the document's [@DOI] marker into a citation, searches the library,
// inserts citations and the reference list, and saves the document. Each step goes to the probe server (server.py) as an image
// request. Elements are found by their ids and places, not their text, so
// any interface language works.
(function () {
  var REPORT = "http://127.0.0.1:8977/report";

  function report(step, ok, detail) {
    var text = detail === undefined ? "" : String(detail);
    new Image().src = REPORT + "?source=pane&step=" + encodeURIComponent(step) + "&ok=" + (ok ? 1 : 0) +
      "&detail=" + encodeURIComponent(text.slice(0, 1500)) + "&n=" + Math.random();
  }

  function wait(what, test, timeout) {
    return new Promise(function (resolve, reject) {
      var until = Date.now() + timeout;
      (function poll() {
        var value;
        try { value = test(); } catch (e) { value = undefined; }
        if (value) return resolve(value);
        if (Date.now() > until) return reject(new Error("timed out waiting for " + what + ": " + paneText()));
        setTimeout(poll, 500);
      })();
    });
  }

  function paneText() {
    var main = document.querySelector("main");
    return main ? main.innerText.replace(/\s+/g, " ").slice(0, 600) : "(no pane)";
  }

  function idle() {
    var main = document.querySelector("main.addin-pane");
    return main && main.getAttribute("aria-busy") !== "true" && !document.querySelector("fieldset:disabled");
  }

  function failure() {
    var error = document.querySelector(".addin-error");
    if (error) throw new Error(error.textContent);
  }

  // React keeps its own copy of an input's value; set it the way typing does.
  function type(input, value) {
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(input, value);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  }

  // Searches, picks the result matching `pick` and inserts it; `updated`
  // matches the status that follows ("Updated 1 citations and 0 references").
  async function cite(search, query, pick, updated) {
    type(search, query);
    var result = await wait("a search result for " + query, function () {
      return Array.prototype.find.call(document.querySelectorAll("ul.addin-results label"), function (label) {
        return pick.test(label.textContent);
      });
    }, 30000);
    result.querySelector("input").click();
    var insert = await wait("the Insert Citation button", function () {
      var button = document.querySelector("#addin-page ~ button");
      return button && !button.disabled && button;
    }, 10000);
    insert.click();
    await wait("the citation of " + query, function () {
      failure();
      var status = document.querySelector(".addin-status");
      return idle() && status && updated.test(status.textContent);
    }, 60000);
  }

  async function walk() {
    var first = await wait("the pane", function () {
      return document.querySelector("#addin-search") || document.querySelector("main.addin-pane section button");
    }, 30000);
    if (first.id !== "addin-search") {
      first.click();
      report("connect", true, paneText());
    }
    var search = await wait("the connection", function () { return document.querySelector("#addin-search"); }, 150000);
    report("connected", true, paneText());
    var document_ = wps.WpsApplication().ActiveDocument;

    // Update Citations, Insert Reference List, Convert Citation Markers,
    // Convert to Plain Text.
    var buttons = document.querySelectorAll("#addin-style ~ button");
    // The paper's "[@10.1038/nature12373]" (the paper added by its DOI)
    // becomes the first citation.
    buttons[2].click();
    await wait("the converted marker", function () {
      failure();
      return idle() && /\b1\b[^0-9]*\b0\b/.test(document.querySelector(".addin-status").textContent);
    }, 60000);
    if (document_.Content.Text.indexOf("[@") >= 0) throw new Error("the marker is still there: " + document_.Content.Text);
    report("converted", true, paneText());

    // Two papers cited one after the other at the start, then the reference
    // list right after them, inside the paragraph: it has to stand on its own.
    await cite(search, "thermometry", /thermometry/i, /\b2\b[^0-9]*\b0\b/);
    await cite(search, "radioactivity", /radioactivity/i, /\b3\b[^0-9]*\b0\b/);
    report("cited", true, paneText());

    buttons[1].click();
    await wait("the reference list", function () {
      failure();
      return idle() && /\b3\b[^0-9]*\b2\b/.test(document.querySelector(".addin-status").textContent);
    }, 60000);
    report("listed", true, paneText());

    var fields = [];
    for (var i = 1; i <= document_.ContentControls.Count; i++) {
      var control = document_.ContentControls.Item(i);
      fields.push(control.Tag + "=" + control.Range.Text);
    }
    report("fields", true, fields.join(" | "));
    // Each field holds only its own text: nothing was inserted inside another.
    var citations = fields.filter(function (field) { return field.indexOf("paperloom:cite:") === 0; });
    // The marker's paper is the first cited at the start too: [1] again.
    if (citations.length !== 3 || !/=\[1\]$/.test(citations[0]) || !/=\[2\]$/.test(citations[1])
      || !/=\[1\]$/.test(citations[2])) {
      throw new Error("a citation field holds more than its number: " + fields.join(" | "));
    }
    document_.Save();
    report("done", true, document_.FullName);
  }

  walk().catch(function (error) { report("failed", false, error && error.message ? error.message : error); });
})();
