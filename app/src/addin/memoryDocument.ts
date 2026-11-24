import DOMPurify from "dompurify";
import type { CitationWork, DocumentAdapter, DocumentCitation } from "./document";

export type DocumentRun =
  | { kind: "text"; text: string }
  | { kind: "citation"; citation: DocumentCitation }
  | { kind: "bibliography"; entries: string[] };

/** Only the formatting promised by the connector is rendered in the preview.
 * Wrapped, so text before the first tag ("[1] <i>…</i>") stays in place: an
 * HTML parser may move a fragment's leading text (happy-dom did). */
export function citationHtml(html: string): string {
  return DOMPurify.sanitize(`<span>${html}</span>`, { ALLOWED_TAGS: ["i", "b", "sup", "sub"], ALLOWED_ATTR: [] });
}

export function plainText(html: string): string {
  const element = document.createElement("div");
  element.innerHTML = citationHtml(html);
  return element.textContent ?? "";
}

export class MemoryDocument implements DocumentAdapter {
  private runs: DocumentRun[];
  private selection: number;
  private nextId = 1;

  constructor(runs: DocumentRun[] = [], private styleId: string | null = null) {
    this.runs = structuredClone(runs);
    this.selection = runs.length;
  }

  getRuns(): DocumentRun[] { return structuredClone(this.runs); }
  getSelection(): number { return this.selection; }

  /** Selection is a boundary before a run, or runs.length for the end. */
  setSelection(position: number): void {
    if (!Number.isInteger(position) || position < 0 || position > this.runs.length) throw new RangeError("Invalid selection");
    this.selection = position;
  }

  async getCitations(): Promise<DocumentCitation[]> {
    return this.runs.flatMap((run) => run.kind === "citation" ? [structuredClone(run.citation)] : []);
  }

  async insertCitation(works: CitationWork[]): Promise<DocumentCitation> {
    let id: string;
    do { id = `citation-${this.nextId++}`; }
    while (this.runs.some((run) => run.kind === "citation" && run.citation.id === id));
    const citation: DocumentCitation = { id, works: structuredClone(works), text: "" };
    this.runs.splice(this.selection++, 0, { kind: "citation", citation });
    return structuredClone(citation);
  }

  private citation(id: string): DocumentCitation {
    const run = this.runs.find((run) => run.kind === "citation" && run.citation.id === id);
    if (!run || run.kind !== "citation") throw new Error("Citation no longer exists.");
    return run.citation;
  }

  async setCitationText(id: string, html: string): Promise<void> { this.citation(id).text = html; }
  async replaceCitationWorks(id: string, works: CitationWork[]): Promise<void> { this.citation(id).works = structuredClone(works); }

  async removeCitation(id: string): Promise<void> {
    this.citation(id);
    const index = this.runs.findIndex((run) => run.kind === "citation" && run.citation.id === id);
    this.runs.splice(index, 1);
    if (index < this.selection) this.selection--;
  }

  async getBibliography(): Promise<string[] | null> {
    const run = this.runs.find((run) => run.kind === "bibliography");
    return run?.kind === "bibliography" ? [...run.entries] : null;
  }

  async setBibliography(entries: string[]): Promise<void> {
    const run = this.runs.find((run) => run.kind === "bibliography");
    if (run?.kind === "bibliography") run.entries = [...entries];
    else this.runs.splice(this.selection++, 0, { kind: "bibliography", entries: [...entries] });
  }

  async getStyleId(): Promise<string | null> { return this.styleId; }
  async setStyleId(id: string): Promise<void> { this.styleId = id; }

  async getText(): Promise<string> {
    return this.runs.map((run) => run.kind === "text" ? run.text
      : run.kind === "citation" ? plainText(run.citation.text) : run.entries.map(plainText).join("\n")).join("");
  }

  async replaceMarker(marker: string, works: CitationWork[]): Promise<number> {
    let count = 0;
    for (let index = 0; index < this.runs.length; index++) {
      const run = this.runs[index];
      const at = run.kind === "text" && marker ? run.text.indexOf(marker) : -1;
      if (run.kind !== "text" || at < 0) continue;
      const citation: DocumentCitation = { id: `citation-${this.nextId++}`, works: structuredClone(works), text: "" };
      const before = run.text.slice(0, at);
      const after = run.text.slice(at + marker.length);
      const parts: DocumentRun[] = [
        ...(before ? [{ kind: "text" as const, text: before }] : []),
        { kind: "citation", citation },
        ...(after ? [{ kind: "text" as const, text: after }] : []),
      ];
      this.runs.splice(index, 1, ...parts);
      if (index < this.selection) this.selection += parts.length - 1;
      // The text after the marker is looked at next.
      index += parts.length - (after ? 2 : 1);
      count++;
    }
    return count;
  }

  async unlink(): Promise<void> {
    this.runs = this.runs.map<DocumentRun>((run) => run.kind === "text" ? run : {
      kind: "text",
      text: run.kind === "citation" ? plainText(run.citation.text) : run.entries.map(plainText).join("\n"),
    });
    this.styleId = null;
  }
}
