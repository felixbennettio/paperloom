import type { CitationWork, DocumentAdapter, DocumentCitation } from "./document";
import { citationHtml } from "./memoryDocument";

const SETTINGS_KEY = "paperloom";
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
    // Do not replace metadata from a damaged document or a newer add-in.
    throw new Error("Paperloom document data could not be read.");
  }
  return structuredClone(state as DocumentSettings);
}

function citation(state: DocumentSettings, controls: Word.ContentControl[], id: string) {
  const control = controls.find(({ tag }) => tag === `${CITATION_TAG}${id}`);
  if (!control) throw new Error("Citation no longer exists.");
  const saved = state.citations.find((entry) => entry.id === id);
  if (!saved) throw new Error("Paperloom citation data is missing.");
  return { control, saved };
}

export class OfficeDocument implements DocumentAdapter {
  private pending: Promise<void> = Promise.resolve();

  private async save(state: DocumentSettings | null): Promise<void> {
    const settings = Office.context.document.settings;
    const previous: unknown = settings.get(SETTINGS_KEY);
    if (state === null) settings.remove(SETTINGS_KEY);
    else settings.set(SETTINGS_KEY, state);
    try {
      await new Promise<void>((resolve, reject) => {
        settings.saveAsync((result) => {
          if (result.status === "succeeded") resolve();
          else reject(new Error("Paperloom document data could not be saved."));
        });
      });
    } catch (error) {
      // A failed save must not make later reads look durably updated.
      if (previous == null) settings.remove(SETTINGS_KEY);
      else settings.set(SETTINGS_KEY, previous);
      throw error;
    }
  }

  private run<T>(
    operation: (context: Word.RequestContext, controls: Word.ContentControl[], state: DocumentSettings) => T | Promise<T>,
    unlink = false,
  ): Promise<T> {
    const result = this.pending.then(() => Word.run(async (context) => {
      const collection = context.document.contentControls;
      collection.load("tag");
      await context.sync();
      const controls = collection.items;
      const state = readSettings(Office.context.document.settings.get(SETTINGS_KEY));
      const before = JSON.stringify(state);
      // Word's controls are authoritative after a person deletes a field.
      const tags = new Set(controls.map(({ tag }) => tag));
      state.citations = state.citations.filter(({ id }) => tags.has(`${CITATION_TAG}${id}`));
      if (!tags.has(BIBLIOGRAPHY_TAG)) state.bibliography = null;
      const value = await operation(context, controls, state);
      await context.sync();
      if (unlink) await this.save(null);
      else if (JSON.stringify(state) !== before) await this.save(state);
      return value;
    }));
    this.pending = result.then(() => undefined, () => undefined);
    return result;
  }

  /** At the cursor, or in a new paragraph after the cursor's (`paragraph`),
   * so that the reference list does not run into the text around it. */
  private insertControl(context: Word.RequestContext, tag: string, title: string, paragraph = false): Word.ContentControl {
    const selection = context.document.getSelection();
    const control = (paragraph ? selection.insertParagraph("", "After") : selection).insertContentControl();
    control.tag = tag;
    control.title = title;
    return control;
  }

  private moveAfter(control: Word.ContentControl): void {
    // Avoid nesting the next citation inside this one on current Word hosts.
    if (Office.context.requirements.isSetSupported("WordApi", "1.3")) control.getRange("After").select();
  }

  getCitations(): Promise<DocumentCitation[]> {
    return this.run((_context, controls, state) => controls
      .filter(({ tag }) => tag.startsWith(CITATION_TAG))
      .map(({ tag }) => structuredClone(citation(state, controls, tag.slice(CITATION_TAG.length)).saved)));
  }

  insertCitation(works: CitationWork[]): Promise<DocumentCitation> {
    const snapshot = structuredClone(works);
    return this.run((context, controls, state) => {
      let id: string;
      do { id = crypto.randomUUID(); }
      while (controls.some(({ tag }) => tag === `${CITATION_TAG}${id}`));
      const saved: DocumentCitation = { id, works: snapshot, text: "" };
      const control = this.insertControl(context, `${CITATION_TAG}${id}`, "Paperloom citation");
      control.insertHtml("", "Replace");
      this.moveAfter(control);
      state.citations.push(saved);
      return structuredClone(saved);
    });
  }

  setCitationText(id: string, html: string): Promise<void> {
    return this.run((_context, controls, state) => {
      const { control, saved } = citation(state, controls, id);
      control.insertHtml(citationHtml(html), "Replace");
      saved.text = html;
    });
  }

  replaceCitationWorks(id: string, works: CitationWork[]): Promise<void> {
    const snapshot = structuredClone(works);
    return this.run((_context, controls, state) => { citation(state, controls, id).saved.works = snapshot; });
  }

  removeCitation(id: string): Promise<void> {
    return this.run((_context, controls, state) => {
      citation(state, controls, id).control.delete(false);
      state.citations = state.citations.filter((entry) => entry.id !== id);
    });
  }

  getBibliography(): Promise<string[] | null> {
    return this.run((_context, controls, state) => {
      if (!controls.some(({ tag }) => tag === BIBLIOGRAPHY_TAG)) return null;
      if (state.bibliography === null) throw new Error("Paperloom citation data is missing.");
      return [...state.bibliography];
    });
  }

  setBibliography(entries: string[]): Promise<void> {
    const snapshot = [...entries];
    return this.run((context, controls, state) => {
      let control = controls.find(({ tag }) => tag === BIBLIOGRAPHY_TAG);
      const inserting = !control;
      control ??= this.insertControl(context, BIBLIOGRAPHY_TAG, "Paperloom bibliography", true);
      control.insertHtml(snapshot.map((entry) => `<p>${citationHtml(entry)}</p>`).join(""), "Replace");
      if (inserting) this.moveAfter(control);
      state.bibliography = snapshot;
    });
  }

  getStyleId(): Promise<string | null> { return this.run((_context, _controls, state) => state.styleId); }

  setStyleId(id: string): Promise<void> {
    return this.run((_context, _controls, state) => { state.styleId = id; });
  }

  getText(): Promise<string> {
    return this.run(async (context) => {
      const body = context.document.body;
      body.load("text");
      await context.sync();
      return body.text;
    });
  }

  replaceMarker(marker: string, works: CitationWork[]): Promise<number> {
    const snapshot = structuredClone(works);
    // Word searches for at most 255 characters, and reads ^ as a code.
    if (!marker || marker.length > 255 || marker.includes("^")) return Promise.resolve(0);
    return this.run(async (context, controls, state) => {
      const found = context.document.body.search(marker, { matchCase: true });
      found.load("text");
      await context.sync();
      const taken = new Set(controls.map(({ tag }) => tag));
      for (const range of found.items) {
        let id: string;
        do { id = crypto.randomUUID(); }
        while (taken.has(`${CITATION_TAG}${id}`));
        taken.add(`${CITATION_TAG}${id}`);
        // The marker stays the control's text until the session formats it.
        const control = range.insertContentControl();
        control.tag = `${CITATION_TAG}${id}`;
        control.title = "Paperloom citation";
        state.citations.push({ id, works: structuredClone(snapshot), text: "" });
      }
      return found.items.length;
    });
  }

  unlink(): Promise<void> {
    return this.run((_context, controls) => {
      for (const control of [...controls].reverse()) {
        if (control.tag.startsWith(CITATION_TAG) || control.tag === BIBLIOGRAPHY_TAG) control.delete(true);
      }
    }, true);
  }
}
