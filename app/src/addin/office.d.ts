// Only the Office.js surface used by the Word adapter; no runtime shim.
declare namespace Office {
  interface Settings {
    get(name: string): any;
    set(name: string, value: any): void;
    remove(name: string): void;
    saveAsync(callback?: (result: AsyncResult<void>) => void): void;
  }

  interface AsyncResult<T> {
    value: T;
    status: "succeeded" | "failed";
    error: { code: number; name: string; message: string };
  }

  const context: {
    displayLanguage: string;
    document: { settings: Settings };
    requirements: { isSetSupported(name: string, minVersion?: string): boolean };
  };
  function onReady(): Promise<{ host: string | null; platform: string | null }>;
}

declare namespace Word {
  function run<T>(batch: (context: RequestContext) => Promise<T>): Promise<T>;

  interface RequestContext {
    document: {
      body: Body;
      contentControls: ContentControlCollection;
      getSelection(): Range;
    };
    sync(): Promise<void>;
  }

  /** WordApi 1.1. */
  interface Body {
    text: string;
    load(properties: "text"): Body;
    search(searchText: string, options?: { matchCase?: boolean }): RangeCollection;
  }

  interface RangeCollection {
    items: Range[];
    load(properties: string): RangeCollection;
  }

  interface ContentControlCollection {
    items: ContentControl[];
    load(properties: string): ContentControlCollection;
  }

  interface ContentControl {
    tag: string;
    title: string;
    insertHtml(html: string, insertLocation: "Replace" | "Start" | "End"): Range;
    delete(keepContent: boolean): void;
    // WordApi 1.3; used only when the host supports it.
    getRange(rangeLocation: "After"): Range;
  }

  interface Range {
    // Omitting the type creates rich text and works with WordApi 1.1.
    insertContentControl(): ContentControl;
    /** A new paragraph before or after the range's (WordApi 1.1). */
    insertParagraph(paragraphText: string, insertLocation: "Before" | "After"): Paragraph;
    select(selectionMode?: "Select" | "Start" | "End"): void;
  }

  interface Paragraph {
    insertContentControl(): ContentControl;
  }
}
