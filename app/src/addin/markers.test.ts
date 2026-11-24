// @vitest-environment happy-dom
import { afterEach, describe, expect, it, vi } from "vitest";
import type { CiteConnector } from "./connector";
import { OfficeDocument } from "./officeDocument";
import { CitationSession } from "./session";
import { WpsDocument } from "./wpsDocument";

// Turning `[@key]` markers into citations in Word and WPS documents whose
// text is modelled, unlike the other adapter tests' fakes.

const markers: CiteConnector["markers"] = async (text) => [...new Set(text.match(/\[@[^\]]*\]/g) ?? [])].map((marker) =>
  marker === "[@nobody]" ? { text: marker, works: [], missing: ["nobody"] } : { text: marker, works: [{ itemId: marker.slice(2, -1) }], missing: [] });

const format: CiteConnector["format"] = async (_style, clusters) => {
  const numbers = new Map<string, number>();
  const citations = clusters.map(({ works }) => `[${works.map(({ itemId }) => {
    if (!numbers.has(itemId)) numbers.set(itemId, numbers.size + 1);
    return numbers.get(itemId);
  }).join(",")}]`);
  return { citations, bibliography: [], missing: [] };
};

function font(): Wps.Font {
  return { Italic: false, Bold: false, Superscript: false, Subscript: false };
}

/** A WPS document as positions: each control's start and end marks are one
 * private-use character each, which take positions but no part of Text. */
class MarkedWps implements Wps.Application {
  private controls: { mark: number; Tag: string; Title: string }[] = [];
  private values = new Map<string, string>();
  readonly ActiveDocument: Wps.Document;
  readonly Selection = { Range: this.range(0, 0), TypeParagraph() {} };

  constructor(public positions: string) {
    const owner = this;
    for (const match of positions.matchAll(/[-]/g)) {
      const code = match[0].charCodeAt(0);
      if (code % 2 === 0) this.controls.push({ mark: code, Tag: "other", Title: "" });
    }
    this.ActiveDocument = {
      get Content() { return owner.range(0, owner.positions.length); },
      ContentControls: {
        get Count() { return owner.controls.length; },
        Item: (index) => this.control(this.controls[index - 1]),
        Add: (type, range) => {
          expect(type).toBe(0);
          const mark = 0xE000 + 2 * this.controls.length;
          const text = this.positions;
          this.positions = text.slice(0, range.Start) + String.fromCharCode(mark) + text.slice(range.Start, range.End)
            + String.fromCharCode(mark + 1) + text.slice(range.End);
          const control = { mark, Tag: "", Title: "" };
          this.controls.push(control);
          return this.control(control);
        },
      },
      Variables: {
        Item: (name) => {
          if (!this.values.has(name)) throw new Error("Variable not found");
          return { get Value() { return owner.values.get(name)!; }, set Value(value) { owner.values.set(name, value); },
            Delete: () => { this.values.delete(name); } };
        },
        Add: (name, value) => { this.values.set(name, value); return this.ActiveDocument.Variables.Item(name)!; },
      },
      Range: (start, end) => this.range(start, end),
    };
  }

  text(): string { return this.positions.replace(/[-]/g, ""); }

  private range(start: number, end: number): Wps.Range {
    const owner = this;
    return {
      Start: start, End: end, Font: font(), SetRange() {},
      get Text() { return owner.positions.slice(start, end).replace(/[-]/g, ""); },
      set Text(value) { owner.positions = owner.positions.slice(0, start) + value + owner.positions.slice(end); },
    };
  }

  private control(control: { mark: number; Tag: string; Title: string }): Wps.ContentControl {
    const owner = this;
    return {
      get Tag() { return control.Tag; }, set Tag(value) { control.Tag = value; },
      get Title() { return control.Title; }, set Title(value) { control.Title = value; },
      get Range() {
        const start = owner.positions.indexOf(String.fromCharCode(control.mark)) + 1;
        return owner.range(start, owner.positions.indexOf(String.fromCharCode(control.mark + 1)));
      },
      Delete() { throw new Error("Not deleted in these tests"); },
    };
  }
}

describe("WPS citation markers", () => {
  it("finds each marker past the fields before it and makes it a citation", async () => {
    const wps = new MarkedWps("Before other heat [@kucsko] and [@curie], again [@kucsko]; [@nobody].\r");
    const document = new WpsDocument(wps);
    expect(await document.getText()).toBe("Before other heat [@kucsko] and [@curie], again [@kucsko]; [@nobody].\r");
    const result = await new CitationSession(document, { format, markers }).convertMarkers();
    expect(result).toMatchObject({ converted: 3, unconverted: ["[@nobody]"], citations: 3 });
    expect(wps.text()).toBe("Before other heat [1] and [2], again [1]; [@nobody].\r");
    expect((await document.getCitations()).map(({ works }) => works[0].itemId)).toEqual(["kucsko", "curie", "kucsko"]);
  });

  it("converts nothing that is not there", async () => {
    const document = new WpsDocument(new MarkedWps("No markers.\r"));
    expect(await document.replaceMarker("[@kucsko]", [{ itemId: "kucsko" }])).toBe(0);
    expect(await document.replaceMarker("", [{ itemId: "kucsko" }])).toBe(0);
  });
});

/** Word's body text, its controls and its search. */
function markedWord(text: string) {
  const controls: { tag: string; title: string; html: string }[] = [];
  const searches: string[] = [];
  const run: typeof Word.run = async (batch) => {
    const queue: (() => void)[] = [];
    const control = (node: { tag: string; title: string; html: string }): Word.ContentControl => ({
      get tag() { return node.tag; }, set tag(value) { queue.push(() => { node.tag = value; }); },
      get title() { return node.title; }, set title(value) { queue.push(() => { node.title = value; }); },
      insertHtml: (html) => { queue.push(() => { node.html = html; }); return range(); },
      delete: () => { throw new Error("Not deleted in these tests"); },
      getRange: () => range(),
    });
    const range = (marker?: string): Word.Range => ({
      insertContentControl: () => {
        const node = { tag: "", title: "", html: marker ?? "" };
        queue.push(() => { controls.push(node); });
        return control(node);
      },
      insertParagraph: () => { throw new Error("Not used"); },
      select: () => {},
    });
    let loaded: Word.ContentControl[] | undefined;
    let found: Word.Range[] | undefined;
    const body: Word.Body = {
      text: "",
      load: () => { queue.push(() => { body.text = text; }); return body; },
      search: (marker, options) => {
        expect(options).toEqual({ matchCase: true });
        searches.push(marker);
        const collection: Word.RangeCollection = {
          get items() { if (!found) throw new Error("Read before sync"); return found; },
          load: () => { queue.push(() => { found = text.split(marker).slice(1).map(() => range(marker)); }); return collection; },
        };
        return collection;
      },
    };
    const contentControls: Word.ContentControlCollection = {
      get items() { if (!loaded) throw new Error("Read before sync"); return loaded; },
      load: () => { queue.push(() => { loaded = controls.map(control); }); return contentControls; },
    };
    return batch({
      document: { body, contentControls, getSelection: () => range() },
      sync: async () => { while (queue.length) queue.shift()!(); },
    });
  };
  return { run, controls, searches };
}

describe("Word citation markers", () => {
  afterEach(() => { vi.unstubAllGlobals(); });

  it("searches for each marker once and makes every place it is found a citation", async () => {
    const word = markedWord("Heat [@kucsko] and [@curie], again [@kucsko]; [@nobody].");
    const memory = new Map<string, unknown>();
    vi.stubGlobal("Word", { run: word.run });
    vi.stubGlobal("Office", { context: {
      document: { settings: { get: (name: string) => memory.get(name), set: (name: string, value: unknown) => memory.set(name, value),
        remove: (name: string) => memory.delete(name),
        saveAsync: (callback: (result: Office.AsyncResult<void>) => void) => callback({ value: undefined, status: "succeeded",
          error: { code: 0, name: "", message: "" } }) } },
      requirements: { isSetSupported: () => true },
    } });
    const document = new OfficeDocument();
    const result = await new CitationSession(document, { format, markers }).convertMarkers();
    expect(word.searches).toEqual(["[@kucsko]", "[@curie]"]);
    expect(result).toMatchObject({ converted: 3, unconverted: ["[@nobody]"] });
    expect(word.controls.map(({ title }) => title)).toEqual(["Paperloom citation", "Paperloom citation", "Paperloom citation"]);
    expect((await document.getCitations()).map(({ works }) => works[0].itemId)).toEqual(["kucsko", "kucsko", "curie"]);
    // Word cannot search for these.
    expect(await document.replaceMarker(`[@${"x".repeat(260)}]`, [{ itemId: "x" }])).toBe(0);
    expect(await document.replaceMarker("[@a^p]", [{ itemId: "a" }])).toBe(0);
  });
});
