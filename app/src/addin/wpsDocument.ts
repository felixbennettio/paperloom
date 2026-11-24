import type { CitationWork, DocumentAdapter, DocumentCitation } from "./document";
import { citationHtml } from "./memoryDocument";

const VARIABLE_NAME = "paperloom";
const CITATION_TAG = "paperloom:cite:";
const BIBLIOGRAPHY_TAG = "paperloom:bibliography";

interface DocumentSettings {
  version: 1;
  citations: DocumentCitation[];
  bibliography: string[] | null;
  styleId: string | null;
}

function readSettings(value: unknown): DocumentSettings {
  if (value == null) return { version: 1, citations: [], bibliography: null, styleId: null };
  const state = value as Partial<DocumentSettings>;
  if (state.version !== 1 || !Array.isArray(state.citations)
    || !state.citations.every((citation) => citation && typeof citation.id === "string"
      && typeof citation.text === "string" && Array.isArray(citation.works)
      && citation.works.every((work) => work && typeof work.itemId === "string"
        && (work.locator === undefined || typeof work.locator === "string")))
    || !(state.bibliography === null || (Array.isArray(state.bibliography)
      && state.bibliography.every((entry) => typeof entry === "string")))
    || !(state.styleId === null || typeof state.styleId === "string")) {
    throw new Error("Paperloom document data could not be read.");
  }
  return structuredClone(state as DocumentSettings);
}

function variable(document: Wps.Document): Wps.Variable | undefined {
  // WPS may throw rather than return undefined for a missing variable.
  try { return document.Variables.Item(VARIABLE_NAME); } catch { return undefined; }
}

function settings(document: Wps.Document): DocumentSettings {
  const saved = variable(document);
  if (!saved) return readSettings(undefined);
  try { return readSettings(JSON.parse(saved.Value)); }
  catch { throw new Error("Paperloom document data could not be read."); }
}

function citation(state: DocumentSettings, controls: Wps.ContentControl[], id: string) {
  const control = controls.find(({ Tag }) => Tag === `${CITATION_TAG}${id}`);
  if (!control) throw new Error("Citation no longer exists.");
  const saved = state.citations.find((entry) => entry.id === id);
  if (!saved) throw new Error("Paperloom citation data is missing.");
  return { control, saved };
}

interface TextRun {
  text: string;
  italic: boolean;
  bold: boolean;
  superscript: boolean;
  subscript: boolean;
}

function textRuns(html: string): TextRun[] {
  const element = document.createElement("div");
  element.innerHTML = citationHtml(html);
  const runs: TextRun[] = [];
  function visit(node: Node, flags: Omit<TextRun, "text">): void {
    if (node.nodeType === Node.TEXT_NODE) {
      // Keep UTF-16 offsets; line ends become one line break (no paragraph
      // marks inside a field).
      const text = (node.textContent ?? "").replace(/\r\n|\n|\r/g, "\v");
      if (text) runs.push({ text, ...flags });
      return;
    }
    const tag = node instanceof Element ? node.tagName.toLowerCase() : "";
    const next = { italic: flags.italic || tag === "i", bold: flags.bold || tag === "b",
      superscript: flags.superscript || tag === "sup", subscript: flags.subscript || tag === "sub" };
    for (const child of node.childNodes) visit(child, next);
  }
  visit(element, { italic: false, bold: false, superscript: false, subscript: false });
  return runs;
}

function deleteControl(control: Wps.ContentControl, deleteContents: boolean): void {
  try { control.Delete(deleteContents); }
  catch { throw new Error("Paperloom fields could not be removed."); }
}

export class WpsDocument implements DocumentAdapter {
  private pending: Promise<void> = Promise.resolve();

  constructor(private readonly application: Wps.Application) {}

  private save(document: Wps.Document, state: DocumentSettings | null): void {
    try {
      const saved = variable(document);
      if (state === null) saved?.Delete();
      else if (saved) saved.Value = JSON.stringify(state);
      else document.Variables.Add(VARIABLE_NAME, JSON.stringify(state));
    } catch { throw new Error("Paperloom document data could not be saved."); }
  }

  private run<T>(operation: (document: Wps.Document, controls: Wps.ContentControl[], state: DocumentSettings) => T,
    unlink = false): Promise<T> {
    const result = this.pending.then(() => {
      const document = this.application.ActiveDocument;
      const collection = document.ContentControls;
      const controls: Wps.ContentControl[] = [];
      for (let index = 1; index <= collection.Count; index++) controls.push(collection.Item(index));
      controls.sort((a, b) => a.Range.Start - b.Range.Start);
      const state = settings(document);
      const before = JSON.stringify(state);
      const tags = new Set(controls.map(({ Tag }) => Tag));
      state.citations = state.citations.filter(({ id }) => tags.has(`${CITATION_TAG}${id}`));
      if (!tags.has(BIBLIOGRAPHY_TAG)) state.bibliography = null;
      const value = operation(document, controls, state);
      if (unlink) this.save(document, null);
      else if (JSON.stringify(state) !== before) this.save(document, state);
      return value;
    });
    this.pending = result.then(() => undefined, () => undefined);
    return result;
  }

  private insertControl(document: Wps.Document, tag: string, title: string): Wps.ContentControl {
    const control = document.ContentControls.Add(0, this.application.Selection.Range);
    control.Tag = tag;
    control.Title = title;
    return control;
  }

  private moveAfter(control: Wps.ContentControl): void {
    try {
      const selection = this.application.Selection;
      // A control's Range is its content; its end mark takes one more
      // position, so End itself is still inside it (WPS put the reference
      // list inside the citation there).
      const after = control.Range.End + 1;
      if (typeof selection.SetRange === "function") selection.SetRange(after, after);
      else {
        const range = selection.Range;
        range.SetRange(after, after);
        if (typeof range.Select !== "function") throw new Error();
        range.Select();
      }
    } catch { throw new Error("The cursor could not be moved after the Paperloom field."); }
  }

  private setText(document: Wps.Document, control: Wps.ContentControl, entries: string[]): void {
    const runs: TextRun[] = [];
    for (const [index, entry] of entries.entries()) {
      // A line break: WPS keeps no paragraph mark inside the field.
      if (index) runs.push({ text: "\v", italic: false, bold: false, superscript: false, subscript: false });
      runs.push(...textRuns(entry));
    }
    const range = control.Range;
    range.Text = runs.map(({ text }) => text).join("");
    // Clear inherited formatting, including a previous style's superscript.
    const baseFont = control.Range.Font;
    baseFont.Italic = false;
    baseFont.Bold = false;
    baseFont.Superscript = false;
    baseFont.Subscript = false;
    let offset = control.Range.Start;
    for (const run of runs) {
      const font = document.Range(offset, offset + run.text.length).Font;
      font.Italic = run.italic;
      font.Bold = run.bold;
      font.Superscript = run.superscript;
      font.Subscript = run.subscript;
      offset += run.text.length;
    }
  }

  getCitations(): Promise<DocumentCitation[]> {
    return this.run((_document, controls, state) => controls
      .filter(({ Tag }) => Tag.startsWith(CITATION_TAG))
      .map(({ Tag }) => structuredClone(citation(state, controls, Tag.slice(CITATION_TAG.length)).saved)));
  }

  insertCitation(works: CitationWork[]): Promise<DocumentCitation> {
    const snapshot = structuredClone(works);
    return this.run((document, controls, state) => {
      let id: string;
      do { id = crypto.randomUUID(); }
      while (controls.some(({ Tag }) => Tag === `${CITATION_TAG}${id}`));
      const saved: DocumentCitation = { id, works: snapshot, text: "" };
      const control = this.insertControl(document, `${CITATION_TAG}${id}`, "Paperloom citation");
      this.setText(document, control, [""]);
      this.moveAfter(control);
      state.citations.push(saved);
      return structuredClone(saved);
    });
  }

  setCitationText(id: string, html: string): Promise<void> {
    return this.run((document, controls, state) => {
      const { control, saved } = citation(state, controls, id);
      this.setText(document, control, [html]);
      saved.text = html;
    });
  }

  replaceCitationWorks(id: string, works: CitationWork[]): Promise<void> {
    const snapshot = structuredClone(works);
    return this.run((_document, controls, state) => { citation(state, controls, id).saved.works = snapshot; });
  }

  removeCitation(id: string): Promise<void> {
    return this.run((_document, controls, state) => {
      deleteControl(citation(state, controls, id).control, true);
      state.citations = state.citations.filter((entry) => entry.id !== id);
    });
  }

  getBibliography(): Promise<string[] | null> {
    return this.run((_document, controls, state) => {
      if (!controls.some(({ Tag }) => Tag === BIBLIOGRAPHY_TAG)) return null;
      if (state.bibliography === null) throw new Error("Paperloom citation data is missing.");
      return [...state.bibliography];
    });
  }

  /** Starts a paragraph at the cursor unless one ends `before` it (or
   * starts after it): the reference list stands in paragraphs of its own. */
  private breakParagraph(document: Wps.Document, before: boolean): void {
    const at = this.application.Selection.Range.Start;
    // Before: anything but a paragraph mark, even a field's end mark (whose
    // Text is empty), means the cursor is inside a paragraph.
    const needed = before
      ? at > 0 && document.Range(at - 1, at).Text !== "\r"
      : !["", "\r"].includes(document.Range(at, at + 1).Text);
    if (needed) this.application.Selection.TypeParagraph();
  }

  setBibliography(entries: string[]): Promise<void> {
    const snapshot = [...entries];
    return this.run((document, controls, state) => {
      const control = controls.find(({ Tag }) => Tag === BIBLIOGRAPHY_TAG);
      if (control) this.setText(document, control, snapshot);
      else {
        this.breakParagraph(document, true);
        const inserted = this.insertControl(document, BIBLIOGRAPHY_TAG, "Paperloom bibliography");
        this.setText(document, inserted, snapshot);
        this.moveAfter(inserted);
        this.breakParagraph(document, false);
      }
      state.bibliography = snapshot;
    });
  }

  getStyleId(): Promise<string | null> { return this.run((_document, _controls, state) => state.styleId); }

  setStyleId(id: string): Promise<void> {
    return this.run((_document, _controls, state) => { state.styleId = id; });
  }

  getText(): Promise<string> {
    return this.run((document) => document.Content.Text);
  }

  replaceMarker(marker: string, works: CitationWork[]): Promise<number> {
    const snapshot = structuredClone(works);
    return this.run((document, controls, state) => {
      if (!marker) return 0;
      const content = document.Content;
      const text = content.Text;
      // The text leaves out fields' start and end marks, which take
      // positions in the document: each marker is looked for near where
      // the text puts it, past the last one found.
      const slack = 2 * (controls.length + text.split(marker).length) + 8;
      const tags = new Set(controls.map(({ Tag }) => Tag));
      let shift = 0;
      let floor = content.Start;
      let count = 0;
      for (let at = text.indexOf(marker); at >= 0; at = text.indexOf(marker, at + marker.length)) {
        const expected = content.Start + at + shift;
        let start: number | null = null;
        for (let offset = 0; offset <= slack && start === null; offset++) {
          for (const candidate of offset ? [expected + offset, expected - offset] : [expected]) {
            if (candidate >= floor && document.Range(candidate, candidate + marker.length).Text === marker) {
              start = candidate;
              break;
            }
          }
        }
        if (start === null) continue;
        let id: string;
        do { id = crypto.randomUUID(); }
        while (tags.has(`${CITATION_TAG}${id}`));
        tags.add(`${CITATION_TAG}${id}`);
        // The marker stays the control's text until the session formats it.
        const control = document.ContentControls.Add(0, document.Range(start, start + marker.length));
        control.Tag = `${CITATION_TAG}${id}`;
        control.Title = "Paperloom citation";
        state.citations.push({ id, works: structuredClone(snapshot), text: "" });
        // The control's two marks move the text after it.
        shift = start - content.Start - at + 2;
        floor = start + marker.length + 2;
        count++;
      }
      return count;
    });
  }

  unlink(): Promise<void> {
    return this.run((_document, controls) => {
      for (const control of [...controls].reverse()) {
        if (control.Tag.startsWith(CITATION_TAG) || control.Tag === BIBLIOGRAPHY_TAG) deleteControl(control, false);
      }
    }, true);
  }
}
