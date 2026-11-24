// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { citationHtml } from "./memoryDocument";
import { OfficeDocument } from "./officeDocument";
import { CitationSession, DEFAULT_STYLE } from "./session";

interface Node {
  tag: string | null;
  title: string;
  html: string;
}

class FakeSettings implements Office.Settings {
  memory = new Map<string, unknown>();
  disk = new Map<string, unknown>();
  saves = 0;
  failNext = false;
  get(name: string): unknown { return this.memory.get(name); }
  set(name: string, value: unknown): void { this.memory.set(name, structuredClone(value)); }
  remove(name: string): void { this.memory.delete(name); }
  saveAsync(callback?: (result: Office.AsyncResult<void>) => void): void {
    queueMicrotask(() => {
      const failed = this.failNext;
      this.failNext = false;
      this.saves++;
      if (!failed) this.disk = structuredClone(this.memory);
      callback?.({ value: undefined, status: failed ? "failed" : "succeeded",
        error: { code: failed ? 1 : 0, name: "SaveError", message: failed ? "Disk error" : "" } });
    });
  }
  reopen(): void { this.memory = structuredClone(this.disk); }
}

class FakeWord {
  nodes: Node[] = [];
  selection = 0;
  supports13 = true;
  syncs = 0;
  deletes: boolean[] = [];
  /** New paragraphs inserted after the cursor's. */
  paragraphs = 0;

  readonly run: typeof Word.run = async (batch) => {
    const queue: (() => void)[] = [];
    let loaded: Word.ContentControl[] | undefined;
    const control = (node: Node): Word.ContentControl => ({
      get tag() { return node.tag ?? ""; },
      set tag(value) { queue.push(() => { node.tag = value; }); },
      get title() { return node.title; },
      set title(value) { queue.push(() => { node.title = value; }); },
      insertHtml: (html, location) => {
        expect(location).toBe("Replace");
        queue.push(() => { node.html = html; });
        return range();
      },
      delete: (keepContent) => {
        queue.push(() => {
          this.deletes.push(keepContent);
          if (keepContent) node.tag = null;
          else {
            const index = this.nodes.indexOf(node);
            this.nodes.splice(index, 1);
            if (index < this.selection) this.selection--;
          }
        });
      },
      getRange: (location) => {
        expect(this.supports13).toBe(true);
        expect(location).toBe("After");
        return range(node);
      },
    });
    const range = (after?: Node): Word.Range => ({
      insertContentControl: () => {
        const node: Node = { tag: "", title: "", html: "selected text" };
        queue.push(() => { this.nodes.splice(this.selection, 0, node); });
        return control(node);
      },
      insertParagraph: (_text, location) => {
        expect(location).toBe("After");
        queue.push(() => { this.paragraphs++; });
        return { insertContentControl: () => range(after).insertContentControl() };
      },
      select: () => { queue.push(() => { if (after) this.selection = this.nodes.indexOf(after) + 1; }); },
    });
    const collection: Word.ContentControlCollection = {
      get items() {
        if (!loaded) throw new Error("Collection was read before sync");
        return loaded;
      },
      load: (properties) => {
        expect(properties).toBe("tag");
        queue.push(() => { loaded = this.nodes.filter(({ tag }) => tag !== null).map(control); });
        return collection;
      },
    };
    const context: Word.RequestContext = {
      // Markers are tested in markers.test.ts, with text.
      document: { body: { text: "", load() { return this; }, search: () => { throw new Error("No text here"); } },
        contentControls: collection, getSelection: () => range() },
      sync: async () => { this.syncs++; while (queue.length) queue.shift()!(); },
    };
    return batch(context);
  };
}

describe("Word document adapter", () => {
  let word: FakeWord;
  let settings: FakeSettings;
  let document: OfficeDocument;

  beforeEach(() => {
    word = new FakeWord();
    settings = new FakeSettings();
    vi.stubGlobal("Word", { run: word.run });
    vi.stubGlobal("Office", { context: { document: { settings }, requirements: {
      isSetSupported: (name: string, version: string) => name === "WordApi" && (version === "1.1" || word.supports13),
    } } });
    document = new OfficeDocument();
  });

  afterEach(() => { vi.unstubAllGlobals(); });

  it("inserts rich-text controls at the cursor, saves detached works, and follows document order", async () => {
    const works = [{ itemId: "first", locator: "12" }];
    const inserted = await document.insertCitation(works);
    works[0].itemId = "mutated";
    inserted.works[0].itemId = "also mutated";
    expect(word.nodes[0]).toMatchObject({ tag: `paperloom:cite:${inserted.id}`, title: "Paperloom citation", html: "" });
    expect(settings.disk.has("paperloom")).toBe(true);
    word.selection = 0;
    const second = await document.insertCitation([{ itemId: "second" }]);
    word.nodes.splice(1, 0, { tag: "other-addin", title: "Other", html: "Keep me" });
    const citations = await document.getCitations();
    expect(citations.map(({ id }) => id)).toEqual([second.id, inserted.id]);
    expect(citations[1].works).toEqual([{ itemId: "first", locator: "12" }]);
    citations[1].text = "mutated";
    expect((await document.getCitations())[1].text).toBe("");
    // Moving a control in Word changes numbering order, regardless of settings order.
    word.nodes.unshift(word.nodes.pop()!);
    expect((await document.getCitations()).map(({ id }) => id)).toEqual([inserted.id, second.id]);
  });

  it("replaces HTML inside a control and keeps the formatted snapshot after reopening", async () => {
    const { id } = await document.insertCitation([{ itemId: "one" }]);
    const html = "<sup>[1]</sup> <i>Title</i> <b>Bold</b> <sub>2</sub> &amp; text";
    await document.setCitationText(id, html);
    expect(word.nodes[0].html).toBe(citationHtml(html));
    expect(word.nodes).toHaveLength(1);
    expect(settings.saves).toBe(2);
    settings.reopen();
    const reopened = new OfficeDocument();
    expect(await reopened.getCitations()).toEqual([{ id, works: [{ itemId: "one" }], text: html }]);
    // Sanitised as the preview is; DOMPurify itself needs a real browser
    // (under happy-dom it only looks at the outermost element).
    const unsafe = '<b onclick="bad()">safe</b><script>bad()</script>';
    await reopened.setCitationText(id, unsafe);
    expect(word.nodes[0].html).toBe(citationHtml(unsafe));
  });

  it("edits works, removes the control and its text, and rejects a missing citation", async () => {
    const { id } = await document.insertCitation([{ itemId: "one" }]);
    const works = [{ itemId: "two", locator: "9" }];
    const edit = document.replaceCitationWorks(id, works);
    works[0].itemId = "mutated before queued write";
    await edit;
    expect((await document.getCitations())[0].works).toEqual([{ itemId: "two", locator: "9" }]);
    await document.removeCitation(id);
    expect(word.nodes).toEqual([]);
    expect(word.deletes).toEqual([false]);
    expect(await document.getCitations()).toEqual([]);
    await expect(document.setCitationText(id, "text")).rejects.toThrow("Citation no longer exists.");
    await expect(document.replaceCitationWorks(id, [])).rejects.toThrow("Citation no longer exists.");
    await expect(document.removeCitation(id)).rejects.toThrow("Citation no longer exists.");
  });

  it("distinguishes an absent list from an empty list and updates one list at its original position", async () => {
    expect(await document.getBibliography()).toBeNull();
    await document.insertCitation([{ itemId: "one" }]);
    word.selection = 0;
    await document.setBibliography([]);
    expect(await document.getBibliography()).toEqual([]);
    // A new list starts a paragraph of its own; citations stay in the text.
    expect(word.paragraphs).toBe(1);
    const list = word.nodes[0];
    expect(list.tag).toBe("paperloom:bibliography");
    const entries = ["[1] <i>One</i>", "[2] <b>Two</b>"];
    await document.setBibliography(entries);
    entries[0] = "mutated";
    expect(word.nodes[0]).toBe(list);
    expect(word.nodes.filter(({ tag }) => tag === "paperloom:bibliography")).toHaveLength(1);
    expect(list.html).toBe("<p>[1] <i>One</i></p><p>[2] <b>Two</b></p>");
    expect(word.paragraphs).toBe(1);
    const snapshot = await document.getBibliography();
    snapshot!.push("mutated");
    settings.reopen();
    expect(await new OfficeDocument().getBibliography()).toEqual(["[1] <i>One</i>", "[2] <b>Two</b>"]);
    word.nodes.shift();
    expect(await document.getBibliography()).toBeNull();
    await document.setBibliography(["New list"]);
    expect(await document.getBibliography()).toEqual(["New list"]);
  });

  it("prunes metadata for a control deleted in Word, including after reopening", async () => {
    const first = await document.insertCitation([{ itemId: "one" }]);
    const second = await document.insertCitation([{ itemId: "two" }]);
    word.nodes.splice(0, 1);
    const saves = settings.saves;
    expect((await document.getCitations()).map(({ id }) => id)).toEqual([second.id]);
    expect(settings.saves).toBe(saves + 1);
    expect(JSON.stringify(settings.disk.get("paperloom"))).not.toContain(first.id);
    settings.reopen();
    expect((await new OfficeDocument().getCitations()).map(({ id }) => id)).toEqual([second.id]);
  });

  it("uses the shared session to renumber, refresh the list and change a persisted style", async () => {
    expect(await document.getStyleId()).toBeNull();
    const session = new CitationSession(document, { markers: async () => [], format: async (style, clusters) => ({
      citations: clusters.map(({ works }, index) => style === DEFAULT_STYLE ? `[${index + 1}]` : works[0].itemId),
      bibliography: clusters.map(({ works }) => `<i>${works[0].itemId}</i>`), missing: [],
    }) });
    await session.insert([{ itemId: "one" }]);
    await session.insertBibliography();
    word.selection = 0;
    await session.insert([{ itemId: "two" }]);
    expect((await document.getCitations()).map(({ text }) => text)).toEqual(["[1]", "[2]"]);
    expect(await document.getBibliography()).toEqual(["<i>two</i>", "<i>one</i>"]);
    await session.setStyle("author-date");
    settings.reopen();
    const reopened = new OfficeDocument();
    expect(await reopened.getStyleId()).toBe("author-date");
    expect((await reopened.getCitations()).map(({ text }) => text)).toEqual(["two", "one"]);
  });

  it("unlinks only Paperloom fields, preserving their actual contents and other add-in settings", async () => {
    const { id } = await document.insertCitation([{ itemId: "one" }]);
    await document.setCitationText(id, "<sup>[1]</sup>");
    await document.setBibliography(["<i>Title</i>"]);
    await document.setStyleId("author-date");
    word.nodes[0].html = "<b>Manually edited in Word</b>";
    word.nodes.push({ tag: "unrelated", title: "Other", html: "Other text" });
    settings.set("other-addin", "keep");
    const before = word.nodes.map(({ html }) => html);
    await document.unlink();
    expect(word.nodes.map(({ html }) => html)).toEqual(before);
    expect(word.nodes.map(({ tag }) => tag)).toEqual([null, null, "unrelated"]);
    expect(word.deletes).toEqual([true, true]);
    expect(settings.disk.has("paperloom")).toBe(false);
    expect(settings.disk.get("other-addin")).toBe("keep");
    settings.reopen();
    const reopened = new OfficeDocument();
    expect(await reopened.getCitations()).toEqual([]);
    expect(await reopened.getBibliography()).toBeNull();
    expect(await reopened.getStyleId()).toBeNull();
  });

  it("awaits save errors and allows a later operation to retry", async () => {
    await document.setStyleId("first");
    settings.failNext = true;
    await expect(document.setStyleId("second")).rejects.toThrow("Paperloom document data could not be saved.");
    expect(await document.getStyleId()).toBe("first");
    await document.setStyleId("second");
    settings.reopen();
    expect(await new OfficeDocument().getStyleId()).toBe("second");
  });

  it("keeps unknown metadata intact and reports controls with missing data", async () => {
    settings.set("paperloom", { version: 99 });
    await expect(document.getCitations()).rejects.toThrow("Paperloom document data could not be read.");
    expect(settings.get("paperloom")).toEqual({ version: 99 });
    settings.remove("paperloom");
    word.nodes.push({ tag: "paperloom:cite:orphan", title: "Paperloom citation", html: "[1]" });
    await expect(document.getCitations()).rejects.toThrow("Paperloom citation data is missing.");
    expect(settings.saves).toBe(0);
    expect(word.nodes[0].html).toBe("[1]");
  });

  it("does not call the optional caret API on WordApi 1.1", async () => {
    word.supports13 = false;
    const inserted = await document.insertCitation([{ itemId: "one" }]);
    await document.setCitationText(inserted.id, "[1]");
    expect(await document.getCitations()).toEqual([{ ...inserted, text: "[1]" }]);
  });
});
