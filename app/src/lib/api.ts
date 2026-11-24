// The app's calls into the Rust core. Inside Tauri they are commands
// (src-tauri/src/lib.rs); in a plain browser (UI development, tests) an
// in-memory stand-in answers instead.

import { invoke as tauriInvoke, type InvokeArgs } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { activeLanguage } from "../i18n";

/** A command; the Rust side writes its errors in English, so a failure
 * comes back in the interface's language (Core's message catalog). */
async function invoke<T>(command: string, args?: InvokeArgs): Promise<T> {
  try {
    return await tauriInvoke<T>(command, args);
  } catch (error) {
    throw await localizeError(error);
  }
}

async function localizeError(error: unknown): Promise<unknown> {
  const language = activeLanguage();
  const message = error instanceof Error ? error.message : typeof error === "string" ? error : null;
  if (language === "en" || message === null) return error;
  const localized = await tauriInvoke<string>("localize_message", { message, language }).catch(() => message);
  return error instanceof Error ? new Error(localized) : localized;
}

export type ReadStatus = "unread" | "reading" | "read";

export interface ItemSummary {
  id: string;
  itemType: string;
  title: string;
  authors: string;
  containerTitle: string | null;
  year: number | null;
  doi: string | null;
  starred: boolean;
  readStatus: ReadStatus;
  tags: string[];
  hasPdf: boolean;
  /** Milliseconds since 1970. */
  dateAdded: number;
  /** How many works cite it, as OpenAlex last counted them. */
  citedBy?: number | null;
}

/** Items that look like the same work, newest first. */
export interface DuplicateGroup {
  items: ItemSummary[];
}

/** A feed followed: a journal's table of contents, a preprint listing. */
export interface FeedInfo {
  id: string;
  title: string;
  url: string;
  unread: number;
  /** Milliseconds since 1970; null before the first reading. */
  lastChecked: number | null;
  /** Why the last reading failed. */
  problem: string | null;
}

/** An entry a feed announced. */
export interface FeedItem {
  id: string;
  feedId: string;
  title: string;
  authors: string;
  containerTitle: string | null;
  year: number | null;
  date: string | null;
  doi: string | null;
  abstractNote: string | null;
  link: string | null;
  read: boolean;
  /** When it was first read from the feed. */
  dateAdded: number;
  /** The library's item for the same work (by DOI), if any. */
  libraryItemId: string | null;
}

/** One test of a saved search: `title contains quantum`. */
export interface SearchCondition {
  field: string;
  op: string;
  /** Words; a year; a number of days; an item type, reading status, tag name or collection ID; `true` or `false`. */
  value: string;
}

/** A named search whose items the library finds afresh each time. */
export interface SavedSearch {
  /** Empty for a search not saved yet. */
  id: string;
  name: string;
  /** Items must pass every condition, or else any one. */
  matchAll: boolean;
  conditions: SearchCondition[];
}

export type SearchFieldKind = "text" | "year" | "days" | "itemType" | "readStatus" | "tag" | "collection" | "flag";

/** A field conditions can test, with the tests it takes (the usual one first). */
export interface SearchField {
  id: string;
  kind: SearchFieldKind;
  ops: string[];
}

/** Which items to list; leave a field out to not filter by it. */
export interface ItemFilter {
  collectionId?: string | null;
  tag?: string | null;
  starredOnly?: boolean;
  readStatus?: ReadStatus | null;
  /** The trash instead of the library. */
  trashed?: boolean;
}

export interface TagInfo {
  name: string;
  /** `#rrggbb`, or null for a plain tag. */
  color: string | null;
  count: number;
}

export interface CollectionInfo {
  id: string;
  name: string;
  parentId: string | null;
  position: number;
}

export interface NewItem {
  itemType: string;
  title: string;
  authors: string[];
  containerTitle: string | null;
  year: number | null;
  doi: string | null;
}

/** Every field of an item (see paperloom-core's `ItemDetail`). */
export interface ItemDetail {
  id: string;
  itemType: string;
  title: string;
  authors: string[];
  editors: string[];
  containerTitle: string | null;
  containerShortTitle: string | null;
  year: number | null;
  date: string | null;
  volume: string | null;
  issue: string | null;
  pages: string | null;
  publisher: string | null;
  place: string | null;
  edition: string | null;
  number: string | null;
  doi: string | null;
  isbn: string | null;
  issn: string | null;
  arxivId: string | null;
  pmid: string | null;
  url: string | null;
  abstractNote: string | null;
  extra: string | null;
  keywords: string[];
  language: string | null;
  /** Shown only; `updateItem` leaves these alone. */
  starred: boolean;
  readStatus: ReadStatus;
  tags: string[];
  /** How many works cite it, as OpenAlex last counted them. */
  citedBy?: number | null;
}

/** A Word document's copy with its citations formatted. */
export interface CitedDocument {
  path: string;
  /** Citations formatted. */
  citations: number;
  /** Entries in the reference list. */
  references: number;
  /** Keys the library does not have, as written. */
  missing: string[];
}

export interface CitationStyle {
  id: string;
  name: string;
  /** Downloaded or added from a file (and so removable), not built in. */
  added: boolean;
}

/** A style in the CSL style repository. */
export interface StyleListing {
  id: string;
  name: string;
  /** `numeric`, `author-date`, `note`, `author` or `label`. */
  format: string;
  fields: string[];
  /** Cites like another style, under a journal's own name. */
  dependent: boolean;
  /** Built in or added already. */
  available: boolean;
}

export interface CitationOutput {
  entries: string[];
  citation: string;
}

export interface AccountUser {
  subject: string;
  displayName: string;
  email: string | null;
  picture: string | null;
}

export interface SyncStatus {
  /** The service the library syncs with; none before the first sync. */
  service: string | null;
  /** Milliseconds since 1970. */
  lastSynced: number | null;
  /** Changes made here that the service does not have yet. */
  waiting: number;
}

/** The AI provider, as the UI keeps it (the key stays on the device). */
export interface AiSettings {
  protocol: "anthropic" | "openai";
  baseUrl: string;
  model: string;
  embeddingModel?: string;
}

export interface AiSemanticStatus {
  model: string;
  indexed: number;
  total: number;
}

export interface AiModel {
  id: string;
  name: string;
}

/** Something an answer about the library drew on, by its number (`[1]`). */
export interface AiSource {
  number: number;
  /** `item`, `annotation`, `pdf` (a page) or `note`. */
  kind: string;
  title: string;
  itemId: string | null;
  attachmentId: string | null;
  /** From 0. */
  pageIndex: number | null;
  notePath: string | null;
}

export interface AiLibraryAnswer {
  /** Markdown, with `[1]`-style citations. */
  text: string;
  cutShort: boolean;
  sources: AiSource[];
}

export interface AiAnswer {
  /** Markdown. */
  text: string;
  /** Stopped at the length limit. */
  cutShort: boolean;
  /** The PDF pages the model read, from 1. */
  pages: number[];
}

export type AiTask = "translate" | "explain" | "summarize" | "polish";

/** Where to sync. */
export type SyncTarget =
  | { kind: "cloud"; service: string }
  | { kind: "folder"; path: string }
  | { kind: "webdav"; address: string; username: string }
  | { kind: "s3"; endpoint: string; region: string; bucket: string; prefix: string; accessKeyId: string }
  | { kind: "seafile"; server: string; account: string; library: string; folder: string };

export interface SyncReport {
  sent: number;
  received: number;
  /** Records other devices changed here. */
  changed: number;
  downloaded: number;
  /** Notes other devices changed here. */
  notes: number;
  finishedAt: number;
}

export interface ReferenceFormat {
  id: string;
  name: string;
  fileExtension: string;
}

export interface ImportReport {
  format: string;
  added: number;
  duplicates: number;
  skipped: number;
  addedIds: string[];
}

/** A database the online search can ask. */
export interface SearchSource {
  id: string;
  name: string;
  homepage: string;
  /** Searching it takes the person's own API key. */
  needsKey: boolean;
}

/** A library catalogue the person added: any that answers SRU. */
export interface SruCatalogue {
  /** Starts the key of each result; made up when the catalogue is added. */
  id: string;
  name: string;
  /** The SRU address, e.g. `https://services.dnb.de/sru/dnb?version=1.1`. */
  url: string;
  /** The CQL index each word is looked for in; empty for the server's choice. */
  index: string;
  /** `marcxml` (the default when empty), the server's name for MARC 21, or `dc`. */
  schema: string;
}

/** A backup written. */
export interface BackupReport {
  path: string;
  /** Files in the archive: the database, attachments and notes. */
  files: number;
  bytes: number;
}

/** What putting a backup back did now; the library follows on restart. */
export interface RestoreReport {
  notes: number;
  attachments: number;
}

/** What moving a Zotero library in did. */
export interface EndNoteImport {
  added: number;
  /** Records whose DOI the library had; their files joined them. */
  duplicates: number;
  files: number;
  /** Files the export names that are not on this computer. */
  missingFiles: number;
  notes: number;
  /** Records with no title. */
  skipped: number;
  problems: string[];
}

export interface ZoteroImport {
  added: number;
  /** Items whose DOI the library had; their files, tags and collections joined them. */
  duplicates: number;
  collections: number;
  files: number;
  /** Files Zotero lists that are not on this computer. */
  missingFiles: number;
  annotations: number;
  notes: number;
  /** Read from Zotero's backup because Zotero held its database. */
  fromBackup: boolean;
  problems: string[];
}

/** A site the in-app browser reads beyond its embedded metadata. */
export interface CaptureSite {
  id: string;
  name: string;
  home: string;
}

/** What the in-app browser (a window of its own) tells the main window. */
export interface WebTranslator {
  id: string;
  label: string;
  target: string;
  priority: number;
}

export interface TranslatorStatus {
  count: number;
  downloaded: string | null;
}

export interface TranslatorRequest {
  url: string;
  method?: string;
  headers?: Record<string, string>;
  body?: string | null;
}

export interface TranslatorResponse {
  status: number;
  text: string;
  url: string;
}

export type BrowserEvent =
  | { kind: "showing"; url: string; title: string }
  | { kind: "navigating"; url: string }
  | { kind: "html"; url: string; html: string }
  | { kind: "reading" }
  | { kind: "read"; page: SearchPage }
  | { kind: "failed"; message: string }
  /** A PDF came down, ready for `importPdf` like a picked one. */
  | { kind: "downloaded"; path: string; name: string }
  /** Another kind of file came down, into the Downloads folder. */
  | { kind: "saved"; path: string };

export interface SearchHit {
  /** What `addSearchResults` takes. */
  key: string;
  /** The work's details; `id` is empty. */
  detail: ItemDetail;
  pdfUrl: string | null;
  citedBy: number | null;
  /** The library's item with the same DOI, if it has one. */
  libraryItemId: string | null;
}

export interface SearchPage {
  hits: SearchHit[];
  total: number | null;
  hasMore: boolean;
}

/** Text another app shared: often a link, sometimes with the page's title. */
export interface SharedText {
  text: string;
  subject: string | null;
}

/** A writing plugin asking to use the library: `client` is its name ("Word"),
 * `origin` the page it runs on (empty when it sent none). */
export interface WriterQuestion {
  id: number;
  client: string;
  origin: string;
}

/** A writing plugin the person allowed; `added` in Unix seconds. */
export interface ConnectedWriter {
  client: string;
  origin: string;
  added: number;
}

/** The WPS add-in: `available` where this package carries it (Linux,
 * Windows), `installed` the version WPS lists. */
export interface WpsAddin {
  available: boolean;
  installed: string | null;
  version: string;
  folder: string;
}

/** The Word add-in: `available` where this package carries its manifest
 * (Windows, once the add-in is hosted), `installed` the version of the
 * manifest Word is given. */
export interface WordAddin {
  available: boolean;
  installed: string | null;
  version: string;
}

/** What other apps shared to Paperloom (Android's share sheet), or files
 * opened with it (Open With on Windows and Linux). */
export interface Shared {
  texts: SharedText[];
  /** PDFs, for `readPickedFile` and `importPdf` like picked ones. */
  pdfs: string[];
  /** Reference files (RIS, BibTeX…), read as text. */
  references: { name: string; text: string }[];
  /** Names of shared files that are neither. */
  unsupported: string[];
  /** Files were opened with Paperloom, rather than shared to it. */
  opened: boolean;
}

/** A file attached to an item. */
/** A change to a PDF's pages; pages count from 0. */
export type PageEdit =
  | { kind: "rotate"; pages: number[]; degrees: number }
  | { kind: "delete"; pages: number[] }
  | { kind: "move"; from: number; to: number }
  | { kind: "insertBlank"; at: number };

/** One of a form field's boxes: its page (from 0) and rectangle, in points from the page's bottom left. */
export interface PdfWidget {
  pageIndex: number;
  x: number;
  y: number;
  width: number;
  height: number;
  /** What a check box or radio button holds when this box is on; empty for other fields. */
  onValue: string;
}

/** A field of a PDF's form. */
export interface PdfFormField {
  /** Its full name (`Address.City`), which values name it by. */
  name: string;
  kind: "text" | "checkbox" | "radio" | "choice" | "button" | "signature";
  /** A text field's text, the on value of the box that is on (`Off` when none is), the choice made. */
  value: string;
  options: string[];
  optionLabels: string[];
  multiline: boolean;
  combo: boolean;
  readOnly: boolean;
  /** The most characters a text field takes; 0 for any number. */
  maxLength: number;
  /** Its text size in points; 0 to fit its box. */
  fontSize: number;
  widgets: PdfWidget[];
}

/** A line read in a page's picture: its words, and its box on the page as
 * shown, as shares of its width and height from its top-left corner. */
export interface RecognizedLine {
  text: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

/** A page's lines, for an invisible text layer; pages from 0. */
export interface RecognizedPage {
  pageIndex: number;
  lines: RecognizedLine[];
}

export interface AttachmentInfo {
  id: string;
  itemId: string;
  title: string;
  filePath: string;
  contentType: string;
  size: number;
  isPdf: boolean;
}

/** A PDF found online and attached. */
export interface FoundPdf {
  attachment: AttachmentInfo;
  /** Where it was fetched from. */
  url: string;
}

/** What adding a PDF did. */
export interface PdfImport {
  item: ItemSummary;
  attachment: AttachmentInfo;
  /** `doi`, `arxiv`, `title` or `none`. */
  matchedBy: string;
  existing: boolean;
  note: string | null;
}

/** In PDF user space: points from the bottom left of the unrotated page. */
export interface AnnotationRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export type AnnotationKind = "highlight" | "underline" | "strikeout" | "note" | "ink" | "text" | "square" | "circle" | "arrow" | "stamp";

/** A point in PDF user space, like {@link AnnotationRect}. */
export interface AnnotationPoint {
  x: number;
  y: number;
}

export interface NewAnnotation {
  attachmentId: string;
  kind: AnnotationKind;
  /** `#rrggbb` */
  color: string;
  /** From 0. */
  pageIndex: number;
  rects: AnnotationRect[];
  text: string | null;
  comment: string | null;
  /** An ink mark's strokes (its rectangle is made around them); an arrow's one stroke, tail to head. */
  strokes?: AnnotationPoint[][];
  /** A text box's font size in points (its text is its comment). */
  fontSize?: number | null;
}

export interface AnnotationInfo extends NewAnnotation {
  tags?: string[];
  id: string;
  dateAdded: number;
}

/** A note in the vault. */
export interface NoteInfo {
  /** `folder/Note.md`, relative to the vault. */
  path: string;
  title: string;
  folder: string;
  modified: number;
  size: number;
}

export interface NoteMention {
  path: string;
  title: string;
  /** From 0. */
  line: number;
  context: string;
}

/** A topic placed on a mind map, in points from its top left. */
export interface MindTopic {
  /** Its path from the centre: `0`, `0.2`, `0.2.0`. */
  id: string;
  parent: string | null;
  /** Its text as shown: a link as its text. */
  text: string;
  note: string;
  /** Where its link goes: a web address, or a note's name; empty when it has none. */
  link: string;
  /** The link goes to a web page (or an app), not a note. */
  webLink: boolean;
  /** Its labels, shown under its text (tags at the end of it in the note). */
  labels: string[];
  depth: number;
  /** −1 left of the centre, 1 right (below, in an org chart), 0 the centre. */
  side: number;
  x: number;
  y: number;
  width: number;
  height: number;
  fontSize: number;
  children: number;
  folded: boolean;
  /** The line from its parent, as SVG path data; empty for the centre. */
  edge: string;
}

/** A relation, summary or boundary placed on a mind map. */
export interface MindExtra {
  /** Its place among the note's extras, which edits name it by. */
  index: number;
  kind: "relation" | "summary" | "boundary";
  text: string;
  /** The topics it joins (a relation, from `first` to `last`) or takes in
   * (the first and last of a run of siblings), by path. */
  first: string;
  last: string;
  /** The relation's curve, the summary's brace or the boundary's frame, as SVG path data. */
  path: string;
  /** The box its text goes in; empty when it has no text. */
  x: number;
  y: number;
  width: number;
  height: number;
}

/** A marker a topic can carry: XMind's ID, the emoji a note keeps, its group (one of a group per topic). */
export interface MindMarker {
  id: string;
  emoji: string;
  group: string;
}

/** How a map is coloured (`#rrggbb`), as XMind's themes colour it. */
export interface MindMapTheme {
  /** `classic`, `ocean`, `forest`, `sunset`, `mono` or `dark`. */
  id: string;
  background: string;
  centre: string;
  centreText: string;
  /** Main topics filled with their branch's colour (text in `centreText`), or outlined in it on `topic`. */
  filled: boolean;
  topic: string;
  text: string;
  /** Relations' and summaries' lines. */
  line: string;
  /** Branch `0.n` takes the n-th, in turn. */
  branches: string[];
}

export interface MindMapLayout {
  topics: MindTopic[];
  /** Relations, summaries and boundaries. */
  extras: MindExtra[];
  width: number;
  height: number;
  /** `logic`, `right`, `org`, `tree`, `timeline` or `fishbone`. */
  style: string;
  /** The colours, from the note's `mindmap-theme` property. */
  theme: MindMapTheme;
}

export type MindMapEdit =
  | { kind: "addChild"; id: string; text: string }
  | { kind: "addSibling"; id: string; text: string }
  | { kind: "rename"; id: string; text: string }
  | { kind: "setNote"; id: string; note: string }
  | { kind: "delete"; id: string }
  | { kind: "move"; id: string; parent: string; index: number }
  /** Puts a marker (`priority-1`, `task-done`…) on a topic, or takes it off. */
  | { kind: "toggleMarker"; id: string; marker: string }
  /** Links a topic to a web page or a note, or takes its link off (`link` empty). */
  | { kind: "setLink"; id: string; link: string }
  /** Puts these labels on a topic in place of its own. */
  | { kind: "setLabels"; id: string; labels: string[] }
  | { kind: "relate"; from: string; to: string; text: string }
  | { kind: "summarize"; first: string; last: string; text: string }
  | { kind: "enclose"; first: string; last: string; text: string }
  | { kind: "retextExtra"; index: number; text: string }
  | { kind: "removeExtra"; index: number };

export interface DiagramPoint {
  x: number;
  y: number;
}

/** A shape or an edge of a diagram as the editor draws it, in page coordinates. */
export interface DiagramDrawn {
  id: string;
  edge: boolean;
  /** The group or container it sits in, if any. */
  parent: string;
  x: number;
  y: number;
  width: number;
  height: number;
  /** Degrees clockwise about its box's middle. */
  rotation: number;
  /** The outline (or the edge's line) and the lines inside, as SVG path data. */
  path: string;
  inner: string;
  arrows: string;
  openArrows: string;
  /** `#rrggbb`, or empty for none. */
  fill: string;
  stroke: string;
  strokeWidth: number;
  dashed: boolean;
  opacity: number;
  /** Its label as plain text. */
  text: string;
  textX: number;
  textY: number;
  textWidth: number;
  textHeight: number;
  fontSize: number;
  fontColor: string;
  bold: boolean;
  italic: boolean;
  underline: boolean;
  align: string;
  verticalAlign: string;
  labelBackground: string;
  wrap: boolean;
  /** An edge's ends, by id; empty for a loose end. */
  source: string;
  target: string;
  /** An edge's line, point by point. */
  points: DiagramPoint[];
  /** An edge's own bends, as stored. */
  bends: DiagramPoint[];
  container: boolean;
  /** Its style as written: `key=value;` pairs. */
  style: string;
  /** A picture's address (a `data:` URI or a web address); empty for other shapes. */
  image: string;
  /** Whether the picture keeps its proportions in its box. */
  imageAspect: boolean;
}

/** A page of a diagram, ready to draw. */
export interface DiagramPageView {
  pages: string[];
  page: number;
  /** In the order they stack, the bottom first. */
  items: DiagramDrawn[];
  x: number;
  y: number;
  width: number;
  height: number;
  background: string;
  /** The grid's spacing, 0 when it is off. */
  grid: number;
}

/** A shape the diagram editor offers, with a small drawing of it. */
export interface DiagramShape {
  id: string;
  name: string;
  group: string;
  width: number;
  height: number;
  path: string;
}

export type DiagramChange =
  | { kind: "addShape"; shape: string; x: number; y: number }
  | { kind: "connect"; source: string; target: string; from: DiagramPoint; to: DiagramPoint }
  | { kind: "move"; ids: string[]; dx: number; dy: number }
  | { kind: "resize"; id: string; x: number; y: number; width: number; height: number }
  | { kind: "setText"; id: string; text: string }
  | { kind: "setStyle"; ids: string[]; key: string; value: string }
  | { kind: "delete"; ids: string[] }
  | { kind: "reconnect"; id: string; source: boolean; cell: string; at: DiagramPoint }
  | { kind: "setBends"; id: string; points: DiagramPoint[] }
  | { kind: "toFront"; ids: string[] }
  | { kind: "toBack"; ids: string[] }
  | { kind: "group"; ids: string[] }
  | { kind: "ungroup"; id: string }
  | { kind: "align"; ids: string[]; how: string }
  | { kind: "distribute"; ids: string[]; horizontal: boolean }
  | { kind: "duplicate"; ids: string[]; dx: number; dy: number }
  /** A picture (a `data:` URI or a web address) in a box, as draw.io places one. */
  | { kind: "addImage"; image: string; x: number; y: number; width: number; height: number }
  | { kind: "addPage"; name: string }
  | { kind: "renamePage"; name: string }
  | { kind: "removePage" };

/** A diagram after a change: its XML, the page to show, and what to select. */
export interface DiagramChanged {
  xml: string;
  page: number;
  selection: string[];
}

/** A card on a canvas: `text` (Markdown), `file` (a note or other vault
 * file), `link` (a web page) or `group` (a labelled area). */
export interface CanvasCard {
  id: string;
  kind: string;
  x: number;
  y: number;
  width: number;
  height: number;
  /** `1`–`6` (Obsidian's colours) or `#rrggbb`. */
  color: string | null;
  text: string | null;
  file: string | null;
  subpath: string | null;
  url: string | null;
  label: string | null;
}

/** An arrow between cards; sides `top`, `right`, `bottom` or `left`. */
export interface CanvasArrow {
  id: string;
  fromNode: string;
  fromSide: string | null;
  fromEnd: string | null;
  toNode: string;
  toSide: string | null;
  toEnd: string | null;
  color: string | null;
  label: string | null;
}

export interface CanvasBoard {
  cards: CanvasCard[];
  arrows: CanvasArrow[];
}

export interface DrawingPoint {
  x: number;
  y: number;
}

/** One element of a drawing (an Excalidraw scene). Points are relative to
 * `x`, `y`; a stroke is `strokeWidth × 4.25 × (0.4 + 1.2 × pressure)` wide. */
export interface DrawingItem {
  id: string;
  /** `freedraw`, `line`, `arrow`, `rectangle`, `ellipse`, `diamond`, `text` or `image`. */
  kind: string;
  x: number;
  y: number;
  width: number;
  height: number;
  /** Radians, clockwise, about the centre. */
  angle: number;
  strokeColor: string;
  /** `transparent` or a colour. */
  backgroundColor: string;
  strokeWidth: number;
  /** 0–100. */
  opacity: number;
  points: DrawingPoint[];
  /** 0–1 for each point of a stroke; empty when the pen had none. */
  pressures: number[];
  text: string;
  fontSize: number;
  /** See-through, under the other strokes. */
  highlighter: boolean;
  /** An arrowhead at the end of a line. */
  arrowhead: boolean;
  /** A picture's `data:` URL; empty for anything else. */
  image?: string;
}

/** An Obsidian plugin installed in the vault (`.obsidian/plugins/<id>/`). */
export interface ObsidianPlugin {
  id: string;
  name: string;
  version: string;
  minAppVersion: string;
  description: string;
  author: string;
  authorUrl: string;
  /** It needs Node or Electron, so it stays off on phones. */
  isDesktopOnly: boolean;
  /** Turned on (listed in `community-plugins.json`). */
  enabled: boolean;
  hasStyles: boolean;
}

/** A plugin in Obsidian's community list. */
export interface CommunityPlugin {
  id: string;
  name: string;
  author: string;
  description: string;
  /** `owner/name` on GitHub. */
  repo: string;
}

export interface ObsidianPluginCode {
  main: string;
  styles: string | null;
  /** Its settings as it saved them (`data.json`). */
  data: string | null;
}

/** A Zotero 7 plugin installed for the library. */
export interface ZoteroPlugin {
  id: string;
  name: string;
  version: string;
  description: string;
  author: string;
  homepage: string;
  enabled: boolean;
}

export interface ZoteroPluginCode {
  /** Its text files (scripts, panes, locales), `bootstrap.js` among them. */
  files: { path: string; text: string }[];
  prefs: string | null;
}

export interface PluginResponse {
  status: number;
  text: string;
}

/** A drawing: its items, bottom first, and its paper (`blank`, `lines`, `grid` or `dots`). */
export interface DrawingPage {
  items: DrawingItem[];
  paper: string;
}

/** A note or library item a note is about too but does not link to yet. */
export interface LinkSuggestion {
  title: string;
  /** What goes in the note: `[[Note]]` or `[[@doi|Kucsko 2013]]`. */
  insert: string;
  path: string | null;
  itemId: string | null;
  /** The passage that matched, as HTML with the words in `<b>`. */
  snippetHtml: string;
}

export interface NoteOutline {
  headings: { level: number; text: string; line: number; anchor: string }[];
  tags: string[];
  aliases: string[];
  properties: { key: string; value: string }[];
  links: { target: string; subpath: string | null; alias: string | null; embed: boolean; line: number; resolved: string | null }[];
  backlinks: NoteMention[];
  unlinkedMentions: NoteMention[];
}

export type NoteProperty = NoteOutline["properties"][number];

/** A local date and time, for templates and daily notes. */
export interface LocalTime {
  year: number;
  month: number;
  day: number;
  hour: number;
  minute: number;
  second: number;
}

export function localNow(at: Date = new Date()): LocalTime {
  return {
    year: at.getFullYear(),
    month: at.getMonth() + 1,
    day: at.getDate(),
    hour: at.getHours(),
    minute: at.getMinutes(),
    second: at.getSeconds(),
  };
}

/** How the vault makes daily notes and fills templates (shared with Obsidian). */
export interface VaultSettings {
  dailyFolder: string;
  /** A Moment.js format: `YYYY-MM-DD`, `YYYY/MM/YYYY-MM-DD`… */
  dailyFormat: string;
  dailyTemplate: string;
  templatesFolder: string;
  dateFormat: string;
  timeFormat: string;
  /** The style `[@citations]` in notes show in, one of `citationStyles()`. */
  citationStyle: string;
}

/** An earlier version of a note: it read like this until `time`. */
export interface NoteVersion {
  time: number;
  size: number;
}

export interface DiffLine {
  change: "same" | "added" | "removed";
  text: string;
}

export interface GraphNode {
  /** The note's path, or the link as written. */
  id: string;
  title: string;
  kind: "note" | "missing" | "item";
  itemId: string | null;
  degree: number;
  x: number;
  y: number;
}

export interface NoteGraph {
  nodes: GraphNode[];
  edges: { from: number; to: number }[];
}

/** What `[@` completion offers. */
export interface CitationCompletion {
  /** What goes after `@`. */
  key: string;
  title: string;
  /** `Kucsko et al. 2013`. */
  cite: string;
}

export interface LinkCompletion {
  /** What goes between `[[` and `]]`. */
  insert: string;
  title: string;
  path: string;
}

/** Where a highlight is, to open its PDF at the page. */
export interface AnnotationLocation {
  annotationId: string;
  attachmentId: string;
  itemId: string;
  itemTitle: string;
  filePath: string;
  pageIndex: number;
}

export type FullTextKind = "item" | "annotation" | "pdf" | "note";

/** A match from `searchEverything`. */
export interface FullTextHit {
  kind: FullTextKind;
  title: string;
  /** HTML with the matched words in `<b>`. */
  snippetHtml: string;
  itemId: string | null;
  attachmentId: string | null;
  pageIndex: number | null;
  annotationId: string | null;
  notePath: string | null;
}

export type IdentifierKind = "doi" | "arxiv" | "pmid" | "isbn";

export interface LibraryInfo {
  coreVersion: string;
  location: string;
}

export interface Backend {
  libraryInfo(): Promise<LibraryInfo>;
  listItems(filter: ItemFilter): Promise<ItemSummary[]>;
  /** Saves edited details; returns them as stored. */
  updateItem(detail: ItemDetail): Promise<ItemDetail>;
  setStarred(itemId: string, starred: boolean): Promise<void>;
  setReadStatus(itemId: string, status: ReadStatus): Promise<void>;
  tags(): Promise<TagInfo[]>;
  setItemTags(itemId: string, tags: string[]): Promise<void>;
  setTagColor(name: string, color: string | null): Promise<void>;
  /** Renaming onto an existing tag merges the two. */
  renameTag(name: string, newName: string): Promise<void>;
  deleteTag(name: string): Promise<void>;
  restoreItem(itemId: string): Promise<void>;
  /** Deletes an item for good, with its files. */
  deleteItem(itemId: string): Promise<void>;
  emptyTrash(): Promise<number>;
  /** Items that look like the same work. */
  duplicateGroups(): Promise<DuplicateGroup[]>;
  /** Merges items into the first: its details stay, gaps filled from the others, whose files, tags and collections come to it; they go to the trash. */
  mergeItems(masterId: string, otherIds: string[]): Promise<ItemDetail>;
  /** The fields saved-search conditions can test. */
  searchFields(): Promise<SearchField[]>;
  savedSearches(): Promise<SavedSearch[]>;
  /** Saves a search (a new one when its id is empty); the search as saved. */
  saveSearch(search: SavedSearch): Promise<SavedSearch>;
  deleteSavedSearch(searchId: string): Promise<void>;
  /** The items (not in the trash) a search finds, saved or being edited. */
  runSearch(search: SavedSearch): Promise<ItemSummary[]>;
  /** Looks up how many works cite some items (all, when none are named) and keeps the counts; how many were counted. */
  countCitations(itemIds: string[]): Promise<number>;
  feeds(): Promise<FeedInfo[]>;
  /** Follows a feed (an RSS or Atom address, or a page that names its feed) and reads it. */
  addFeed(url: string): Promise<FeedInfo>;
  /** Reads every feed again; one that fails keeps its entries and says why. */
  refreshFeeds(): Promise<FeedInfo[]>;
  renameFeed(feedId: string, title: string): Promise<void>;
  /** Stops following a feed; its entries go with it. */
  removeFeed(feedId: string): Promise<void>;
  /** A feed's entries, or every feed's (null): the newest reading first. */
  feedItems(feedId: string | null): Promise<FeedItem[]>;
  setFeedItemRead(itemId: string, read: boolean): Promise<void>;
  markFeedRead(feedId: string | null): Promise<void>;
  /** Adds a feed entry to the library, looked up by DOI when it has one; marks it read. */
  addFeedItem(itemId: string, collectionId: string | null): Promise<ItemSummary>;
  /** The items related to an item, not counting the trash. */
  relatedItems(itemId: string): Promise<ItemSummary[]>;
  /** Relates two items, each to the other. */
  relateItems(itemId: string, otherId: string): Promise<void>;
  unrelateItems(itemId: string, otherId: string): Promise<void>;
  addItem(item: NewItem, collectionId: string | null): Promise<ItemSummary>;
  /** Looks up a DOI, arXiv ID, PMID or ISBN and adds the item with all its details. */
  addByIdentifier(input: string, collectionId: string | null): Promise<ItemSummary>;
  itemDetail(itemId: string): Promise<ItemDetail | null>;
  recognizeIdentifier(input: string): Promise<IdentifierKind | null>;
  citationStyles(): Promise<CitationStyle[]>;
  /** Searches the CSL style repository (its list is downloaded once a week). */
  searchCitationStyles(query: string, limit: number): Promise<StyleListing[]>;
  /** Downloads a style, with the style it cites like when needed. */
  downloadCitationStyle(id: string): Promise<CitationStyle>;
  /** Asks for a `.csl` file and adds its style; null if cancelled. */
  addCitationStyleFile(): Promise<CitationStyle | null>;
  /** Asks for a Word document and where to save a copy of it with its
   * `[@citations]` formatted in a style and the reference list added;
   * null if cancelled. `copyName` names the copy (`{name}`: the document's). */
  citeWordDocument(styleId: string, copyName: string): Promise<CitedDocument | null>;
  removeCitationStyle(id: string): Promise<void>;
  referenceFormats(): Promise<ReferenceFormat[]>;
  importReferences(text: string, collectionId: string | null): Promise<ImportReport>;
  /** Asks where to save and writes the file; the path, or null when cancelled. */
  exportReferencesFile(itemIds: string[], format: string): Promise<string | null>;
  searchSources(): Promise<SearchSource[]>;
  captureSites(): Promise<CaptureSite[]>;
  /** Asks where to save a backup (suggesting `name`) and writes it; null if cancelled. */
  backUpLibrary(name: string): Promise<BackupReport | null>;
  /** Backs up into `folder` (null: beside the library) when the newest
   * automatic backup is older than `everyHours`, keeping the newest `keep`;
   * null when none was due. */
  backUpIfDue(folder: string | null, everyHours: number, keep: number): Promise<BackupReport | null>;
  /** Asks for a backup and puts it back; the library follows on restart. */
  restoreLibrary(): Promise<RestoreReport | null>;
  restartApp(): Promise<void>;
  /** Asks for Zotero's data folder and moves its library in; null if cancelled. */
  importZotero(): Promise<ZoteroImport | null>;
  /** Asks for an EndNote XML export (and its `.Data` folder) and moves the library in; null if cancelled. */
  importEndNote(): Promise<EndNoteImport | null>;
  /** Opens the in-app browser at an address (desktop). */
  browserOpen(address: string): Promise<void>;
  /** Reads the page the browser shows; the works come as a `read` event. */
  browserRead(): Promise<void>;
  browserSnapshot(): Promise<void>;
  browserGo(to: "back" | "forward" | "reload"): Promise<void>;
  onBrowserEvent(callback: (event: BrowserEvent) => void): Promise<() => void>;
  /** Calls back with how many works the browser extension added. */
  onConnectorAdded(callback: (count: number) => void): Promise<() => void>;
  /** A writing plugin (Word, WPS) asks to use the library; answer with `answerWriter`. */
  onWriterQuestion(callback: (question: WriterQuestion) => void): Promise<() => void>;
  answerWriter(id: number, allow: boolean): Promise<void>;
  connectedWriters(): Promise<ConnectedWriter[]>;
  /** Takes back every writing plugin's access; each asks again. */
  forgetConnectedWriters(): Promise<void>;
  wpsAddin(): Promise<WpsAddin>;
  /** Puts this version of the add-in in WPS's add-in folder. */
  installWpsAddin(): Promise<WpsAddin>;
  removeWpsAddin(): Promise<WpsAddin>;
  wordAddin(): Promise<WordAddin>;
  /** Lists the add-in's manifest among Word's add-ins (Windows). */
  installWordAddin(): Promise<WordAddin>;
  removeWordAddin(): Promise<WordAddin>;
  /** Takes what other apps shared since last asked (Android). */
  takeShared(): Promise<Shared>;
  /** Calls back when something is shared while the app is open. */
  onShared(callback: () => void): Promise<() => void>;
  /** Reads the works on a web page, for `addSearchResults`. */
  readWebPage(url: string): Promise<SearchPage>;
  /** Searches an online database; `page` counts from 0. */
  searchOnline(sourceId: string, text: string, page: number): Promise<SearchPage>;
  /** National libraries' catalogues to start from when adding one. */
  cataloguePresets(): Promise<SruCatalogue[]>;
  /** Searches a library catalogue the person added; `page` counts from 0. */
  searchCatalogue(catalogue: SruCatalogue, text: string, page: number): Promise<SearchPage>;
  /** Adds results of recent searches, by their keys. */
  addSearchResults(keys: string[], collectionId: string | null): Promise<ImportReport>;
  /** Opens a web page or PDF link in the system browser. */
  openUrl(url: string): Promise<void>;
  /** Asks for PDFs; their paths, empty when cancelled. */
  pickPdfFiles(): Promise<string[]>;
  readPickedFile(path: string): Promise<ArrayBuffer>;
  /** Adds a picked PDF, given its first pages' text and stored title. */
  importPdf(path: string, text: string, title: string | null, collectionId: string | null): Promise<PdfImport>;
  attachments(itemId: string): Promise<AttachmentInfo[]>;
  /** Asks for files and attaches them. */
  attachFiles(itemId: string): Promise<AttachmentInfo[]>;
  /** Looks for an open-access PDF of the item (arXiv, OpenAlex, the publisher's page) and attaches it; null when there is none. */
  findAvailablePdf(itemId: string): Promise<FoundPdf | null>;
  readAttachment(attachmentId: string): Promise<ArrayBuffer>;
  /** Changes a PDF's pages; marks and page text follow their pages. */
  editPdfPages(attachmentId: string, edit: PageEdit): Promise<AttachmentInfo>;
  /** Asks where to save a copy with the marks written in, and writes it; the path, or null. */
  exportAnnotatedPdf(attachmentId: string): Promise<string | null>;
  /** The fields of a PDF's form; none when it has no form. */
  pdfFormFields(attachmentId: string): Promise<PdfFormField[]>;
  /** Fills in a PDF's form, rewriting the file. */
  fillPdfForm(attachmentId: string, values: { name: string; value: string }[]): Promise<AttachmentInfo>;
  /** Opens an attachment in the system's app for its type. */
  openAttachment(attachmentId: string): Promise<void>;
  removeAttachment(attachmentId: string): Promise<void>;
  annotations(attachmentId: string): Promise<AnnotationInfo[]>;
  addAnnotation(annotation: NewAnnotation): Promise<AnnotationInfo>;
  removeAnnotation(annotationId: string): Promise<void>;
  updateAnnotation(annotationId: string, color: string, comment: string | null): Promise<AnnotationInfo>;
  /** Markdown embedding a highlight in a note: `![[@key#hl-…]]`. */
  annotationEmbed(annotationId: string): Promise<string>;
  /** Markdown linking a note to an item: `[[@doi|Author 2013]]`. */
  itemLink(itemId: string): Promise<string>;
  /** The `[@key]` citation of some items, as notes and Word documents take it. */
  citationMarker(itemIds: string[]): Promise<string>;
  /** Whether this computer can read text from pictures of pages: Windows'
   * own text recognition, or Tesseract on Linux. */
  textRecognitionAvailable(): Promise<boolean>;
  /** The text in a picture of a page (a PNG, in base64), line by line;
   * null if none. */
  recognizePageText(png: string): Promise<string | null>;
  /** Writes why a page could not be drawn for recognition to the app's log. */
  noteTextRecognition(note: string): Promise<void>;
  /** The lines in a picture of a page (a PNG, in base64) with their boxes. */
  recognizePageLines(png: string): Promise<RecognizedLine[]>;
  /** Writes recognised lines into an attached PDF as invisible text over
   * its scanned pages, so they can be selected and searched. */
  addTextLayer(attachmentId: string, pages: RecognizedPage[]): Promise<AttachmentInfo>;
  annotationLocation(annotationId: string): Promise<AnnotationLocation | null>;
  /** The item a `[[@reference]]` names, if any. */
  resolveItemReference(reference: string): Promise<string | null>;
  attachmentInfo(attachmentId: string): Promise<AttachmentInfo | null>;
  /** Makes (once) a literature note for an item: details, abstract, highlights. */
  vaultCreateLiteratureNote(itemId: string): Promise<NoteInfo>;
  /** Lines of notes that link to an item or embed its highlights. */
  vaultItemMentions(itemId: string): Promise<NoteMention[]>;
  /** Notes and items about what a note is about that it does not link to yet. */
  vaultLinkSuggestions(path: string, limit: number): Promise<LinkSuggestion[]>;
  /** Searches items, annotations, PDF text and notes; empty `kinds` means all. */
  searchEverything(query: string, kinds: FullTextKind[], limit: number): Promise<FullTextHit[]>;
  /** Keeps a PDF's page text for full-text search. */
  setAttachmentText(attachmentId: string, pages: string[]): Promise<void>;
  attachmentHasText(attachmentId: string): Promise<boolean>;
  vaultRoot(): Promise<string>;
  /** Asks for a folder (an Obsidian vault works) and opens it; its path, or null. */
  vaultChooseFolder(): Promise<string | null>;
  /** Picks up edits made by other apps; true if anything changed. */
  vaultRefresh(): Promise<boolean>;
  vaultNotes(): Promise<NoteInfo[]>;
  /** A note's outline laid out as a mind map (`style` empty: the note's own). */
  mindmapLayout(text: string, title: string, style: string, folded: string[]): Promise<MindMapLayout>;
  /** The themes a map can take, the default first. */
  mindmapThemes(): Promise<MindMapTheme[]>;
  /** The markers topics can carry, in the order they show. */
  mindmapMarkers(): Promise<MindMarker[]>;
  /** Changes a mind map: the note's new text and the topic to show next. */
  mindmapEdit(text: string, title: string, edit: MindMapEdit): Promise<{ text: string; focus: string }>;
  newMindmapText(title: string): Promise<string>;
  /** A mind map note as an SVG picture. */
  mindmapSvg(text: string, title: string, style: string, folded: string[]): Promise<string>;
  /**
   * Asks where to save a mind map as `format` (`xmind`, `opml`, `mm`, `svg`,
   * or `png` with `png` the picture in base64) and writes it; its path, or
   * null if cancelled.
   */
  mindmapExport(text: string, title: string, format: string, png: string | null): Promise<string | null>;
  /** Asks for an XMind, OPML or FreeMind file and makes a mind map note of it; null if cancelled. */
  vaultImportMindmap(folder: string): Promise<NoteInfo | null>;
  /** The vault's canvases (JSON Canvas files), by path. */
  vaultCanvases(): Promise<string[]>;
  vaultReadCanvas(path: string): Promise<CanvasBoard>;
  vaultWriteCanvas(path: string, board: CanvasBoard): Promise<void>;
  /** Makes an empty canvas in a folder; its path. */
  vaultCreateCanvas(folder: string, name: string): Promise<string>;
  /** The vault's drawings (Excalidraw scenes), by path. */
  vaultDrawings(): Promise<string[]>;
  vaultReadDrawing(path: string): Promise<DrawingPage>;
  /** Saves a drawing; what it holds that this app does not draw stays. */
  vaultWriteDrawing(path: string, page: DrawingPage): Promise<void>;
  /** What rubbing along a path leaves: the parts of each stroke more than `radius` from it, each a stroke of its own. */
  drawingErase(items: DrawingItem[], path: DrawingPoint[], radius: number): Promise<DrawingItem[]>;
  /** Makes an empty drawing on `paper` in a folder; its path. */
  vaultCreateDrawing(folder: string, name: string, paper: string): Promise<string>;
  /** Keeps a file the app made (a recording, in base64) in a folder of the vault; its path. */
  vaultAddFile(folder: string, name: string, data: string): Promise<string>;
  /** A drawing as an SVG picture. */
  vaultDrawingSvg(path: string): Promise<string>;
  /** Asks where to save a drawing as `svg`, or `png` (the picture in base64), and writes it; its path, or null if cancelled. */
  drawingExport(path: string, format: string, png: string | null): Promise<string | null>;
  /** The Obsidian plugins in the vault, by name. */
  obsidianPlugins(): Promise<ObsidianPlugin[]>;
  /** Turns a plugin on or off (for Obsidian too). */
  setObsidianPluginEnabled(id: string, enabled: boolean): Promise<void>;
  obsidianPluginCode(id: string): Promise<ObsidianPluginCode>;
  /** Every note's metadata as Obsidian gives it to plugins, as JSON by path. */
  obsidianMetadata(): Promise<string>;
  /** Keeps a plugin's settings (JSON), as its `saveData` asks. */
  saveObsidianPluginData(id: string, data: string): Promise<void>;
  /** The Zotero plugins installed for the library, by name. */
  downloadTranslators(): Promise<number>;
  translatorsStatus(): Promise<TranslatorStatus>;
  removeTranslators(): Promise<void>;
  webTranslators(): Promise<WebTranslator[]>;
  translatorCatalog(): Promise<WebTranslator[]>;
  translatorCode(id: string): Promise<string>;
  readTranslatedItems(json: string, pageUrl: string): Promise<SearchPage>;
  translatorRequest(request: TranslatorRequest): Promise<TranslatorResponse>;
  zoteroPlugins(): Promise<ZoteroPlugin[]>;
  /** Asks for a Zotero plugin file (.xpi) and installs it; null if cancelled. */
  installZoteroPlugin(): Promise<ZoteroPlugin | null>;
  setZoteroPluginEnabled(id: string, enabled: boolean): Promise<void>;
  zoteroPluginCode(id: string): Promise<ZoteroPluginCode>;
  saveZoteroPluginPrefs(id: string, prefs: string): Promise<void>;
  /** The library's items as Zotero gives them to plugins (JSON). */
  zoteroItems(): Promise<string>;
  /** A Zotero plugin's change to an item: fields (JSON) and, if given, tags. */
  zoteroUpdateItem(key: string, fields: string, tags: string[] | null): Promise<void>;
  /** A note a Zotero plugin makes, in the open vault; its key. */
  zoteroCreateNote(parent: string | null, html: string): Promise<string>;
  /** A Zotero plugin's change to a note it made. */
  zoteroUpdateNote(key: string, html: string): Promise<void>;
  /** The community plugins Obsidian lists. */
  communityPlugins(): Promise<CommunityPlugin[]>;
  /** Downloads a community plugin's latest release into the vault (off until turned on). */
  installObsidianPlugin(repo: string): Promise<ObsidianPlugin>;
  /** A plugin's web request (`requestUrl`), to a site the person allowed. */
  pluginFetch(url: string, method: string, headers: { name: string; value: string }[], body: string | null): Promise<PluginResponse>;
  /** The vault's diagrams (`.drawio.svg` and `.drawio`), by path. */
  vaultDiagrams(): Promise<string[]>;
  /** A diagram's draw.io XML. */
  vaultReadDiagram(path: string): Promise<string>;
  /** Saves a diagram: its XML, and for a `.drawio.svg` a picture holding it (made by the core when null). */
  vaultWriteDiagram(path: string, xml: string, svg: string | null): Promise<void>;
  /** A page of a diagram laid out for the editor. */
  diagramView(xml: string, page: number): Promise<DiagramPageView>;
  /** Changes a diagram's XML. */
  diagramChange(xml: string, page: number, change: DiagramChange): Promise<DiagramChanged>;
  /** The shapes the diagram editor offers. */
  diagramShapes(): Promise<DiagramShape[]>;
  /** A page of a diagram as an SVG picture. */
  diagramPageSvg(xml: string, page: number): Promise<string>;
  /** Asks where to save a page as `svg` or `png` (`png` holds its Base64), or the diagram as `drawio`; the path, or null. */
  diagramExport(name: string, xml: string, page: number, format: string, png: string | null): Promise<string | null>;
  /** Makes an empty `.drawio.svg` in a folder; its path. */
  vaultCreateDiagram(folder: string, name: string): Promise<string>;
  vaultFolders(): Promise<string[]>;
  vaultRead(path: string): Promise<string>;
  vaultWrite(path: string, text: string): Promise<NoteInfo>;
  vaultCreateNote(folder: string, title: string): Promise<NoteInfo>;
  vaultCreateFolder(path: string): Promise<void>;
  /** Renames or moves a note and updates links to it; how many notes changed. */
  vaultRename(path: string, newPath: string): Promise<number>;
  /** Moves a note to the vault's `.trash`. */
  vaultDelete(path: string): Promise<void>;
  vaultOutline(path: string): Promise<NoteOutline>;
  vaultSwitcher(query: string, limit: number): Promise<NoteInfo[]>;
  vaultLinkCompletions(from: string, query: string, limit: number): Promise<LinkCompletion[]>;
  vaultTags(): Promise<{ name: string; count: number }[]>;
  vaultSearch(query: string, limit: number): Promise<NoteMention[]>;
  /** The note as HTML for the reading view. */
  vaultRender(path: string, text: string): Promise<string>;
  vaultResolve(target: string, from: string): Promise<string | null>;
  vaultFile(path: string): Promise<ArrayBuffer>;
  /** Writes a note's properties (front matter), keeping the rest. */
  vaultSetProperties(path: string, properties: NoteProperty[]): Promise<NoteInfo>;
  vaultSettings(): Promise<VaultSettings>;
  vaultSetSettings(settings: VaultSettings): Promise<void>;
  /** The notes in the templates folder. */
  vaultTemplates(): Promise<NoteInfo[]>;
  /** A template filled in for a note titled `title`. */
  vaultTemplateText(template: string, title: string, now: LocalTime): Promise<string>;
  vaultCreateFromTemplate(folder: string, title: string, template: string | null, now: LocalTime): Promise<NoteInfo>;
  /** Today's daily note, made the first time. */
  vaultDailyNote(now: LocalTime): Promise<NoteInfo>;
  /** A note's earlier versions, newest first. */
  vaultVersions(path: string): Promise<NoteVersion[]>;
  vaultVersion(path: string, time: number): Promise<string>;
  /** What changed from a version to the note now. */
  vaultCompareVersion(path: string, time: number): Promise<DiffLine[]>;
  vaultRestoreVersion(path: string, time: number): Promise<NoteInfo>;
  /** The graph of notes; around `center` within `depth` links, or all. */
  vaultGraph(center: string | null, depth: number): Promise<NoteGraph>;
  /** Asks where to save a note as a Word document (citations formatted,
   * references at the end) and writes it; the path, or null if cancelled. */
  vaultExportDocx(path: string): Promise<string | null>;
  /** Asks where to save a note as LaTeX (biblatex citations, the BibTeX
   * inside) and writes it; the path, or null if cancelled. */
  vaultExportLatex(path: string): Promise<string | null>;
  /** Library items matching every word typed, for `[@` completion. */
  citationCompletions(query: string, limit: number): Promise<CitationCompletion[]>;
  accountAvailable(): Promise<boolean>;
  accountStatus(): Promise<AccountUser | null>;
  /** Opens the sign-in page; the outcome arrives through `onAccountChange`. */
  accountSignIn(): Promise<void>;
  accountSignOut(): Promise<void>;
  /** Whether the library syncs, when it last did, and what waits to go. */
  syncStatus(): Promise<SyncStatus>;
  /** One sync of the library and the notes with `target`. */
  syncNow(target: SyncTarget): Promise<SyncReport>;
  /** Asks for the folder to sync through (desktop); null if cancelled. */
  syncPickFolder(): Promise<string | null>;
  /** Keeps the WebDAV password on this device; an empty one forgets it. */
  webdavPasswordSet(password: string): Promise<void>;
  webdavPasswordKnown(): Promise<boolean>;
  /** Keeps the Seafile password or API token on this device; an empty one forgets it. */
  seafilePasswordSet(password: string): Promise<void>;
  seafilePasswordKnown(): Promise<boolean>;
  /** Keeps the S3 secret key on this device; an empty one forgets it. */
  s3SecretSet(secret: string): Promise<void>;
  s3SecretKnown(): Promise<boolean>;
  /** Keeps the passphrase that seals sync end to end; an empty one stops sealing. */
  syncPassphraseSet(passphrase: string): Promise<void>;
  syncPassphraseKnown(): Promise<boolean>;
  /** Keeps the AI provider's key on this device; an empty one forgets it. */
  aiKeySet(key: string): Promise<void>;
  aiKeyKnown(): Promise<boolean>;
  /** Keeps the person's key for a database; an empty one forgets it. */
  databaseKeySet(sourceId: string, key: string): Promise<void>;
  /** The databases a key is kept for. */
  databaseKeysKnown(): Promise<string[]>;
  /** The models the provider offers the key. */
  aiModels(settings: AiSettings): Promise<AiModel[]>;
  aiSemanticStatus(settings: AiSettings): Promise<AiSemanticStatus>;
  aiSemanticIndex(settings: AiSettings, limit: number): Promise<AiSemanticStatus>;
  aiSummarize(settings: AiSettings, itemId: string, language: string): Promise<AiAnswer>;
  aiAsk(settings: AiSettings, attachmentId: string, question: string, language: string): Promise<AiAnswer>;
  aiTransform(settings: AiSettings, task: AiTask, text: string, language: string): Promise<AiAnswer>;
  /** An answer from the whole library, citing its sources by number. */
  aiAskLibrary(settings: AiSettings, question: string, language: string): Promise<AiLibraryAnswer>;
  /** A mind map drafted from a note: the text of a new map note. */
  aiMindmap(settings: AiSettings, title: string, text: string, language: string): Promise<string>;
  /** A mind map drafted from a paper: the paper's title and the map note's text. */
  aiMindmapItem(settings: AiSettings, itemId: string, language: string): Promise<{ title: string; text: string }>;
  /** Calls back with the user after signing in or out, or with an error. Returns an unsubscribe function. */
  onAccountChange(callback: (user: AccountUser | null, error?: string) => void): Promise<() => void>;
  formatCitations(itemIds: string[], styleId: string, html: boolean): Promise<CitationOutput>;
  trashItem(itemId: string): Promise<void>;
  listCollections(): Promise<CollectionInfo[]>;
  createCollection(name: string, parentId: string | null): Promise<CollectionInfo>;
  normalizeDoi(input: string): Promise<string | null>;
  itemTypes(): Promise<string[]>;
}

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

export const tauriBackend: Backend = {
  libraryInfo: () => invoke("library_info"),
  listItems: (filter) => invoke("list_items", { filter }),
  updateItem: (detail) => invoke("update_item", { detail }),
  setStarred: (itemId, starred) => invoke("set_starred", { itemId, starred }),
  setReadStatus: (itemId, status) => invoke("set_read_status", { itemId, status }),
  tags: () => invoke("tags"),
  setItemTags: (itemId, tags) => invoke("set_item_tags", { itemId, tags }),
  setTagColor: (name, color) => invoke("set_tag_color", { name, color }),
  renameTag: (name, newName) => invoke("rename_tag", { name, newName }),
  deleteTag: (name) => invoke("delete_tag", { name }),
  restoreItem: (itemId) => invoke("restore_item", { itemId }),
  deleteItem: (itemId) => invoke("delete_item", { itemId }),
  emptyTrash: () => invoke("empty_trash"),
  duplicateGroups: () => invoke("duplicate_groups"),
  mergeItems: (masterId, otherIds) => invoke("merge_items", { masterId, otherIds }),
  searchFields: () => invoke("search_fields"),
  savedSearches: () => invoke("saved_searches"),
  saveSearch: (search) => invoke("save_search", { search }),
  deleteSavedSearch: (searchId) => invoke("delete_saved_search", { searchId }),
  runSearch: (search) => invoke("run_search", { search }),
  countCitations: (itemIds) => invoke("count_citations", { itemIds }),
  feeds: () => invoke("feeds"),
  addFeed: (url) => invoke("add_feed", { url }),
  refreshFeeds: () => invoke("refresh_feeds"),
  renameFeed: (feedId, title) => invoke("rename_feed", { feedId, title }),
  removeFeed: (feedId) => invoke("remove_feed", { feedId }),
  feedItems: (feedId) => invoke("feed_items", { feedId }),
  setFeedItemRead: (itemId, read) => invoke("set_feed_item_read", { itemId, read }),
  markFeedRead: (feedId) => invoke("mark_feed_read", { feedId }),
  addFeedItem: (itemId, collectionId) => invoke("add_feed_item", { itemId, collectionId }),
  relatedItems: (itemId) => invoke("related_items", { itemId }),
  relateItems: (itemId, otherId) => invoke("relate_items", { itemId, otherId }),
  unrelateItems: (itemId, otherId) => invoke("unrelate_items", { itemId, otherId }),
  addItem: (item, collectionId) => invoke("add_item", { item, collectionId }),
  addByIdentifier: (input, collectionId) => invoke("add_by_identifier", { input, collectionId }),
  itemDetail: (itemId) => invoke("item_detail", { itemId }),
  recognizeIdentifier: (input) => invoke("recognize_identifier", { input }),
  citationStyles: () => invoke("citation_styles"),
  searchCitationStyles: (query, limit) => invoke("search_citation_styles", { query, limit }),
  downloadCitationStyle: (id) => invoke("download_citation_style", { id }),
  addCitationStyleFile: () => invoke("add_citation_style_file"),
  citeWordDocument: (styleId, copyName) => invoke("cite_word_document", { styleId, copyName }),
  removeCitationStyle: (id) => invoke("remove_citation_style", { id }),
  referenceFormats: () => invoke("reference_formats"),
  importReferences: (text, collectionId) => invoke("import_references", { text, collectionId }),
  exportReferencesFile: (itemIds, format) => invoke("export_references_file", { itemIds, format }),
  searchSources: () => invoke("search_sources"),
  captureSites: () => invoke("capture_sites"),
  importZotero: () => invoke("import_zotero"),
  importEndNote: () => invoke("import_endnote"),
  backUpLibrary: (name) => invoke("back_up_library", { name }),
  backUpIfDue: (folder, everyHours, keep) => invoke("back_up_if_due", { folder, everyHours, keep }),
  restoreLibrary: () => invoke("restore_library"),
  restartApp: () => invoke("restart_app"),
  browserOpen: (address) => invoke("browser_open", { address }),
  browserRead: () => invoke("browser_read"),
  browserSnapshot: () => invoke("browser_snapshot"),
  browserGo: (to) => invoke("browser_go", { to }),
  async onBrowserEvent(callback) {
    return listen<BrowserEvent>("browser", (e) => callback(e.payload));
  },
  async onConnectorAdded(callback) {
    return listen<number>("connector-added", (e) => callback(e.payload));
  },
  async onWriterQuestion(callback) {
    return listen<WriterQuestion>("connector-writer", (e) => callback(e.payload));
  },
  answerWriter: (id, allow) => invoke("connector_writer_answer", { id, allow }),
  connectedWriters: () => invoke("connected_writers"),
  forgetConnectedWriters: () => invoke("forget_connected_writers"),
  wpsAddin: () => invoke("wps_addin"),
  installWpsAddin: () => invoke("install_wps_addin"),
  removeWpsAddin: () => invoke("remove_wps_addin"),
  wordAddin: () => invoke("word_addin"),
  installWordAddin: () => invoke("install_word_addin"),
  removeWordAddin: () => invoke("remove_word_addin"),
  takeShared: () => invoke("take_shared"),
  async onShared(callback) {
    return listen("share://received", () => callback());
  },
  readWebPage: (url) => invoke("read_web_page", { url }),
  searchOnline: (sourceId, text, page) => invoke("search_online", { sourceId, text, page }),
  cataloguePresets: () => invoke("catalogue_presets"),
  searchCatalogue: (catalogue, text, page) => invoke("search_catalogue", { catalogue, text, page }),
  addSearchResults: (keys, collectionId) => invoke("add_search_results", { keys, collectionId }),
  openUrl: (url) => invoke("open_url", { url }),
  searchEverything: (query, kinds, limit) => invoke("search_everything", { query, kinds, limit }),
  setAttachmentText: (attachmentId, pages) => invoke("set_attachment_text", { attachmentId, pages }),
  attachmentHasText: (attachmentId) => invoke("attachment_has_text", { attachmentId }),
  vaultRoot: () => invoke("vault_root"),
  vaultChooseFolder: () => invoke("vault_choose_folder"),
  vaultRefresh: () => invoke("vault_refresh"),
  vaultNotes: () => invoke("vault_notes"),
  mindmapLayout: (text, title, style, folded) => invoke("mindmap_layout", { text, title, style, folded }),
  mindmapThemes: () => invoke("mindmap_themes"),
  mindmapMarkers: () => invoke("mindmap_markers"),
  mindmapEdit: (text, title, edit) => invoke("mindmap_edit", { text, title, edit }),
  newMindmapText: (title) => invoke("new_mindmap_text", { title }),
  mindmapSvg: (text, title, style, folded) => invoke("mindmap_svg", { text, title, style, folded }),
  mindmapExport: (text, title, format, png) => invoke("mindmap_export", { text, title, format, png }),
  vaultImportMindmap: (folder) => invoke("vault_import_mindmap", { folder }),
  vaultCanvases: () => invoke("vault_canvases"),
  vaultReadCanvas: (path) => invoke("vault_read_canvas", { path }),
  vaultWriteCanvas: (path, board) => invoke("vault_write_canvas", { path, board }),
  vaultCreateCanvas: (folder, name) => invoke("vault_create_canvas", { folder, name }),
  vaultDrawings: () => invoke("vault_drawings"),
  vaultReadDrawing: (path) => invoke("vault_read_drawing", { path }),
  vaultWriteDrawing: (path, page) => invoke("vault_write_drawing", { path, page }),
  drawingErase: (items, path, radius) => invoke("drawing_erase", { items, path, radius }),
  vaultCreateDrawing: (folder, name, paper) => invoke("vault_create_drawing", { folder, name, paper }),
  vaultAddFile: (folder, name, data) => invoke("vault_add_file", { folder, name, data }),
  vaultDrawingSvg: (path) => invoke("vault_drawing_svg", { path }),
  drawingExport: (path, format, png) => invoke("drawing_export", { path, format, png }),
  obsidianPlugins: () => invoke("obsidian_plugins"),
  setObsidianPluginEnabled: (id, enabled) => invoke("set_obsidian_plugin_enabled", { id, enabled }),
  obsidianPluginCode: (id) => invoke("obsidian_plugin_code", { id }),
  obsidianMetadata: () => invoke("obsidian_metadata"),
  saveObsidianPluginData: (id, data) => invoke("save_obsidian_plugin_data", { id, data }),
  pluginFetch: (url, method, headers, body) => invoke("plugin_fetch", { url, method, headers, body }),
  communityPlugins: () => invoke("community_plugins"),
  downloadTranslators: () => invoke("download_translators"),
  translatorsStatus: () => invoke("translators_status"),
  removeTranslators: () => invoke("remove_translators"),
  webTranslators: () => invoke("web_translators"),
  translatorCatalog: () => invoke("translator_catalog"),
  translatorCode: (id) => invoke("translator_code", { id }),
  readTranslatedItems: (json, pageUrl) => invoke("read_translated_items", { json, pageUrl }),
  translatorRequest: (request) => invoke("translator_request", {
    url: request.url, method: request.method ?? "GET", body: request.body ?? null,
    headers: Object.entries(request.headers ?? {}).map(([name, value]) => ({ name, value })),
  }),
  zoteroPlugins: () => invoke("zotero_plugins"),
  installZoteroPlugin: () => invoke("install_zotero_plugin"),
  setZoteroPluginEnabled: (id, enabled) => invoke("set_zotero_plugin_enabled", { id, enabled }),
  zoteroPluginCode: (id) => invoke("zotero_plugin_code", { id }),
  saveZoteroPluginPrefs: (id, prefs) => invoke("save_zotero_plugin_prefs", { id, prefs }),
  zoteroItems: () => invoke("zotero_items"),
  zoteroUpdateItem: (key, fields, tags) => invoke("zotero_update_item", { key, fields, tags }),
  zoteroCreateNote: (parent, html) => invoke("zotero_create_note", { parent, html }),
  zoteroUpdateNote: (key, html) => invoke("zotero_update_note", { key, html }),
  installObsidianPlugin: (repo) => invoke("install_obsidian_plugin", { repo }),
  vaultDiagrams: () => invoke("vault_diagrams"),
  vaultReadDiagram: (path) => invoke("vault_read_diagram", { path }),
  vaultWriteDiagram: (path, xml, svg) => invoke("vault_write_diagram", { path, xml, svg }),
  diagramView: (xml, page) => invoke("diagram_view", { xml, page }),
  diagramChange: (xml, page, change) => invoke("diagram_change", { xml, page, change }),
  diagramShapes: () => invoke("diagram_shapes"),
  diagramPageSvg: (xml, page) => invoke("diagram_page_svg", { xml, page }),
  diagramExport: (name, xml, page, format, png) => invoke("diagram_export", { name, xml, page, format, png }),
  vaultCreateDiagram: (folder, name) => invoke("vault_create_diagram", { folder, name }),
  vaultFolders: () => invoke("vault_folders"),
  vaultRead: (path) => invoke("vault_read", { path }),
  vaultWrite: (path, text) => invoke("vault_write", { path, text }),
  vaultCreateNote: (folder, title) => invoke("vault_create_note", { folder, title }),
  vaultCreateFolder: (path) => invoke("vault_create_folder", { path }),
  vaultRename: (path, newPath) => invoke("vault_rename", { path, newPath }),
  vaultDelete: (path) => invoke("vault_delete", { path }),
  vaultOutline: (path) => invoke("vault_outline", { path }),
  vaultSwitcher: (query, limit) => invoke("vault_switcher", { query, limit }),
  vaultLinkCompletions: (from, query, limit) => invoke("vault_link_completions", { from, query, limit }),
  vaultTags: () => invoke("vault_tags"),
  vaultSearch: (query, limit) => invoke("vault_search", { query, limit }),
  vaultRender: (path, text) => invoke("vault_render", { path, text }),
  vaultResolve: (target, from) => invoke("vault_resolve", { target, from }),
  vaultFile: (path) => invoke("vault_file", { path }),
  vaultSetProperties: (path, properties) => invoke("vault_set_properties", { path, properties }),
  vaultSettings: () => invoke("vault_settings"),
  vaultSetSettings: (settings) => invoke("vault_set_settings", { settings }),
  vaultTemplates: () => invoke("vault_templates"),
  vaultTemplateText: (template, title, now) => invoke("vault_template_text", { template, title, now }),
  vaultCreateFromTemplate: (folder, title, template, now) => invoke("vault_create_from_template", { folder, title, template, now }),
  vaultDailyNote: (now) => invoke("vault_daily_note", { now }),
  vaultVersions: (path) => invoke("vault_versions", { path }),
  vaultVersion: (path, time) => invoke("vault_version", { path, time }),
  vaultCompareVersion: (path, time) => invoke("vault_compare_version", { path, time }),
  vaultRestoreVersion: (path, time) => invoke("vault_restore_version", { path, time }),
  vaultGraph: (center, depth) => invoke("vault_graph", { center, depth }),
  vaultExportDocx: (path) => invoke("vault_export_docx", { path }),
  vaultExportLatex: (path) => invoke("vault_export_latex", { path }),
  citationCompletions: (query, limit) => invoke("citation_completions", { query, limit }),
  pickPdfFiles: () => invoke("pick_pdf_files"),
  readPickedFile: (path) => invoke("read_picked_file", { path }),
  importPdf: (path, text, title, collectionId) => invoke("import_pdf", { path, text, title, collectionId }),
  attachments: (itemId) => invoke("attachments", { itemId }),
  attachFiles: (itemId) => invoke("attach_files", { itemId }),
  findAvailablePdf: (itemId) => invoke("find_available_pdf", { itemId }),
  readAttachment: (attachmentId) => invoke("read_attachment", { attachmentId }),
  editPdfPages: (attachmentId, edit) => invoke("edit_pdf_pages", { attachmentId, edit }),
  exportAnnotatedPdf: (attachmentId) => invoke("export_annotated_pdf", { attachmentId }),
  pdfFormFields: (attachmentId) => invoke("pdf_form_fields", { attachmentId }),
  fillPdfForm: (attachmentId, values) => invoke("fill_pdf_form", { attachmentId, values }),
  openAttachment: (attachmentId) => invoke("open_attachment", { attachmentId }),
  removeAttachment: (attachmentId) => invoke("remove_attachment", { attachmentId }),
  annotations: (attachmentId) => invoke("annotations", { attachmentId }),
  addAnnotation: (annotation) => invoke("add_annotation", { annotation }),
  removeAnnotation: (annotationId) => invoke("remove_annotation", { annotationId }),
  updateAnnotation: (annotationId, color, comment) => invoke("update_annotation", { annotationId, color, comment }),
  annotationEmbed: (annotationId) => invoke("annotation_embed", { annotationId }),
  itemLink: (itemId) => invoke("item_link", { itemId }),
  citationMarker: (itemIds) => invoke("citation_marker", { itemIds }),
  textRecognitionAvailable: () => invoke("text_recognition_available"),
  // In base64: Android's bridge does not carry raw request bodies.
  recognizePageText: (png) => invoke("recognize_page_text", { png }),
  noteTextRecognition: (note) => invoke("text_recognition_note", { note }),
  recognizePageLines: (png) => invoke("recognize_page_lines", { png }),
  addTextLayer: (attachmentId, pages) => invoke("add_text_layer", { attachmentId, pages }),
  annotationLocation: (annotationId) => invoke("annotation_location", { annotationId }),
  resolveItemReference: (reference) => invoke("resolve_item_reference", { reference }),
  attachmentInfo: (attachmentId) => invoke("attachment_info", { attachmentId }),
  vaultCreateLiteratureNote: (itemId) => invoke("vault_create_literature_note", { itemId }),
  vaultItemMentions: (itemId) => invoke("vault_item_mentions", { itemId }),
  vaultLinkSuggestions: (path, limit) => invoke("vault_link_suggestions", { path, limit }),
  accountAvailable: () => invoke("account_available"),
  accountStatus: () => invoke("account_status"),
  accountSignIn: () => invoke("account_sign_in"),
  accountSignOut: () => invoke("account_sign_out"),
  syncStatus: () => invoke("sync_status"),
  syncNow: (target) => invoke("sync_now", { target }),
  syncPickFolder: () => invoke("sync_pick_folder"),
  webdavPasswordSet: (password) => invoke("webdav_password_set", { password }),
  webdavPasswordKnown: () => invoke("webdav_password_known"),
  seafilePasswordSet: (password) => invoke("seafile_password_set", { password }),
  seafilePasswordKnown: () => invoke("seafile_password_known"),
  s3SecretSet: (secret) => invoke("s3_secret_set", { secret }),
  s3SecretKnown: () => invoke("s3_secret_known"),
  syncPassphraseSet: (passphrase) => invoke("sync_passphrase_set", { passphrase }),
  syncPassphraseKnown: () => invoke("sync_passphrase_known"),
  aiKeySet: (key) => invoke("ai_key_set", { key }),
  aiKeyKnown: () => invoke("ai_key_known"),
  databaseKeySet: (sourceId, key) => invoke("database_key_set", { sourceId, key }),
  databaseKeysKnown: () => invoke("database_keys_known"),
  aiModels: (settings) => invoke("ai_models", { settings }),
  aiSemanticStatus: (settings) => invoke("ai_semantic_status", { settings }),
  aiSemanticIndex: (settings, limit) => invoke("ai_semantic_index", { settings, limit }),
  aiSummarize: (settings, itemId, language) => invoke("ai_summarize", { settings, itemId, language }),
  aiAsk: (settings, attachmentId, question, language) => invoke("ai_ask", { settings, attachmentId, question, language }),
  aiTransform: (settings, task, text, language) => invoke("ai_transform", { settings, task, text, language }),
  aiMindmap: (settings, title, text, language) => invoke("ai_mindmap", { settings, title, text, language }),
  aiAskLibrary: (settings, question, language) => invoke("ai_ask_library", { settings, question, language }),
  aiMindmapItem: (settings, itemId, language) => invoke("ai_mindmap_item", { settings, itemId, language }),
  async onAccountChange(callback) {
    const offChange = await listen<AccountUser | null>("account-changed", (e) => callback(e.payload));
    const offError = await listen<string>("account-error", (e) => callback(null, e.payload));
    return () => {
      offChange();
      offError();
    };
  },
  formatCitations: (itemIds, styleId, html) => invoke("format_citations", { itemIds, styleId, html }),
  trashItem: (itemId) => invoke("trash_item", { itemId }),
  listCollections: () => invoke("list_collections"),
  createCollection: (name, parentId) => invoke("create_collection", { name, parentId }),
  normalizeDoi: (input) => invoke("normalize_doi", { input }),
  itemTypes: () => invoke("item_types"),
};

export const ITEM_TYPES = [
  "journal-article",
  "conference-paper",
  "book",
  "book-chapter",
  "thesis",
  "report",
  "preprint",
  "patent",
  "standard",
  "dataset",
  "webpage",
  "other",
];

/** A simplified version of the core's DOI parser, for the browser stand-in. */
export function normalizeDoiLocally(input: string): string | null {
  let s = input.trim();
  for (;;) {
    const m = /^(https?:\/\/(dx\.)?doi\.org\/|(dx\.)?doi\.org\/|doi[:：]|doi )\s*/i.exec(s);
    if (!m) break;
    s = s.slice(m[0].length);
  }
  try {
    s = decodeURIComponent(s);
  } catch {
    // keep as typed
  }
  s = s.replace(/[.,;:'"。，；]+$/, "");
  return /^10\.\d{4,9}(\.\d+)*\/\S+$/.test(s) ? s.toLowerCase() : null;
}

/** A rough version of the core's identifier recognition, for the browser stand-in. */
export function recognizeIdentifierLocally(input: string): IdentifierKind | null {
  const text = input.trim();
  if (/^(arxiv:|https?:\/\/arxiv\.org\/(abs|pdf)\/)?\d{4}\.\d{4,5}(v\d+)?(\.pdf)?$/i.test(text)) return "arxiv";
  if (normalizeDoiLocally(text)) return "doi";
  if (/^(pmid:?\s*\d{1,8}|\d{7,8})$/i.test(text)) return "pmid";
  const isbn = text.replace(/^isbn(-1[03])?:?\s*/i, "").replace(/[-\s]/g, "");
  if (/^(97[89]\d{10}|\d{9}[\dX])$/i.test(isbn)) return "isbn";
  return null;
}

const CJK = /[\u3040-\u30ff\u3400-\u9fff\uac00-\ud7af]/;

function shortAuthors(names: string[]): string {
  if (names.length === 0) return "";
  const family = names.map((n) => (n.includes(",") ? n.split(",")[0] : (n.trim().split(/\s+/).pop() ?? "")).trim());
  const cjk = CJK.test(names[0]);
  const first = cjk ? names[0].trim() : family[0];
  if (names.length === 1) return first;
  if (names.length === 2) return cjk ? `${first}、${names[1].trim()}` : `${first} and ${family[1]}`;
  return cjk ? `${first} 等` : `${first} et al.`;
}

/** Keeps everything in memory; for the browser preview and tests. */
export function memoryBackend(): Backend {
  interface Stored {
    detail: ItemDetail;
    collections: string[];
    trashed: boolean;
    dateAdded: number;
  }
  let items: Stored[] = [];
  let translatorStatus: TranslatorStatus = { count: 0, downloaded: null };
  const translated = new Map<string, SearchHit>();
  const browserListeners = new Set<(event: BrowserEvent) => void>();
  const browserHistory: string[] = [];
  let browserPosition = -1;
  let browserPage = { url: "", title: "", html: "" };
  const browserTell = (event: BrowserEvent) => browserListeners.forEach((callback) => callback(event));
  const browserShow = (url: string) => {
    const title = new URL(url).hostname;
    const escaped = title.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
    browserPage = { url, title, html: `<html><head><title>${escaped}</title></head><body><h1>${escaped}</h1></body></html>` };
    browserTell({ kind: "navigating", url });
    browserTell({ kind: "showing", url, title });
    if (translatorStatus.count) browserTell({ kind: "html", url, html: browserPage.html });
  };
  const collections: CollectionInfo[] = [];
  const tagColors = new Map<string, string | null>();
  /** Each item's related items, kept both ways. */
  const relations = new Map<string, Set<string>>();
  let next = 0;
  const id = () => `mem-${++next}`;
  const live = () => items.filter((i) => !i.trashed);
  const find = (itemId: string) => {
    const item = items.find((i) => i.detail.id === itemId);
    if (!item) throw new Error(`item ${itemId} not found`);
    return item;
  };
  const summary = ({ detail: d, dateAdded }: Stored): ItemSummary => ({
    id: d.id,
    itemType: d.itemType,
    title: d.title,
    authors: shortAuthors(d.authors),
    containerTitle: d.containerTitle,
    year: d.year,
    doi: d.doi,
    starred: d.starred,
    readStatus: d.readStatus,
    tags: d.tags,
    hasPdf: false,
    dateAdded,
  });
  const checkedDoi = (input: string | null, itemId: string | null): string | null => {
    if (!input || !input.trim()) return null;
    const doi = normalizeDoiLocally(input);
    if (!doi) throw new Error(`not a DOI: "${input}"`);
    if (live().some((i) => i.detail.doi === doi && i.detail.id !== itemId)) {
      throw new Error(`an item with DOI ${doi} is already in the library`);
    }
    return doi;
  };
  const byName = (a: string, b: string) => a.toLowerCase().localeCompare(b.toLowerCase()) || a.localeCompare(b);

  const store = (item: NewItem, collectionId: string | null): Stored => {
    const title = item.title.trim();
    if (!title) throw new Error("An item needs a title.");
    const stored: Stored = {
      detail: {
        id: id(),
        itemType: item.itemType || "journal-article",
        title,
        authors: item.authors.map((a) => a.trim()).filter(Boolean),
        editors: [],
        containerTitle: item.containerTitle?.trim() || null,
        containerShortTitle: null,
        year: item.year,
        date: null,
        volume: null,
        issue: null,
        pages: null,
        publisher: null,
        place: null,
        edition: null,
        number: null,
        doi: checkedDoi(item.doi, null),
        isbn: null,
        issn: null,
        arxivId: null,
        pmid: null,
        url: null,
        abstractNote: null,
        extra: null,
        keywords: [],
        language: null,
        starred: false,
        readStatus: "unread",
        tags: [],
      },
      collections: collectionId ? [collectionId] : [],
      trashed: false,
      dateAdded: Date.now() + next,
    };
    items = [...items, stored];
    return stored;
  };

  if (typeof window !== "undefined" && !isTauri() && new URLSearchParams(window.location.search).has("demo")) {
    const collection: CollectionInfo = { id: id(), name: "Research reading", parentId: null, position: 0 };
    collections.push(collection);
    const samples: (NewItem & { tags: string[] })[] = [
      { itemType: "journal-article", title: "A Mathematical Theory of Communication", authors: ["Shannon, Claude E."], containerTitle: "Bell System Technical Journal", year: 1948, doi: "10.1002/j.1538-7305.1948.tb01338.x", tags: ["Information theory"] },
      { itemType: "book", title: "Silent Spring", authors: ["Carson, Rachel"], containerTitle: null, year: 1962, doi: null, tags: ["Ecology"] },
      { itemType: "conference-paper", title: "Designing collaborative reading spaces for research teams: a longitudinal study of multilingual annotation, shared bibliographies, and knowledge discovery across disciplines", authors: ["Lin, Mei", "Morgan, Alex"], containerTitle: "Conference on Digital Libraries", year: 2024, doi: null, tags: ["Research tools"] },
      { itemType: "thesis", title: "城市绿地与热岛效应：多尺度遥感分析", authors: ["陈思远", "李明"], containerTitle: "环境科学研究", year: 2023, doi: null, tags: ["Ecology", "遥感"] },
      { itemType: "dataset", title: "全球城市气候观测数据集（2000—2024）", authors: ["王晓涵", "Chen, Lin"], containerTitle: "Urban Climate Archive", year: 2024, doi: null, tags: ["Data", "遥感"] },
      { itemType: "journal-article", title: "The FAIR Guiding Principles for scientific data management and stewardship", authors: ["Wilkinson, Mark D.", "Dumontier, Michel", "Aalbersberg, IJsbrand Jan"], containerTitle: "Scientific Data", year: 2016, doi: "10.1038/sdata.2016.18", tags: ["Data", "Research tools"] },
    ];
    samples.forEach((sample, index) => {
      const stored = store(sample, collection.id);
      stored.detail.tags = sample.tags;
      stored.detail.starred = index === 0;
      stored.detail.readStatus = index === 1 ? "read" : index === 2 ? "reading" : "unread";
      for (const tag of sample.tags) tagColors.set(tag, tag === "Ecology" ? "#0d9488" : null);
    });
  }

  return {
    async libraryInfo() {
      return { coreVersion: "browser preview", location: "memory" };
    },
    async listItems(filter) {
      return items
        .filter(
          ({ detail: d, collections: c, trashed }) =>
            trashed === !!filter.trashed &&
            (!filter.collectionId || c.includes(filter.collectionId)) &&
            (!filter.tag || d.tags.includes(filter.tag)) &&
            (!filter.starredOnly || d.starred) &&
            (!filter.readStatus || d.readStatus === filter.readStatus),
        )
        .reverse()
        .map(summary);
    },
    async addItem(item, collectionId) {
      return summary(store(item, collectionId));
    },
    async updateItem(detail) {
      const stored = find(detail.id);
      const title = detail.title.trim();
      if (!title) throw new Error("An item needs a title.");
      const { starred, readStatus, tags } = stored.detail;
      stored.detail = { ...detail, title, doi: checkedDoi(detail.doi, detail.id), starred, readStatus, tags };
      return stored.detail;
    },
    async setStarred(itemId, starred) {
      find(itemId).detail.starred = starred;
    },
    async setReadStatus(itemId, status) {
      find(itemId).detail.readStatus = status;
    },
    async tags() {
      return [...tagColors.entries()]
        .map(([name, color]) => ({ name, color, count: live().filter((i) => i.detail.tags.includes(name)).length }))
        .sort((a, b) => byName(a.name, b.name));
    },
    async setItemTags(itemId, tags) {
      const names = [...new Set(tags.map((t) => t.trim()).filter(Boolean))].sort(byName);
      for (const name of names) if (!tagColors.has(name)) tagColors.set(name, null);
      find(itemId).detail.tags = names;
    },
    async setTagColor(name, color) {
      tagColors.set(name.trim(), color ? color.toLowerCase() : null);
    },
    async renameTag(name, newName) {
      const to = newName.trim();
      if (!to || to === name) return;
      if (!tagColors.has(to)) tagColors.set(to, tagColors.get(name) ?? null);
      tagColors.delete(name);
      for (const { detail } of items) {
        if (detail.tags.includes(name)) detail.tags = [...new Set(detail.tags.map((t) => (t === name ? to : t)))].sort(byName);
      }
    },
    async deleteTag(name) {
      tagColors.delete(name);
      for (const { detail } of items) detail.tags = detail.tags.filter((t) => t !== name);
    },
    async restoreItem(itemId) {
      find(itemId).trashed = false;
    },
    async deleteItem(itemId) {
      find(itemId);
      items = items.filter((i) => i.detail.id !== itemId);
    },
    async emptyTrash() {
      const count = items.filter((i) => i.trashed).length;
      items = items.filter((i) => !i.trashed);
      return count;
    },
    async duplicateGroups() {
      return [];
    },
    async mergeItems(): Promise<ItemDetail> {
      throw new Error("Merging items needs the Paperloom app.");
    },
    async searchFields() {
      return [];
    },
    async savedSearches() {
      return [];
    },
    async saveSearch(): Promise<SavedSearch> {
      throw new Error("Saved searches need the Paperloom app.");
    },
    async deleteSavedSearch() {},
    async runSearch() {
      return [];
    },
    async countCitations() {
      return 0;
    },
    async feeds() {
      return [];
    },
    async addFeed(): Promise<FeedInfo> {
      throw new Error("Feeds need the Paperloom app.");
    },
    async refreshFeeds() {
      return [];
    },
    async renameFeed() {},
    async removeFeed() {},
    async feedItems() {
      return [];
    },
    async setFeedItemRead() {},
    async markFeedRead() {},
    async addFeedItem(): Promise<ItemSummary> {
      throw new Error("Feeds need the Paperloom app.");
    },
    async relatedItems(itemId) {
      const ids = relations.get(itemId) ?? new Set<string>();
      return live()
        .filter((i) => ids.has(i.detail.id))
        .reverse()
        .map(summary);
    },
    async relateItems(itemId, otherId) {
      if (itemId === otherId) throw new Error("An item cannot be related to itself.");
      find(itemId);
      find(otherId);
      for (const [a, b] of [
        [itemId, otherId],
        [otherId, itemId],
      ]) {
        relations.set(a, new Set([...(relations.get(a) ?? []), b]));
      }
    },
    async unrelateItems(itemId, otherId) {
      relations.get(itemId)?.delete(otherId);
      relations.get(otherId)?.delete(itemId);
    },
    async addByIdentifier() {
      throw new Error("Looking up identifiers needs the Paperloom app.");
    },
    async itemDetail(itemId) {
      return items.find((i) => i.detail.id === itemId)?.detail ?? null;
    },
    async recognizeIdentifier(input) {
      return recognizeIdentifierLocally(input);
    },
    async citationStyles() {
      return [{ id: "apa", name: "APA 7th", added: false }];
    },
    async searchCitationStyles(): Promise<StyleListing[]> {
      throw new Error("The style repository needs the Paperloom app.");
    },
    async downloadCitationStyle(): Promise<CitationStyle> {
      throw new Error("The style repository needs the Paperloom app.");
    },
    async addCitationStyleFile(): Promise<CitationStyle | null> {
      throw new Error("Style files need the Paperloom app.");
    },
    async citeWordDocument(): Promise<CitedDocument | null> {
      throw new Error("Word documents need the Paperloom app.");
    },
    async removeCitationStyle() {},
    async referenceFormats() {
      return [{ id: "ris", name: "RIS", fileExtension: "ris" }];
    },
    async importReferences() {
      throw new Error("Importing needs the Paperloom app.");
    },
    async exportReferencesFile() {
      throw new Error("Exporting needs the Paperloom app.");
    },
    async searchSources() {
      return [
        { id: "crossref", name: "Crossref", homepage: "https://search.crossref.org/", needsKey: false },
        { id: "openalex", name: "OpenAlex", homepage: "https://openalex.org/", needsKey: false },
        { id: "arxiv", name: "arXiv", homepage: "https://arxiv.org/", needsKey: false },
        { id: "pubmed", name: "PubMed", homepage: "https://pubmed.ncbi.nlm.nih.gov/", needsKey: false },
        { id: "semantic-scholar", name: "Semantic Scholar", homepage: "https://www.semanticscholar.org/", needsKey: false },
        { id: "loc", name: "Library of Congress", homepage: "https://catalog.loc.gov/", needsKey: false },
        { id: "k10plus", name: "K10plus", homepage: "https://kxp.k10plus.de/", needsKey: false },
        { id: "wos", name: "Web of Science", homepage: "https://www.webofscience.com/", needsKey: true },
        { id: "scopus", name: "Scopus", homepage: "https://www.scopus.com/", needsKey: true },
        { id: "sciencedirect", name: "ScienceDirect", homepage: "https://www.sciencedirect.com/", needsKey: true },
        { id: "ieee", name: "IEEE Xplore", homepage: "https://ieeexplore.ieee.org/", needsKey: true },
        { id: "springer", name: "Springer Nature", homepage: "https://link.springer.com/", needsKey: true },
      ];
    },
    async searchOnline() {
      throw new Error("Searching online needs the Paperloom app.");
    },
    async cataloguePresets() {
      return [
        { id: "dnb", name: "Deutsche Nationalbibliothek", url: "https://services.dnb.de/sru/dnb?version=1.1", index: "", schema: "MARC21-xml" },
        { id: "bnf", name: "Bibliothèque nationale de France", url: "https://catalogue.bnf.fr/api/SRU?version=1.2", index: "bib.anywhere", schema: "dublincore" },
      ];
    },
    async searchCatalogue() {
      throw new Error("Searching online needs the Paperloom app.");
    },
    async captureSites() {
      return [
        { id: "cnki", name: "CNKI", home: "https://www.cnki.net/" },
        { id: "wanfang", name: "Wanfang Data", home: "https://www.wanfangdata.com.cn/" },
        { id: "cqvip", name: "CQVIP", home: "https://www.cqvip.com/" },
      ];
    },
    async browserOpen(address) {
      const url = new URL(address.includes("://") ? address : `https://${address}`).href;
      if (!/^https?:/.test(url)) throw new Error("Not a web address.");
      browserHistory.splice(browserPosition + 1);
      browserHistory.push(url);
      browserPosition = browserHistory.length - 1;
      browserShow(url);
    },
    async backUpIfDue(): Promise<BackupReport | null> {
      return null;
    },
    async backUpLibrary(): Promise<BackupReport | null> {
      throw new Error("Backups need the Paperloom app.");
    },
    async restoreLibrary(): Promise<RestoreReport | null> {
      throw new Error("Backups need the Paperloom app.");
    },
    async restartApp() {
      window.location.reload();
    },
    async importZotero(): Promise<ZoteroImport | null> {
      throw new Error("Moving a Zotero library in needs the Paperloom app.");
    },
    async importEndNote(): Promise<EndNoteImport | null> {
      throw new Error("Moving an EndNote library in needs the Paperloom app.");
    },
    async browserSnapshot() {
      if (translatorStatus.count && browserPage.url) browserTell({ kind: "html", url: browserPage.url, html: browserPage.html });
    },
    async browserRead() {
      if (!browserPage.url) return;
      browserTell({ kind: "reading" });
      if (translatorStatus.count) browserTell({ kind: "html", url: browserPage.url, html: browserPage.html });
      const page = await this.readTranslatedItems(JSON.stringify([{ itemType: "webpage", title: browserPage.title, url: browserPage.url }]), browserPage.url);
      browserTell({ kind: "read", page });
    },
    async browserGo(to) {
      if (to === "back") browserPosition = Math.max(0, browserPosition - 1);
      else if (to === "forward") browserPosition = Math.min(browserHistory.length - 1, browserPosition + 1);
      const url = browserHistory[browserPosition];
      if (url) browserShow(url);
    },
    async onBrowserEvent(callback) {
      browserListeners.add(callback);
      return () => { browserListeners.delete(callback); };
    },
    async onConnectorAdded() {
      return () => undefined;
    },
    async onWriterQuestion() {
      return () => undefined;
    },
    async answerWriter() {},
    async connectedWriters() {
      return [];
    },
    async forgetConnectedWriters() {},
    async wpsAddin() {
      return { available: false, installed: null, version: "", folder: "" };
    },
    async installWpsAddin() {
      return { available: false, installed: null, version: "", folder: "" };
    },
    async removeWpsAddin() {
      return { available: false, installed: null, version: "", folder: "" };
    },
    async wordAddin() {
      return { available: false, installed: null, version: "" };
    },
    async installWordAddin() {
      return { available: false, installed: null, version: "" };
    },
    async removeWordAddin() {
      return { available: false, installed: null, version: "" };
    },
    async takeShared() {
      return { texts: [], pdfs: [], references: [], unsupported: [], opened: false };
    },
    async onShared() {
      return () => undefined;
    },
    async readWebPage() {
      throw new Error("Reading web pages needs the Paperloom app.");
    },
    async addSearchResults(keys, collectionId) {
      const addedIds: string[] = [];
      let duplicates = 0;
      for (const key of keys) {
        const hit = translated.get(key);
        if (!hit) throw new Error("This search result is no longer available.");
        const detail = hit.detail;
        if (detail.doi && live().some((item) => item.detail.doi === detail.doi)) { duplicates++; continue; }
        const saved = store({ itemType: detail.itemType, title: detail.title, authors: detail.authors, containerTitle: detail.containerTitle, year: detail.year, doi: detail.doi }, collectionId);
        saved.detail = { ...detail, id: saved.detail.id };
        addedIds.push(saved.detail.id);
      }
      return { format: "search", added: addedIds.length, duplicates, skipped: 0, problems: [], addedIds };
    },
    async openUrl(url) {
      window.open(url, "_blank", "noopener");
    },
    ...memoryVault(),
    async downloadTranslators() {
      translatorStatus = { count: 1, downloaded: new Date().toISOString().slice(0, 10) };
      return 1;
    },
    async translatorsStatus() { return { ...translatorStatus }; },
    async removeTranslators() { translatorStatus = { count: 0, downloaded: null }; },
    async webTranslators() {
      return translatorStatus.count ? [{ id: "sample", label: "Sample metadata", target: "", priority: 100 }] : [];
    },
    async translatorCatalog() { return this.webTranslators(); },
    async translatorCode(translatorId) {
      if (translatorId !== "sample" || !translatorStatus.count) throw new Error("Translator not found.");
      return JSON.stringify({ translatorID: "sample", label: "Sample metadata", target: "", priority: 100, translatorType: 4 }) + `
        function detectWeb(doc) { return doc.title ? "webpage" : false; }
        function doWeb(doc, url) { const item = new Zotero.Item("webpage"); item.title = doc.title; item.url = url; item.complete(); }
      `;
    },
    async translatorRequest(request) { return { status: 200, text: "", url: request.url }; },
    async readTranslatedItems(json, pageUrl) {
      const values = JSON.parse(json) as Record<string, unknown>[];
      const hits: SearchHit[] = [];
      for (const value of values) {
        if (typeof value.title !== "string" || !value.title.trim()) continue;
        const string = (key: string) => typeof value[key] === "string" ? value[key] as string : null;
        const creators = (value.creators ?? []) as { firstName?: string; lastName?: string; name?: string }[];
        const authors = creators.map((c) => c.name ?? (/[\u3400-\u9fff]/.test((c.lastName ?? "") + (c.firstName ?? ""))
          ? (c.lastName ?? "") + (c.firstName ?? "") : [c.firstName, c.lastName].filter(Boolean).join(" ")));
        const saved = store({ itemType: ({ journalArticle: "journal-article", bookSection: "book-chapter", conferencePaper: "conference-paper" } as Record<string, string>)[string("itemType") ?? ""] ?? string("itemType") ?? "webpage", title: value.title, authors, containerTitle: string("publicationTitle"), year: Number(string("date")?.slice(0, 4)) || null, doi: string("DOI") }, null);
        items = items.filter((item) => item !== saved);
        const detail = { ...saved.detail, id: "", date: string("date"), url: string("url") ?? pageUrl, abstractNote: string("abstractNote"), extra: string("extra"), language: string("language"), volume: string("volume"), issue: string("issue"), pages: string("pages"), publisher: string("publisher"), place: string("place"), isbn: string("ISBN"), issn: string("ISSN"), tags: ((value.tags ?? []) as (string | { tag: string })[]).map((tag) => typeof tag === "string" ? tag : tag.tag) };
        const pdf = ((value.attachments ?? []) as { url?: string; mimeType?: string }[]).find((a) => a.mimeType === "application/pdf" || a.url?.endsWith(".pdf"));
        const hit: SearchHit = { key: id(), detail, pdfUrl: pdf?.url ?? null, citedBy: null, libraryItemId: null };
        translated.set(hit.key, hit);
        hits.push(hit);
      }
      return { hits, total: hits.length, hasMore: false };
    },

    async searchEverything(query) {
      const words = query.trim().toLowerCase();
      if (!words) return [];
      return items
        .filter((i) => !i.trashed && [i.detail.title, ...i.detail.authors, i.detail.abstractNote ?? ""].join(" ").toLowerCase().includes(words))
        .map((i) => ({
          kind: "item" as const,
          title: i.detail.title,
          snippetHtml: "",
          itemId: i.detail.id,
          attachmentId: null,
          pageIndex: null,
          annotationId: null,
          notePath: null,
        }));
    },
    async setAttachmentText() {},
    async attachmentHasText() {
      return true;
    },
    async pickPdfFiles() {
      throw new Error("Adding PDFs needs the Paperloom app.");
    },
    async readPickedFile() {
      throw new Error("Adding PDFs needs the Paperloom app.");
    },
    async importPdf() {
      throw new Error("Adding PDFs needs the Paperloom app.");
    },
    async attachments() {
      return [];
    },
    async attachFiles() {
      throw new Error("Attaching files needs the Paperloom app.");
    },
    async findAvailablePdf(): Promise<FoundPdf | null> {
      throw new Error("Finding PDFs needs the Paperloom app.");
    },
    async editPdfPages(): Promise<AttachmentInfo> {
      throw new Error("Editing PDFs needs the Paperloom app.");
    },
    async exportAnnotatedPdf(): Promise<string | null> {
      throw new Error("Editing PDFs needs the Paperloom app.");
    },
    async pdfFormFields(): Promise<PdfFormField[]> {
      return [];
    },
    async fillPdfForm(): Promise<AttachmentInfo> {
      throw new Error("Editing PDFs needs the Paperloom app.");
    },
    async readAttachment() {
      throw new Error("Reading PDFs needs the Paperloom app.");
    },
    async openAttachment() {
      throw new Error("Opening files needs the Paperloom app.");
    },
    async removeAttachment() {},
    async annotations() {
      return [];
    },
    async addAnnotation() {
      throw new Error("Marking up PDFs needs the Paperloom app.");
    },
    async removeAnnotation() {},
    async updateAnnotation() {
      throw new Error("Marking up PDFs needs the Paperloom app.");
    },
    async annotationEmbed() {
      throw new Error("Embedding highlights needs the Paperloom app.");
    },
    async textRecognitionAvailable() {
      return false;
    },
    async recognizePageText() {
      return null;
    },
    async noteTextRecognition() {},
    async recognizePageLines(): Promise<RecognizedLine[]> {
      return [];
    },
    async addTextLayer(): Promise<AttachmentInfo> {
      throw new Error("Recognizing text needs the Paperloom app.");
    },
    async citationMarker(itemIds) {
      return `[${itemIds.map((id) => `@${id}`).join("; ")}]`;
    },
    async itemLink(itemId) {
      const item = items.find((i) => i.detail.id === itemId);
      return `[[@${itemId}|${item ? shortAuthors(item.detail.authors) || item.detail.title : itemId}]]`;
    },
    async annotationLocation() {
      return null;
    },
    async resolveItemReference(reference) {
      const id = reference.replace(/^@/, "");
      return items.some((i) => i.detail.id === id) ? id : null;
    },
    async attachmentInfo() {
      return null;
    },
    async vaultCreateLiteratureNote() {
      throw new Error("Literature notes need the Paperloom app.");
    },
    async accountAvailable() {
      return false;
    },
    async accountStatus() {
      return null;
    },
    async accountSignIn() {
      throw new Error("Signing in needs the Paperloom app.");
    },
    async accountSignOut() {},
    async syncStatus() {
      return { service: null, lastSynced: null, waiting: 0 };
    },
    async syncNow() {
      throw new Error("Syncing needs the Paperloom app.");
    },
    async syncPickFolder() {
      return null;
    },
    async webdavPasswordSet() {},
    async seafilePasswordSet() {},
    async seafilePasswordKnown() {
      return false;
    },
    async s3SecretSet() {},
    async syncPassphraseSet() {},
    async syncPassphraseKnown() {
      return false;
    },
    async s3SecretKnown() {
      return false;
    },
    async webdavPasswordKnown() {
      return false;
    },
    async aiKeySet() {},
    async aiKeyKnown() {
      return false;
    },
    async databaseKeySet() {},
    async databaseKeysKnown() {
      return [];
    },
    async aiModels(): Promise<AiModel[]> {
      throw new Error("AI needs the Paperloom app.");
    },
    async aiSemanticStatus(settings): Promise<AiSemanticStatus> {
      return { model: settings.embeddingModel ?? "", indexed: 0, total: 0 };
    },
    async aiSemanticIndex(): Promise<AiSemanticStatus> {
      throw new Error("Semantic indexing needs the Paperloom app.");
    },
    async aiSummarize(): Promise<AiAnswer> {
      throw new Error("AI needs the Paperloom app.");
    },
    async aiAsk(): Promise<AiAnswer> {
      throw new Error("AI needs the Paperloom app.");
    },
    async aiTransform(): Promise<AiAnswer> {
      throw new Error("AI needs the Paperloom app.");
    },
    async aiAskLibrary(): Promise<AiLibraryAnswer> {
      throw new Error("AI needs the Paperloom app.");
    },
    async aiMindmap(): Promise<string> {
      throw new Error("AI needs the Paperloom app.");
    },
    async aiMindmapItem(): Promise<{ title: string; text: string }> {
      throw new Error("AI needs the Paperloom app.");
    },
    async onAccountChange() {
      return () => {};
    },
    async formatCitations(itemIds) {
      // A rough APA-like stand-in; the app formats with the core's CSL engine.
      const entries = live()
        .map(summary)
        .filter((i) => itemIds.includes(i.id))
        .map((i) => `${i.authors}${i.year ? ` (${i.year})` : ""}. ${i.title}.${i.containerTitle ? ` ${i.containerTitle}.` : ""}`);
      return { entries, citation: entries.length ? "(…)" : "" };
    },
    async trashItem(itemId) {
      find(itemId).trashed = true;
    },
    async listCollections() {
      return [...collections];
    },
    async createCollection(name, parentId) {
      const trimmed = name.trim();
      if (!trimmed) throw new Error("A collection needs a name.");
      const position = collections.filter((c) => c.parentId === parentId).length;
      const created = { id: id(), name: trimmed, parentId, position };
      collections.push(created);
      return created;
    },
    async normalizeDoi(input) {
      return normalizeDoiLocally(input);
    },
    async itemTypes() {
      return [...ITEM_TYPES];
    },
    async citationCompletions(query: string, limit: number) {
      const words = query.toLowerCase().split(/\s+/).filter(Boolean);
      return items
        .filter((i) => !i.trashed)
        .filter((i) => words.every((w) => `${i.detail.title} ${i.detail.authors.join(" ")} ${i.detail.year ?? ""}`.toLowerCase().includes(w)))
        .slice(0, limit)
        .map((i) => ({ key: i.detail.id, title: i.detail.title, cite: `${i.detail.authors[0] ?? ""} ${i.detail.year ?? ""}`.trim() }));
    },
  };
}

/** A vault kept in memory for the browser preview: notes, links by title,
 * backlinks and a plain renderer. The app uses the core's vault. */
function memoryVault() {
  const notes = new Map<string, { text: string; modified: number }>([
    ["Welcome.md", { text: "# Welcome\n\nNotes are Markdown files. Link them with [[Ideas]].\n", modified: Date.now() }],
    ["Ideas.md", { text: "# Ideas\n\n- #todo read more\n", modified: Date.now() }],
  ]);
  const canvases = new Map<string, CanvasBoard>();
  const drawings = new Map<string, DrawingPage>();
  const title = (path: string) => path.replace(/^.*\//, "").replace(/\.md$/i, "");
  const info = (path: string): NoteInfo => {
    const note = notes.get(path)!;
    return { path, title: title(path), folder: path.includes("/") ? path.replace(/\/[^/]*$/, "") : "", modified: note.modified, size: note.text.length };
  };
  const resolve = (target: string) =>
    [...notes.keys()].find((p) => title(p).toLowerCase() === target.trim().replace(/\.md$/i, "").toLowerCase()) ?? null;
  const links = (text: string) => [...text.matchAll(/\[\[([^\]|#]*)[^\]]*\]\]/g)].map((m) => m[1]);
  const mentions = (match: (line: string) => boolean) =>
    [...notes.entries()].flatMap(([path, { text }]) =>
      text.split("\n").flatMap((line, n) => (match(line) ? [{ path, title: title(path), line: n, context: line.trim() }] : [])),
    );
  const escape = (text: string) => text.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]!);
  let settings: VaultSettings = {
    dailyFolder: "",
    dailyFormat: "YYYY-MM-DD",
    dailyTemplate: "",
    templatesFolder: "Templates",
    dateFormat: "YYYY-MM-DD",
    timeFormat: "HH:mm",
    citationStyle: "apa",
  };
  const pad = (n: number) => String(n).padStart(2, "0");
  const fill = (template: string, name: string, now: LocalTime) =>
    template
      .replace(/\{\{\s*title\s*\}\}/g, name)
      .replace(/\{\{\s*date\s*\}\}/g, `${now.year}-${pad(now.month)}-${pad(now.day)}`)
      .replace(/\{\{\s*time\s*\}\}/g, `${pad(now.hour)}:${pad(now.minute)}`);
  return {
    async vaultRoot() {
      return "memory";
    },
    async vaultChooseFolder() {
      throw new Error("Choosing a vault folder needs the Paperloom app.");
    },
    async vaultRefresh() {
      return false;
    },
    async vaultNotes() {
      return [...notes.keys()].sort().map(info);
    },
    async mindmapLayout(): Promise<MindMapLayout> {
      throw new Error("Mind maps need the Paperloom app.");
    },
    async mindmapThemes() {
      return [];
    },
    async mindmapMarkers() {
      return [];
    },
    async mindmapEdit(): Promise<{ text: string; focus: string }> {
      throw new Error("Mind maps need the Paperloom app.");
    },
    async newMindmapText(title: string) {
      return `---\nmindmap: logic\n---\n# ${title.trim()}\n`;
    },
    async mindmapSvg(): Promise<string> {
      throw new Error("Mind maps need the Paperloom app.");
    },
    async mindmapExport(): Promise<string | null> {
      throw new Error("Mind maps need the Paperloom app.");
    },
    async vaultImportMindmap(): Promise<NoteInfo | null> {
      throw new Error("Mind maps need the Paperloom app.");
    },
    async vaultCanvases() {
      return [...canvases.keys()].sort();
    },
    async vaultReadCanvas(path: string) {
      const board = canvases.get(path);
      if (!board) throw new Error(`${path} not found`);
      return board;
    },
    async vaultWriteCanvas(path: string, board: CanvasBoard) {
      canvases.set(path, board);
    },
    async vaultCreateCanvas(folder: string, name: string) {
      const base = name.trim().replace(/\.canvas$/i, "") || "Untitled";
      let path = folder ? `${folder}/${base}.canvas` : `${base}.canvas`;
      for (let n = 2; canvases.has(path); n++) path = folder ? `${folder}/${base} ${n}.canvas` : `${base} ${n}.canvas`;
      canvases.set(path, { cards: [], arrows: [] });
      return path;
    },
    async vaultDrawings() {
      return [...drawings.keys()].sort();
    },
    async vaultReadDrawing(path: string) {
      const page = drawings.get(path);
      if (!page) throw new Error(`${path} not found`);
      return page;
    },
    async vaultWriteDrawing(path: string, page: DrawingPage) {
      drawings.set(path, page);
    },
    async drawingErase(items: DrawingItem[]) {
      // Rubbing out parts of strokes is the app's; here strokes stay whole.
      return items;
    },
    async vaultAddFile(): Promise<string> {
      throw new Error("Recording needs the Paperloom app.");
    },
    async vaultCreateDrawing(folder: string, name: string, paper: string) {
      const base = name.trim().replace(/\.excalidraw$/i, "") || "Untitled";
      let path = folder ? `${folder}/${base}.excalidraw` : `${base}.excalidraw`;
      for (let n = 2; drawings.has(path); n++) path = folder ? `${folder}/${base} ${n}.excalidraw` : `${base} ${n}.excalidraw`;
      drawings.set(path, { items: [], paper });
      return path;
    },
    async vaultDrawingSvg(): Promise<string> {
      throw new Error("Drawings need the Paperloom app.");
    },
    async drawingExport(): Promise<string | null> {
      throw new Error("Drawings need the Paperloom app.");
    },
    async obsidianPlugins(): Promise<ObsidianPlugin[]> {
      return [];
    },
    async setObsidianPluginEnabled(): Promise<void> {
      throw new Error("Plugins need the Paperloom app.");
    },
    async obsidianPluginCode(): Promise<ObsidianPluginCode> {
      throw new Error("Plugins need the Paperloom app.");
    },
    async obsidianMetadata(): Promise<string> {
      return "{}";
    },
    async saveObsidianPluginData(): Promise<void> {
      throw new Error("Plugins need the Paperloom app.");
    },
    async pluginFetch(): Promise<PluginResponse> {
      throw new Error("Plugins need the Paperloom app.");
    },
    async zoteroPlugins(): Promise<ZoteroPlugin[]> {
      return [];
    },
    async installZoteroPlugin(): Promise<ZoteroPlugin | null> {
      throw new Error("Plugins need the Paperloom app.");
    },
    async setZoteroPluginEnabled(): Promise<void> {
      throw new Error("Plugins need the Paperloom app.");
    },
    async zoteroPluginCode(): Promise<ZoteroPluginCode> {
      throw new Error("Plugins need the Paperloom app.");
    },
    async saveZoteroPluginPrefs(): Promise<void> {
      throw new Error("Plugins need the Paperloom app.");
    },
    async zoteroItems(): Promise<string> {
      return "[]";
    },
    async zoteroUpdateItem(): Promise<void> {
      throw new Error("Plugins need the Paperloom app.");
    },
    async zoteroCreateNote(): Promise<string> {
      throw new Error("Plugins need the Paperloom app.");
    },
    async zoteroUpdateNote(): Promise<void> {
      throw new Error("Plugins need the Paperloom app.");
    },
    async communityPlugins(): Promise<CommunityPlugin[]> {
      throw new Error("Plugins need the Paperloom app.");
    },
    async installObsidianPlugin(): Promise<ObsidianPlugin> {
      throw new Error("Plugins need the Paperloom app.");
    },
    async vaultDiagrams(): Promise<string[]> {
      return [];
    },
    async vaultReadDiagram(): Promise<string> {
      throw new Error("Diagrams need the Paperloom app.");
    },
    async vaultWriteDiagram(): Promise<void> {
      throw new Error("Diagrams need the Paperloom app.");
    },
    async diagramView(): Promise<DiagramPageView> {
      throw new Error("Diagrams need the Paperloom app.");
    },
    async diagramChange(): Promise<DiagramChanged> {
      throw new Error("Diagrams need the Paperloom app.");
    },
    async diagramShapes(): Promise<DiagramShape[]> {
      return [];
    },
    async diagramPageSvg(): Promise<string> {
      throw new Error("Diagrams need the Paperloom app.");
    },
    async diagramExport(): Promise<string | null> {
      throw new Error("Diagrams need the Paperloom app.");
    },
    async vaultCreateDiagram(): Promise<string> {
      throw new Error("Diagrams need the Paperloom app.");
    },
    async vaultFolders() {
      return [...new Set([...notes.keys()].filter((p) => p.includes("/")).map((p) => p.replace(/\/[^/]*$/, "")))].sort();
    },
    async vaultRead(path: string) {
      const note = notes.get(path);
      if (!note) throw new Error(`${path} not found`);
      return note.text;
    },
    async vaultWrite(path: string, text: string) {
      const full = /\.md$/i.test(path) ? path : `${path}.md`;
      notes.set(full, { text, modified: Date.now() });
      return info(full);
    },
    async vaultCreateNote(folder: string, name: string) {
      const base = name.trim() || "Untitled";
      let n = 1;
      const pathFor = (k: number) => `${folder ? `${folder}/` : ""}${k === 1 ? base : `${base} ${k}`}.md`;
      while (notes.has(pathFor(n))) n++;
      notes.set(pathFor(n), { text: "", modified: Date.now() });
      return info(pathFor(n));
    },
    async vaultCreateFolder() {},
    async vaultRename(path: string, newPath: string) {
      const note = notes.get(path);
      if (!note) throw new Error(`${path} not found`);
      notes.delete(path);
      notes.set(newPath, note);
      return 0;
    },
    async vaultDelete(path: string) {
      notes.delete(path);
      canvases.delete(path);
      drawings.delete(path);
    },
    async vaultOutline(path: string): Promise<NoteOutline> {
      const text = notes.get(path)?.text ?? "";
      const lines = text.split("\n");
      return {
        headings: lines.flatMap((line, n) => {
          const m = /^(#{1,6})\s+(.*)$/.exec(line);
          return m ? [{ level: m[1].length, text: m[2], line: n, anchor: m[2].toLowerCase().replace(/\s+/g, "-") }] : [];
        }),
        tags: [...text.matchAll(/(?:^|\s)#([\w/-]+)/g)].map((m) => m[1]),
        aliases: [],
        properties: [],
        links: links(text).map((target) => ({ target, subpath: null, alias: null, embed: false, line: 0, resolved: resolve(target) })),
        backlinks: mentions((line) => links(line).some((t) => resolve(t) === path)).filter((m) => m.path !== path),
        unlinkedMentions: [],
      };
    },
    async vaultSwitcher(query: string, limit: number) {
      const q = query.trim().toLowerCase();
      return [...notes.keys()].filter((p) => title(p).toLowerCase().includes(q)).slice(0, limit).map(info);
    },
    async vaultLinkCompletions(_from: string, query: string, limit: number) {
      const q = query.trim().toLowerCase();
      return [...notes.keys()]
        .filter((p) => title(p).toLowerCase().includes(q))
        .slice(0, limit)
        .map((path) => ({ insert: title(path), title: title(path), path }));
    },
    async vaultTags() {
      return [];
    },
    async vaultLinkSuggestions(): Promise<LinkSuggestion[]> {
      return [];
    },
    async vaultItemMentions(itemId: string) {
      return mentions((line) => line.includes(`[[@${itemId}`) || line.includes(`![[@${itemId}`));
    },
    async vaultSearch(query: string, limit: number) {
      const q = query.trim().toLowerCase();
      return q ? mentions((line) => line.toLowerCase().includes(q)).slice(0, limit) : [];
    },
    async vaultRender(_path: string, text: string) {
      return text
        .split(/\n{2,}/)
        .map((block) => {
          const html = escape(block).replace(/\[\[([^\]|]+)(?:\|([^\]]+))?\]\]/g, (_m, target: string, alias?: string) => {
            const found = resolve(target);
            return found
              ? `<a class="wikilink" href="paperloom:note/${encodeURIComponent(found)}" data-note="${escape(found)}">${alias ?? target}</a>`
              : `<a class="wikilink unresolved" href="paperloom:new/${encodeURIComponent(target)}" data-target="${escape(target)}">${alias ?? target}</a>`;
          });
          const heading = /^(#{1,6})\s+(.*)$/.exec(html);
          return heading ? `<h${heading[1].length}>${heading[2]}</h${heading[1].length}>` : `<p>${html.replace(/\n/g, "<br>")}</p>`;
        })
        .join("\n");
    },
    async vaultResolve(target: string) {
      return resolve(target);
    },
    async vaultFile(): Promise<ArrayBuffer> {
      throw new Error("Vault files need the Paperloom app.");
    },
    async vaultSetProperties(path: string, properties: NoteProperty[]) {
      const text = notes.get(path)?.text ?? "";
      const body = text.replace(/^---\n[\s\S]*?\n---\n/, "");
      const front = properties.filter((p) => p.key.trim()).map((p) => `${p.key.trim()}: ${p.value.trim()}`);
      notes.set(path, { text: front.length ? `---\n${front.join("\n")}\n---\n${body}` : body, modified: Date.now() });
      return info(path);
    },
    async vaultSettings() {
      return { ...settings };
    },
    async vaultSetSettings(next: VaultSettings) {
      settings = { ...next };
    },
    async vaultTemplates() {
      const prefix = `${settings.templatesFolder}/`.toLowerCase();
      return [...notes.keys()].filter((p) => p.toLowerCase().startsWith(prefix)).map(info);
    },
    async vaultTemplateText(template: string, name: string, now: LocalTime) {
      const path = resolve(template.replace(/^.*\//, "")) ?? template;
      return fill(notes.get(path)?.text ?? "", name, now);
    },
    async vaultCreateFromTemplate(folder: string, name: string, template: string | null, now: LocalTime) {
      const base = name.trim() || "Untitled";
      let path = folder ? `${folder}/${base}.md` : `${base}.md`;
      for (let n = 2; notes.has(path); n++) path = folder ? `${folder}/${base} ${n}.md` : `${base} ${n}.md`;
      const source = template ? (resolve(template.replace(/^.*\//, "")) ?? template) : null;
      notes.set(path, { text: source ? fill(notes.get(source)?.text ?? "", title(path), now) : "", modified: Date.now() });
      return info(path);
    },
    async vaultDailyNote(now: LocalTime) {
      const day = `${now.year}-${pad(now.month)}-${pad(now.day)}`;
      const path = settings.dailyFolder ? `${settings.dailyFolder}/${day}.md` : `${day}.md`;
      if (!notes.has(path)) notes.set(path, { text: "", modified: Date.now() });
      return info(path);
    },
    async vaultVersions() {
      return [];
    },
    async vaultVersion(): Promise<string> {
      throw new Error("Earlier versions need the Paperloom app.");
    },
    async vaultCompareVersion(): Promise<DiffLine[]> {
      throw new Error("Earlier versions need the Paperloom app.");
    },
    async vaultRestoreVersion(): Promise<NoteInfo> {
      throw new Error("Earlier versions need the Paperloom app.");
    },
    async vaultExportDocx(): Promise<string | null> {
      throw new Error("Word documents need the Paperloom app.");
    },
    async vaultExportLatex(): Promise<string | null> {
      throw new Error("LaTeX documents need the Paperloom app.");
    },
    async vaultGraph(): Promise<NoteGraph> {
      const paths = [...notes.keys()].sort();
      const edges = paths.flatMap((path, from) =>
        links(notes.get(path)!.text).flatMap((target) => {
          const to = paths.indexOf(resolve(target) ?? "");
          return to >= 0 && to !== from ? [{ from, to }] : [];
        }),
      );
      const nodes = paths.map((path, i) => ({
        id: path,
        title: title(path),
        kind: "note" as const,
        itemId: null,
        degree: edges.filter((e) => e.from === i || e.to === i).length,
        x: 120 * Math.cos((2 * Math.PI * i) / paths.length),
        y: 120 * Math.sin((2 * Math.PI * i) / paths.length),
      }));
      return { nodes, edges };
    },
  };
}

export const backend: Backend = isTauri() ? tauriBackend : memoryBackend();

/** A file's text: UTF-8, or GB18030 for older CNKI exports. */
export async function readTextFile(file: Blob): Promise<string> {
  const bytes = await file.arrayBuffer();
  try {
    return new TextDecoder("utf-8", { fatal: true }).decode(bytes);
  } catch {
    return new TextDecoder("gb18030").decode(bytes);
  }
}
