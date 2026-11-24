// Sorting what another app shared (Android's share sheet) into what the
// importers take: reference data, identifiers, web pages.

import type { SharedText } from "./api";

/** What to do with a piece of shared text. */
export type SharedTextPlan =
  /** RIS, BibTeX, EndNote, PubMed… pasted as text. */
  | { kind: "references"; text: string }
  /** Candidates for `recognizeIdentifier`, the likeliest first; a page to
   * read if none is an identifier. */
  | { kind: "lookup"; candidates: string[]; page: string | null }
  | { kind: "nothing" };

/** Starts of the reference formats the importer reads. */
const REFERENCE_DATA = [
  /^TY {2}- /m, // RIS
  /^\s*@[A-Za-z]+\s*\{/, // BibTeX, BibLaTeX
  /^%0 /m, // EndNote tagged, Refer
  /^PMID- /m, // MEDLINE
  /^FN /m, // Web of Science
  /^\s*<\?xml[\s\S]*<records>/, // EndNote XML
];

/** Whether the text is reference data rather than a link or identifier. */
export function looksLikeReferences(text: string): boolean {
  return REFERENCE_DATA.some((pattern) => pattern.test(text));
}

const OPENING: Record<string, string> = { ")": "(", "]": "[", "}": "{", ">": "<" };

/** The web addresses in the text, in order, without trailing punctuation. */
export function findUrls(text: string): string[] {
  const urls: string[] = [];
  for (const match of text.matchAll(/https?:\/\/[^\s<>"'“”‘’]+/gi)) {
    let url = match[0].replace(/[.,;:!?。，；：！？]+$/u, "");
    // A closing bracket belongs to the address only if it opened one.
    while (/[)\]}>]$/.test(url)) {
      const close = url.slice(-1);
      const open = OPENING[close];
      if (url.split(open).length >= url.split(close).length) break;
      url = url.slice(0, -1);
    }
    if (!urls.includes(url)) urls.push(url);
  }
  return urls;
}

/** What to do with shared text: import it as references, or look up the
 * identifier it is or names, else read the page it links to. */
export function planSharedText(shared: SharedText): SharedTextPlan {
  const text = shared.text.trim();
  if (!text) return { kind: "nothing" };
  // As it came: RIS ends each record with `ER  - `.
  if (looksLikeReferences(text)) return { kind: "references", text: shared.text };
  const urls = findUrls(text);
  const candidates = [text, ...urls].filter((c, i, all) => all.indexOf(c) === i);
  return { kind: "lookup", candidates, page: urls[0] ?? null };
}
