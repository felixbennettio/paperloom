import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import { afterEach, describe, expect, it, vi } from "vitest";

// These DOM fixtures follow the existing runtime tests and require no browser.
function runtime(codes: Record<string, string>, chosen: string[] = ["b"]) {
  type Message = Record<string, unknown>;
  type Event = { source: unknown; data: Message };
  let listener: (event: Event) => Promise<void> = async () => {};
  const messages: Message[] = [];
  const requests: Message[] = [];
  let api: Record<string, unknown> | undefined;
  const app = { postMessage: (message: Message) => {
    messages.push(message);
    if (message.type !== "call") return;
    const args = message.args as unknown[];
    let value: unknown;
    let error: string | undefined;
    if (message.method === "translatorCode") { value = codes[String(args[0])]; if (!value) error = "Missing translator"; }
    else if (message.method === "selectItems") value = chosen;
    else if (message.method === "translators") value = Object.keys(codes).map((id) => ({ id }));
    else if (message.method === "request") {
      const request = args[0] as Message;
      requests.push(request);
      value = { status: request.url === "https://example.org/missing" ? 404 : 200, text: request.url === "https://example.org/json" ? '{"title":"Network"}' : "TY  - BOOK\nTI  - Imported\nER  - \n", url: "https://example.org/redirected" };
    }
    queueMicrotask(() => { void listener({ source: app, data: { type: "reply", id: message.id, value, error } }); });
  } };
  class Parser {
    parseFromString(html: string) {
      const base = { getAttribute: () => null, setAttribute: vi.fn() };
      const doc = {
        title: html.match(/<title>(.*?)<\/title>/)?.[1] ?? "",
        documentElement: { textContent: html.replace(/<[^>]*>/g, "") },
        head: { prepend: vi.fn() }, createElement: () => base,
        querySelector: (selector: string) => selector === "base" ? null : { textContent: "Heading" },
        querySelectorAll: () => [{ textContent: "Heading", getAttribute: () => "attribute" }],
      };
      Object.defineProperty(doc, "location", { configurable: false, get: () => null });
      return doc;
    }
  }
  const window = { parent: app, addEventListener: (_: string, fn: typeof listener) => { listener = fn; } };
  const source = readFileSync(new URL("../../../core/plugin-runtime/translate.js", import.meta.url), "utf8");
  runInNewContext(source, { window, DOMParser: Parser, URL, console: { debug: vi.fn() }, setTimeout, clearTimeout, capture: (value: typeof api) => { api = value; } });
  const receive = (data: Message, from: unknown = app) => listener({ source: from, data });
  const detect = (translators: { id: string; priority?: number; target?: string }[]) => receive({ type: "translate", id: 1, url: "https://example.org/page", html: "<title>Fixture</title>", translators: translators.map((t) => ({ label: t.id, priority: 100, target: "", ...t })) });
  return { receive, detect, messages, requests, getAPI: () => api };
}

function code(id: string, body: string, kind = 4) {
  return JSON.stringify({ translatorID: id, label: id, target: "", priority: 100, translatorType: kind }) + "\n" + body;
}
const basic = 'function detectWeb(doc, url) { return doc.URL === url && doc.documentURI === url && doc.location.href === url ? "book" : false; }';
afterEach(() => vi.useRealTimers());

describe("translation runtime", () => {
  it("matches targets in priority order, skips invalid regexes and code, and ignores other senders", async () => {
    const tr = runtime({
      high: code("high", basic),
      no: code("no", "function detectWeb() { return false; }"),
      low: code("low", basic),
      broken: "{invalid}",
    });
    await tr.receive({ type: "translate", id: 8 }, {});
    await tr.detect([{ id: "bad-regex", target: "[", priority: 0 }, { id: "wrong-host", target: "other.org", priority: 0 }, { id: "no", priority: 1 }, { id: "broken", priority: 2 }, { id: "high", priority: 10 }, { id: "low", priority: 50 }]);
    expect(tr.messages.at(-1)).toEqual({ type: "detected", id: 1, translator: "high", label: "high", itemType: "book" });
    expect(tr.messages.filter((m) => m.method === "translatorCode").map((m) => (m.args as string[])[0])).toEqual(["no", "broken", "high"]);
  });

  it("allows translators to declare their own utility aliases", async () => {
    const tr = runtime({ local: code("local", 'const ZU = Zotero.Utilities; const text = ZU.text; ' + basic + 'function doWeb(doc) { const item = new Zotero.Item("book"); item.title = text(doc, "h1"); item.complete(); }') });
    await tr.detect([{ id: "local" }]);
    await tr.receive({ type: "save", id: 2 });
    expect((tr.messages.at(-1)?.items as { title: string }[])[0].title).toBe("Heading");
  });

  it("preserves detection globals for saving and supports saving again", async () => {
    const tr = runtime({ state: code("state", `
      let detectedTitle;
      function detectWeb(doc) { detectedTitle = doc.title; return "book"; }
      function doWeb() { const item = new Zotero.Item("book"); item.title = detectedTitle; item.complete(); }
    `) });
    await tr.detect([{ id: "state" }]);
    for (const id of [2, 3]) {
      await tr.receive({ type: "save", id });
      expect((tr.messages.at(-1)?.items as { title: string }[])[0].title).toBe("Fixture");
    }
  });

  it("answers null when all candidates decline", async () => {
    const tr = runtime({ no: code("no", "function detectWeb() { return false; }") });
    await tr.detect([{ id: "no" }]);
    expect(tr.messages.at(-1)).toEqual({ type: "detected", id: 1, translator: null });
  });

  it("waits for callback selection and Zotero.done and returns only completed items", async () => {
    const tr = runtime({ select: code("select", basic + `
      function doWeb(doc, url) {
        Zotero.wait();
        Zotero.selectItems({ a: "First", b: "Second" }, function (selected) {
          setTimeout(function () {
            if (selected) for (const key of Object.keys(selected)) {
              const item = new Z.Item("book"); item.title = selected[key];
              item.attachments.push({ document: doc, title: "Snapshot" });
              item.complete(); item.complete();
            }
            new Zotero.Item("book").title = "Incomplete";
            Zotero.done();
          }, 5);
        });
      }
    `) }, ["b", "unlisted"]);
    await tr.detect([{ id: "select" }]);
    await tr.receive({ type: "save", id: 2 });
    expect(tr.messages.at(-1)).toEqual({ type: "translated", id: 2, items: [{ itemType: "book", title: "Second", creators: [], tags: [], attachments: [{ url: "https://example.org/page", title: "Snapshot" }] }] });
  });

  it("supports promise selection and cancellation", async () => {
    const tr = runtime({ select: code("select", basic + `
      async function doWeb() {
        const chosen = await Zotero.selectItems({ a: "First" });
        if (!chosen) return;
        const item = new Zotero.Item("book"); item.title = "unexpected"; item.complete();
      }
    `) }, []);
    await tr.detect([{ id: "select" }]);
    await tr.receive({ type: "save", id: 2 });
    expect(tr.messages.at(-1)).toEqual({ type: "translated", id: 2, items: [] });
  });

  it("runs nested web and import translators and forwards itemDone edits", async () => {
    const tr = runtime({
      parent: code("parent", basic + `
        async function doWeb(doc) {
          const web = Zotero.loadTranslator("web"); web.setTranslator("metadata"); web.setDocument(doc);
          web.setHandler("itemDone", function (_, item) { item.title += " edited"; item.complete(); });
          await web.translate();
          const importer = Zotero.loadTranslator("import"); importer.setTranslator("import"); importer.setString("First\\r\\nSecond");
          const detected = await importer.getTranslators();
          if (detected.length !== 1) throw new Error("No importer");
          await importer.translate();
        }
      `),
      metadata: code("metadata", basic + 'function doWeb(doc) { const item = new Zotero.Item("webpage"); item.title = doc.title; item.complete(); }'),
      import: code("import", 'function detectImport() { return Zotero.read() === "First"; } function doImport() { let line; while ((line = Zotero.read()) !== false) { const item = new Zotero.Item("book"); item.title = line; item.complete(); } }', 3),
    });
    await tr.detect([{ id: "parent" }]);
    await tr.receive({ type: "save", id: 2 });
    expect((tr.messages.at(-1)?.items as { title: string }[]).map((i) => i.title)).toEqual(["Fixture edited", "First", "Second"]);
  });

  it("filters nested web discovery by target and priority", async () => {
    const tr = runtime({
      parent: code("parent", basic + `
        async function doWeb(doc) {
          const nested = Zotero.loadTranslator("web"); nested.setDocument(doc);
          const choices = await nested.getTranslators();
          if (choices[0].translatorID !== "matched") throw new Error("Wrong nested translator");
          nested.setTranslator(choices); await nested.translate();
        }
      `),
      wrong: code("wrong", basic + 'function doWeb() { throw new Error("Wrong target"); }'),
      matched: code("matched", basic + 'function doWeb() { const item = new Zotero.Item("book"); item.title = "Matched"; item.complete(); }').replace('"priority":100', '"priority":1'),
    });
    await tr.detect([{ id: "parent", priority: 0 }, { id: "wrong", target: "other.org", priority: 1 }, { id: "matched", priority: 5 }]);
    // The parent is itself detected, so discovery still needs a usable priority.
    await tr.receive({ type: "save", id: 2 });
    expect((tr.messages.at(-1)?.items as { title: string }[])[0].title).toBe("Matched");
  });

  it("discovers import translators and supports byte reading and exported translator objects", async () => {
    const tr = runtime({
      parent: code("parent", basic + `
        async function doWeb(doc) {
          const helper = Zotero.loadTranslator("web"); helper.setTranslator("helper"); helper.setDocument(doc);
          const object = await helper.getTranslatorObject(); object.prefix = "Modified";
          object.doWeb(doc);
          const importer = Zotero.loadTranslator("import"); importer.setString("ABCD");
          const candidates = await importer.getTranslators(); importer.setTranslator(candidates);
          await importer.translate();
        }
      `),
      helper: code("helper", basic + 'var prefix = "Original"; function doWeb() { setTimeout(() => { const item = new Zotero.Item("book"); item.title = prefix; item.complete(); }, 5); }'),
      import: code("import", 'function detectImport() { return Zotero.read(2) === "AB"; } function doImport() { const item = new Zotero.Item("book"); item.title = Zotero.read(4); item.complete(); }', 1),
    });
    await tr.detect([{ id: "parent" }]);
    await tr.receive({ type: "save", id: 2 });
    expect((tr.messages.at(-1)?.items as { title: string }[]).map((i) => i.title).sort()).toEqual(["ABCD", "Modified"]);
  });

  it("routes async requests and legacy callbacks through the host, preserving redirected document URLs", async () => {
    const tr = runtime({ network: code("network", basic + `
      async function doWeb() {
        const json = await requestJSON("/json");
        ZU.processDocuments("/document", function (doc) {
          const item = new Zotero.Item("book"); item.title = json.title; item.url = doc.URL; item.complete();
        });
        ZU.doPost("/post", "x=1", function (text, response) {
          if (response.status !== 200) throw new Error("Status missing");
          const item = new Zotero.Item("book"); item.title = "Posted"; item.complete();
        }, { "Content-Type": "application/x-www-form-urlencoded" });
      }
    `) });
    await tr.detect([{ id: "network" }]);
    await tr.receive({ type: "save", id: 2 });
    const items = tr.messages.at(-1)?.items as { title: string; url?: string }[];
    expect(items.find((i) => i.title === "Network")?.url).toBe("https://example.org/redirected");
    expect(tr.requests).toEqual(expect.arrayContaining([{ url: "https://example.org/post", method: "POST", headers: { "Content-Type": "application/x-www-form-urlencoded" }, body: "x=1" }]));
  });

  it("allows a translator to recover from a rejected promise request", async () => {
    const tr = runtime({ recover: code("recover", basic + `
      async function doWeb() {
        try { await requestText("/missing"); } catch (error) {
          const item = new Zotero.Item("book"); item.title = "Fallback"; item.complete();
        }
      }
    `) });
    await tr.detect([{ id: "recover" }]);
    await tr.receive({ type: "save", id: 2 });
    expect((tr.messages.at(-1)?.items as { title: string }[])[0].title).toBe("Fallback");
  });

  it("provides utility functions with whole Chinese names and checked identifiers", async () => {
    const tr = runtime({ utilities: code("utilities", basic + 'function doWeb() { capture(ZU); }') });
    await tr.detect([{ id: "utilities" }]);
    await tr.receive({ type: "save", id: 2 });
    const utilities = tr.getAPI() as Record<string, (...args: unknown[]) => unknown>;
    expect(utilities.cleanAuthor("张三", "author")).toEqual({ firstName: "", lastName: "张三", creatorType: "author", fieldMode: 1 });
    expect(utilities.cleanAuthor("Lovelace, Ada", "author", true)).toEqual({ firstName: "Ada", lastName: "Lovelace", creatorType: "author" });
    expect(utilities.cleanISBN("978-0-306-40615-7")).toBe("9780306406157");
    expect(utilities.cleanISBN("9780306406158")).toBe(false);
    expect(utilities.fieldIsValidForType("ISBN", "book")).toBe(true);
    expect(utilities.fieldIsValidForType("ISBN", "journalArticle")).toBe(false);
    expect(utilities.fieldIsValidForType("unknownField", "book")).toBe(false);
    expect(utilities.cleanISSN("2049-3630")).toBe("2049-3630");
    expect(utilities.cleanDOI("https://doi.org/10.1000/example.")).toBe("10.1000/example");
    expect(utilities.strToISO("2024-2-3")).toBe("2024-02-03");
    expect(utilities.strToISO("2024-2-31")).toBe(false);
    expect(utilities.trimInternal(" a  b\n c ")).toBe("a b c");
  });

  it("reports errors in callbacks and caps unfinished async work at sixty seconds", async () => {
    const tr = runtime({ fail: code("fail", basic + 'function doWeb() { Zotero.wait(); }') });
    vi.useFakeTimers();
    // The context captures the timer functions when it starts.
    const timed = runtime({ fail: code("fail", basic + 'function doWeb() { Zotero.wait(); }') });
    const detecting = timed.detect([{ id: "fail" }]);
    await vi.advanceTimersByTimeAsync(10);
    await detecting;
    const saving = timed.receive({ type: "save", id: 2 });
    await vi.advanceTimersByTimeAsync(60010);
    await saving;
    expect(timed.messages.at(-1)).toEqual({ type: "translate-error", id: 2, message: "Translation exceeded 60 seconds." });
    vi.useRealTimers();
    const failed = runtime({ fail: code("fail", basic + 'function doWeb() { setTimeout(() => { throw new Error("Callback failure"); }, 0); }') });
    await failed.detect([{ id: "fail" }]);
    await failed.receive({ type: "save", id: 3 });
    expect(failed.messages.at(-1)).toEqual({ type: "translate-error", id: 3, message: "Callback failure" });
    expect(tr.messages[0]).toEqual({ type: "loaded" });
  });
});
