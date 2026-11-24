// pdf.js for reading PDFs in the web UI: the first pages' text (to
// recognise a paper), and geometry helpers for marks.

import * as pdfjs from "pdfjs-dist";
// The legacy build, as for pdf.js itself (see vite.config.ts).
import workerUrl from "pdfjs-dist/legacy/build/pdf.worker.min.mjs?url";
import type { AnnotationRect, RecognizedLine, RecognizedPage } from "./api";
import { base64Of } from "./recorder";

pdfjs.GlobalWorkerOptions.workerSrc = workerUrl;

export type PDFDocumentProxy = Awaited<ReturnType<typeof pdfjs.getDocument>["promise"]>;
export type PDFPageProxy = Awaited<ReturnType<PDFDocumentProxy["getPage"]>>;
export type PageViewport = ReturnType<PDFPageProxy["getViewport"]>;

/** Opens PDF bytes. pdf.js hands its input to a worker (which empties it),
 * so it gets a copy and the caller can read the same bytes again. */
export function openPdf(data: ArrayBuffer): Promise<PDFDocumentProxy> {
  return pdfjs.getDocument({ data: new Uint8Array(data.slice(0)) }).promise;
}

/** Reads the words in a picture of a page (a PNG, in base64); null if it
 * cannot. `problem` hears why a page could not be drawn. */
export type Recognize = ((png: string) => Promise<string | null>) & { problem?: (note: string) => void };

/** A page with fewer letters than this is a scanned picture. */
const SCANNED_LETTERS = 20;
/** Scanned pages read from their pictures per document, at most. */
const RECOGNIZED_PAGES = 300;

/** A page drawn on white as a PNG (in base64), at about 200 dots per inch
 * and no more than 2400 pixels on its long side. */
async function pagePicture(page: PDFPageProxy): Promise<string> {
  const natural = page.getViewport({ scale: 1 });
  const viewport = page.getViewport({ scale: Math.min(200 / 72, 2400 / Math.max(natural.width, natural.height)) });
  const canvas = document.createElement("canvas");
  canvas.width = Math.ceil(viewport.width);
  canvas.height = Math.ceil(viewport.height);
  const context = canvas.getContext("2d");
  if (!context) throw new Error("no 2D canvas");
  context.fillStyle = "#ffffff";
  context.fillRect(0, 0, canvas.width, canvas.height);
  await page.render({ canvasContext: context, viewport }).promise;
  const blob = await new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, "image/png"));
  if (!blob) throw new Error(`the ${canvas.width}×${canvas.height} canvas gave no PNG`);
  return base64Of(blob);
}

/** A page's own text, lines kept. */
async function ownText(page: PDFPageProxy): Promise<string> {
  const content = await page.getTextContent();
  return content.items.map((item) => ("str" in item ? item.str + (item.hasEOL ? "\n" : "") : "")).join("");
}

/** Whether a page is a scanned picture: (almost) no text of its own. */
function scanned(text: string): boolean {
  return (text.match(/\p{L}/gu)?.length ?? 0) < SCANNED_LETTERS;
}

/** Reads pages' text: their own, or for scanned pages (while `left` allows)
 * what `recognize` reads in their pictures. */
function pageReader(recognize?: Recognize) {
  let left = RECOGNIZED_PAGES;
  return async (page: PDFPageProxy): Promise<string> => {
    const own = await ownText(page);
    if (!recognize || left <= 0 || !scanned(own)) return own;
    left--;
    const picture = await pagePicture(page).catch((e: unknown) => {
      recognize.problem?.(e instanceof Error ? e.message : String(e));
      return null;
    });
    const read =
      picture &&
      (await recognize(picture).catch((e: unknown) => {
        recognize.problem?.(e instanceof Error ? e.message : String(e));
        return null;
      }));
    return read?.trim() ? read : own;
  };
}

/** The scanned pages' lines with their boxes, read from their pictures (the
 * page as shown), for an invisible text layer; up to `RECOGNIZED_PAGES`. Also
 * how many pages looked scanned, including those where no words were found. */
export async function scannedPageLines(
  doc: PDFDocumentProxy,
  readLines: (png: string) => Promise<RecognizedLine[]>,
): Promise<{ pages: RecognizedPage[]; scanned: number }> {
  const pages: RecognizedPage[] = [];
  let count = 0;
  for (let n = 1; n <= doc.numPages && count < RECOGNIZED_PAGES; n++) {
    const page = await doc.getPage(n);
    if (!scanned(await ownText(page))) continue;
    count++;
    const lines = await readLines(await pagePicture(page));
    if (lines.length > 0) pages.push({ pageIndex: n - 1, lines });
  }
  return { pages, scanned: count };
}

/** The text of the first pages (lines kept) and the title stored in the PDF;
 * scanned pages read from their pictures when `recognize` is given, so a
 * scanned paper can still be recognised. */
export async function firstPages(
  data: ArrayBuffer,
  pages = 2,
  recognize?: Recognize,
): Promise<{ text: string; title: string | null }> {
  const doc = await openPdf(data);
  try {
    const read = pageReader(recognize);
    const parts: string[] = [];
    for (let n = 1; n <= Math.min(pages, doc.numPages); n++) {
      parts.push(await read(await doc.getPage(n)));
    }
    const meta = await doc.getMetadata().catch(() => null);
    const title = (meta?.info as { Title?: unknown } | undefined)?.Title;
    return { text: parts.join("\n").slice(0, 20000), title: typeof title === "string" && title.trim() ? title.trim() : null };
  } finally {
    await doc.destroy();
  }
}

/** Every page's text, for full-text search; scanned pages read from their
 * pictures when `recognize` is given. */
export async function pageTexts(doc: PDFDocumentProxy, recognize?: Recognize): Promise<string[]> {
  const read = pageReader(recognize);
  const pages: string[] = [];
  for (let n = 1; n <= doc.numPages; n++) {
    pages.push(await read(await doc.getPage(n)));
  }
  return pages;
}

/** Reads a PDF's pages and keeps their text in the library. */
export async function indexPdf(
  attachmentId: string,
  data: ArrayBuffer,
  keep: (id: string, pages: string[]) => Promise<void>,
  recognize?: Recognize,
) {
  const doc = await openPdf(data);
  try {
    await keep(attachmentId, await pageTexts(doc, recognize));
  } finally {
    await doc.destroy();
  }
}

export interface Box {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

/**
 * Joins the many small boxes of a text selection (one per span) into one box
 * per line: boxes whose vertical middles are within half a line belong
 * together.
 */
export function mergeLines(boxes: Box[]): Box[] {
  const sorted = boxes
    .filter((b) => b.right - b.left > 0.5 && b.bottom - b.top > 0.5)
    .sort((a, b) => a.top - b.top || a.left - b.left);
  const lines: Box[] = [];
  for (const box of sorted) {
    const middle = (box.top + box.bottom) / 2;
    const line = lines.find((l) => Math.abs((l.top + l.bottom) / 2 - middle) < (l.bottom - l.top) / 2);
    if (line) {
      line.left = Math.min(line.left, box.left);
      line.right = Math.max(line.right, box.right);
      line.top = Math.min(line.top, box.top);
      line.bottom = Math.max(line.bottom, box.bottom);
    } else {
      lines.push({ ...box });
    }
  }
  return lines;
}

/** Two corner points (in any order) as a rectangle. */
export function rectFromPoints([x1, y1]: number[], [x2, y2]: number[]): AnnotationRect {
  return { x: Math.min(x1, x2), y: Math.min(y1, y2), width: Math.abs(x2 - x1), height: Math.abs(y2 - y1) };
}

export function intersects(a: AnnotationRect, b: AnnotationRect): boolean {
  return a.x < b.x + b.width && b.x < a.x + a.width && a.y < b.y + b.height && b.y < a.y + a.height;
}

/** The colors for marks; the same five as on the Apple apps. */
export const MARK_COLORS = [
  { name: "Yellow", hex: "#ffd400" },
  { name: "Red", hex: "#ff6666" },
  { name: "Green", hex: "#5fb236" },
  { name: "Blue", hex: "#2ea8e5" },
  { name: "Purple", hex: "#a28ae5" },
];

/** The two barbs of an arrowhead at `head`, pointing away from `tail`:
 * `length` long, 25° off the shaft. Works in any coordinates. */
export function arrowHead([tx, ty]: number[], [hx, hy]: number[], length: number): [number, number][] {
  const angle = Math.atan2(hy - ty, hx - tx);
  const spread = (25 * Math.PI) / 180;
  return [angle + Math.PI - spread, angle + Math.PI + spread].map((a) => [hx + length * Math.cos(a), hy + length * Math.sin(a)]);
}

/** The words stamps take, as Acrobat's standard stamps. */
export const STAMP_WORDS = ["Approved", "Not Approved", "Draft", "Final", "Confidential", "For Comment", "Reviewed"];

/** A stamp's box in PDF points, centred on a point: as wide as its word. */
export function stampRect(word: string, x: number, y: number): AnnotationRect {
  const height = 36;
  const width = Math.max(80, word.length * 0.72 * 20 + 24);
  return { x: x - width / 2, y: y - height / 2, width, height };
}

/** The size of a stamp's word that fits its box. */
export function stampFontSize(word: string, width: number, height: number): number {
  return Math.max(4, Math.min(height * 0.55, (width - 12) / (0.72 * Math.max(1, word.length))));
}

/** A handwritten signature, kept on this device: its strokes in the
 * pixels of the pad it was written on (y down). */
export interface Signature {
  width: number;
  height: number;
  strokes: { x: number; y: number }[][];
}

const SIGNATURE = "paperloom.signature";

export function loadSignature(storage: Pick<Storage, "getItem"> | null): Signature | null {
  try {
    const kept = JSON.parse(storage?.getItem(SIGNATURE) ?? "null") as Signature | null;
    return kept && kept.width > 0 && kept.height > 0 && Array.isArray(kept.strokes) && kept.strokes.length ? kept : null;
  } catch {
    return null;
  }
}

export function saveSignature(storage: Pick<Storage, "setItem" | "removeItem"> | null, signature: Signature | null): void {
  try {
    if (signature) storage?.setItem(SIGNATURE, JSON.stringify(signature));
    else storage?.removeItem(SIGNATURE);
  } catch {
    // Storage unavailable: the signature lasts as long as the page.
  }
}

/** A signature's strokes on a PDF page, `width` points wide and centred on
 * a point (PDF y grows upwards). */
export function placeSignature(signature: Signature, x: number, y: number, width: number): { x: number; y: number }[][] {
  const scale = width / signature.width;
  const height = signature.height * scale;
  return signature.strokes.map((stroke) =>
    stroke.map((p) => ({ x: x - width / 2 + p.x * scale, y: y + height / 2 - p.y * scale })),
  );
}
