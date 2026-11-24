// @vitest-environment happy-dom
import { beforeEach, describe, expect, it } from "vitest";
import { citationHtml, plainText } from "./memoryDocument";
import { CitationSession, DEFAULT_STYLE } from "./session";
import { WpsDocument } from "./wpsDocument";

function font(): Wps.Font {
  return { Italic: false, Bold: false, Superscript: false, Subscript: false };
}

class FakeVariables implements Wps.Variables {
  values = new Map<string, string>();
  adds = 0;
  writes = 0;
  failWrite = false;
  failDelete = false;
  missingThrows = true;

  Item(name: string): Wps.Variable | undefined {
    if (!this.values.has(name)) {
      if (this.missingThrows) throw new Error("Variable not found");
      return undefined;
    }
    const owner = this;
    return {
      get Value() { return owner.values.get(name)!; },
      set Value(value) {
        if (owner.failWrite) { owner.failWrite = false; throw new Error("Write failed"); }
        owner.values.set(name, value);
        owner.writes++;
      },
      Delete() {
        if (owner.failDelete) throw new Error("Delete failed");
        owner.values.delete(name);
      },
    };
  }

  Add(name: string, value: string): Wps.Variable {
    if (this.values.has(name)) throw new Error("Variable already exists");
    if (this.failWrite) { this.failWrite = false; throw new Error("Write failed"); }
    this.values.set(name, value);
    this.adds++;
    this.writes++;
    return this.Item(name)!;
  }

  state(): { version: number; citations: { id: string }[]; bibliography: string[] | null; styleId: string | null } {
    return JSON.parse(this.values.get("paperloom")!);
  }
}

class FakeControl implements Wps.ContentControl {
  Tag = "";
  Title = "";
  text = "Selected text";
  readonly formatting = font();

  constructor(private readonly owner: FakeWps) {}

  /** Where its start mark is; the content follows, then the end mark. */
  get start(): number {
    return this.owner.nodes.slice(0, this.owner.nodes.indexOf(this)).reduce((sum, node) => sum + node.text.length + 2, 0);
  }

  /** The first position after the control. */
  get after(): number {
    return this.start + this.text.length + 2;
  }

  // As in WPS, the Range is the content alone: End is inside the control.
  get Range(): Wps.Range {
    const control = this;
    return {
      get Start() { return control.start + 1; },
      get End() { return control.start + 1 + control.text.length; },
      get Text() { return control.text; },
      set Text(value) {
        const oldAfter = control.after;
        const delta = value.length - control.text.length;
        control.text = value;
        if (control.owner.cursor >= oldAfter) control.owner.cursor += delta;
      },
      Font: control.formatting,
      SetRange() { throw new Error("Not a selection range"); },
    };
  }

  Delete(deleteContents: boolean): void {
    if (this.owner.failDelete) throw new Error("Control is locked");
    this.owner.deletes.push({ tag: this.Tag, deleteContents });
    if (deleteContents) this.owner.nodes.splice(this.owner.nodes.indexOf(this), 1);
    else this.Tag = "";
  }
}

class FakeWps implements Wps.Application {
  nodes: FakeControl[] = [];
  created: FakeControl[] = [];
  cursor = 0;
  failDelete = false;
  readonly variables = new FakeVariables();
  readonly deletes: { tag: string; deleteContents: boolean }[] = [];
  readonly ranges: { start: number; end: number; font: Wps.Font }[] = [];
  readonly moves: number[] = [];
  /** Controls added and paragraphs typed, in order. */
  readonly events: ("control" | "paragraph")[] = [];
  /** The text at a position; paragraph marks unless a test says otherwise. */
  charAt: (position: number) => string = () => "\r";

  private selectionRange(): Wps.Range {
    let start = this.cursor;
    let end = start;
    return { get Start() { return start; }, get End() { return end; }, Text: "", Font: font(),
      SetRange: (a, b) => { start = a; end = b; },
      Select: () => { expect(start).toBe(end); this.cursor = start; this.moves.push(start); },
    };
  }

  readonly Selection: Wps.Application["Selection"];
  readonly ActiveDocument: Wps.Document;

  constructor() {
    // Selection.Range is a duplicate: SetRange alone does not select it.
    const owner = this;
    this.Selection = {
      get Range() { return owner.selectionRange(); },
      SetRange: (start, end) => { expect(start).toBe(end); this.cursor = start; this.moves.push(start); },
      TypeParagraph: () => { this.events.push("paragraph"); this.cursor++; },
    };
    this.ActiveDocument = {
      // Markers are tested in markers.test.ts, with text.
      get Content(): Wps.Range { throw new Error("No text here"); },
      ContentControls: {
        get Count() { return owner.controls().length; },
        Item: (index) => {
          if (index < 1 || index > this.controls().length) throw new Error("Use 1-based indexing");
          return this.controls()[index - 1];
        },
        Add: (type, range) => {
          expect(type).toBe(0);
          const position = range.Start;
          for (const node of this.controls()) {
            if (position > node.start && position < node.after) throw new Error("Nested control");
          }
          const node = new FakeControl(this);
          this.events.push("control");
          const index = this.nodes.findIndex((entry) => entry.start >= position);
          this.nodes.splice(index === -1 ? this.nodes.length : index, 0, node);
          this.created.push(node);
          return node;
        },
      },
      Variables: this.variables,
      Range: (start, end) => {
        const formatting = font();
        this.ranges.push({ start, end, font: formatting });
        return { Start: start, End: end, Text: end - start === 1 ? this.charAt(start) : "", Font: formatting, SetRange() {} };
      },
    };
  }

  // Deliberately return creation order reversed, never document order.
  private controls(): FakeControl[] { return this.created.filter((node) => this.nodes.includes(node) && node.Tag).reverse(); }
}

describe("WPS document adapter", () => {
  let wps: FakeWps;
  let document: WpsDocument;

  beforeEach(() => { wps = new FakeWps(); document = new WpsDocument(wps); });

  it("serializes insertion, moves the cursor outside controls and returns detached snapshots in document order", async () => {
    const works = [{ itemId: "one", locator: "12" }];
    const pending = document.insertCitation(works);
    works[0].itemId = "mutated before the queued operation";
    const first = await pending;
    first.works[0].itemId = "mutated result";
    const [second, third] = await Promise.all([
      document.insertCitation([{ itemId: "two" }]), document.insertCitation([{ itemId: "three" }]),
    ]);
    expect(wps.nodes.map(({ Tag }) => Tag)).toEqual([first, second, third].map(({ id }) => `paperloom:cite:${id}`));
    expect(wps.nodes[0]).toMatchObject({ Title: "Paperloom citation", text: "" });
    expect(wps.moves).toHaveLength(3);
    wps.cursor = 0;
    const fourth = await document.insertCitation([{ itemId: "four" }]);
    expect((await document.getCitations()).map(({ id }) => id)).toEqual([fourth.id, first.id, second.id, third.id]);
    wps.nodes.unshift(wps.nodes.pop()!);
    const citations = await document.getCitations();
    expect(citations.map(({ id }) => id)).toEqual([third.id, fourth.id, first.id, second.id]);
    expect(citations[2].works).toEqual([{ itemId: "one", locator: "12" }]);
    citations[2].works[0].itemId = "mutated read";
    expect((await document.getCitations())[2].works[0].itemId).toBe("one");
    expect(wps.variables.adds).toBe(1);
    expect(wps.variables.state().version).toBe(1);
  });

  it("writes sanitized text and exact UTF-16 formatting ranges, clearing old formatting", async () => {
    const { id } = await document.insertCitation([{ itemId: "one" }]);
    const html = "<sup>[1]</sup> <i>😀<b>Title</b></i> <sub>2</sub> &amp; text";
    wps.ranges.length = 0;
    await document.setCitationText(id, html);
    const control = wps.nodes[0];
    expect(control.text).toBe(plainText(html));
    expect(wps.ranges).toEqual([
      { start: 1, end: 4, font: { ...font(), Superscript: true } },
      { start: 4, end: 5, font: font() },
      { start: 5, end: 7, font: { ...font(), Italic: true } },
      { start: 7, end: 12, font: { ...font(), Italic: true, Bold: true } },
      { start: 12, end: 13, font: font() },
      { start: 13, end: 14, font: { ...font(), Subscript: true } },
      { start: 14, end: 21, font: font() },
    ]);
    control.formatting.Superscript = -1;
    await document.setCitationText(id, "Plain");
    expect(control.formatting).toEqual(font());
    const unsafe = '<b onclick="bad()">safe</b><script>bad()</script>';
    await document.setCitationText(id, unsafe);
    const sanitized = window.document.createElement("div");
    sanitized.innerHTML = citationHtml(unsafe);
    expect(control.text).toBe(sanitized.textContent);
    const reopened = new WpsDocument(wps);
    expect(await reopened.getCitations()).toEqual([{ id, works: [{ itemId: "one" }], text: unsafe }]);
  });

  it("edits detached works and removes a citation and its text", async () => {
    const { id } = await document.insertCitation([{ itemId: "one" }]);
    const works = [{ itemId: "two", locator: "9" }];
    const pending = document.replaceCitationWorks(id, works);
    works[0].itemId = "changed";
    await pending;
    expect((await document.getCitations())[0].works).toEqual([{ itemId: "two", locator: "9" }]);
    await document.removeCitation(id);
    expect(wps.nodes).toEqual([]);
    expect(wps.deletes).toEqual([{ tag: `paperloom:cite:${id}`, deleteContents: true }]);
    expect(wps.variables.state().citations).toEqual([]);
    await expect(document.setCitationText(id, "text")).rejects.toThrow("Citation no longer exists.");
    await expect(document.replaceCitationWorks(id, [])).rejects.toThrow("Citation no longer exists.");
    await expect(document.removeCitation(id)).rejects.toThrow("Citation no longer exists.");
  });

  it("creates one reference list, distinguishes empty from absent and formats paragraphs in place", async () => {
    expect(await document.getBibliography()).toBeNull();
    await document.insertCitation([{ itemId: "one" }]);
    await document.setBibliography([]);
    const list = wps.nodes[1];
    expect(list).toMatchObject({ Tag: "paperloom:bibliography", Title: "Paperloom bibliography", text: "" });
    expect(await document.getBibliography()).toEqual([]);
    const start = list.Range.Start;
    const entries = ["[1] <i>One</i>", "[2] <b>Two</b>"];
    const pending = document.setBibliography(entries);
    entries[0] = "mutated";
    await pending;
    expect(wps.nodes[1]).toBe(list);
    expect(list.text).toBe("[1] One\v[2] Two");
    expect(wps.ranges).toContainEqual({ start: start + 4, end: start + 7, font: { ...font(), Italic: true } });
    expect(wps.ranges).toContainEqual({ start: start + 12, end: start + 15, font: { ...font(), Bold: true } });
    expect(wps.moves).toHaveLength(2);
    (await document.getBibliography())!.push("mutated read");
    expect(await new WpsDocument(wps).getBibliography()).toEqual(["[1] <i>One</i>", "[2] <b>Two</b>"]);
    wps.nodes.pop();
    expect(await document.getBibliography()).toBeNull();
    expect(wps.variables.state().bibliography).toBeNull();
    wps.cursor = 0;
    await document.setBibliography(["New list"]);
    expect(await document.getBibliography()).toEqual(["New list"]);
  });

  it("puts a new reference list in paragraphs of its own", async () => {
    await document.insertCitation([{ itemId: "one" }]);
    wps.events.length = 0;
    // Text on both sides of the cursor: the paragraph is split before and after.
    wps.charAt = () => "x";
    await document.setBibliography(["[1] One"]);
    expect(wps.events).toEqual(["paragraph", "control", "paragraph"]);
    wps.events.length = 0;
    await document.setBibliography(["[1] One", "[2] Two"]);
    expect(wps.events).toEqual([]);
  });

  it("starts a paragraph after a field's end mark, whose text reads as empty", async () => {
    await document.insertCitation([{ itemId: "one" }]);
    wps.events.length = 0;
    wps.charAt = (position) => position < wps.cursor ? "" : "\r";
    await document.setBibliography(["[1] One"]);
    expect(wps.events).toEqual(["paragraph", "control"]);
  });

  it("adds no paragraph where the cursor already stands between paragraphs", async () => {
    await document.setBibliography(["[1] One"]);
    expect(wps.events).toEqual(["control"]);
  });

  it("prunes metadata for controls removed by a person, including after reopening", async () => {
    const first = await document.insertCitation([{ itemId: "one" }]);
    const second = await document.insertCitation([{ itemId: "two" }]);
    wps.nodes.shift();
    const writes = wps.variables.writes;
    expect((await document.getCitations()).map(({ id }) => id)).toEqual([second.id]);
    expect(wps.variables.writes).toBe(writes + 1);
    expect(wps.variables.values.get("paperloom")).not.toContain(first.id);
    expect((await new WpsDocument(wps).getCitations()).map(({ id }) => id)).toEqual([second.id]);
  });

  it("uses the shared session to renumber in document order, refresh references and persist style", async () => {
    expect(await document.getStyleId()).toBeNull();
    const session = new CitationSession(document, { markers: async () => [], format: async (style, clusters) => ({
      citations: clusters.map(({ works }, index) => style === DEFAULT_STYLE ? `[${index + 1}]` : works[0].itemId),
      bibliography: clusters.map(({ works }) => `<i>${works[0].itemId}</i>`), missing: [],
    }) });
    await session.insert([{ itemId: "one" }]);
    await session.insertBibliography();
    wps.cursor = 0;
    await session.insert([{ itemId: "two" }]);
    expect((await document.getCitations()).map(({ text }) => text)).toEqual(["[1]", "[2]"]);
    expect(await document.getBibliography()).toEqual(["<i>two</i>", "<i>one</i>"]);
    expect(wps.nodes.map(({ text }) => text)).toEqual(["[1]", "[2]", "two\vone"]);
    await session.setStyle("author-date");
    const reopened = new WpsDocument(wps);
    expect(await reopened.getStyleId()).toBe("author-date");
    expect((await reopened.getCitations()).map(({ text }) => text)).toEqual(["two", "one"]);
    expect(wps.variables.adds).toBe(1);
  });

  it("unlinks only Paperloom controls and its variable, retaining manual text and formatting", async () => {
    const { id } = await document.insertCitation([{ itemId: "one" }]);
    await document.setCitationText(id, "<sup>[1]</sup>");
    await document.setBibliography(["<i>Title</i>"]);
    await document.setStyleId("author-date");
    wps.nodes[0].Range.Text = "Manually edited in WPS";
    wps.nodes[0].formatting.Bold = -1;
    const other = wps.ActiveDocument.ContentControls.Add(0, wps.Selection.Range);
    other.Tag = "other-addin";
    wps.variables.Add("other-addin", "keep");
    const before = wps.nodes.map(({ text, formatting }) => ({ text, formatting: { ...formatting } }));
    await document.unlink();
    expect(wps.nodes.map(({ text, formatting }) => ({ text, formatting }))).toEqual(before);
    expect(wps.nodes.map(({ Tag }) => Tag)).toEqual(["", "", "other-addin"]);
    expect(wps.deletes).toEqual([
      { tag: "paperloom:bibliography", deleteContents: false }, { tag: `paperloom:cite:${id}`, deleteContents: false },
    ]);
    expect(wps.variables.values.has("paperloom")).toBe(false);
    expect(wps.variables.values.get("other-addin")).toBe("keep");
    const reopened = new WpsDocument(wps);
    expect(await reopened.getCitations()).toEqual([]);
    expect(await reopened.getBibliography()).toBeNull();
    expect(await reopened.getStyleId()).toBeNull();
    await reopened.unlink();
  });

  it("supports the Range.SetRange/Select caret fallback", async () => {
    delete wps.Selection.SetRange;
    await document.insertCitation([{ itemId: "one" }]);
    await document.insertCitation([{ itemId: "two" }]);
    expect(wps.moves).toEqual([2, 4]);
  });

  it("surfaces a failed caret move instead of silently allowing nested insertion", async () => {
    wps.Selection.SetRange = () => { throw new Error("Unsupported caret operation"); };
    await expect(document.insertCitation([{ itemId: "one" }]))
      .rejects.toThrow("The cursor could not be moved after the Paperloom field.");
  });

  it("reports write and deletion failures and allows queued operations to recover", async () => {
    wps.variables.missingThrows = false;
    wps.variables.failWrite = true;
    await expect(document.setStyleId("first")).rejects.toThrow("Paperloom document data could not be saved.");
    expect(wps.variables.values.has("paperloom")).toBe(false);
    await document.setStyleId("first");
    wps.variables.failWrite = true;
    const failed = document.setStyleId("second");
    const next = document.getStyleId();
    await expect(failed).rejects.toThrow("Paperloom document data could not be saved.");
    expect(await next).toBe("first");
    await document.setStyleId("third");
    const { id } = await document.insertCitation([{ itemId: "one" }]);
    wps.failDelete = true;
    await expect(document.removeCitation(id)).rejects.toThrow("Paperloom fields could not be removed.");
    await expect(document.unlink()).rejects.toThrow("Paperloom fields could not be removed.");
    expect(wps.variables.state().citations).toHaveLength(1);
    wps.failDelete = false;
    wps.variables.failDelete = true;
    await expect(document.unlink()).rejects.toThrow("Paperloom document data could not be saved.");
    expect(wps.variables.values.has("paperloom")).toBe(true);
    wps.variables.failDelete = false;
    await document.unlink();
    expect(wps.variables.values.has("paperloom")).toBe(false);
  });

  it.each([
    "not JSON", JSON.stringify({ version: 2, citations: [], bibliography: null, styleId: null }),
    JSON.stringify({ version: 1, citations: [{ id: "bad", works: [{ itemId: 7 }], text: "" }], bibliography: null, styleId: null }),
    JSON.stringify({ version: 1, citations: [], bibliography: [7], styleId: null }),
    JSON.stringify({ version: 1, citations: [], bibliography: null, styleId: 7 }),
  ])("rejects damaged or newer metadata without replacing it: %s", async (raw) => {
    wps.variables.values.set("paperloom", raw);
    await expect(document.getCitations()).rejects.toThrow("Paperloom document data could not be read.");
    await expect(document.setStyleId("other")).rejects.toThrow("Paperloom document data could not be read.");
    expect(wps.variables.values.get("paperloom")).toBe(raw);
    expect(wps.variables.writes).toBe(0);
  });

  it("rejects controls whose citation or bibliography metadata is missing", async () => {
    const { id } = await document.insertCitation([{ itemId: "one" }]);
    await document.setBibliography(["Title"]);
    wps.variables.values.delete("paperloom");
    await expect(document.getCitations()).rejects.toThrow("Paperloom citation data is missing.");
    await expect(document.setCitationText(id, "replacement")).rejects.toThrow("Paperloom citation data is missing.");
    await expect(document.getBibliography()).rejects.toThrow("Paperloom citation data is missing.");
  });
});
