export interface CitationWork {
  itemId: string;
  locator?: string;
}

export interface DocumentCitation {
  id: string;
  works: CitationWork[];
  /** The last formatted HTML, retained even when the library is unavailable. */
  text: string;
}

/** Hosts own selection, field metadata and persistence; callers await every write. */
export interface DocumentAdapter {
  /** Returns detached snapshots in document order. */
  getCitations(): Promise<DocumentCitation[]>;
  /** Inserts a field at the current selection and returns its saved metadata. */
  insertCitation(works: CitationWork[]): Promise<DocumentCitation>;
  setCitationText(id: string, html: string): Promise<void>;
  replaceCitationWorks(id: string, works: CitationWork[]): Promise<void>;
  removeCitation(id: string): Promise<void>;
  /** Null means absent; an empty array means an existing, empty reference list. */
  getBibliography(): Promise<string[] | null>;
  /** Updates the existing list, or inserts one at the selection. Entries are HTML. */
  setBibliography(entries: string[]): Promise<void>;
  getStyleId(): Promise<string | null>;
  setStyleId(id: string): Promise<void>;
  /** Removes fields and their metadata, preserving visible text in document order. */
  unlink(): Promise<void>;
  /** The document's text, to find `[@key]` citation markers in. */
  getText(): Promise<string>;
  /** Makes each place the text reads `marker` a citation of `works`, in
   * place of the marker; returns how many. */
  replaceMarker(marker: string, works: CitationWork[]): Promise<number>;
}
