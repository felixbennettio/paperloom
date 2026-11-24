// Only the synchronous WPS JS add-in surface used by the document adapter.
declare namespace Wps {
  interface Font {
    Italic: boolean | number;
    Bold: boolean | number;
    Superscript: boolean | number;
    Subscript: boolean | number;
  }

  interface Range {
    readonly Start: number;
    readonly End: number;
    Text: string;
    readonly Font: Font;
    SetRange(start: number, end: number): void;
    // Caret APIs still need verification in the packaged add-in.
    Select?(): void;
  }

  interface ContentControl {
    Tag: string;
    Title: string;
    readonly Range: Range;
    Delete(deleteContents: boolean): void;
  }

  interface ContentControls {
    readonly Count: number;
    Item(index: number): ContentControl;
    Add(type: 0, range: Range): ContentControl;
  }

  interface Variable {
    Value: string;
    Delete(): void;
  }

  interface Variables {
    Item(name: string): Variable | undefined;
    Add(name: string, value: string): Variable;
  }

  interface Document {
    /** The main text. */
    readonly Content: Range;
    readonly ContentControls: ContentControls;
    readonly Variables: Variables;
    Range(start: number, end: number): Range;
  }

  interface Application {
    readonly ActiveDocument: Document;
    readonly Selection: {
      readonly Range: Range;
      SetRange?(start: number, end: number): void;
      /** Starts a new paragraph at the cursor. */
      TypeParagraph(): void;
    };
  }

  interface Addin {
    WpsApplication(): Application;
  }
}

interface Window {
  wps?: Wps.Addin;
}
