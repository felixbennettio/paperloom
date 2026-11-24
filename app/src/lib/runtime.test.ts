// The plugin runtimes are plain JavaScript shared with the Apple apps, so
// TypeScript never reads them; this at least compiles them.
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { Window } from "happy-dom";
import { describe, expect, it, vi } from "vitest";

describe("plugin runtimes", () => {
  for (const name of ["obsidian.js", "zotero.js", "translate.js"]) {
    it(`${name} compiles`, () => {
      const source = readFileSync(new URL(`../../../core/plugin-runtime/${name}`, import.meta.url), "utf8");
      expect(() => new Function(source)).not.toThrow();
      expect(source).toContain('send({ type: "loaded" });');
    });
  }
});

function zoteroRuntime(globals: Record<string, unknown> = {}) {
  type Options = Record<string, unknown>;
  type Event = { source: unknown; data: Record<string, unknown> };
  const listeners = new Map<string, (event: Event) => Promise<void>>();
  const app = { postMessage: vi.fn() };
  class DomEvent {
    target: Element | null = null;
    readonly bubbles: boolean;
    constructor(readonly type: string, options: { bubbles?: boolean } = {}) { this.bubbles = options.bubbles ?? false; }
  }
  class Element {
    children: Element[] = [];
    parentElement: Element | null = null;
    declare hidden: boolean;
    declare disabled: boolean;
    declare checked: boolean;
    declare label: string;
    private text = "";
    get textContent(): string { return this.text + this.children.map((child) => child.textContent).join(""); }
    set textContent(value: string) { this.text = String(value); this.children = []; }
    private attrs = new Map<string, string>();
    get attributes() { return [...this.attrs].map(([name, value]) => ({ name, value })); }
    nodeType = 1;
    get nodeName() { return this.localName; }
    readonly namespaceURI = null;
    get childNodes() { return this.children; }
    get parentNode() { return this.parentElement; }
    get firstChild() { return this.children[0] ?? null; }
    get id() { return this.getAttribute("id") ?? ""; }
    set id(value: string) { this.setAttribute("id", value); }
    className = "";
    type = "";
    value = "";
    get selectedIndex() { return this.children.findIndex((child) => child.value === this.value); }
    set selectedIndex(index: number) { this.value = this.children[index]?.value ?? ""; }
    contains(element: Element): boolean { return this === element || this.children.some((child) => child.contains(element)); }
    closest(selector: string): Element | null { return this.matches(selector) ? this : this.parentElement?.closest(selector) ?? null; }
    matches(selector: string) {
      if (selector === "*") return true;
      if (selector.startsWith("#")) return this.id === selector.slice(1);
      if (selector.startsWith(".")) return (this.getAttribute("class") ?? "").split(" ").includes(selector.slice(1));
      const attr = /^(\w+)?\[([^=\]]+)(?:="([^"]*)")?\]$/.exec(selector);
      if (attr) return (!attr[1] || this.localName === attr[1]) && this.hasAttribute(attr[2]) && (attr[3] === undefined || this.getAttribute(attr[2]) === attr[3]);
      return this.localName === selector;
    }
    querySelectorAll(selector: string): Element[] {
      return this.children.flatMap((child) => [...(child.matches(selector) ? [child] : []), ...child.querySelectorAll(selector)]);
    }
    replaceChildren(...nodes: Element[]) { this.children.forEach((child) => { child.parentElement = null; }); this.children = []; this.append(...nodes); }
    prepend(node: Element) { node.remove(); node.parentElement = this; this.children.unshift(node); }
    replaceWith(node: Element) {
      const parent = this.parentElement;
      if (!parent) return;
      const index = parent.children.indexOf(this);
      this.remove(); node.remove(); node.parentElement = parent; parent.children.splice(index, 0, node);
    }
    hasAttribute(name: string) { return this.attrs.has(name); }
    private handlers = new Map<string, { handler: (event: DomEvent) => unknown; capture: boolean }[]>();
    constructor(readonly localName: string) {}
    append(...nodes: unknown[]) {
      for (const node of nodes as Element[]) {
        if (node instanceof Element) {
          node.remove();
          node.parentElement = this;
        }
        this.children.push(node);
      }
    }
    remove() {
      if (this.parentElement) this.parentElement.children = this.parentElement.children.filter((child) => child !== this);
      this.parentElement = null;
    }
    get innerHTML(): string { return this.children.map((node) => node.innerHTML ?? node.textContent ?? "").join(""); }
    getAttribute(name: string) { return this.attrs.get(name) ?? null; }
    setAttribute(name: string, value: string) { this.attrs.set(name, String(value)); if (name === "type") this.type = value; if (name === "value") this.value = value; }
    removeAttribute(name: string) { this.attrs.delete(name); }
    addEventListener(type: string, handler: (event: DomEvent) => unknown, _capture?: boolean) {
      this.handlers.set(type, [...(this.handlers.get(type) ?? []), { handler, capture: _capture ?? false }]);
    }
    dispatchEvent(event: DomEvent) {
      event.target ??= this;
      const path: Element[] = [this];
      for (let parent = this.parentElement; parent; parent = parent.parentElement) path.push(parent);
      for (const node of [...path].reverse()) for (const { handler, capture } of node.handlers.get(event.type) ?? []) if (capture) handler(event);
      for (const node of event.bubbles ? path : [this]) for (const { handler, capture } of node.handlers.get(event.type) ?? []) if (!capture) handler(event);
      return true;
    }
    querySelector(selector: string): Element | null {
      for (const child of this.children) {
        if (selector.startsWith("#") ? child.getAttribute("id") === selector.slice(1) : child.localName === selector) return child;
        const found = child.querySelector(selector);
        if (found) return found;
      }
      return null;
    }
  }
  const doc = {
    body: new Element("body"),
    implementation: { createHTMLDocument: () => ({
      title: "", head: new Element("head"), body: new Element("body"),
      createElement: (tag: string) => new Element(tag),
    }) },
    createElement: (tag: string) => new Element(tag),
    createXULElement: undefined as unknown as (tag: string) => Element,
    querySelector: (selector: string) => doc.body.querySelector(selector),
    querySelectorAll: (selector: string) => doc.body.querySelectorAll(selector),
    getElementById: (id: string) => doc.body.querySelector(`#${id}`),
    createElementNS: (_namespace: string, tag: string) => new Element(tag),
    createTextNode: (text: string) => Object.assign(new Element("#text"), { nodeType: 3, textContent: text }),
  };
  type Item = { key: string; id: number; hasTag(tag: string): boolean; getField?: (field: string) => string };
  type Pane = { document: typeof doc; getSelectedItems(): Item[] };
  type ReaderProps = {
    reader: { itemID: number | false; _item: { key: string } | false; type: string; tabID: string | null };
    doc?: typeof doc;
    params: Options;
    append: (...nodes: unknown[]) => void;
  };
  type ReaderHandler = (props: ReaderProps) => unknown;
  const window = {
    parent: app,
    addEventListener: (type: string, listener: (event: Event) => Promise<void>) => listeners.set(type, listener),
    ZoteroPane: undefined as unknown as Pane,
    ZoteroPane_Local: undefined as unknown as Pane,
    Zotero: undefined as unknown as {
      getActiveZoteroPane(): Pane;
      ItemTreeManager: {
        registerColumn(options: unknown): string | false;
        registerColumns(options: Options | Options[]): string | false | (string | false)[];
        unregisterColumn(key: string): boolean;
        unregisterColumns(keys: string | string[]): boolean | boolean[];
      };
      Items: { getByLibraryAndKey(library: number, key: string): Item | false };
      Reader: {
        registerEventListener(type: string, handler: ReaderHandler, pluginID?: string): void;
        unregisterEventListener(type: string, handler: ReaderHandler): void;
      };
    },
  };
  const source = readFileSync(new URL("../../../core/plugin-runtime/zotero.js", import.meta.url), "utf8");
  const context = Object.assign(window, { window, navigator: { userAgent: "Linux" }, document: doc, Event: DomEvent, HTMLElement: Element, console, setTimeout }, globals);
  runInNewContext(source, context);
  const receive = async (data: Event["data"]) => { await listeners.get("message")?.({ source: app, data }); };
  const messages = () => app.postMessage.mock.calls.map(([message]) => message);
  return { Element, DomEvent, manager: window.Zotero.ItemTreeManager, items: window.Zotero.Items, reader: window.Zotero.Reader, receive, messages, doc, window, context };
}


describe("Zotero preference panes", () => {
  function setup() {
    let fixture: () => unknown[] = () => [];
    const parse = vi.fn((_text: string, _type: string) => ({ documentElement: { childNodes: fixture() }, querySelector: () => null }));
    const runtime = zoteroRuntime({ DOMParser: class { parseFromString = parse; }, URL });
    const node = (tag: string, attributes: Record<string, string> = {}, children: InstanceType<typeof runtime.Element>[] = []) => {
      const element = runtime.doc.createElement(tag);
      for (const [key, value] of Object.entries(attributes)) element.setAttribute(key, value);
      element.append(...children);
      return element;
    };
    fixture = () => [node("vbox", {
      id: "pane", onload: `window.loads = (window.loads || 0) + 1;
        window.boundAtLoad = document.getElementById("enabled").checked;
        window.loadTarget = event.target.id;
        const menu = document.getElementById("choice");
        menu.appendItem("Third", "c"); menu.selectedIndex = 0;
        window.paneHelper();`,
      onunload: "window.unloads = (window.unloads || 0) + 1",
    }, [
      node("h2", { "data-l10n-id": "heading" }),
      node("label", { id: "caption", "data-l10n-id": "caption" }),
      node("checkbox", { id: "enabled", preference: "extensions.test.enabled", "data-l10n-id": "enabled-label" }),
      node("input", { id: "count", type: "number", preference: "extensions.test.count" }),
      node("input", { id: "shortcut", type: "text", preference: "extensions.test.shortcut" }),
      node("menulist", { id: "choice", preference: "extensions.test.choice" }, [node("menupopup", {}, [
        node("menuitem", { label: "First", value: "a" }), node("menuitem", { label: "Second", value: "b" }),
      ])]),
      node("radiogroup", { id: "mode", preference: "extensions.test.mode" }, [node("radio", { value: "0" }), node("radio", { value: "1" })]),
      node("button", { id: "save", "data-l10n-id": "save-label", onclick: "window.clicked = (window.clicked || 0) + 1" }),
      node("link", { rel: "stylesheet", href: "pane.css" }),
      node("script", {}, [runtime.doc.createTextNode("window.inlineRuns = (window.inlineRuns || 0) + 1")]),
    ])];
    const start = (prefs?: string) => runtime.receive({ type: "start", manifest: { id: "test", version: "1" }, code: { prefs, files: [
      { path: "bootstrap.js", text: `function startup() { Zotero.PreferencePanes.register({pluginID: "test", src: "plugin://zotero/pane.xhtml", scripts: ["helper.js"], stylesheets: ["extra.css"]}); }` },
      { path: "pane.xhtml", text: '<vbox><html:input /></vbox>' },
      { path: "prefs.js", text: ['pref("extensions.test.enabled", true);', 'pref("extensions.test.count", 7);', 'pref("extensions.test.shortcut", "Alt+1");', 'pref("extensions.test.choice", "b");', 'pref("extensions.test.mode", 1);'].join("\n") },
      { path: "helper.js", text: `var paneScriptRuns = (window.paneScriptRuns || 0) + 1; function paneHelper() { window.helperRuns = (window.helperRuns || 0) + 1; } document.getElementById("pane").addEventListener("load", () => { window.loadEvents = (window.loadEvents || 0) + 1; });` },
      { path: "pane.css", text: "h2 {color: red}" },
      { path: "extra.css", text: "vbox {display: block}" },
      { path: "locale/en-US/pane.ftl", text: "heading = Preferences\nenabled-label =\n    .label = Enabled\nsave-label = Save\ncaption =\n    .value = Disable automation" },
    ] } });
    return { ...runtime, parse, start };
  }

  it("loads scripts/styles, binds before onload, localizes controls and cleans up each opening", async () => {
    const runtime = setup();
    await runtime.start(JSON.stringify({ "extensions.test.enabled": false, "extensions.test.choice": "c" }));
    await runtime.receive({ type: "settings" });
    const { doc, context, messages } = runtime;
    expect(messages().filter((m) => m.type === "error")).toEqual([]);
    expect(runtime.parse).toHaveBeenCalledWith(expect.stringContaining('xmlns:html='), "application/xml");
    expect(context.loadTarget).toBe("pane");
    expect(doc.getElementById("enabled")?.checked).toBe(false);
    expect(doc.getElementById("enabled")?.querySelector("span")?.textContent).toBe("Enabled");
    expect(doc.getElementById("save")?.textContent).toBe("Save");
    expect(doc.querySelector("h2")?.textContent).toBe("Preferences");
    expect(doc.getElementById("caption")?.textContent).toBe("Disable automation");
    expect(doc.getElementById("choice")?.value).toBe("c");
    expect(doc.getElementById("mode")?.querySelectorAll("radio").map((radio) => radio.checked)).toEqual([false, true]);
    expect(doc.querySelectorAll("style").map((style) => style.textContent)).toEqual(["h2 {color: red}", "vbox {display: block}"]);
    expect(runInNewContext("[loads, boundAtLoad, inlineRuns, paneScriptRuns, helperRuns, loadEvents]", context)).toEqual([1, false, 1, 1, 1, 1]);
    doc.getElementById("save")?.dispatchEvent(new runtime.DomEvent("click"));
    await runtime.receive({ type: "settings" });
    expect(runInNewContext("[loads, unloads, inlineRuns, paneScriptRuns, helperRuns, clicked]", context)).toEqual([2, 1, 2, 2, 2, 1]);
    expect(doc.querySelectorAll("#pane")).toHaveLength(1);
    await runtime.receive({ type: "settings-close" });
    expect(doc.getElementById("pane")).toBeNull();
    expect(runInNewContext("unloads", context)).toBe(2);
    expect(messages().filter((m) => m.type === "error")).toEqual([]);
  });

  it("persists booleans, numeric and string values through prefs.save and restores them on reload", async () => {
    const runtime = setup();
    await runtime.start();
    await runtime.receive({ type: "settings" });
    const { doc, DomEvent } = runtime;
    const checkbox = doc.getElementById("enabled")!;
    runInNewContext(`document.getElementById("enabled").addEventListener("command", () => { window.savedWhenCommandRuns = Zotero.Prefs.get("extensions.test.enabled", true); });`, runtime.context);
    checkbox.checked = false;
    checkbox.dispatchEvent(new DomEvent("command", { bubbles: true }));
    expect(runInNewContext("savedWhenCommandRuns", runtime.context)).toBe(false);
    const count = doc.getElementById("count")!;
    count.value = "12";
    count.dispatchEvent(new DomEvent("input", { bubbles: true }));
    count.dispatchEvent(new DomEvent("change", { bubbles: true }));
    count.value = "";
    count.dispatchEvent(new DomEvent("input", { bubbles: true }));
    count.value = "not a number";
    count.dispatchEvent(new DomEvent("input", { bubbles: true }));
    const shortcut = doc.getElementById("shortcut")!;
    shortcut.value = "Ctrl+K";
    shortcut.dispatchEvent(new DomEvent("input", { bubbles: true }));
    const menu = doc.getElementById("choice")!;
    menu.value = "c";
    menu.dispatchEvent(new DomEvent("command", { bubbles: true }));
    const radio = doc.getElementById("mode")!.querySelector("radio")!;
    radio.querySelector("input")!.dispatchEvent(new DomEvent("change", { bubbles: true }));
    const saves = runtime.messages().filter((m) => m.method === "prefs.save");
    expect(saves).toHaveLength(5);
    const saved = saves.at(-1).args[0];
    expect(JSON.parse(saved)).toEqual({ "extensions.test.enabled": false, "extensions.test.count": 12, "extensions.test.shortcut": "Ctrl+K", "extensions.test.choice": "c", "extensions.test.mode": 0 });
    const reloaded = setup();
    await reloaded.start(saved);
    await reloaded.receive({ type: "settings" });
    expect(reloaded.doc.getElementById("enabled")?.checked).toBe(false);
    expect(reloaded.doc.getElementById("count")?.value).toBe("12");
    expect(reloaded.doc.getElementById("shortcut")?.value).toBe("Ctrl+K");
    expect(reloaded.doc.getElementById("choice")?.value).toBe("c");
    expect(reloaded.messages().filter((m) => m.type === "error")).toEqual([]);
  });

  it("provides table modules while rejecting unavailable modules clearly", () => {
    const { context } = setup();
    expect(runInNewContext('typeof require("react").createElement', context)).toBe("function");
    expect(runInNewContext('Zotero.Intl.strings', context)).toEqual({});
    expect(() => runInNewContext('require("other")', context)).toThrow("Module other is not available");
  });

  it("restores working menulist adapters on cloned template rows", async () => {
    const runtime = setup();
    await runtime.start();
    await runtime.receive({ type: "settings" });
    expect(runInNewContext(`
      const clone = document.getElementById("choice").cloneNode(true);
      clone.appendItem("Fourth", "d");
      clone.selectedIndex = 3;
      [clone.value, clone.querySelectorAll("select").length, clone.querySelectorAll("menuitem").length];
    `, runtime.context)).toEqual(["d", 1, 4]);
  });
});


describe("Zotero Linter metadata APIs", () => {
  it("looks up supported types, fields and creators without leaking registry mutations", () => {
    const { context } = zoteroRuntime();
    expect(runInNewContext(String.raw`
      const types = Zotero.ItemTypes.getAll();
      const fields = Zotero.ItemFields.getAll();
      const creators = Zotero.CreatorTypes.getAll();
      window.registryResult = [
        types.map(({ name }) => name),
        types.every(({ id, name }) => Zotero.ItemTypes.getID(name) === id && Zotero.ItemTypes.getName(id) === name),
        fields.every(({ id, name }) => Zotero.ItemFields.getID(name) === id && Zotero.ItemFields.getName(id) === name),
        creators.every(({ id, name }) => Zotero.CreatorTypes.getID(name) === id && Zotero.CreatorTypes.getName(id) === name),
        Zotero.ItemTypes.getID("unknown"), Zotero.ItemFields.getName(99999),
        Zotero.CreatorTypes.getName(Zotero.CreatorTypes.getPrimaryIDForType("patent")),
        Zotero.CreatorTypes.getTypesForItemType("bookSection").map(({ name }) => name),
        Zotero.ItemFields.isValidForType("DOI", "journalArticle"),
        Zotero.ItemFields.isValidForType("university", "journalArticle"),
        Zotero.ItemFields.isValidForType("unknown", "document"),
      ];
      types[0].name = "mutated";
      registryResult.push(Zotero.ItemTypes.getAll()[0].name);
      registryResult;
    `, context)).toEqual([
      ["journalArticle", "conferencePaper", "book", "bookSection", "thesis", "report", "preprint", "patent", "standard", "dataset", "webpage", "document", "attachment", "annotation"],
      true, true, true, false, false, "inventor",
      ["author", "contributor", "editor", "bookAuthor", "translator", "seriesEditor"],
      true, false, false, "journalArticle",
    ]);
  });

  it("maps base fields in both directions and returns type field IDs", () => {
    const { context } = zoteroRuntime();
    expect(runInNewContext(`
      const F = Zotero.ItemFields;
      [
        ...[["conferencePaper", "publicationTitle"], ["bookSection", "publicationTitle"],
          ["webpage", "publicationTitle"], ["thesis", "publisher"], ["report", "publisher"],
          ["preprint", "number"], ["patent", "number"], ["dataset", "number"], ["patent", "date"]]
          .map(([type, base]) => F.getName(F.getFieldIDFromTypeAndBase(Zotero.ItemTypes.getID(type), F.getID(base)))),
        F.getName(F.getBaseIDFromTypeAndField("bookSection", "bookTitle")),
        F.getFieldIDFromTypeAndBase("thesis", "publicationTitle"),
        F.getBaseIDFromTypeAndField("unknown", "title"),
        F.getTypeFieldsFromBase("publicationTitle", true),
        F.getTypeFieldsFromBase("publisher").map(F.getName),
        F.getItemTypeFields("patent").map(F.getName).includes("issueDate"),
        Zotero.ItemTypes.getTypesForField("university").map(Zotero.ItemTypes.getName),
      ];
    `, context)).toEqual([
      "proceedingsTitle", "bookTitle", "websiteTitle", "university", "institution", "archiveID", "patentNumber", "identifier", "issueDate",
      "publicationTitle", false, false, ["proceedingsTitle", "bookTitle", "websiteTitle"],
      ["university", "institution", "repository"], true, ["thesis"],
    ]);
  });

  it("normalizes dates with partial precision, leap-day validation and explicit SQL time zones", () => {
    const { context } = zoteroRuntime();
    expect(runInNewContext(`
      const D = Zotero.Date;
      [
        ...["2024", "2024-2", "2024/2/29", "February 29, 2024", "29 Feb 2024", "Feb 2024",
          "10/15/2026", "2023-02-29", "2024-13", "unknown", ""].map(D.strToISO),
        D.strToDate("February 29, 2024"), D.strToDate("2024"), D.strToDate("unknown"),
        D.dateToSQL(new Date("2024-02-29T23:04:05Z"), true),
        D.sqlToDate("2024-02-29 23:04:05", true).toISOString(),
        D.dateToSQL(D.sqlToDate("2024-02-29 23:04:05")),
        D.sqlToDate("2023-02-29", true), D.sqlToDate("2024-02-29 24:00:00", true),
        D.dateToSQL(new Date("invalid"), true),
        D.isSQLDate("2024-02-29"), D.isSQLDate("2023-02-29"), D.isSQLDate("2024-02"),
        D.isSQLDate("2024-00-00", true),
      ];
    `, context)).toEqual([
      "2024", "2024-02", "2024-02-29", "2024-02-29", "2024-02-29", "2024-02",
      "2026-10-15", false, false, false, false,
      { year: 2024, month: 1, day: 29, part: "" }, { year: 2024, part: "" }, { part: "unknown" },
      "2024-02-29 23:04:05", "2024-02-29T23:04:05.000Z", "2024-02-29 23:04:05",
      false, false, false, true, false, false, true,
    ]);
  });

  it("handles title/name capitalization and identifier cleanup", () => {
    const { context } = zoteroRuntime();
    expect(runInNewContext(`
      const U = Zotero.Utilities;
      [
        U.sentenceCase("AN ALL CAPS TITLE. SECOND SENTENCE"),
        U.sentenceCase("A Study of DNA and iPhone"),
        U.sentenceCase('<i>AN ALL CAPS TITLE</i>'),
        U.sentenceCase('A <span class="nocase">Protected Name</span> Title'),
        U.capitalizeName("JOHN GUTIÉRREZ-ALBILLA o'neal McGregor"),
        U.capitalizeName("martha McMiddlename WASHINGTON"),
        U.capitalizeTitle("AN INTRODUCTION TO TESTING", true),
        U.cleanDOI("https://doi.org/10.1000/Test "),
        U.cleanDOI("https://doi.org/10.1000%2FTest"),
        U.cleanDOI("(doi:10.1000/test)."),
        U.cleanDOI("10.1000/test(1)"),
        U.cleanDOI("not a DOI"),
        U.cleanISBN("ISBN 978-0-306-40615-7"),
        U.cleanISBN("ISBN-10: 0-8044-2957-X"),
        U.cleanISBN("0-8044-2957-X"), U.cleanISBN("9780306406158"), U.cleanISBN("123"),
      ];
    `, context)).toEqual([
      "An all caps title. Second sentence", "A study of DNA and iPhone", "<i>An all caps title</i>",
      'A <span class="nocase">Protected Name</span> title',
      "John Gutiérrez-Albilla O'Neal McGregor", "Martha McMiddlename Washington",
      "An Introduction to Testing", "10.1000/Test", "10.1000/Test", "10.1000/test", "10.1000/test(1)",
      null, "9780306406157", "080442957X", "080442957X", false, false,
    ]);
  });

  it("saves mapped fields through items.update, clears acknowledged changes and stages undo metadata", async () => {
    const { context, receive, messages } = zoteroRuntime();
    await receive({ type: "start", manifest: { id: "linter-test", version: "1" },
      items: [{ key: "one", itemType: "bookSection", title: "ALL CAPS TITLE", publicationTitle: "OLD BOOK", date: "February 29, 2024", tags: [] }],
      code: { files: [] },
    });
    expect(runInNewContext(`
      window.item = Zotero.Items.get(1);
      [item.itemTypeID === Zotero.ItemTypes.getID("bookSection"), item.hasChanged(),
        item.getUsedFields().map(Zotero.ItemFields.getName), item.getField("bookTitle"),
        item.isFieldOfBase("bookTitle", "publicationTitle"),
        item.setField("title", "ALL CAPS TITLE"), item.hasChanged()];
    `, context)).toEqual([true, false, ["title", "bookTitle", "date"], "OLD BOOK", true, false, false]);
    runInNewContext(`
      item.setField("title", "Temporary");
      item.setField("title", "ALL CAPS TITLE");
    `, context);
    expect(runInNewContext("item.hasChanged()", context)).toBe(false);
    runInNewContext(`
      item.setField("title", Zotero.Utilities.sentenceCase(item.getField("title")));
      item.setField(Zotero.ItemFields.getID("bookTitle"), "New book");
      item.setField("date", Zotero.Date.strToISO(item.getField("date")));
      item.addTag("/unread");
      item.removeTag("absent");
    `, context);
    expect(runInNewContext("item.hasChanged()", context)).toBe(true);
    const saved = runInNewContext(`item.saveTx().then(() => Zotero.UndoHistory.stageAction("lint", { count: 1 }))`, context) as Promise<void>;
    const update = messages().find((message) => message.method === "items.update");
    expect(update.args).toEqual(["one", JSON.stringify({ title: "All caps title", publicationTitle: "New book", date: "2024-02-29" }), ["/unread"]]);
    await receive({ type: "reply", id: update.id, ok: true });
    await saved;
    expect(runInNewContext("[item.hasChanged(), item.getField('bookTitle'), Zotero.UndoHistory.actions]", context))
      .toEqual([false, "New book", [{ name: "lint", args: { count: 1 } }]]);
    await expect(runInNewContext("item.saveTx()", context)).resolves.toBe(false);
  });

  it("saves creator edits as Zotero JSON and keeps failed saves dirty", async () => {
    const { context, receive, messages } = zoteroRuntime();
    await receive({ type: "start", manifest: { id: "creators-test", version: "1" },
      items: [{ key: "one", itemType: "journalArticle", title: "Original",
        creators: [{ firstName: "jane", lastName: "doe", creatorType: "author" }, { name: "test institute", creatorType: "contributor" }] }],
      code: { files: [] },
    });
    runInNewContext(`
      window.item = Zotero.Items.get(1);
      const creators = item.getCreators();
      creators[0].firstName = Zotero.Utilities.capitalizeName(creators[0].firstName);
      creators[0].lastName = Zotero.Utilities.capitalizeName(creators[0].lastName);
      item.setCreators(creators);
      const institution = item.getCreator(1);
      institution.lastName = "Test Institute";
      item.setCreator(1, institution);
    `, context);
    expect(runInNewContext("[item.hasChanged(), item.getField('firstCreator'), item.getCreator(0).fieldMode, item.getCreator(1).fieldMode]", context))
      .toEqual([true, "Doe", 0, 1]);
    const savingCreators = runInNewContext("item.saveTx()", context) as Promise<unknown>;
    const creatorUpdate = messages().find((message) => message.method === "items.update");
    expect(creatorUpdate.args).toEqual(["one", JSON.stringify({ creators: [
      { firstName: "Jane", lastName: "Doe", creatorType: "author" }, { name: "Test Institute", creatorType: "contributor" },
    ] }), null]);
    expect(runInNewContext("item.hasChanged()", context)).toBe(true);
    await receive({ type: "reply", id: creatorUpdate.id, ok: true });
    await expect(savingCreators).resolves.toBe(true);
    expect(runInNewContext("[item.hasChanged(), item.toJSON().creators]", context)).toEqual([false, [
      { firstName: "Jane", lastName: "Doe", creatorType: "author" }, { name: "Test Institute", creatorType: "contributor" },
    ]]);
    await expect(runInNewContext("item.saveTx()", context)).resolves.toBe(false);
    runInNewContext("item.setField('title', 'Changed'); item.setCreators([])", context);
    const saving = runInNewContext("item.saveTx()", context) as Promise<unknown>;
    const rejected = expect(saving).rejects.toBeTruthy();
    const update = messages().filter((message) => message.method === "items.update").at(-1);
    expect(JSON.parse(update.args[1])).toEqual({ title: "Changed", creators: [] });
    await receive({ type: "reply", id: update.id, ok: false, error: "save failed" });
    await rejected;
    expect(runInNewContext("item.hasChanged()", context)).toBe(true);
    const retry = runInNewContext("item.saveTx()", context) as Promise<unknown>;
    const retryUpdate = messages().filter((message) => message.method === "items.update").at(-1);
    expect(retryUpdate.args).toEqual(update.args);
    // A later creator edit must remain dirty when the earlier save completes.
    runInNewContext("item.setCreators([{ firstName: 'Ada', lastName: 'Lovelace', creatorType: 'author' }])", context);
    await receive({ type: "reply", id: retryUpdate.id, ok: true });
    await retry;
    expect(runInNewContext("item.hasChanged()", context)).toBe(true);
    expect(messages().some((message) => message.type === "partial" && message.what === "persist creators")).toBe(false);
  });

  it("gives Linter empty duplicate and unresolved OpenURL lookup results", async () => {
    const { context, messages } = zoteroRuntime();
    await expect(runInNewContext(`(async () => {
      const duplicates = new Zotero.Duplicates(1);
      const search = await duplicates.getSearchObject();
      return await search.search();
    })()`, context)).resolves.toEqual([]);
    const result = runInNewContext(`(async () => {
      const ctx = Zotero.OpenURL.createContextObject(new Zotero.Item({ key: 'one' }, 1), '1.0');
      if (!ctx) throw new Error('Could not build OpenURL context');
      const req = await Zotero.HTTP.request('GET', 'https://www.crossref.org/openurl?pid=linter&' + ctx + '&multihit=true', { responseType: 'xml' });
      const response = req.responseXML?.getElementsByTagName('query')[0];
      return [req.status, response?.getAttribute('status'), response?.getElementsByTagName('doi')];
    })()`, context);
    await expect(result).resolves.toEqual([200, "unresolved", []]);
    expect(messages().some((message) => message.type === "error" || message.method === "requestUrl")).toBe(false);
  });


});

describe("Zotero toolkit foundations", () => {
  it("exposes item prototypes, the reader registry and wrapper helpers through startup", async () => {
    const { receive, messages, context } = zoteroRuntime();
    await receive({
      type: "start", manifest: { id: "toolkit", version: "1" },
      items: [{ key: "one", title: "Original" }],
      code: { files: [{ path: "bootstrap.js", text: `
        async function startup() {
          const item = Zotero.Items.get(1);
          window.foundation = {
            instance: item instanceof Zotero.Item,
            get: Zotero.Item.prototype.getField.call(item, "title"),
            set: Zotero.Item.prototype.setField.call(item, "title", "Updated"),
            same: item.isFieldOfBase("title", "title"),
            different: item.isFieldOfBase("title", "shortTitle"),
            readers: Zotero.Reader._readers,
            global: Components.utils.getGlobalForObject(item) === window,
            dead: Cu.isDeadWrapper(item),
          };
        }
      ` }] },
    });
    expect(runInNewContext("foundation", context)).toEqual({
      instance: true, get: "Original", set: true, same: true, different: false,
      readers: [], global: true, dead: false,
    });
    expect(messages().at(-1)).toEqual({ type: "ready" });
  });

  it("delays promises and exposes both deferred settlement functions", async () => {
    vi.useFakeTimers();
    try {
      const { context } = zoteroRuntime();
      const done = vi.fn();
      const delayed = runInNewContext("Zotero.Promise.delay(25)", context) as Promise<void>;
      void delayed.then(done);
      await vi.advanceTimersByTimeAsync(24);
      expect(done).not.toHaveBeenCalled();
      await vi.advanceTimersByTimeAsync(1);
      await delayed;
      expect(done).toHaveBeenCalledOnce();
      const deferred = runInNewContext("Zotero.Promise.defer()", context) as {
        promise: Promise<string>; resolve(value: string): void; reject(error: Error): void;
      };
      deferred.resolve("settled");
      await expect(deferred.promise).resolves.toBe("settled");
      const rejected = runInNewContext("Zotero.Promise.defer()", context) as typeof deferred;
      const result = expect(rejected.promise).rejects.toThrow("rejected");
      rejected.reject(new Error("rejected"));
      await result;
      await expect(runInNewContext("Zotero.Promise.all([Zotero.Promise.resolve(1), 2])", context)).resolves.toEqual([1, 2]);
    } finally {
      vi.useRealTimers();
    }
  });

  it("triggers Actions & Tags' default tag action only for new items with stable IDs", async () => {
    const { receive, context, messages, items } = zoteroRuntime();
    const get = (key: string) => {
      const item = items.getByLibraryAndKey(1, key);
      if (!item) throw new Error(`Missing test item ${key}`);
      return item;
    };
    await receive({ type: "start", manifest: { id: "actions", version: "1" }, items: [{ key: "one" }], code: { files: [
      { path: "bootstrap.js", text: `function startup() {
        window.added = [];
        Zotero.Notifier.registerObserver({ async notify(event, type, ids) {
          if (event !== "add" || type !== "item") return;
          for (const item of Zotero.Items.get(ids).filter(item => !(item instanceof Zotero.FeedItem))) {
            added.push(item.key);
            item.addTag("/unread", 1);
            await item.saveTx();
          }
        } }, ["item"]);
      }` },
    ] } });
    const originalID = get("one").id;
    await receive({ type: "items", items: [{ key: "two" }, { key: "one" }], events: [{ event: "add", objectType: "item", itemKeys: ["two"] }] });
    expect(get("one").id).toBe(originalID);
    expect(get("two").id).not.toBe(originalID);
    expect(runInNewContext("added", context)).toEqual(["two"]);
    const saved = messages().find((message) => message.method === "items.update");
    expect(saved).toMatchObject({ args: ["two", "{}", ["/unread"]] });
    await receive({ type: "reply", id: saved.id, ok: true });
    await receive({ type: "items", items: [{ key: "one" }, { key: "two", tags: [{ tag: "/unread" }] }] });
    expect(runInNewContext("added", context)).toEqual(["two"]);
    expect(get("two").hasTag("/unread")).toBe(true);
    await receive({ type: "items", items: [{ key: "one" }] });
    await receive({ type: "items", items: [{ key: "three" }, { key: "one" }], events: [{ event: "add", objectType: "item", itemKeys: ["three"] }] });
    expect(runInNewContext("added", context)).toEqual(["two", "three"]);
    expect(get("three").id).toBeGreaterThan(originalID + 1);
  });

  it("reports asynchronous notifier failures under the plugin", async () => {
    const { context, receive, messages } = zoteroRuntime();
    runInNewContext(`Zotero.Notifier.registerObserver({ async notify() { throw new Error("async observer failed"); } }, ["item"])`, context);
    await receive({ type: "items", items: [{ key: "one" }], events: [{ event: "add", objectType: "item", itemKeys: ["one"] }] });
    await vi.waitFor(() => expect(messages().at(-1)).toMatchObject({ type: "error", message: expect.stringContaining("async observer failed") }));
  });

  it("loads strict packaged defaults before startup without executing preference code", async () => {
    const { receive, context, messages } = zoteroRuntime();
    await receive({
      type: "start", manifest: { id: "prefs", version: "1" },
      code: { prefs: JSON.stringify({ "extensions.zotero.test.enabled": false, "test.userOnly": 7 }), files: [
        { path: "prefs.js", text: String.raw`
          // Only literal declarations may become defaults.
          pref("extensions.zotero.test.enabled", true);
          pref("test.count", 3);
          pref("test.text", "quoted \"value\" and \\ path"); // comment
          pref("test.zero", 0)
          pref("test.no", false);
          pref("test.invalidEscape", "\x41");
          pref("test.array", [1]);
          pref("test.object", {});
          pref("test.null", null);
          pref("test.computed", (window.executed = true));
          pref("test.trailing", 1); window.executed = true;
          window.executed = true;
        ` },
        { path: "bootstrap.js", text: `function startup() {
          window.startupPrefs = [Zotero.Prefs.get("test.enabled"), Zotero.Prefs.get("test.count", true)];
        }` },
      ] },
    });
    expect(runInNewContext("startupPrefs", context)).toEqual([false, 3]);
    expect(runInNewContext("window.executed", context)).toBeUndefined();
    expect(runInNewContext(`[
      Zotero.Prefs.get("test.text", true), Zotero.Prefs.get("test.zero", true), Zotero.Prefs.get("test.no", true),
      ...["invalidEscape", "array", "object", "null", "computed", "trailing"].map(name => Zotero.Prefs.get("test." + name, true))
    ]`, context)).toEqual(['quoted "value" and \\ path', 0, false, ...Array(6).fill(undefined)]);
    expect(runInNewContext(`Services.prefs.getChildList("test.")`, context))
      .toEqual(["test.count", "test.no", "test.text", "test.userOnly", "test.zero"]);
    expect(runInNewContext(`Services.prefs.getDefaultBranch("test.").getChildList("")`, context))
      .toEqual(["count", "no", "text", "userOnly", "zero"]);
    expect(runInNewContext(`[
      Services.prefs.getBoolPref("extensions.zotero.test.enabled"),
      Services.prefs.getDefaultBranch("extensions.zotero.test.").getBoolPref("enabled"),
      Services.prefs.getBranch("test.").getIntPref("count"),
      Services.prefs.getDefaultBranch("test.").getIntPref("userOnly", 9),
      Services.prefs.getCharPref("missing", "fallback")
    ]`, context)).toEqual([false, true, 3, 9, "fallback"]);
    runInNewContext(`
      window.changes = [];
      const observer = Zotero.Prefs.registerObserver("test.enabled", value => changes.push(value));
      Zotero.Prefs.set("test.enabled", false);
      Zotero.Prefs.clear("test.enabled");
      Zotero.Prefs.unregisterObserver(observer);
      Zotero.Prefs.set("test.enabled", false);
      Zotero.Prefs.set("test.keep", "keep");
      Zotero.Prefs.resetBranch(["keep"], "extensions.zotero.test.");
      Services.prefs.setIntPref("test.count", 10);
      Services.prefs.clearUserPref("test.count");
    `, context);
    expect(runInNewContext("changes", context)).toEqual([false, true]);
    expect(runInNewContext(`Zotero.Prefs.get("test.enabled")`, context)).toBe(true);
    expect(runInNewContext(`Zotero.Prefs.get("test.count", true)`, context)).toBe(3);
    expect(runInNewContext(`Zotero.Prefs.get("test.keep")`, context)).toBe("keep");
    runInNewContext(`Zotero.Prefs.resetBranch([])`, context);
    expect(runInNewContext(`Zotero.Prefs.get("test.keep")`, context)).toBeUndefined();
    const saved = messages().filter((message) => message.type === "call").map((message) => JSON.parse(message.args[0]));
    expect(saved.at(-1)).toEqual({ "test.userOnly": 7 });
    expect(messages().filter((message) => message.type === "error")).toEqual([]);
  });

  it("loads Linter's ConcurrentCaller constructor from a plain mjs Gecko module", () => {
    const { context } = zoteroRuntime();
    expect(runInNewContext(`(() => {
      const { ConcurrentCaller } = ChromeUtils.importESModule("resource://zotero/concurrentCaller.mjs");
      return new ConcurrentCaller({ numConcurrent: 2, stopOnError: false }).options;
    })()`, context)).toEqual({ numConcurrent: 2, stopOnError: false });
  });

  it("reads packaged HTTP resources locally while keeping web requests on the host", async () => {
    const { context, receive, messages } = zoteroRuntime();
    await receive({ type: "start", manifest: { id: "icons", version: "1" }, code: { files: [
      { path: "bootstrap.js", text: `function startup() {
        Components.classes["@mozilla.org/addons/addon-manager-startup;1"].getService()
          .registerChrome(null, [["content", "icons", "content/"]]);
      }` },
      { path: "content/icons/test.svg", text: "<svg/>" },
      { path: "data.json", text: '{"value":3}' },
    ] } });
    await expect(runInNewContext(`Zotero.HTTP.request("GET", "chrome://icons/content/icons/test.svg")`, context))
      .resolves.toEqual({ status: 200, responseText: "<svg/>", response: "<svg/>" });
    await expect(runInNewContext(`Zotero.HTTP.request("GET", "plugin://zotero/data.json", { responseType: "json" })`, context))
      .resolves.toMatchObject({ response: { value: 3 } });
    await expect(runInNewContext(`Zotero.HTTP.request("GET", "chrome://icons/content/missing.svg")`, context))
      .rejects.toThrow("is not in the plugin");
    await expect(runInNewContext(`Zotero.HTTP.request("POST", "plugin://zotero/data.json")`, context))
      .rejects.toThrow("only support GET");
    expect(messages().filter((message) => message.type === "call")).toEqual([]);
    const web = runInNewContext(`Zotero.HTTP.request("GET", "https://example.test/data")`, context);
    const request = messages().at(-1);
    expect(request).toMatchObject({ type: "call", method: "requestUrl", args: [{ url: "https://example.test/data" }] });
    await receive({ type: "reply", id: request.id, ok: true, value: { status: 200, text: "web" } });
    await expect(web).resolves.toMatchObject({ response: "web" });
  });

  it("runs Linter batches with bounded concurrency and waits until queued work finishes", async () => {
    const { context } = zoteroRuntime();
    const caller = runInNewContext(`new (ChromeUtils.importESModule("resource://zotero/concurrentCaller.mjs").ConcurrentCaller)({ numConcurrent: 2 })`, context);
    let active = 0, maximum = 0;
    const started: number[] = [];
    const release: (() => void)[] = [];
    const jobs = [0, 1, 2].map((index) => caller.start(async () => {
      started.push(index);
      maximum = Math.max(maximum, ++active);
      await new Promise<void>((resolve) => { release[index] = resolve; });
      active--;
      return index;
    }));
    let finished = false;
    const waiting = caller.wait().then(() => { finished = true; });
    await Promise.resolve();
    expect(started).toEqual([0, 1]);
    expect(finished).toBe(false);
    release[0]();
    await jobs[0];
    // Settlement schedules the next queued task after the result promise.
    await vi.waitFor(() => expect(started).toEqual([0, 1, 2]));
    expect(finished).toBe(false);
    release[1]();
    release[2]();
    await waiting;
    expect(await Promise.all(jobs)).toEqual([0, 1, 2]);
    expect(maximum).toBe(2);
    await expect(caller.start(() => "next batch")).resolves.toBe("next batch");
    await caller.wait();
  });

  it("reports queue errors, continues when requested and cancels queued work on stop", async () => {
    const onError = vi.fn();
    const { context, messages } = zoteroRuntime({ onError });
    const make = (stopOnError: boolean) => runInNewContext(`new (ChromeUtils.importESModule("resource://zotero/concurrentCaller.mjs").ConcurrentCaller)({ numConcurrent: 1, stopOnError: ${stopOnError}, onError })`, context);
    const caller = make(false);
    const failed = caller.start(() => { throw new Error("failed"); });
    const next = caller.start(() => "continued");
    await expect(failed).rejects.toThrow("failed");
    await caller.wait();
    await expect(next).resolves.toBe("continued");
    expect(onError).toHaveBeenCalledOnce();
    expect(messages().filter((message) => message.type === "error")).toEqual([
      { type: "error", message: expect.stringContaining("failed") },
    ]);
    for (const stopOnError of [false, true]) {
      const stopped = make(stopOnError);
      let release!: () => void;
      const first = stopped.start(async () => {
        await new Promise<void>((resolve) => { release = resolve; });
        if (stopOnError) throw new Error("stop after error");
      });
      const queued = vi.fn();
      const canceled = stopped.start(queued);
      const rejection = expect(canceled).rejects.toMatchObject({ name: "CanceledException" });
      await Promise.resolve();
      if (!stopOnError) stopped.stop();
      const result = stopOnError ? expect(first).rejects.toThrow("stop after error") : first;
      release();
      await result;
      await stopped.wait();
      await rejection;
      expect(queued).not.toHaveBeenCalled();
    }
  });

  it("supports named and iterable Fluent attributes, interpolation and document localization", async () => {
    const { receive, context, messages, doc } = zoteroRuntime();
    await receive({
      type: "start", manifest: { id: "locale", version: "1" }, code: { files: [
        { path: "bootstrap.js", text: "function startup() {}" },
        { path: "locale/en-US/test.ftl", text: `greeting = Hello { $name } ({$count})
    .label = Greet { $name }
    .tooltiptext = Count { $count }
only-attribute =
    .label = Label only
remove-confirm =
    Delete the selected { $count ->
        [one] { $count } action?
       *[other] { $count } actions?
    } This cannot be undone.` },
      ] },
    });
    const message = runInNewContext(`new Localization().formatMessagesSync([{ id: "greeting", args: { name: "Ada", count: 0 } }])[0]`, context);
    expect(message.value).toBe("Hello Ada (0)");
    expect(runInNewContext('new Localization().formatValueSync("remove-confirm", { count: 1 })', context)).toBe("Delete the selected 1 action? This cannot be undone.");
    expect(runInNewContext('new Localization().formatValueSync("remove-confirm", { count: 2 })', context)).toBe("Delete the selected 2 actions? This cannot be undone.");
    expect(message.attributes.label).toBe("Greet Ada");
    expect([...message.attributes]).toEqual([{ name: "label", value: "Greet Ada" }, { name: "tooltiptext", value: "Count 0" }]);
    const asyncMessage = await runInNewContext(`new Localization().formatMessages([{ id: "greeting", args: { name: "Ada", count: 0 } }])`, context);
    expect(asyncMessage[0]).toEqual(message);
    expect(runInNewContext(`new Localization().formatMessagesSync(["missing", "only-attribute"])`, context)[0]).toBeNull();
    expect(runInNewContext(`new Localization().formatValueSync("greeting", { name: "Ada" })`, context)).toBe("Hello Ada ({$count})");
    expect(runInNewContext(`new Localization().formatValuesSync([{ id: "greeting", args: { name: "Ada", count: 2 } }, "missing"])`, context))
      .toEqual(["Hello Ada (2)", null]);
    expect(await runInNewContext(`document.l10n.formatValue("greeting", { name: "Ada", count: 3 })`, context)).toBe("Hello Ada (3)");
    expect(await runInNewContext(`document.l10n.formatValues([{ id: "greeting", args: { name: "Ada", count: 4 } }])`, context))
      .toEqual(["Hello Ada (4)"]);
    runInNewContext(`
      window.localized = document.createXULElement("label");
      document.body.append(localized);
      document.l10n.setAttributes(localized, "greeting", { name: "Ada", count: 5 });
    `, context);
    const element = doc.body.children.at(-1)!;
    expect(element.textContent).toBe("Hello Ada (5)");
    expect(element.label).toBe("Greet Ada");
    expect(element.getAttribute("data-l10n-id")).toBe("greeting");
    expect(JSON.parse(element.getAttribute("data-l10n-args")!)).toEqual({ name: "Ada", count: 5 });
    runInNewContext(`document.l10n.setAttributes(localized, "only-attribute")`, context);
    expect(element.getAttribute("data-l10n-args")).toBeNull();
    expect(element.label).toBe("Label only");
    await expect(runInNewContext(`document.l10n.translateFragment(document.body)`, context)).resolves.toBeUndefined();
    await expect(runInNewContext(`document.l10n.translateElements([localized])`, context)).resolves.toBeUndefined();
    expect(runInNewContext(`[
      Zotero.ftl.addResourceIds(["test.ftl"]), Zotero.ftl.removeResourceIds(["test.ftl"]),
      MozXULElement.insertFTLIfNeeded("test.ftl")
    ]`, context)).toEqual([undefined, undefined, undefined]);
    expect(runInNewContext(`typeof MozXULElement.insertFTLIfNeeded`, context)).toBe("function");
    expect(messages().filter((message) => message.type === "error")).toEqual([]);
  });

  it("provides HTMLElement bases and static XUL helpers for custom element definitions", () => {
    const define = vi.fn();
    const { context } = zoteroRuntime({ customElements: { define } });
    runInNewContext(`
      window.fragment = {};
      window.markup = null;
      document.createElement = (tag) => ({
        content: fragment,
        set innerHTML(value) { markup = value; }
      });
      customElements.define("plugin-element", class extends XULElementBase {});
      window.parsed = MozXULElement.parseXULToFragment('<label value="Hello"/>');
      MozXULElement.implementCustomInterface(function() {}, []);
    `, context);
    const customClass = define.mock.calls[0][1];
    expect(new customClass()).toBeInstanceOf(context.HTMLElement);
    expect(runInNewContext("XULElementBase.prototype instanceof HTMLElement", context)).toBe(true);
    expect(runInNewContext("parsed === fragment", context)).toBe(true);
    expect(runInNewContext("markup", context)).toBe('<label value="Hello"/>');
  });

  it("throttles calls with the latest arguments and receiver", () => {
    vi.useFakeTimers();
    try {
      const { context } = zoteroRuntime({ Date, clearTimeout });
      runInNewContext(`
        window.calls = [];
        window.receiver = { name: "receiver", run: Zotero.Utilities.throttle(function(value) {
          calls.push([this.name, value]); return value;
        }, 100) };
        receiver.run(1); receiver.run(2); receiver.run(3);
      `, context);
      expect(runInNewContext("calls", context)).toEqual([["receiver", 1]]);
      vi.advanceTimersByTime(100);
      expect(runInNewContext("calls", context)).toEqual([["receiver", 1], ["receiver", 3]]);
      vi.advanceTimersByTime(100);
      expect(runInNewContext("receiver.run(4)", context)).toBe(4);
    } finally {
      vi.useRealTimers();
    }
  });

  it("preserves an existing MozXULElement", () => {
    const existing = { insertFTLIfNeeded: vi.fn(), customElement: true };
    const { context } = zoteroRuntime({ MozXULElement: existing });
    expect(runInNewContext("MozXULElement", context)).toBe(existing);
  });

  it("enumerates windows with iterable and XPCOM methods sharing a cursor", () => {
    const { context } = zoteroRuntime();
    expect(runInNewContext(`Array.from(Services.wm.getEnumerator()).map(win => win === window)`, context)).toEqual([true]);
    expect(runInNewContext(`(() => {
      const enumerator = Services.wm.getEnumerator();
      return [enumerator.hasMoreElements(), enumerator.getNext() === window,
        enumerator.hasMoreElements(), enumerator.next().done];
    })()`, context)).toEqual([true, true, false, true]);
    expect(runInNewContext(`(() => {
      const enumerator = Services.wm.getEnumerator();
      enumerator.next();
      return [enumerator.hasMoreElements(), ...enumerator];
    })()`, context)).toEqual([false]);
  });

  it("provides progress line text and image elements without losing notices", () => {
    const { context, messages } = zoteroRuntime();
    const line = runInNewContext(`
      window.progress = new Zotero.ProgressWindow();
      window.line = new progress.ItemProgress("book", "Starting");
      line;
    `, context);
    expect(line._itemText.textContent).toBe("Starting");
    expect(line._image.getAttribute("data-item-type")).toBe("book");
    runInNewContext(`line.setText("Finished"); line.setIcon("article"); progress.changeHeadline("Status"); progress.show();`, context);
    expect(line._itemText.textContent).toBe("Finished");
    expect(line._image.getAttribute("data-item-type")).toBe("article");
    expect(messages().at(-1)).toEqual({ type: "notice", text: "Status\nFinished" });
  });

  it("publishes the final progress lines when a toolkit plugin closes its popup", () => {
    const { context, messages } = zoteroRuntime();
    runInNewContext(`
      const progress = new Zotero.ProgressWindow();
      progress.changeHeadline("Linter");
      const line = new progress.ItemProgress("", "Preparing");
      progress.show();
      line.setText("Finished: 1 failed");
      progress.startCloseTimer(5000);
    `, context);
    expect(messages().filter((message) => message.type === "notice")).toEqual([
      { type: "notice", text: "Linter\nPreparing" },
      { type: "notice", text: "Linter\nFinished: 1 failed" },
    ]);
  });

  it("closes registered progress windows before an Actions & Tags action notice", () => {
    const { context, messages } = zoteroRuntime();
    runInNewContext(`
      window.closed = 0;
      const previous = new Zotero.ProgressWindow();
      previous.addDescription("Previous");
      previous.show();
      const close = previous.close.bind(previous);
      previous.close = () => { closed++; close(); };
      Zotero.ProgressWindowSet.closeAll();
      Zotero.ProgressWindowSet.closeAll();
      const action = new Zotero.ProgressWindow();
      action.changeHeadline("Actions & Tags");
      action.addDescription("Added /unread");
      action.show();
      action.startCloseTimer(5000);
      Zotero.ProgressWindowSet.closeAll();
    `, context);
    expect(runInNewContext("closed", context)).toBe(1);
    expect(messages().at(-1)).toEqual({ type: "notice", text: "Actions & Tags\nAdded /unread" });
  });
});

describe("Zotero MenuManager", () => {
  it("appends localized menus after legacy entries and supplies command selections", async () => {
    const { receive, context, messages, items } = zoteroRuntime();
    await receive({ type: "start", manifest: { id: "menus", version: "1" }, code: { files: [
      { path: "bootstrap.js", text: `
        function startup() {
          window.calls = [];
          const legacy = document.createXULElement("menuitem");
          legacy.label = "Legacy";
          document.querySelector("#zotero-itemmenu").append(legacy);
          window.registration = Zotero.MenuManager.registerMenu({
            pluginID: "test", menuID: "item", target: "main/library/item", menus: [
              { menuType: "separator" },
              { menuType: "menuitem", l10nID: "hidden", onShowing(e, c) { c.setVisible(false); } },
              { menuType: "submenu", l10nID: "group", menus: [
                { menuType: "menuitem", l10nID: "run", l10nArgs: { count: 0 },
                  onShowing(e, c) { c.setL10nArgs({ count: c.items.length }); },
                  onShown(e, c) { window.shown = [e.type, c.menuElem.label]; },
                  onCommand(e, c) { calls.push([e.type, c.items, c.menuElem.label, c.fieldName]); } },
                { menuType: "separator" },
                { menuType: "submenu", l10nID: "deeper", onShowing(e, c) { c.setEnabled(false); }, menus: [
                  { menuType: "menuitem", l10nID: "deep" }
                ] },
              ] },
              { menuType: "menuitem", l10nID: "hidden", onShowing(e, c) { c.menuElem.hidden = true; } },
              { menuType: "menuitem", l10nID: "run", onShowing(e, c) { c.menuElem.label = "Custom"; c.menuElem.disabled = true; } },
            ]
          });
          window.otherRegistration = Zotero.MenuManager.registerMenu({
            pluginID: "test", menuID: "other", target: "main/library/collection",
            menus: [{ menuType: "menuitem", l10nID: "hidden", onShowing() { throw new Error("wrong target"); } }]
          });
        }` },
      { path: "locale/en-US/menus.ftl", text: `group = Value ignored
    .label = Group
run = Run { $count }
hidden = Hidden
deeper = Deeper
deep = { deep-label.label }
deep-label =
    .label = Deep` },
    ] }, items: [{ key: "one" }, { key: "two" }] });
    await receive({ type: "item-menu", id: 1, itemKeys: ["one", "missing"] });
    expect(messages().at(-1)).toEqual({ type: "item-menu", id: 1, items: [
      { index: 0, label: "Legacy", disabled: false },
      { separator: true },
      { label: "Group", disabled: false, children: [
        { index: 1, label: "Run 1", disabled: false }, { separator: true }, { index: 2, label: "Deep", disabled: true },
      ] },
      { index: 3, label: "Custom", disabled: true },
    ] });
    expect(runInNewContext("shown", context)).toEqual(["popupshown", "Run 1"]);
    await receive({ type: "item-menu", id: 2, itemKeys: ["two"] });
    await receive({ type: "item-command", menu: 1, index: 1, path: ["Group", "Run 1"], itemKeys: ["two"] });
    expect(runInNewContext("calls", context)).toEqual([["command", [items.getByLibraryAndKey(1, "two")], "Run 1", undefined]]);
    // A unique path still works when the per-request options have been evicted.
    await receive({ type: "item-command", menu: -1, index: 0, path: ["Group", "Run 1"], itemKeys: ["one"] });
    expect(runInNewContext("calls.length", context)).toBe(2);
    await receive({ type: "item-command", menu: 1, index: 2, path: ["Group", "Deep"], itemKeys: ["one"] });
    expect(runInNewContext("calls.length", context)).toBe(2);
    expect(runInNewContext("Zotero.MenuManager.unregisterMenu(otherRegistration)", context)).toBe(true);
    expect(runInNewContext("Zotero.MenuManager.unregisterMenu(registration)", context)).toBe(true);
    await receive({ type: "item-command", menu: 1, index: 1, path: ["Group", "Run 1"], itemKeys: ["one"] });
    expect(runInNewContext("calls.length", context)).toBe(2);
    await receive({ type: "item-menu", id: 3, itemKeys: ["one"] });
    expect(messages().at(-1)).toMatchObject({ items: [{ index: 0, label: "Legacy", disabled: false }] });
  });

  it("honors l10n attribute changes, parent visibility and reports hook failures once per request", async () => {
    const { receive, context, messages } = zoteroRuntime();
    await receive({ type: "start", manifest: { id: "menus", version: "1" }, code: { files: [
      { path: "bootstrap.js", text: `function startup() {} ` },
      { path: "locale/en-US/test.ftl", text: "changed = Changed { $name }" },
    ] } });
    runInNewContext(`Zotero.MenuManager.registerMenu({ target: "main/library/item", menus: [
      { menuType: "menuitem", l10nID: "old", onShowing(e, c) {
        c.menuElem.setAttribute("data-l10n-id", "changed");
        c.menuElem.setAttribute("data-l10n-args", JSON.stringify({ name: "Ada" }));
      } },
      { menuType: "submenu", l10nID: "hidden", onShowing(e, c) { c.setVisible(false); }, menus: [
        { menuType: "menuitem", l10nID: "child", onShowing() { throw new Error("hidden child"); } }
      ] },
      ...[1, 2].map(() => ({ menuType: "menuitem", onShowing() { throw new Error("hook failed"); } }))
    ] })`, context);
    await receive({ type: "item-menu", id: 1, itemKeys: [] });
    expect(messages().at(-1)).toMatchObject({ items: [{ index: 0, label: "Changed Ada", disabled: false }] });
    expect(messages().filter((message) => message.type === "error")).toHaveLength(1);
  });
});

describe("Zotero item menus", () => {
  it("shows toolkit submenus for the selection and commands read the bare global pane", async () => {
    const { receive, messages, doc, window, context, items } = zoteroRuntime();
    await receive({ type: "items", items: [{ key: "one", title: "First item" }, { key: "two", title: "Second item" }] });
    const popup = doc.querySelector("#zotero-itemmenu")!;
    expect(popup.getAttribute("hidden")).toBe("true");
    expect(window.Zotero.getActiveZoteroPane()).toBe(window.ZoteroPane);
    expect(window.ZoteroPane_Local).toBe(window.ZoteroPane);
    expect(window.ZoteroPane.document).toBe(doc);
    const commands = { selectedCommand: vi.fn(), submenuShowing: vi.fn() };
    runInNewContext(`
      const popup = document.querySelector("#zotero-itemmenu");
      const menu = document.createXULElement("menu");
      menu.setAttribute("label", "Read Status");
      menu.setAttribute("hidden", "false");
      menu.setAttribute("disabled", "false");
      const submenu = document.createXULElement("menupopup");
      for (const label of ["Read", "Unread"]) {
        const item = document.createXULElement("menuitem");
        item.setAttribute("label", label);
        item.setAttribute("disabled", "false");
        item.addEventListener("command", () => window.selectedCommand(label, ZoteroPane.getSelectedItems()));
        submenu.append(item);
      }
      submenu.addEventListener("popupshowing", () => window.submenuShowing());
      menu.append(submenu);
      popup.append(menu);
      popup.addEventListener("popupshowing", () => {
        if (ZoteroPane.getSelectedItems().length) menu.removeAttribute("hidden");
        else menu.setAttribute("hidden", "true");
      });
    `, Object.assign(context, commands));
    await receive({ type: "item-menu", id: 1, itemKeys: [] });
    expect(messages().at(-1)).toEqual({ type: "item-menu", id: 1, items: [] });
    await receive({ type: "item-menu", id: 2, itemKeys: ["one", "missing"] });
    expect(messages().at(-1)).toEqual({ type: "item-menu", id: 2, items: [{
      label: "Read Status", disabled: false, children: [
        { index: 0, label: "Read", disabled: false }, { index: 1, label: "Unread", disabled: false },
      ],
    }] });
    expect(commands.submenuShowing).toHaveBeenCalledOnce();
    expect(window.ZoteroPane.getSelectedItems()).toEqual([]);
    await receive({ type: "item-command", menu: 2, index: 1, path: ["Read Status", "Unread"], itemKeys: ["two"] });
    expect(commands.selectedCommand).toHaveBeenCalledExactlyOnceWith("Unread", [items.getByLibraryAndKey(1, "two")]);
    expect(window.ZoteroPane.getSelectedItems()).toEqual([]);
  });

  it("filters hidden and empty entries, cleans separators, and blocks disabled commands", async () => {
    const { doc, receive, messages } = zoteroRuntime();
    const popup = doc.querySelector("#zotero-itemmenu")!;
    const command = vi.fn();
    for (const [tag, label, hidden, disabled] of [
      ["menuseparator", "", "false", "false"], ["menuitem", "Hidden", "true", "false"],
      ["menuitem", "", "false", "false"], ["menuitem", "Run", "false", "false"],
      ["menuseparator", "", "false", "false"], ["menuseparator", "", "false", "false"],
      ["menuitem", "Disabled", "false", "true"], ["menuseparator", "", "false", "false"],
    ]) {
      const element = doc.createXULElement(tag);
      for (const [name, value] of Object.entries({ label, hidden, disabled })) element.setAttribute(name, value);
      element.addEventListener("command", command);
      popup.append(element);
    }
    await receive({ type: "item-menu", id: 1, itemKeys: [] });
    expect(messages().at(-1)).toEqual({ type: "item-menu", id: 1, items: [
      { index: 0, label: "Run", disabled: false }, { separator: true }, { index: 1, label: "Disabled", disabled: true },
    ] });
    for (const [index, label] of [[0, "Run"], [1, "Disabled"], [99, "Missing"]]) {
      await receive({ type: "item-command", menu: 1, index, path: [label], itemKeys: [] });
    }
    expect(command).toHaveBeenCalledOnce();
    popup.children[3].setAttribute("disabled", "true");
    await receive({ type: "item-command", menu: 1, index: 0, path: ["Run"], itemKeys: [] });
    expect(command).toHaveBeenCalledOnce();
  });

  it("reports popup failures once per request and restores the previous selection", async () => {
    const { doc, receive, messages, window } = zoteroRuntime();
    await receive({ type: "items", items: [{ key: "one" }, { key: "two" }] });
    const popup = doc.querySelector("#zotero-itemmenu")!;
    popup.addEventListener("popupshowing", () => { throw new Error("Cannot update menu"); });
    const menu = doc.createXULElement("menu");
    menu.setAttribute("label", "Status");
    const submenu = doc.createXULElement("menupopup");
    submenu.addEventListener("popupshowing", () => { throw new Error("Cannot update submenu"); });
    const item = doc.createXULElement("menuitem");
    item.setAttribute("label", "Run");
    const command = vi.fn();
    item.addEventListener("command", command);
    submenu.append(item);
    menu.append(submenu);
    popup.append(menu);
    await receive({ type: "item-menu", id: 1, itemKeys: ["one"] });
    await receive({ type: "item-command", menu: 1, index: 0, path: ["Status", "Run"], itemKeys: ["one"] });
    expect(command).toHaveBeenCalledOnce();
    await receive({ type: "item-menu", id: 2, itemKeys: ["two"] });
    expect(messages().filter((message) => message.type === "error")).toHaveLength(2);
    expect(window.ZoteroPane.getSelectedItems()).toEqual([]);
  });

  it("keeps an async command's selection across a later menu prefetch", async () => {
    const { doc, receive, window, items } = zoteroRuntime();
    await receive({ type: "items", items: [{ key: "one" }, { key: "two" }] });
    const popup = doc.querySelector("#zotero-itemmenu")!;
    const item = doc.createXULElement("menuitem");
    item.label = "Run";
    popup.append(item);
    let resume!: () => void;
    const paused = new Promise<void>((resolve) => { resume = resolve; });
    const command = vi.fn();
    const finished = new Promise<void>((resolve) => {
      item.addEventListener("command", async () => {
        await paused;
        command(window.ZoteroPane.getSelectedItems());
        resolve();
      });
    });
    const showing = vi.fn(() => window.ZoteroPane.getSelectedItems().map((item) => item.key));
    popup.addEventListener("popupshowing", showing);
    await receive({ type: "item-menu", id: 1, itemKeys: ["one"] });
    const running = receive({ type: "item-command", menu: 1, index: 0, path: ["Run"], itemKeys: ["one"] });
    await receive({ type: "item-menu", id: 2, itemKeys: ["two"] });
    expect(showing.mock.results.map((result) => result.value)).toEqual([["one"], ["two"]]);
    resume();
    await finished;
    await running;
    expect(window.ZoteroPane.getSelectedItems()).toEqual([]);
    expect(command).toHaveBeenCalledExactlyOnceWith([items.getByLibraryAndKey(1, "one")]);
  });

  it("retains 200 menus and finds a unique path after eviction or element replacement", async () => {
    const { doc, receive, messages, window } = zoteroRuntime();
    await receive({ type: "items", items: [{ key: "one" }, { key: "two" }] });
    const popup = doc.querySelector("#zotero-itemmenu")!;
    const menu = doc.createXULElement("menu");
    menu.label = "Status";
    const submenu = doc.createXULElement("menupopup");
    menu.append(submenu);
    popup.append(menu);
    const command = vi.fn();
    const showing = vi.fn(() => window.ZoteroPane.getSelectedItems().map((item) => item.key));
    popup.addEventListener("popupshowing", showing);
    const add = (label: string) => {
      const item = doc.createXULElement("menuitem");
      item.label = label;
      item.addEventListener("command", () => command(label));
      submenu.append(item);
      return item;
    };
    const remembered = add("Run");
    for (let id = 1; id <= 200; id++) await receive({ type: "item-menu", id, itemKeys: ["two"] });
    showing.mockClear();
    await receive({ type: "item-command", menu: 1, index: 0, path: ["Status", "Run"], itemKeys: ["one"] });
    expect(showing).not.toHaveBeenCalled();
    await receive({ type: "item-menu", id: 201, itemKeys: ["two"] });
    showing.mockClear();
    await receive({ type: "item-command", menu: 1, index: 0, path: ["Status", "Run"], itemKeys: ["one"] });
    expect(showing.mock.results.map((result) => result.value)).toEqual([["one"], ["one"]]);
    remembered.remove();
    add("Other");
    add("Run");
    await receive({ type: "item-command", menu: 201, index: 0, path: ["Status", "Run"], itemKeys: ["one"] });
    expect(command.mock.calls).toEqual([["Run"], ["Run"], ["Run"]]);
    // A changed parent label must not dispatch the old command.
    menu.label = "Changed";
    await receive({ type: "item-command", menu: 201, index: 0, path: ["Status", "Run"], itemKeys: ["one"] });
    expect(messages().at(-1)).toMatchObject({ type: "error", message: expect.stringContaining("missing") });
    expect(command).toHaveBeenCalledTimes(3);
  });

  it("refuses ambiguous and missing fallback paths with a plugin error", async () => {
    const { doc, receive, messages } = zoteroRuntime();
    const popup = doc.querySelector("#zotero-itemmenu")!;
    const command = vi.fn();
    for (let i = 0; i < 2; i++) {
      const item = doc.createXULElement("menuitem");
      item.label = "Run";
      item.addEventListener("command", command);
      popup.append(item);
    }
    await receive({ type: "item-command", menu: -1, index: 0, path: ["Run"], itemKeys: [] });
    expect(messages().at(-1)).toMatchObject({ type: "error", message: expect.stringContaining("ambiguous") });
    await receive({ type: "item-command", menu: -1, index: 0, path: ["Missing"], itemKeys: [] });
    expect(messages().at(-1)).toMatchObject({ type: "error", message: expect.stringContaining("missing") });
    expect(command).not.toHaveBeenCalled();
  });

  it("maps XUL properties to attributes and bubbles popup and command events", async () => {
    const { doc, receive, messages } = zoteroRuntime();
    const popup = doc.querySelector("#zotero-itemmenu")!;
    const menu = doc.createXULElement("menu");
    menu.label = "Status";
    const submenu = doc.createXULElement("menupopup");
    const item = doc.createXULElement("menuitem");
    item.label = "Run";
    item.checked = true;
    expect(item.getAttribute("checked")).toBe("true");
    item.checked = false;
    expect(item.getAttribute("checked")).toBeNull();
    item.setAttribute("checked", "true");
    expect(item.checked).toBe(true);
    submenu.append(item);
    menu.append(submenu);
    popup.append(menu);
    const showing = vi.fn((event) => event.target);
    const command = vi.fn((event) => event.target);
    popup.addEventListener("popupshowing", showing);
    popup.addEventListener("command", command);
    item.hidden = true;
    await receive({ type: "item-menu", id: 1, itemKeys: [] });
    expect(messages().at(-1)).toMatchObject({ items: [] });
    item.hidden = false;
    item.disabled = true;
    await receive({ type: "item-menu", id: 2, itemKeys: [] });
    expect(messages().at(-1)).toMatchObject({ items: [{ label: "Status", disabled: false, children: [{ index: 0, label: "Run", disabled: true }] }] });
    await receive({ type: "item-command", menu: 2, index: 0, path: ["Status", "Run"], itemKeys: [] });
    expect(command).not.toHaveBeenCalled();
    item.disabled = false;
    menu.disabled = true;
    await receive({ type: "item-command", menu: 2, index: 0, path: ["Status", "Run"], itemKeys: [] });
    expect(command).not.toHaveBeenCalled();
    menu.disabled = false;
    item.hidden = true;
    await receive({ type: "item-command", menu: 2, index: 0, path: ["Status", "Run"], itemKeys: [] });
    expect(command).not.toHaveBeenCalled();
    item.hidden = false;
    await receive({ type: "item-command", menu: 2, index: 0, path: ["Status", "Run"], itemKeys: [] });
    expect(command).toHaveBeenCalledOnce();
    expect(command.mock.results[0].value).toBe(item);
    expect(showing.mock.results.slice(0, 2).map((result) => result.value)).toEqual([popup, submenu]);
  });
});

describe("Zotero notes", () => {
  it("gives vault notes with an identity as child note items, read-only for now", async () => {
    const { window, receive } = zoteroRuntime();
    await receive({ type: "items", items: [
      { key: "PAPER", itemType: "journalArticle", title: "Deep learning" },
      { key: "note-1", itemType: "note", parentItem: "PAPER", title: "LeCun 2015", note: '<div data-schema-version="9"><h1>Deep learning</h1></div>' },
      { key: "note-2", itemType: "note", title: "Ideas", note: '<div data-schema-version="9"><p>Alone</p></div>' },
    ] });
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const Zotero = (window as any).Zotero;
    const paper = Zotero.Items.getByLibraryAndKey(1, "PAPER");
    const note = Zotero.Items.getByLibraryAndKey(1, "note-1");
    expect(note.isNote()).toBe(true);
    expect(note.isRegularItem()).toBe(false);
    expect(note.getNote()).toContain("<h1>Deep learning</h1>");
    expect(note.getNoteTitle()).toBe("LeCun 2015");
    expect(paper.getNotes()).toEqual([note.id]);
    expect(Zotero.Items.getTopLevel([note])).toEqual([paper]);
    expect(Zotero.Items.getByLibraryAndKey(1, "note-2").isTopLevelItem()).toBe(true);
    expect(() => note.setNote("<p>changed</p>")).toThrow(/only the notes they made/);
    expect(Zotero.Notes.noteToTitle("<div><h1>Title</h1><p>Body</p></div>")).toBe("Title");
  });

  it("lets plugins make notes and change the ones they made", async () => {
    const { window, receive, messages } = zoteroRuntime();
    await receive({ type: "items", items: [
      { key: "PAPER", itemType: "journalArticle", title: "Deep learning" },
      { key: "mine", itemType: "note", parentItem: "PAPER", title: "Old", note: "<p>Old</p>", paperloomPluginNote: true },
    ] });
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    const Zotero = (window as any).Zotero;
    const paper = Zotero.Items.getByLibraryAndKey(1, "PAPER");
    const made = new Zotero.Item("note");
    made.parentID = paper.id;
    made.setNote("<h1>Summary</h1><p>Text</p>");
    const saving = made.saveTx();
    await Promise.resolve();
    const create = messages().find((m) => m.type === "call" && m.method === "notes.create");
    expect(create.args).toEqual(["PAPER", "<h1>Summary</h1><p>Text</p>"]);
    await receive({ type: "reply", id: create.id, ok: true, value: "made-1" });
    expect(await saving).toBe(true);
    expect(made.key).toBe("made-1");
    expect(Zotero.Items.getByLibraryAndKey(1, "made-1")).toBe(made);
    expect(made.getNoteTitle()).toBe("Summary");

    const mine = Zotero.Items.getByLibraryAndKey(1, "mine");
    mine.setNote("<p>New</p>");
    const updating = mine.saveTx();
    await Promise.resolve();
    const update = messages().find((m) => m.type === "call" && m.method === "notes.update");
    expect(update.args).toEqual(["mine", "<p>New</p>"]);
    await receive({ type: "reply", id: update.id, ok: true, value: null });
    expect(await updating).toBe(true);
    expect(() => mine.setField("title", "x")).toThrow();
    expect(() => new Zotero.Item("journalArticle").setField("title", "x")).toThrow(/Only new notes/);
  });
});

describe("Zotero item columns", () => {
  it("normalizes keys, rejects invalid or duplicate columns, and supports aliases", () => {
    const { manager, messages } = zoteroRuntime();
    const options = { pluginID: "test@example.org", dataKey: "my_score", label: "Score" };
    for (const invalid of [null, undefined, {}, [], "column", ...["dataKey", "label", "pluginID"].flatMap((name) => [
      { ...options, [name]: undefined }, { ...options, [name]: 42 }, { ...options, [name]: {} },
    ]), { ...options, dataKey: "" }, { ...options, label: "" },
    { ...options, dataProvider: "provider" }, { ...options, renderCell: true }]) {
      expect(manager.registerColumn(invalid)).toBe(false);
    }
    expect(messages().filter((message) => message.type === "column")).toEqual([]);
    expect(manager.registerColumn(options)).toBe("test-example-org-my-score");
    expect(messages().at(-1)).toEqual({ type: "column", key: "test-example-org-my-score", label: "Score" });
    expect(manager.registerColumn({ ...options, dataKey: "my-score" })).toBe(false);
    expect(manager.registerColumns({ ...options, dataKey: "single" })).toBe("test-example-org-single");
    expect(manager.registerColumns([{ ...options, dataKey: "one" }, { ...options, dataKey: "two" }]))
      .toEqual(["test-example-org-one", "test-example-org-two"]);
    expect(messages().filter((message) => message.type === "column")).toHaveLength(4);
    expect(manager.unregisterColumn("test-example-org-my-score")).toBe(true);
    expect(messages().at(-1)).toEqual({ type: "column-removed", key: "test-example-org-my-score" });
    expect(manager.unregisterColumn("test-example-org-my-score")).toBe(false);
    expect(manager.unregisterColumns("test-example-org-single")).toBe(true);
    expect(manager.unregisterColumns(["test-example-org-one", "test-example-org-two"])).toEqual([true, true]);
    expect(messages().filter((message) => message.type === "column-removed")).toHaveLength(4);
    expect(manager.registerColumn(options)).toBe("test-example-org-my-score");
    // Zotero Reading List registers with an empty pluginID.
    expect(manager.registerColumn({ ...options, pluginID: "", dataKey: "read_status" })).toBe("read-status");
  });

  it("awaits providers and converts nullish and falsy values to text", async () => {
    const { manager, receive, messages } = zoteroRuntime();
    await receive({ type: "items", items: ["null", "undefined", "zero", "false"].map((key) => ({ key })) });
    const data: Record<string, unknown> = { null: null, undefined: undefined, zero: 0, false: false };
    const key = manager.registerColumn({
      pluginID: "test", dataKey: "value", label: "Value", dataProvider: async (item: { key: string }) => data[item.key],
    });
    await receive({ type: "column-values", id: 1, key, itemKeys: Object.keys(data) });
    expect(messages().at(-1)).toEqual({ type: "column-values", id: 1, values: { null: "", undefined: "", zero: "0", false: "false" } });
    await receive({ type: "column-values", id: 2, key, itemKeys: [] });
    expect(messages().at(-1)).toEqual({ type: "column-values", id: 2, values: {} });
  });

  it("batches provider values with original data keys, string limits, and one error report", async () => {
    const { manager, receive, messages } = zoteroRuntime();
    await receive({ type: "items", items: ["one", "two", "bad", "also-bad"].map((key) => ({ key, itemType: "book" })) });
    const provider = vi.fn((item: { key: string }, dataKey: string) => {
      expect(dataKey).toBe("my_score");
      if (item.key.includes("bad")) throw new Error("Failed to read score");
      return item.key === "one" ? "x".repeat(600) : 42;
    });
    const render = vi.fn();
    const key = manager.registerColumn({ pluginID: "test", dataKey: "my_score", label: "Score", dataProvider: provider, renderCell: render });
    await receive({ type: "column-values", id: 7, key, itemKeys: ["one", "two", "bad", "also-bad", "missing"] });
    expect(provider).toHaveBeenCalledTimes(4);
    expect(render).not.toHaveBeenCalled();
    expect(messages().at(-1)).toEqual({
      type: "column-values", id: 7, values: { one: "x".repeat(500), two: "42", bad: "", "also-bad": "", missing: "" },
    });
    expect(messages().filter((message) => message.type === "error")).toHaveLength(1);
  });

  it("reads item fields and extracts text from cells when only a renderer is supplied", async () => {
    const { manager, items, receive, messages, doc } = zoteroRuntime();
    await receive({ type: "items", items: [{ key: "one", title: "<b>Title</b>", itemType: "book" }, { key: "two", itemType: "book" }] });
    const field = manager.registerColumn({ pluginID: "test", dataKey: "title", label: "Title" });
    await receive({ type: "column-values", id: 1, key: field, itemKeys: ["one", "two"] });
    expect(messages().at(-1).values).toEqual({ one: "<b>Title</b>", two: "" });
    const item = items.getByLibraryAndKey(1, "one");
    if (item) item.getField = undefined;
    await receive({ type: "column-values", id: 4, key: field, itemKeys: ["one"] });
    expect(messages().at(-1).values).toEqual({ one: "" });
    const render = vi.fn((index: number, data: string, column: { dataKey: string }, first: boolean, document: unknown) => {
      expect(column.dataKey).toBe("test-cell");
      expect(first).toBe(false);
      expect(document).toBe(doc);
      expect(data).toBe("");
      return { textContent: index === 0 ? "cell text" : "y".repeat(600), innerHTML: "<b>ignored</b>" };
    });
    const cell = manager.registerColumn({ pluginID: "test", dataKey: "cell", label: "Cell", renderCell: render });
    await receive({ type: "column-values", id: 2, key: cell, itemKeys: ["one", "two"] });
    expect(messages().at(-1).values).toEqual({ one: "cell text", two: "y".repeat(500) });
    manager.unregisterColumn(String(cell));
    await receive({ type: "column-values", id: 3, key: cell, itemKeys: ["one"] });
    expect(messages().at(-1).values).toEqual({ one: "" });
  });

  it("isolates renderer errors and keeps empty cells empty", async () => {
    const { manager, receive, messages } = zoteroRuntime();
    const itemKeys = ["bad", "also-bad", "empty", "good"];
    await receive({ type: "items", items: itemKeys.map((key) => ({ key })) });
    const key = manager.registerColumn({
      pluginID: "test", dataKey: "cell", label: "Cell", renderCell: (index: number) => {
        if (index < 2) throw new Error("Failed to draw cell");
        return index === 2 ? null : { textContent: "cell text" };
      },
    });
    await receive({ type: "column-values", id: 1, key, itemKeys });
    expect(messages().at(-1)).toEqual({
      type: "column-values", id: 1, values: { bad: "", "also-bad": "", empty: "", good: "cell text" },
    });
    expect(messages().filter((message) => message.type === "error")).toHaveLength(1);
  });
});

describe("Zotero reader extensions", () => {
  it("publishes only changes to the set of supported listener types", () => {
    const { reader, messages } = zoteroRuntime();
    const first = vi.fn();
    const second = vi.fn();
    reader.registerEventListener("renderToolbar", first, "test");
    reader.registerEventListener("renderToolbar", first, "test");
    reader.registerEventListener("renderToolbar", second, "test");
    reader.registerEventListener("createViewContextMenu", first, "test");
    reader.unregisterEventListener("renderToolbar", vi.fn());
    reader.unregisterEventListener("renderToolbar", first);
    reader.unregisterEventListener("renderToolbar", second);
    reader.unregisterEventListener("createViewContextMenu", first);
    expect(messages().filter((message) => message.type === "reader-events")).toEqual([
      { type: "reader-events", types: ["renderToolbar"] },
      { type: "reader-events", types: ["renderToolbar", "createViewContextMenu"] },
      { type: "reader-events", types: ["createViewContextMenu"] },
      { type: "reader-events", types: [] },
    ]);
    reader.registerEventListener("unknown", first, "test");
    reader.unregisterEventListener("unknown", first);
    expect(messages().filter((message) => message.type === "partial")).toEqual([{ type: "partial", what: "reader extensions" }]);
  });

  it("gives readers the initialised views zotero-plugin-toolkit waits for", async () => {
    const { reader, receive } = zoteroRuntime();
    await receive({ type: "items", items: [{ key: "parent", attachments: [{ key: "attachment", contentType: "application/pdf" }] }] });
    await receive({ type: "tabs", seq: 1, readers: [{ viewID: "view", attachmentKey: "attachment", parentItemKey: "parent" }], selectedViewID: "view" });
    const [open] = (reader as unknown as { _readers: Record<string, any>[] })._readers;
    // As the toolkit's waitForReader does before touching a reader.
    await open._initPromise;
    await open._lastView.initializedPromise;
    await open._lastView._iframeWindow.PDFViewerApplication.initializedPromise;
    expect(open._internalReader._lastView).toBe(open._lastView);
  });

  it("renders every handler in a fresh container with reader, document and annotation data", async () => {
    const { reader, receive, messages, doc } = zoteroRuntime();
    await receive({ type: "items", items: [{ key: "parent", attachments: [{ key: "attachment", contentType: "application/pdf" }] }] });
    await receive({ type: "tabs", seq: 1, readers: [{ viewID: "view", attachmentKey: "attachment", parentItemKey: "parent" }], selectedViewID: "view" });
    for (const event of ["renderTextSelectionPopup", "renderSidebarAnnotationHeader", "renderToolbar"]) {
      const first = { innerHTML: "<b>first</b>" };
      reader.registerEventListener(event, async (props) => {
        expect(props.reader).toMatchObject({ itemID: 2, _item: { key: "attachment" }, type: "pdf", tabID: "view" });
        expect(props.doc).toBe(doc);
        expect(props.params).toEqual({ annotation: { text: "Selected text" } });
        await Promise.resolve();
        props.append(first, { innerHTML: "<i>second</i>" });
        setTimeout(() => { first.innerHTML = "<b>updated</b>"; }, 10);
      }, "test");
      reader.registerEventListener(event, ({ append }) => append({ innerHTML: "<span>third</span>" }), "test");
      await receive({ type: "reader-render", id: 1, event, attachmentKey: "attachment", itemKey: "parent", params: { annotation: { text: "Selected text" } } });
      expect(messages().at(-1)).toEqual({ type: "reader-rendered", id: 1, html: "<b>updated</b><i>second</i><span>third</span>" });
    }
  });

  it("replies with empty HTML when nothing is appended and reports render errors once", async () => {
    const { reader, receive, messages } = zoteroRuntime();
    reader.registerEventListener("renderToolbar", ({ reader }) => {
      expect(reader._item).toBe(false);
      throw new Error("Cannot draw toolbar");
    }, "test");
    reader.registerEventListener("renderToolbar", async () => { throw new Error("Another failure"); }, "test");
    await receive({ type: "reader-render", id: 2, event: "renderToolbar", attachmentKey: "attachment", itemKey: "missing" });
    expect(messages().at(-1)).toEqual({ type: "reader-rendered", id: 2, html: "" });
    expect(messages().filter((message) => message.type === "error")).toHaveLength(1);
    await receive({ type: "reader-render", id: 3, event: "unregistered" });
    expect(messages().at(-1)).toEqual({ type: "reader-rendered", id: 3, html: "" });
  });

  it("collects menu items, awaits handlers, isolates failures and dispatches enabled commands", async () => {
    const { reader, receive, messages } = zoteroRuntime();
    const command = vi.fn(async () => {});
    const disabled = vi.fn();
    for (const event of ["createAnnotationContextMenu", "createViewContextMenu"]) {
      reader.registerEventListener(event, async ({ append, params, doc, reader }) => {
        expect(doc).toBeUndefined();
        expect(reader._item).toBe(false);
        expect(params).toEqual(event === "createAnnotationContextMenu" ? { ids: ["mark"], currentID: "mark" } : {});
        await Promise.resolve();
        append({ label: "Run", onCommand: command }, { label: "Unavailable", disabled: true, onCommand: disabled });
        append(null, { label: 42 }, { label: "No command" });
      }, "test");
      reader.registerEventListener(event, () => { throw new Error("Cannot create menu"); }, "test");
      reader.registerEventListener(event, async () => { throw new Error("Another failure"); }, "test");
      reader.registerEventListener(event, ({ append }) => append({ label: "Last", onCommand: command }), "test");
      await receive({ type: "reader-menu", id: 4, event, attachmentKey: "attachment", params: event === "createAnnotationContextMenu" ? { ids: ["mark"], currentID: "mark" } : {} });
      expect(messages().at(-1)).toEqual({ type: "reader-menu", id: 4, items: [
        { index: 0, label: "Run", disabled: false }, { index: 1, label: "Unavailable", disabled: true },
        { index: 2, label: "No command", disabled: false }, { index: 3, label: "Last", disabled: false },
      ] });
      for (const index of [0, 1, 2, 3, 99]) await receive({ type: "reader-command", menu: 4, index });
    }
    expect(command).toHaveBeenCalledTimes(4);
    expect(disabled).not.toHaveBeenCalled();
    expect(messages().filter((message) => message.type === "error")).toHaveLength(2);
  });

  it("retains only twenty menus and clears listeners and commands on stop", async () => {
    const { reader, receive, messages } = zoteroRuntime();
    const command = vi.fn();
    reader.registerEventListener("createViewContextMenu", ({ append }) => append({ label: "Run", onCommand: command }), "test");
    for (let id = 1; id <= 21; id++) await receive({ type: "reader-menu", id, event: "createViewContextMenu" });
    await receive({ type: "reader-command", menu: 1, index: 0 });
    expect(command).not.toHaveBeenCalled();
    await receive({ type: "reader-command", menu: 2, index: 0 });
    await receive({ type: "reader-command", menu: 21, index: 0 });
    expect(command).toHaveBeenCalledTimes(2);
    await receive({ type: "stop" });
    expect(messages().at(-1)).toEqual({ type: "reader-events", types: [] });
    await receive({ type: "reader-command", menu: 21, index: 0 });
    expect(command).toHaveBeenCalledTimes(2);
    await receive({ type: "reader-menu", id: 22, event: "createViewContextMenu" });
    expect(messages().at(-1)).toEqual({ type: "reader-menu", id: 22, items: [] });
  });
});

/** The Obsidian runtime in a bare context: only what it touches as it loads. */
function obsidianRuntime() {
  class Node {}
  class Element extends Node {}
  class HTMLElement extends Element {}
  class DocumentFragment extends Node {}
  class Document extends Node {}
  const element = () => Object.assign(new HTMLElement(), { style: {}, appendChild: () => undefined, classList: { add() {} } });
  const window: Record<string, unknown> = {
    parent: { postMessage: vi.fn() },
    addEventListener: () => undefined,
  };
  const context = {
    window, Node, Element, HTMLElement, DocumentFragment, Document,
    document: { createElement: element, createDocumentFragment: () => new DocumentFragment(), head: element(), body: element() },
    navigator: { userAgent: "Linux" }, console, setTimeout, clearTimeout, structuredClone,
    MutationObserver: class { observe() {} disconnect() {} },
    requestAnimationFrame: (fn: () => void) => setTimeout(fn, 0),
  };
  const source = readFileSync(new URL("../../../core/plugin-runtime/obsidian.js", import.meta.url), "utf8");
  runInNewContext(source, context);
  return window;
}

type Req = { result: unknown; onsuccess?: () => void; onerror?: () => void; onupgradeneeded?: (e: { oldVersion: number }) => void; transaction?: unknown };
const settle = (request: Req) => new Promise<unknown>((resolve, reject) => {
  request.onsuccess = () => resolve(request.result);
  request.onerror = () => reject(new Error("request failed"));
});

describe("Obsidian runtime storage", () => {
  it("gives plugins IndexedDB in memory, as localForage and Dexie use it", async () => {
    const window = obsidianRuntime();
    type Store = { put(v: unknown, k?: unknown): Req; get(k: unknown): Req; count(): Req; openCursor(range?: unknown, dir?: string): Req; delete(k: unknown): Req };
    type Db = { createObjectStore(name: string, options?: unknown): Store; transaction(names: string | string[], mode?: string): { objectStore(name: string): Store; oncomplete?: () => void }; objectStoreNames: { contains(n: string): boolean }; version: number };
    const idb = window.indexedDB as { open(name: string, version?: number): Req };
    const range = window.IDBKeyRange as { bound(a: unknown, b: unknown): unknown };

    const opening = idb.open("dataview/cache", 1);
    let upgraded = -1;
    opening.onupgradeneeded = (event) => {
      upgraded = event.oldVersion;
      (opening.result as Db).createObjectStore("files");
    };
    const db = (await settle(opening)) as Db;
    expect(upgraded).toBe(0);
    expect(db.objectStoreNames.contains("files")).toBe(true);

    const writing = db.transaction("files", "readwrite");
    const files = writing.objectStore("files");
    for (const key of ["b.md", "a.md", "c.md"]) await settle(files.put({ path: key, tags: ["研究"] }, key));
    const done = new Promise<void>((resolve) => { writing.oncomplete = resolve; });
    await done;

    const reading = db.transaction(["files"]).objectStore("files");
    expect(await settle(reading.get("a.md"))).toEqual({ path: "a.md", tags: ["研究"] });
    expect(await settle(reading.get("missing.md"))).toBeUndefined();
    expect(await settle(reading.count())).toBe(3);

    const keys: unknown[] = [];
    await new Promise<void>((resolve) => {
      const walking = reading.openCursor(range.bound("a.md", "b.md"));
      walking.onsuccess = () => {
        const cursor = walking.result as { key: unknown; continue(): void } | null;
        if (!cursor) return resolve();
        keys.push(cursor.key);
        cursor.continue();
      };
    });
    expect(keys).toEqual(["a.md", "b.md"]);

    // A second store comes with a newer version, as localForage adds one.
    const reopening = idb.open("dataview/cache", 2);
    reopening.onupgradeneeded = () => (reopening.result as Db).createObjectStore("other", { keyPath: "id", autoIncrement: true });
    const db2 = (await settle(reopening)) as Db;
    const other = db2.transaction("other", "readwrite").objectStore("other");
    expect(await settle(other.put({ name: "x" }))).toBe(1);
    expect(await settle(db2.transaction("files").objectStore("files").count())).toBe(3);
  });

  it("stands in for Obsidian's CodeMirror global", () => {
    const window = obsidianRuntime();
    const codeMirror = window.CodeMirror as { defineMode(name: string, mode: unknown): unknown };
    expect(() => codeMirror.defineMode("dataview", () => ({}))).not.toThrow();
  });
});

describe("Zotero authoritative item events and selection", () => {
  it("preserves IDs and Item references, dispatches only explicit events and resolves deletes during async observers", async () => {
    const { context, receive } = zoteroRuntime();
    await receive({ type: "items", seq: 1, items: [{ key: "one", title: "Original" }] });
    runInNewContext(`
      window.original = Zotero.Items.get(1);
      window.events = [];
      window.deletedItem = null;
      Zotero.Notifier.registerObserver({ async notify(event, type, ids, extraData) {
        events.push([event, type, ids, extraData]);
        if (event === "delete") { await Promise.resolve(); deletedItem = Zotero.Items.get(ids[0]); }
      } }, ["item"]);
    `, context);
    await receive({ type: "items", seq: 2, items: [{ key: "one", title: "Changed" }],
      events: [{ event: "modify", objectType: "item", itemKeys: ["one"], changed: { one: ["title"] }, extraData: { skipAutoSync: true } }] });
    expect(runInNewContext("[original === Zotero.Items.get(1), original.getField('title'), events]", context)).toEqual([
      true, "Changed", [["modify", "item", [1], { skipAutoSync: true, 1: { changed: { title: true } } }]],
    ]);
    await receive({ type: "items", seq: 2, items: [{ key: "one", title: "Stale" }], events: [{ event: "add", objectType: "item", itemKeys: ["one"] }] });
    await receive({ type: "items", items: [{ key: "one", title: "Changed" }, { key: "two" }] });
    expect(runInNewContext("[original.getField('title'), events.length]", context)).toEqual(["Changed", 1]);
    await receive({ type: "items", seq: 3, items: [{ key: "one", title: "Changed", deleted: true }], events: [{ event: "trash", objectType: "item", itemKeys: ["one"] }] });
    await expect(runInNewContext("Zotero.Items.getAll()", context)).resolves.toEqual([]);
    expect(runInNewContext("[original === Zotero.Items.get(1), original.deleted]", context)).toEqual([true, true]);
    await receive({ type: "items", seq: 4, items: [{ key: "one", deleted: false }], events: [{ event: "modify", objectType: "item", itemKeys: ["one"] }] });
    expect(runInNewContext("[original === Zotero.Items.get(1), original.deleted]", context)).toEqual([true, false]);
    await receive({ type: "items", seq: 5, items: [], events: [{ event: "delete", objectType: "item", itemKeys: ["one"] }] });
    expect(runInNewContext("deletedItem === original", context)).toBe(true);
    await vi.waitFor(() => expect(runInNewContext("Zotero.Items.get(1)", context)).toBe(false));
    await receive({ type: "items", seq: 6, items: [{ key: "one" }] });
    expect(runInNewContext("Zotero.Items.getByLibraryAndKey(1, 'one').id", context)).toBe(1);
  });

  it("orders observers by priority, honors types and unregister and isolates synchronous/asynchronous failures", async () => {
    const { context, receive, messages } = zoteroRuntime();
    runInNewContext(`
      window.order = [];
      window.removed = Zotero.Notifier.registerObserver({ notify() { order.push('removed'); } }, ['item'], 'removed', 999);
      Zotero.Notifier.registerObserver({ notify() { order.push('late'); } }, ['item'], 'late', 666);
      Zotero.Notifier.registerObserver({ notify() { order.push('wrong'); } }, ['file'], 'wrong', 0);
      Zotero.Notifier.registerObserver({ notify() { order.push('early'); Zotero.Notifier.unregisterObserver(removed); throw new Error('sync failure'); } }, ['item'], 'early', 1);
      Zotero.Notifier.registerObserver({ async notify() { order.push('async'); throw new Error('async failure'); } }, null, 'async', 2);
    `, context);
    await receive({ type: "items", items: [{ key: "one" }], events: [{ event: "add", objectType: "item", itemKeys: ["one"] }] });
    expect(runInNewContext("order", context)).toEqual(["early", "async", "late"]);
    expect(messages().filter((message) => message.type === "error")).toHaveLength(2);
  });

  it("keeps pending fields, creators and tags across snapshots and emits no local save event", async () => {
    const { context, receive, messages } = zoteroRuntime();
    await receive({ type: "items", items: [{ key: "one", title: "Original", tags: [] }] });
    runInNewContext(`
      window.item = Zotero.Items.get(1);
      window.events = [];
      Zotero.Notifier.registerObserver({ notify(event) { events.push(event); } }, ['item']);
      item.setField('title', 'Pending'); item.addTag('pending'); item.setCreators([{ name: 'Pending Institute' }]);
      window.saved = item.saveTx();
    `, context);
    const update = messages().find((message) => message.method === "items.update");
    await receive({ type: "items", items: [{ key: "one", title: "Original", url: "new", tags: [{ tag: "other" }] }] });
    expect(runInNewContext("[item === Zotero.Items.get(1), item.getField('title'), item.getField('url'), item.hasTag('pending'), item.getCreator(0).lastName]", context))
      .toEqual([true, "Pending", "new", true, "Pending Institute"]);
    runInNewContext("item.addTag('later'); item.setField('title', 'Later')", context);
    await receive({ type: "reply", id: update.id, ok: true });
    await runInNewContext("saved", context);
    expect(runInNewContext("[item.hasChanged(), item.getField('title'), item.hasTag('later'), events]", context)).toEqual([true, "Later", true, []]);
  });

  it("retains local reverts made while a save is awaiting acknowledgment", async () => {
    const { context, receive, messages } = zoteroRuntime();
    const original = { key: "one", title: "Original", creators: [{ name: "Original Institute" }], tags: [{ tag: "original" }] };
    await receive({ type: "items", items: [original] });
    runInNewContext(`
      window.item = Zotero.Items.get(1);
      item.setField('title', 'Submitted'); item.setCreators([{ name: 'Submitted Institute' }]); item.addTag('submitted');
      window.saved = item.saveTx();
      item.setField('title', 'Original'); item.setCreators([{ name: 'Original Institute' }]); item.removeTag('submitted');
    `, context);
    const update = messages().find((message) => message.method === "items.update");
    await receive({ type: "items", items: [{ ...original, title: "Submitted", creators: [{ name: "Submitted Institute" }], tags: [{ tag: "original" }, { tag: "submitted" }] }] });
    expect(runInNewContext("[item.getField('title'), item.getCreator(0).lastName, item.hasTag('submitted')]", context))
      .toEqual(["Original", "Original Institute", false]);
    await receive({ type: "reply", id: update.id, ok: true });
    await runInNewContext("saved", context);
    expect(runInNewContext("item.hasChanged()", context)).toBe(true);
    const reverted = runInNewContext("item.saveTx()", context) as Promise<boolean>;
    const revert = messages().at(-1);
    expect(revert.args).toEqual(["one", JSON.stringify({ title: "Original", creators: [{ name: "Original Institute", creatorType: "author" }] }), ["original"]]);
    await receive({ type: "reply", id: revert.id, ok: true });
    await reverted;
    expect(runInNewContext("item.hasChanged()", context)).toBe(false);
  });

  it("supplies consumer-shaped Reading List and Linter add observers while Linter ignores modify", async () => {
    const { context, receive, messages } = zoteroRuntime();
    runInNewContext(`
      window.linted = [];
      Zotero.Notifier.registerObserver({ notify(event, type, ids) {
        if (event === 'add' && type === 'item') linted.push(...ids);
      } }, ['item'], 'linter', 666);
      Zotero.Notifier.registerObserver({ async notify(event, type, ids) {
        if (event !== 'add' || type !== 'item') return;
        for (const item of Zotero.Items.get(ids).filter(item => item.isRegularItem() && !item.deleted)) {
          item.setField('extra', 'Read_Status: Unread'); await item.saveTx();
        }
      } }, ['item'], 'reading-list');
    `, context);
    await receive({ type: "items", items: [{ key: "old" }] });
    expect(runInNewContext("linted", context)).toEqual([]);
    await receive({ type: "items", items: [{ key: "old" }, { key: "new" }], events: [{ event: "add", objectType: "item", itemKeys: ["new"] }] });
    const update = messages().find((message) => message.method === "items.update");
    expect(update.args).toEqual(["new", '{"extra":"Read_Status: Unread"}', null]);
    await receive({ type: "reply", id: update.id, ok: true });
    await receive({ type: "items", items: [{ key: "old" }, { key: "new", extra: "Read_Status: Unread" }], events: [{ event: "modify", objectType: "item", itemKeys: ["new"] }] });
    expect(runInNewContext("linted", context)).toEqual([2]);
    expect(messages().filter((message) => message.method === "items.update")).toHaveLength(1);
  });

  it("restores the latest persistent selection after menus/commands and awaits selectItems host acknowledgment", async () => {
    const { context, doc, receive, window, messages } = zoteroRuntime();
    await receive({ type: "items", items: [{ key: "one" }, { key: "two" }] });
    await receive({ type: "selection", seq: 1, itemKeys: ["one"] });
    const menu = doc.createXULElement("menuitem"); menu.label = "Run";
    doc.querySelector("#zotero-itemmenu")!.append(menu);
    let resume!: () => void;
    const paused = new Promise<void>((resolve) => { resume = resolve; });
    menu.addEventListener("command", async () => {
      await paused;
      expect(window.ZoteroPane.getSelectedItems().map((item) => item.key)).toEqual(["two"]);
    });
    await receive({ type: "item-menu", id: 1, itemKeys: ["two"] });
    expect(runInNewContext("ZoteroPane.getSelectedItems(true)", context)).toEqual([1]);
    const command = receive({ type: "item-command", menu: 1, index: 0, path: ["Run"], itemKeys: ["two"] });
    await receive({ type: "selection", seq: 3, itemKeys: ["one", "two"] });
    await receive({ type: "selection", seq: 2, itemKeys: [] });
    await receive({ type: "item-menu", id: 2, itemKeys: ["one"] });
    resume(); await command;
    expect(runInNewContext("ZoteroPane.getSelectedItems(true)", context)).toEqual([1, 2]);
    const selected = runInNewContext("ZoteroPane.selectItem(2)", context) as Promise<boolean>;
    const rpc = messages().at(-1);
    expect(rpc).toMatchObject({ method: "pane.selectItems", args: [["two"]] });
    const settled = vi.fn(); void selected.then(settled);
    await Promise.resolve(); expect(settled).not.toHaveBeenCalled();
    await receive({ type: "selection", seq: 4, itemKeys: ["two"] });
    await receive({ type: "reply", id: rpc.id, ok: true, value: true });
    await expect(selected).resolves.toBe(true);
    expect(runInNewContext("ZoteroPane.getSelectedItems(true)", context)).toEqual([2]);
    const multi = runInNewContext("ZoteroPane.selectItems([1, 2])", context) as Promise<boolean>;
    const multiRPC = messages().at(-1);
    expect(multiRPC.args).toEqual([["one", "two"]]);
    await receive({ type: "reply", id: multiRPC.id, ok: false, error: "Selection failed" });
    await expect(multi).rejects.toThrow("Selection failed");
  });
});


describe("Zotero attachment, successful open and tab context", () => {
  const items = [{ key: "parent", attachments: [
    { key: "pdf", contentType: "application/pdf", title: "Paper" },
    { key: "text", contentType: "text/plain", title: "Supplement" },
  ] }];
  const readers = [
    { viewID: "view-a", attachmentKey: "pdf", parentItemKey: "parent" },
    { viewID: "view-b", attachmentKey: "pdf", parentItemKey: "parent" },
  ];

  it("resolves numeric attachment IDs to deduplicated parents and sends consumer-shaped open events once", async () => {
    const { context, receive, messages } = zoteroRuntime();
    await receive({ type: "start", seq: 1, items, manifest: { id: "open", version: "1" }, code: { files: [
      { path: "bootstrap.js", text: `function startup() {
        window.opens = [];
        Zotero.Notifier.registerObserver({ notify(event, type, ids, extra) {
          if (event !== "open" || type !== "file" || extra.skipAutoSync) return;
          const attachments = Zotero.Items.get(ids);
          const parents = Zotero.Items.getTopLevel(attachments);
          opens.push([attachments.map(item => item.key), parents.map(item => item.key)]);
          for (const parent of parents) parent.setField("extra", "Read_Status: Reading");
        } }, ["file"]);
      }` },
    ] } });
    expect(runInNewContext(`(() => {
      const parent = Zotero.Items.getByLibraryAndKey(1, "parent");
      const files = Zotero.Items.get(parent.getAttachments());
      return [files.map(item => [typeof item.id, item.isAttachment(), item.isRegularItem(), item.parentItemID, item.parentItemKey, item.isPDFAttachment()]),
        Zotero.Items.getTopLevel([files[0], parent, files[1]]).map(item => item.key)];
    })()`, context)).toEqual([[["number", true, false, 1, "parent", true], ["number", true, false, 1, "parent", false]], ["parent"]]);
    const event = { event: "open", objectType: "file", itemKeys: ["pdf", "text"] };
    await receive({ type: "notifier", seq: 2, events: [event] });
    await receive({ type: "notifier", seq: 2, events: [event] });
    await receive({ type: "notifier", seq: 1, events: [event] });
    await receive({ type: "notifier", seq: 3, events: [{ ...event, extraData: { skipAutoSync: true } }] });
    await receive({ type: "notifier", seq: 4, events: [event] });
    expect(runInNewContext("opens", context)).toEqual([[["pdf", "text"], ["parent"]], [["pdf", "text"], ["parent"]]]);
    expect(runInNewContext('Zotero.Items.get(1).getField("extra")', context)).toBe("Read_Status: Reading");
    expect(messages().filter(message => message.type === "error")).toEqual([]);
  });

  it("installs tab state before add, preserves reader identity, and closes using A&T cached numeric item IDs", async () => {
    const { context, receive, messages } = zoteroRuntime();
    await receive({ type: "start", seq: 1, items, manifest: { id: "tabs", version: "1" }, code: { files: [
      { path: "bootstrap.js", text: `function startup() {
        window.cached = new Map(); window.events = [];
        Zotero.Notifier.registerObserver({ notify(event, type, ids) {
          if (type !== "tab") return;
          if (event === "add") for (const tab of Zotero_Tabs._tabs) cached.set(tab.id, tab.data.itemID);
          events.push([event, ids, ids.map(id => Zotero.Items.get(cached.get(id)).key)]);
        } }, ["tab"]);
      }` },
    ] } });
    await receive({ type: "tabs", seq: 2, readers, selectedViewID: "view-b" });
    expect(runInNewContext("events", context)).toEqual([["add", ["view-a", "view-b"], ["pdf", "pdf"]]]);
    runInNewContext('window.firstReader = Zotero.Reader.getByTabID("view-a")', context);
    await receive({ type: "items", seq: 3, items: [{ ...items[0], title: "Updated" }] });
    await receive({ type: "tabs", seq: 4, readers: readers.slice(0, 1), selectedViewID: "view-a" });
    await receive({ type: "tabs", seq: 3, readers, selectedViewID: "view-b" });
    expect(runInNewContext('[Zotero.Reader.getByTabID("view-a") === firstReader, firstReader.itemID, firstReader._item.key, firstReader._instanceID, Zotero.Reader.getByTabID("view-b"), Zotero_Tabs.selectedID, Zotero_Tabs.selectedType]', context))
      .toEqual([true, 2, "pdf", "view-a", null, "view-a", "reader"]);
    expect(runInNewContext("events", context)).toEqual([["add", ["view-a", "view-b"], ["pdf", "pdf"]], ["close", ["view-b"], ["pdf"]]]);
    // Projection removal must retain IDs long enough for cached close-tab actions.
    await receive({ type: "items", seq: 5, items: [] });
    const selected = runInNewContext('Zotero_Tabs.select("zotero-pane")', context);
    const request = messages().at(-1);
    expect(request).toMatchObject({ method: "tabs.select", args: ["zotero-pane"] });
    await receive({ type: "tabs", seq: 6, readers: [], selectedViewID: "zotero-pane" });
    await receive({ type: "reply", id: request.id, ok: true, value: true });
    await expect(selected).resolves.toBe(true);
    expect(runInNewContext('[Zotero_Tabs.selectedID, Zotero_Tabs.selectedType, Zotero.Reader._readers.length]', context)).toEqual(["zotero-pane", "library", 0]);
    expect(messages().filter(message => message.type === "error")).toEqual([]);
  });

  it("makes the existing reader registry available during startup and seeds A&T's tab cache afterward", async () => {
    const { context, receive } = zoteroRuntime();
    await receive({ type: "start", seq: 1, items, tabs: { seq: 1, readers, selectedViewID: "view-b" }, manifest: { id: "tabs", version: "1" }, code: { files: [
      { path: "bootstrap.js", text: `function startup() {
        window.startupReader = Zotero.Reader.getByTabID(Zotero_Tabs.selectedID).itemID;
        window.closed = []; window.cached = new Map();
        Zotero.Notifier.registerObserver({ notify(event, type, ids) {
          if (event === "add") for (const tab of Zotero_Tabs._tabs) cached.set(tab.id, tab.data.itemID);
          if (event === "close") closed.push(...Zotero.Items.getTopLevel(Zotero.Items.get(ids.map(id => cached.get(id)))).map(item => item.key));
        } }, ["tab"]);
      }` },
    ] } });
    expect(runInNewContext("startupReader", context)).toBe(2);
    await receive({ type: "tabs", seq: 2, readers: [], selectedViewID: "zotero-pane" });
    expect(runInNewContext("closed", context)).toEqual(["parent"]);
  });
});


describe("Zotero in-frame table and dialogs", () => {
  function setup() {
    const window = new Window({ url: "http://plugin.localhost/zotero.html" });
    const messages: Record<string, unknown>[] = [];
    Object.defineProperty(window, "parent", { value: { postMessage: (message: Record<string, unknown>) => messages.push(message) } });
    window.eval(readFileSync(new URL("../../../core/plugin-runtime/zotero.js", import.meta.url), "utf8"));
    return { window, messages };
  }

  it("keeps reader DOM and form values live, forwards events, and drops released views", async () => {
    const { window, messages } = setup();
    const send = (data: object) => window.dispatchEvent(new window.MessageEvent("message", { data, source: window.parent }));
    try {
      window.eval(`
        var popupDocs = [], clicks = 0;
        Zotero.Reader.registerEventListener("renderTextSelectionPopup", ({doc, append}) => {
          popupDocs.push(doc);
          if (!doc.querySelector(".selection-popup .colors")) throw Error("missing selection shell");
          const area = doc.createElement("textarea"); area.value = "waiting";
          const button = doc.createElement("button"); button.textContent = "Translate";
          button.addEventListener("click", () => { clicks++; area.value = "clicked"; });
          append(area, button);
          setTimeout(() => { area.value = "async result"; }, 140);
        });
      `);
      send({ type: "reader-render", live: true, id: 50, event: "renderTextSelectionPopup" });
      await vi.waitFor(() => expect(messages.some((m) => m.id === 50 && String(m.html).includes("async result"))).toBe(true));
      expect(window.eval("popupDocs[0].body.style.visibility")).toBe("hidden");
      expect(window.eval("popupDocs[0].body.isConnected")).toBe(true);
      const snapshot = window.document.createElement("div");
      snapshot.innerHTML = String(messages.filter((m) => m.id === 50).at(-1)!.html);
      const token = snapshot.querySelector("button")!.getAttribute("data-reader-node");
      send({ type: "reader-event", id: 50, node: "unknown", event: "click" });
      expect(window.eval("clicks")).toBe(0);
      send({ type: "reader-event", id: 50, node: token, event: "click" });
      expect(window.eval("clicks")).toBe(1);
      await vi.waitFor(() => expect(String(messages.filter((m) => m.id === 50).at(-1)!.html)).toContain("clicked"));
      send({ type: "reader-release", id: 50 });
      const count = messages.length;
      send({ type: "reader-event", id: 50, node: token, event: "click" });
      window.eval("popupDocs[0].body.textContent = 'late result'");
      await new Promise((resolve) => setTimeout(resolve, 150));
      expect(messages).toHaveLength(count);
      expect(window.eval("clicks")).toBe(1);
      expect(messages.filter((m) => m.type === "error")).toEqual([]);
    } finally { await window.happyDOM.abort(); }
  });

  it("honors a section disabled by onItemChange before rendering reader-only content", async () => {
    const { window, messages } = setup();
    const send = (data: object) => window.dispatchEvent(new window.MessageEvent("message", { data, source: window.parent }));
    try {
      window.eval(`var mounts = 0; Zotero.ItemPaneManager.registerSection({paneID: "reader-only", header: {},
        onInit: () => { mounts++; }, onDestroy: () => { mounts--; },
        onItemChange: ({tabType, setEnabled}) => setEnabled(tabType === "reader"),
        onRender: () => { throw Error("disabled section rendered"); }
      });`);
      send({ type: "items", seq: 1, items: [{key: "PARENT", itemType: "book", title: "Book"}] });
      send({ type: "render-section", id: 60, paneID: "reader-only", itemKey: "PARENT" });
      await vi.waitFor(() => expect(messages.some((m) => m.type === "rendered" && m.id === 60)).toBe(true));
      expect(messages.filter((m) => m.type === "error")).toEqual([]);
      expect(window.eval("mounts")).toBe(0);
    } finally { await window.happyDOM.abort(); }
  });

  it("renders toolkit descriptors, selects with mouse/keyboard, activates once, sorts and refreshes", async () => {
    const { window } = setup();
    try {
      window.eval(`
        var rows = [{name: "Beta"}, {name: "Alpha"}, {name: "Gamma"}];
        var changes = [], activations = 0, sorts = [], block = false;
        var React = require("react"), Table = require("components/virtualized-table");
        var container = document.createElement("div"); document.body.append(container);
        require("react-dom").createRoot(container).render(React.createElement(require("react-intl").IntlProvider, {},
          React.createElement(Table, { columns: [{dataKey: "name", label: "Name", width: 200}], showHeader: true, multiSelect: true,
            getRowCount: () => rows.length, renderItem: Table.makeRowRenderer(i => rows[i]), ref: value => window.tree = value,
            onSelectionChange: selection => changes.push([...selection.selected]),
            onActivate: () => activations++, onKeyDown: () => !block,
            onColumnSort: (i, direction) => { sorts.push([i, direction]); rows.sort((a,b) => direction * a.name.localeCompare(b.name)); }
          })));
      `);
      const doc = window.document;
      const table = doc.querySelector("table")!;
      const row = (i: number) => doc.querySelectorAll("tbody tr")[i];
      expect(table.textContent).toContain("BetaAlphaGamma");
      row(0).dispatchEvent(new window.MouseEvent("click", { bubbles: true }));
      row(2).dispatchEvent(new window.MouseEvent("click", { ctrlKey: true, bubbles: true }));
      expect(window.eval("[...tree.selection.selected]")).toEqual([0, 2]);
      row(0).dispatchEvent(new window.MouseEvent("click", { shiftKey: true, bubbles: true }));
      expect(window.eval("[...tree.selection.selected].sort()")).toEqual([0, 1, 2]);
      expect(row(1).getAttribute("aria-selected")).toBe("true");
      row(1).dispatchEvent(new window.MouseEvent("dblclick"));
      table.dispatchEvent(new window.KeyboardEvent("keydown", { key: "Enter" }));
      expect(window.eval("activations")).toBe(2);
      window.eval("block = true");
      const blocked = new window.KeyboardEvent("keydown", { key: "Enter", cancelable: true });
      table.dispatchEvent(blocked);
      expect(blocked.defaultPrevented).toBe(true);
      expect(window.eval("activations")).toBe(2);
      window.eval("block = false; tree.selection.clearSelection()");
      table.dispatchEvent(new window.KeyboardEvent("keydown", { key: "ArrowDown" }));
      expect(window.eval("tree.selection.focused")).toBe(0);
      doc.querySelector("th button")!.dispatchEvent(new window.MouseEvent("click"));
      expect(row(0).textContent).toBe("Alpha");
      expect(doc.querySelector("th")!.getAttribute("aria-sort")).toBe("ascending");
      doc.querySelector("th button")!.dispatchEvent(new window.MouseEvent("click"));
      expect(window.eval("sorts")).toEqual([[0, 1], [0, -1]]);
      expect(row(0).textContent).toBe("Gamma");
      window.eval("tree.selection.select(2); rows = [{name: 'Saved'}]; tree.invalidate(); tree.forceUpdate()");
      expect(doc.querySelectorAll("tbody tr")).toHaveLength(1);
      expect(row(0).textContent).toBe("Saved");
      expect(window.eval("tree.selection.isSelected(2)")).toBe(false);
    } finally { await window.happyDOM.close(); }
  });

  it("emulates a scoped dialog without iframes, binds before unload and settles locks", async () => {
    const { window, messages } = setup();
    try {
      window.eval(`
        var pane = document.createElement("div"); pane.id = "pane"; document.body.append(pane);
        var data = { loadLock: Zotero.Promise.defer(), unloadLock: Zotero.Promise.defer() };
        var dialog = Services.ww.openWindow(window, "about:blank", "editor", "", data);
        var loaded = 0, unloaded = 0;
        dialog.addEventListener("DOMContentLoaded", () => { loaded++; data.loadLock.resolve(); });
        data.loadLock.promise.then(() => {
          var input = dialog.document.createElement("input"); input.value = "Edited"; dialog.document.body.append(input);
        });
        dialog.addEventListener("beforeunload", function() { data.name = this.document.querySelector("input").value; });
        dialog.addEventListener("unload", () => { unloaded++; data.unloadLock.resolve(); });
      `);
      await vi.waitFor(() => expect(window.eval("loaded")).toBe(1));
      expect(window.eval("dialog.opener === window && dialog.arguments[0] === data")).toBe(true);
      expect(window.eval("dialog.document.querySelector('input').value")).toBe("Edited");
      expect(window.document.querySelector("iframe")).toBeNull();
      expect(window.eval("dialog.document.defaultView === dialog && dialog.window === dialog")).toBe(true);
      expect(window.eval("dialog.document.getElementById('pane')")).toBeNull();
      window.eval(`
        dialog.document.title = "Editor";
        var fragment = dialog.document.createDocumentFragment();
        var label = dialog.document.createElementNS("http://www.w3.org/1999/xhtml", "label");
        label.id = "dialog-only"; label.append(dialog.document.createTextNode("Scoped")); fragment.append(label);
        dialog.document.body.append(fragment);
        var documentLoaded = 0, keys = [];
        dialog.document.addEventListener("test-event", function() { if (this === dialog.document) documentLoaded++; });
        dialog.document.dispatchEvent(new Event("test-event"));
        dialog.addEventListener("keydown", function(event) { if (this === dialog) keys.push(event.key); event.preventDefault(); });
        var key = new KeyboardEvent("keydown", { key: "a", bubbles: true, cancelable: true });
        dialog.document.querySelector("input").dispatchEvent(key);
      `);
      expect(window.eval("[documentLoaded, keys, key.defaultPrevented, dialog.document.getElementsByTagName('label').length, dialog.document.getElementById('dialog-only').textContent]")).toEqual([1, ["a"], true, 1, "Scoped"]);
      expect(window.eval("dialog.document.title")).toBe("Editor");
      expect(window.eval("typeof dialog.setTimeout === 'function' && typeof dialog.clearTimeout === 'function' && typeof dialog.requestAnimationFrame === 'function' && !!dialog.getComputedStyle(dialog.document.body)")).toBe(true);
      window.eval("dialog.close(); dialog.close()");
      await window.eval("data.unloadLock.promise");
      expect(window.eval("[data.name, unloaded, dialog.closed]")).toEqual(["Edited", 1, true]);
      expect(window.document.querySelector(".plugin-dialog-layer")).toBeNull();
      expect(messages.at(-1)).toMatchObject({ type: "hide" });
      expect(() => window.eval('openDialog("chrome://missing", "", "")')).toThrow("not available");
    } finally { await window.happyDOM.close(); }
  });

  it("deletes only after explicit confirmation and never reuses an approval", async () => {
    const { window } = setup();
    try {
      window.eval(`var deleted = 0; var button = document.createXULElement("button"); document.body.append(button);
        button.addEventListener("command", () => { if (confirm("Remove rule?")) deleted++; });`);
      const button = window.document.querySelector("button")!;
      button.click();
      expect(window.eval("deleted")).toBe(0);
      window.document.querySelectorAll(".plugin-confirm-layer button")[0].dispatchEvent(new window.MouseEvent("click"));
      expect(window.eval("deleted")).toBe(0);
      button.click();
      window.document.querySelectorAll(".plugin-confirm-layer button")[1].dispatchEvent(new window.MouseEvent("click"));
      expect(window.eval("deleted")).toBe(1);
      button.click();
      expect(window.eval("deleted")).toBe(1);
      expect(window.document.querySelector(".plugin-confirm-layer")).not.toBeNull();
    } finally { await window.happyDOM.close(); }
  });
});


describe("Zotero annotation items", () => {
  const parent = { key: "paper", itemType: "journalArticle", attachments: [{ key: "pdf", title: "Nested descriptor" }] };
  const attachment = { key: "pdf", itemType: "attachment", parentItem: "paper", contentType: "application/pdf", title: "Canonical attachment" };
  const annotation = { key: "annotation-uuid", itemType: "annotation", parentItem: "pdf", annotationType: "highlight",
    annotationText: "  quotation\n", annotationComment: "A comment", annotationColor: "#abcdef",
    annotationPosition: '{"pageIndex":2,"rects":[[10,20,40,24]]}', annotationSortIndex: "00002|999760|00100",
    annotationPageLabel: "3", tags: [], dateAdded: "2024-02-29 00:00:00", dateModified: "2024-03-01 00:00:00" };

  it("installs canonical children once, exposes accessors and climbs the entire deduplicated parent chain", async () => {
    const { context, receive } = zoteroRuntime();
    await receive({ type: "items", items: [parent, attachment, annotation,
      { ...annotation, key: "sticky", annotationType: "note", annotationSortIndex: "00001|999760|00100" }] });
    expect(runInNewContext(`
      window.mark = Zotero.Items.getByLibraryAndKey(1, 'annotation-uuid');
      window.pdf = mark.parentItem;
      [mark.isAnnotation(), mark.isRegularItem(), mark.isAttachment(), mark.isNote(),
        pdf.isAttachment(), pdf.isAnnotation(), pdf.isRegularItem(),
        mark.parentItemKey, mark.parentID === pdf.id, mark.parentItemID === pdf.id,
        pdf.parentItem.key, pdf.getField('title'),
        pdf.getAnnotations().map(item => item.key),
        pdf.parentItem.getAttachments(),
        Zotero.Items.getTopLevel([mark, pdf, pdf.parentItem, mark]).map(item => item.key),
        Zotero.Items.getByLibraryAndKey(1, 'sticky').isNote(),
        mark.annotationType, mark.annotationText, mark.annotationComment, mark.annotationColor,
        mark.annotationPosition, mark.annotationSortIndex, mark.annotationPageLabel, mark.dateAdded, mark.dateModified];
    `, context)).toEqual([true, false, false, false, true, false, false, "pdf", true, true,
      "paper", "Canonical attachment", ["sticky", "annotation-uuid"], [2], ["paper"], false,
      annotation.annotationType, annotation.annotationText, annotation.annotationComment, annotation.annotationColor,
      annotation.annotationPosition, annotation.annotationSortIndex, "3", annotation.dateAdded, annotation.dateModified]);
    await expect(runInNewContext("Zotero.Annotations.toJSON(mark)", context)).resolves.toEqual({
      key: annotation.key, type: "highlight", text: annotation.annotationText, comment: annotation.annotationComment,
      color: "#abcdef", position: { pageIndex: 2, rects: [[10, 20, 40, 24]] }, sortIndex: annotation.annotationSortIndex,
      pageLabel: "3", tags: [], dateModified: annotation.dateModified,
    });
    await receive({ type: "items", items: [parent, attachment, { ...annotation, deleted: true }] });
    expect(runInNewContext("[mark === Zotero.Items.getByLibraryAndKey(1, mark.key), pdf.getAnnotations().length, pdf.getAnnotations(true).length]", context)).toEqual([true, 0, 1]);
    await receive({ type: "items", items: [{ key: "a", parentItem: "b" }, { key: "b", parentItem: "a" }, { key: "orphan", parentItem: "missing" }] });
    expect(runInNewContext("Zotero.Items.getTopLevel([Zotero.Items.getByLibraryAndKey(1, 'a'), Zotero.Items.getByLibraryAndKey(1, 'orphan')])", context)).toEqual([]);
  });

  it("rejects unsupported annotation writes and invalid projections without replacing the last-good items", async () => {
    const { context, receive, messages } = zoteroRuntime();
    await receive({ type: "items", items: [parent, attachment, annotation] });
    runInNewContext("window.mark = Zotero.Items.getByLibraryAndKey(1, 'annotation-uuid')", context);
    for (const [expression, field] of [
      ["mark.annotationText = 'new'", "annotationText"],
      ["mark.annotationPosition = '{}'", "annotationPosition"],
      ["mark.annotationType = 'note'", "annotationType"],
      ["mark.setField('title', 'new')", "title"],
      ["mark.deleted = true", "deleted"], ["mark.itemType = 'note'", "itemType"],
      ["mark.parentID = 1", "parentID"], ["mark.parentItemKey = 'other'", "parentItemKey"],
      ["mark.setCreators([])", "creators"], ["mark.eraseTx()", "deletion"],
    ]) {
      expect(() => runInNewContext(expression, context)).toThrow(`Annotation field ${field}`);
    }
    await expect(runInNewContext("mark.saveTx()", context)).resolves.toBe(false);
    expect(messages().filter(message => message.method === "items.update")).toHaveLength(0);
    for (const items of [[parent, parent], [parent, annotation], [parent, attachment, { ...annotation, annotationType: "square" }]]) {
      await receive({ type: "items", items });
      expect(runInNewContext("Zotero.Items.getByLibraryAndKey(1, mark.key) === mark", context)).toBe(true);
    }
  });

  it("saves annotation comment, colour and tags through items.update and retains edits across committed snapshots", async () => {
    const { context, receive, messages } = zoteroRuntime();
    await receive({ type: "items", items: [parent, attachment, annotation] });
    runInNewContext(`
      window.mark = Zotero.Items.getByLibraryAndKey(1, 'annotation-uuid');
      mark.annotationComment = '  translated\n';
      mark.annotationColor = '#AABBCC';
      mark.addTag('#auto');
      mark.addTag('#auto');
      window.saved = mark.saveTx();
    `.replace("translated\n", "translated\\n"), context);
    const update = messages().find(message => message.method === "items.update");
    expect(update.args).toEqual([annotation.key, JSON.stringify({ annotationComment: "  translated\n", annotationColor: "#AABBCC" }), ["#auto"]]);
    await receive({ type: "items", items: [parent, attachment, { ...annotation, annotationComment: "  translated\n", annotationColor: "#aabbcc", tags: [{ tag: "#auto", type: 0 }] }] });
    runInNewContext("mark.annotationComment = 'later'; mark.setTags([{tag: '#auto'}, {tag: 'second'}]); mark.removeTag('#auto')", context);
    await receive({ type: "reply", id: update.id, ok: true });
    await runInNewContext("saved", context);
    expect(runInNewContext("[mark.annotationComment, mark.getTags(), mark.hasChanged()]", context)).toEqual(["later", [{ tag: "second", type: 0 }], true]);
    const next = runInNewContext("mark.saveTx()", context) as Promise<boolean>;
    const request = messages().at(-1);
    expect(request.args).toEqual([annotation.key, '{"annotationComment":"later"}', ["second"]]);
    await receive({ type: "reply", id: request.id, ok: true });
    await next;
    await expect(runInNewContext("mark.saveTx()", context)).resolves.toBe(false);
  });

  it("delivers create once and only real colour modifications to an Actions & Tags shaped observer, retaining cascade lookup", async () => {
    const { context, receive } = zoteroRuntime();
    runInNewContext(`
      window.created = []; window.recoloured = []; window.deleted = [];
      Zotero.Notifier.registerObserver({ async notify(event, type, ids, extraData) {
        if (event === 'add') created.push(...Zotero.Items.get(ids).filter(item => item.isAnnotation()).map(item => item.key));
        if (event === 'modify') recoloured.push(...Zotero.Items.get(ids).filter(item => item.isAnnotation() && extraData[item.id]?.changed?.annotationColor).map(item => item.key));
        if (event === 'delete') { await Promise.resolve();
          for (const item of Zotero.Items.get(ids).filter(item => item.isAnnotation())) {
            deleted.push([item.key, Zotero.Items.getTopLevel([item]).map(parent => parent.key), extraData[item.id], Zotero.Items.getByLibraryAndKey(1, item.key) === item]);
          }
        }
      } }, ['item']);
    `, context);
    await receive({ type: "items", items: [parent, attachment] });
    await receive({ type: "items", items: [parent, attachment, annotation], events: [{ event: "add", objectType: "item", itemKeys: [annotation.key] }] });
    await receive({ type: "items", items: [parent, attachment, annotation] });
    const commented = { ...annotation, annotationComment: "Edited" };
    await receive({ type: "items", items: [parent, attachment, commented], events: [{ event: "modify", objectType: "item", itemKeys: [annotation.key], changed: { [annotation.key]: ["annotationComment"] } }] });
    expect(runInNewContext("[created, recoloured]", context)).toEqual([[annotation.key], []]);
    await receive({ type: "items", items: [parent, attachment, { ...commented, annotationColor: "#ffffff" }], events: [{ event: "modify", objectType: "item", itemKeys: [annotation.key], changed: { [annotation.key]: ["annotationColor"] } }] });
    expect(runInNewContext("recoloured", context)).toEqual([annotation.key]);
    await receive({ type: "items", items: [], events: [{ event: "delete", objectType: "item", itemKeys: [parent.key, attachment.key, annotation.key] }] });
    expect(runInNewContext("deleted", context)).toEqual([[annotation.key, ["paper"], { libraryID: 1, key: annotation.key }, true]]);
  });
});

/** Run real callbacks with a source-bound, synchronous Editor facade. */
async function editorRuntime(body: string) {
  const window = new Window({ url: "http://plugin.localhost/host.html" });
  const messages: Record<string, any>[] = [];
  let receive: (event: { source: unknown; data: object }) => Promise<void> = async () => {};
  const listen = window.addEventListener.bind(window);
  window.addEventListener = ((type: string, callback: any, ...rest: any[]) => {
    if (type === "message") receive = callback;
    else (listen as any)(type, callback, ...rest);
  }) as typeof window.addEventListener;
  const app = { postMessage: (message: Record<string, any>) => {
    messages.push(message);
    if (message.type === "call") queueMicrotask(() => void send({ type: "reply", id: message.id, ok: true, value: null }));
  } };
  Object.defineProperty(window, "parent", { value: app });
  const send = (data: object) => receive({ source: app, data });
  window.eval(readFileSync(new URL("../../../core/plugin-runtime/obsidian.js", import.meta.url), "utf8"));
  await send({ type: "start", manifest: { id: "test", name: "Test", version: "1" },
    code: { main: `const {Plugin} = require('obsidian'); module.exports = class extends Plugin { onload() { ${body} } };`, styles: null, data: null }, files: [], metadata: {}, platform: {} });
  return { window, messages, send };
}

const editorSnapshot = (value = "A😀e\u0301Z\nsecond") => ({ protocol: 1, session: "s1", generation: 1, revision: 4, contextRevision: 6,
  path: "one.md", value, ranges: [{ anchor: 5, head: 1 }, { anchor: 8, head: 10 }], mainIndex: 0,
  mode: "source", focused: true, composing: false });

describe("Obsidian revisioned editor", () => {
  it("round trips reversed multi-selections and UTF-16 emoji/combining offsets", async () => {
    const r = await editorRuntime(`this.addCommand({ id: 'read', editorCallback: (editor) => {
      window.reads = [editor.getSelection(), editor.getCursor(), editor.getCursor('anchor'), editor.getCursor('from'), editor.posToOffset({line: 0, ch: 5}), editor.offsetToPos(8)];
      editor.setSelections(editor.listSelections(), 0);
    }});`);
    try {
      await r.send({ type: "command/run", id: "test:read", editor: editorSnapshot() });
      expect((r.window as any).reads).toEqual(["😀e\u0301", { line: 0, ch: 1 }, { line: 0, ch: 5 }, { line: 0, ch: 1 }, 5, { line: 1, ch: 1 }]);
      expect(r.messages.filter(m => m.type === "editor/propose")).toMatchObject([{ edits: [], ranges: editorSnapshot().ranges, mainIndex: 0 }]);
    } finally { await r.window.happyDOM.close(); }
  });

  it("collects several edits, cursor and focus in one proposal after an async command", async () => {
    const r = await editorRuntime(`this.addCommand({ id: 'edit', editorCallback: async (editor) => {
      editor.replaceSelection('<mark>' + editor.getSelection() + '</mark>');
      await Promise.resolve();
      editor.replaceRange('!', {line: 1, ch: 0}); editor.setCursor(0, 16); editor.focus();
      window.result = editor.getValue();
    }});`);
    try {
      await r.send({ type: "command", id: "test:edit", editor: editorSnapshot() });
      const proposals = r.messages.filter(m => m.type === "editor/propose");
      expect(proposals).toHaveLength(1);
      expect(proposals[0]).toMatchObject({ session: "s1", baseRevision: 4, baseContextRevision: 6, focus: true,
        edits: [{ from: 1, to: 5, insert: "<mark>😀e\u0301</mark>" }, { from: 20, to: 20, insert: "!" }], ranges: [{ anchor: 16, head: 16 }] });
      expect((r.window as any).result).toBe("A<mark>😀e\u0301</mark>Z\n!second");
    } finally { await r.window.happyDOM.close(); }
  });

  it("keeps the source session across await and resnapshots a rejection without retry", async () => {
    const r = await editorRuntime(`window.calls = 0; this.addCommand({ id: 'delay', editorCallback: async (editor) => {
      window.calls++; await new Promise(resolve => window.resume = resolve); editor.replaceSelection('old');
      window.paths = [this.app.workspace.getActiveFile().path, this.app.workspace.activeEditor.file.path];
    }}); this.addCommand({id: 'read', editorCallback: editor => window.value = editor.getValue()});`);
    try {
      const running = r.send({ type: "command/run", id: "test:delay", editor: editorSnapshot() });
      await r.send({ type: "editor/open", editor: { ...editorSnapshot("new note"), session: "s2", path: "two.md" } });
      (r.window as any).resume(); await running;
      expect(r.messages.find(m => m.type === "editor/propose")).toMatchObject({ session: "s1", baseRevision: 4 });
      expect((r.window as any).paths).toEqual(["one.md", "one.md"]);
      await r.send({ type: "editor/reject", operation: "editor-1", editor: { ...editorSnapshot("new note"), session: "s2" } });
      expect((r.window as any).calls).toBe(1);
    } finally { await r.window.happyDOM.close(); }
  });

  it("refuses mutations in check-only callbacks and reports disabled", async () => {
    const r = await editorRuntime(`this.addCommand({ id: 'check', editorCheckCallback: (checking, editor) => { editor.replaceSelection('bad'); return true; }});`);
    try {
      await r.send({ type: "command/check", id: "test:check", request: "check-1", editor: editorSnapshot() });
      expect(r.messages.filter(m => m.type === "editor/propose")).toEqual([]);
      expect(r.messages).toContainEqual(expect.objectContaining({ type: "command/check", request: "check-1", enabled: false }));
      expect(r.messages.find(m => m.type === "error")?.message).toContain("command/check");
    } finally { await r.window.happyDOM.close(); }
  });

  it("refuses check-only edits through a previously cached workspace editor", async () => {
    const r = await editorRuntime(`this.registerEvent(this.app.workspace.on('file-open', () => window.cached = this.app.workspace.activeEditor.editor));
      this.addCommand({ id: 'check', editorCheckCallback: () => { window.cached.setCursor(0, 2); return true; }});`);
    try {
      await r.send({ type: "active", editor: editorSnapshot() });
      await r.send({ type: "command/check", id: "test:check", editor: editorSnapshot() });
      expect(r.messages.filter(m => m.type === "editor/propose")).toEqual([]);
      expect(r.messages).toContainEqual(expect.objectContaining({ type: "command/check", enabled: false }));
    } finally { await r.window.happyDOM.close(); }
  });

  it("fires editor-change only on the host's accepted change event", async () => {
    const r = await editorRuntime(`window.changes = []; this.registerEvent(this.app.workspace.on('editor-change', (editor, view) => window.changes.push([editor.getValue(), view.file.path])));
      this.addCommand({id: 'edit', editorCallback: editor => editor.replaceSelection('ok')});`);
    try {
      await r.send({ type: "command", id: "test:edit", editor: editorSnapshot() });
      expect((r.window as any).changes).toEqual([]);
      const accepted = { ...editorSnapshot("accepted"), revision: 5 };
      await r.send({ type: "editor/update", editor: accepted });
      await r.send({ type: "editor/ack", operation: "editor-1", editor: accepted });
      expect((r.window as any).changes).toEqual([]);
      await r.send({ type: "event", name: "editor-change", editor: accepted });
      expect((r.window as any).changes).toEqual([["accepted", "one.md"]]);
    } finally { await r.window.happyDOM.close(); }
  });
});


describe("Obsidian editor suggestions and menus", () => {
  const snapshot = (value = "@tom", revision = 1) => ({ ...editorSnapshot(value), revision, contextRevision: revision,
    ranges: [{ anchor: value.length, head: value.length }], mainIndex: 0 });
  const body = `
    const {EditorSuggest, Scope} = require('obsidian');
    window.scope = new Scope(); window.keys = [];
    const removed = window.scope.register([], 'x', () => window.keys.push('removed'));
    window.scope.unregister(removed);
    window.scope.register(['Shift'], 'Enter', e => window.keys.push(e.shiftKey));
    class Suggest extends EditorSuggest {
      onTrigger(cursor, editor) { return {start: {line: 0, ch: 0}, end: cursor, query: editor.getValue()}; }
      getSuggestions(context) { return context.query === 'slow' ? new Promise(resolve => window.resolveSlow = resolve) : [{label: context.query}]; }
      renderSuggestion(value, el) { el.createSpan({text: value.label}); }
      async selectSuggestion(value, event) { window.chosen = [value.label, event.shiftKey, event.ctrlKey, event.altKey, event.metaKey]; await Promise.resolve(); this.context.editor.replaceRange(event.shiftKey ? 'alias' : 'date', this.context.start, this.context.end); }
    }
    window.suggest = new Suggest(this.app);
    window.suggest.scope.register(['Shift'], 'Enter', event => window.suggest.suggestions.useSelectedItem(event));
    this.registerEditorSuggest(window.suggest);
    this.registerEvent(this.app.workspace.on('editor-menu', (menu, editor) => {
      menu.addItem(item => item.setTitle('Plugin action').onClick(async event => { await Promise.resolve(); editor.replaceSelection(event.shiftKey ? 'shift menu' : 'menu'); }));
      menu.addSeparator(); menu.addItem(item => item.setTitle('Disabled').setDisabled(true).onClick(() => { throw Error('disabled'); }));
      menu.addItem(item => { item.setTitle('Group'); item.setSubmenu().addItem(child => child.setTitle('Child').onClick(() => editor.replaceSelection('child'))); });
    }));`;

  it("keeps values in the frame, dispatches Scope modifiers, proposes selection edits, and invalidates close", async () => {
    const r = await editorRuntime(body);
    try {
      const window = r.window as any;
      window.scope.handleKey(new r.window.KeyboardEvent('keydown', {key: 'Enter', shiftKey: true}));
      window.scope.handleKey(new r.window.KeyboardEvent('keydown', {key: 'x'}));
      expect(window.keys).toEqual([true]);
      expect(window.scope.keys).toHaveLength(1);
      const editor = snapshot();
      await r.send({type: 'editor/open', editor});
      await r.send({type: 'suggest/query', request: 1, editor});
      const result = r.messages.find(m => m.type === 'suggest/result')!;
      expect(result).toMatchObject({request: 1, start: 0, end: 4, session: 's1', contextRevision: 1, items: [{id: '1:0', label: '@tom'}]});
      expect(result.items[0]).not.toHaveProperty('value');
      await r.send({...result, type: 'suggest/choose', id: '1:0', key: 'Enter', shiftKey: true, ctrlKey: true, altKey: true, metaKey: true});
      expect(window.chosen).toEqual(['@tom', true, true, true, true]);
      expect(r.messages.filter(m => m.type === 'editor/propose')).toMatchObject([{baseRevision: 1, edits: [{from: 0, to: 4, insert: 'alias'}]}]);
      expect(r.messages.some(m => m.type === 'suggest/close')).toBe(true);
      await r.send({...result, type: 'suggest/choose', id: '1:0', key: 'Enter'});
      expect(r.messages.filter(m => m.type === 'editor/propose')).toHaveLength(1);
      await r.send({type: 'suggest/query', request: 2, editor});
      await r.send({type: 'suggest/close', request: 2});
      await r.send({...editor, type: 'suggest/choose', request: 2, id: '2:0', key: 'Enter'});
      expect(r.messages.filter(m => m.type === 'editor/propose')).toHaveLength(1);
    } finally { await r.window.happyDOM.close(); }
  });

  it("drops async results and choices after typing, note switches, close, and composition", async () => {
    const r = await editorRuntime(body);
    try {
      const editor = snapshot('slow');
      await r.send({type: 'editor/open', editor});
      const pending = r.send({type: 'suggest/query', request: 1, editor});
      const next = snapshot('fresh', 2);
      await r.send({type: 'editor/update', editor: next});
      await r.send({type: 'suggest/query', request: 2, editor: next});
      (r.window as any).resolveSlow([{label: 'stale'}]); await pending;
      expect(r.messages.filter(m => m.type === 'suggest/result').map(m => m.request)).toEqual([2]);
      const result = r.messages.find(m => m.type === 'suggest/result')!;
      await r.send({type: 'editor/update', editor: {...next, contextRevision: 3}});
      await r.send({...result, type: 'suggest/choose', id: '2:0', key: 'Enter'});
      expect(r.messages.some(m => m.type === 'editor/propose')).toBe(false);
      await r.send({type: 'editor/open', editor: {...next, composing: true}});
      await r.send({type: 'suggest/query', request: 3, editor: {...next, composing: true}});
      expect(r.messages.filter(m => m.type === 'suggest/result')).toHaveLength(1);
      await r.send({type: 'editor/close', session: 's1', generation: 1});
      expect((r.window as any).suggest.context).toBeNull();
    } finally { await r.window.happyDOM.close(); }
  });

  it("runs configured navigation/escape Scope handlers without choosing, and ignores stale close", async () => {
    const r = await editorRuntime(body + `
      window.suggest.getSuggestions = () => [{label: 'first'}, {label: 'second'}];
      const scope = window.suggest.scope;
      scope.keys.filter(k => k.key === 'ArrowDown').forEach(k => scope.unregister(k));
      scope.register([], 'ArrowDown', () => { window.navigated = true; window.suggest.suggestions.setSelectedItem(1); return false; });
      scope.register(['Ctrl'], 'n', () => { window.customKey = true; window.suggest.suggestions.setSelectedItem(1); return false; });
      scope.keys.find(k => k.key === 'Escape').func = () => { window.escaped = true; window.suggest.close(); return false; };`);
    try {
      const editor = snapshot(); await r.send({type: 'editor/open', editor});
      await r.send({type: 'suggest/query', request: 1, editor});
      const result = r.messages.find(m => m.type === 'suggest/result')!;
      await r.send({type: 'suggest/close', request: 0});
      await r.send({...result, type: 'suggest/choose', id: '1:0', key: 'ArrowDown'});
      expect((r.window as any).navigated).toBe(true);
      expect(r.messages.filter(m => m.type === 'suggest/result').at(-1)).toMatchObject({selected: 1});
      await r.send({...result, type: 'suggest/choose', id: '1:0', key: 'n', ctrlKey: true});
      expect((r.window as any).customKey).toBe(true);
      expect(r.messages.filter(m => m.type === 'editor/propose')).toHaveLength(0);
      await r.send({...result, type: 'suggest/choose', id: '1:1', key: 'Escape'});
      expect((r.window as any).escaped).toBe(true);
      expect((r.window as any).suggest.context).toBeNull();
      expect(r.messages.filter(m => m.type === 'editor/propose')).toHaveLength(0);
    } finally { await r.window.happyDOM.close(); }
  });

  it("pins async selection callbacks to their source while new queries wait, and prevents duplicate choices", async () => {
    const r = await editorRuntime(body + `window.suggest.selectSuggestion = async function() {
      await new Promise(resolve => window.finishSelection = resolve);
      this.context.editor.replaceSelection('source');
    };`);
    try {
      const editor = snapshot();
      await r.send({type: 'editor/open', editor});
      await r.send({type: 'suggest/query', request: 1, editor});
      const result = r.messages.find(m => m.type === 'suggest/result')!;
      const choosing = r.send({...result, type: 'suggest/choose', id: '1:0', key: 'Enter'});
      await r.send({...result, type: 'suggest/choose', id: '1:0', key: 'Enter'});
      const next = {...snapshot('new', 2), session: 's2'};
      await r.send({type: 'editor/open', editor: next});
      await r.send({type: 'suggest/query', request: 2, editor: next});
      (r.window as any).finishSelection(); await choosing;
      expect(r.messages.filter(m => m.type === 'editor/propose')).toMatchObject([{session: 's1', baseRevision: 1, edits: [{from: 4, to: 4, insert: 'source'}]}]);
      expect(r.messages.filter(m => m.type === 'suggest/result').at(-1)).toMatchObject({request: 2, session: 's2', items: [{label: 'new'}]});
      expect(r.messages.filter(m => m.type === 'error')).toEqual([]);
    } finally { await r.window.happyDOM.close(); }
  });

  it("runs the highest priority trigger, breaking ties by registration order", async () => {
    const r = await editorRuntime(`
      const {EditorSuggest} = require('obsidian'); window.queries = [];
      for (const [label, priority] of [['low', 0], ['first', 10], ['second', 10]]) {
        const s = new EditorSuggest(this.app); s.priority = priority;
        s.onTrigger = cursor => ({start: {line: 0, ch: 0}, end: cursor, query: ''});
        s.getSuggestions = () => { window.queries.push(label); return [label]; };
        this.registerEditorSuggest(s);
      }`);
    try {
      const editor = snapshot(); await r.send({type: 'editor/open', editor});
      await r.send({type: 'suggest/query', request: 1, editor});
      expect((r.window as any).queries).toEqual(['first']);
      expect(r.messages.find(m => m.type === 'suggest/result')).toMatchObject({priority: 10, items: [{label: 'first'}]});
    } finally { await r.window.happyDOM.close(); }
  });

  it("exports bounded menu data and runs a callback once through the source proposal", async () => {
    const r = await editorRuntime(body);
    try {
      const editor = snapshot();
      await r.send({type: 'editor/open', editor});
      await r.send({type: 'menu/request', request: 10, editor});
      const menu = r.messages.find(m => m.type === 'menu/result')!;
      expect(menu.items).toEqual([{index: 0, label: 'Plugin action', disabled: false}, {separator: true},
        {index: 1, label: 'Disabled', disabled: true}, {label: 'Group', disabled: false, children: [{index: 2, label: 'Child', disabled: false}]}]);
      await r.send({...menu, type: 'menu/choose', index: 0, shiftKey: true});
      await r.send({...menu, type: 'menu/choose', index: 0});
      expect(r.messages.filter(m => m.type === 'editor/propose')).toMatchObject([{edits: [{from: 4, to: 4, insert: 'shift menu'}]}]);
      await r.send({type: 'menu/request', request: 11, editor});
      await r.send({type: 'editor/update', editor: snapshot('new', 2)});
      await r.send({...editor, type: 'menu/choose', request: 11, index: 0});
      expect(r.messages.filter(m => m.type === 'editor/propose')).toHaveLength(1);
      expect(r.messages.filter(m => m.type === 'error')).toEqual([]);
    } finally { await r.window.happyDOM.close(); }
  });
});
