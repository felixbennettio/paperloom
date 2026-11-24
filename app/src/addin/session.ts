import type { CiteConnector } from "./connector";
import type { CitationWork, DocumentAdapter } from "./document";

export const DEFAULT_STYLE = "gb-7714-2015-numeric";

export interface RefreshResult {
  citations: number;
  references: number;
  missing: string[];
  /** After converting markers: how many became citations, and the ones
   * left as written. */
  converted?: number;
  unconverted?: string[];
}

export class CitationSession {
  private pending: Promise<void> = Promise.resolve();

  constructor(readonly document: DocumentAdapter, private readonly connector: Pick<CiteConnector, "format" | "markers">) {}

  /** Keep delayed format responses from overwriting a subsequent edit. */
  private run<T>(operation: () => Promise<T>): Promise<T> {
    const result = this.pending.then(operation);
    this.pending = result.then(() => undefined, () => undefined);
    return result;
  }

  private async refreshDocument(insertBibliography = false): Promise<RefreshResult> {
    const citations = await this.document.getCitations();
    const styleId = await this.document.getStyleId() ?? DEFAULT_STYLE;
    const result = await this.connector.format(styleId, citations.map(({ works }) => ({ works })), true);
    if (result.citations.length !== citations.length) throw new Error("Paperloom returned an invalid response.");
    await this.document.setStyleId(styleId);
    for (const [index, citation] of citations.entries()) {
      const text = result.citations[index];
      // An empty cluster has no surviving works; its snapshot remains readable.
      if (text !== "" && text !== citation.text) await this.document.setCitationText(citation.id, text);
    }
    const bibliography = await this.document.getBibliography();
    const hasBibliography = insertBibliography || bibliography !== null;
    if (hasBibliography
      && JSON.stringify(bibliography) !== JSON.stringify(result.bibliography)) {
      await this.document.setBibliography(result.bibliography);
    }
    return { citations: citations.length, references: hasBibliography ? result.bibliography.length : 0, missing: [...new Set(result.missing)] };
  }

  refresh(): Promise<RefreshResult> { return this.run(() => this.refreshDocument()); }

  insert(works: CitationWork[]): Promise<RefreshResult> {
    const snapshot = structuredClone(works);
    return this.run(async () => {
      await this.document.insertCitation(snapshot);
      return this.refreshDocument();
    });
  }

  insertBibliography(): Promise<RefreshResult> { return this.run(() => this.refreshDocument(true)); }

  setStyle(id: string): Promise<RefreshResult> {
    return this.run(async () => {
      await this.document.setStyleId(id);
      return this.refreshDocument();
    });
  }

  edit(id: string, works: CitationWork[]): Promise<RefreshResult> {
    const snapshot = structuredClone(works);
    return this.run(async () => {
      await this.document.replaceCitationWorks(id, snapshot);
      return this.refreshDocument();
    });
  }

  remove(id: string): Promise<RefreshResult> {
    return this.run(async () => {
      await this.document.removeCitation(id);
      return this.refreshDocument();
    });
  }

  /** Turns the document's `[@key]` markers (as Cite in Word Document reads
   * them) into citations; a marker naming a work the library lacks stays. */
  convertMarkers(): Promise<RefreshResult> {
    return this.run(async () => {
      const markers = await this.connector.markers(await this.document.getText());
      let converted = 0;
      const unconverted: string[] = [];
      const seen = new Set<string>();
      for (const marker of markers) {
        // Each place with the same text is replaced at once.
        if (seen.has(marker.text)) continue;
        seen.add(marker.text);
        const replaced = marker.works.length ? await this.document.replaceMarker(marker.text, marker.works) : 0;
        if (replaced) converted += replaced;
        else unconverted.push(marker.text);
      }
      return { ...await this.refreshDocument(), converted, unconverted };
    });
  }

  unlink(): Promise<void> { return this.run(() => this.document.unlink()); }
}
