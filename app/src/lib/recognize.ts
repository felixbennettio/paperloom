import { backend } from "./api";
import type { Recognize } from "./pdf";

let available: Promise<boolean> | null = null;

/** Reads text from pictures of scanned pages with the app, when this
 * computer can (asked once); otherwise undefined. */
export async function pageRecognizer(): Promise<Recognize | undefined> {
  available ??= backend.textRecognitionAvailable().catch(() => false);
  if (!(await available)) return undefined;
  const read: Recognize = (png) => backend.recognizePageText(png);
  read.problem = (note) => void backend.noteTextRecognition(note).catch(() => undefined);
  return read;
}
