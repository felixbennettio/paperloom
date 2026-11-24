// AI helpers shared by the views.

import type { AiSettings } from "./api";
import { browserStorage, loadPreferences } from "./preferences";

/** The AI provider, as Settings keeps it. */
export function aiSettings(): AiSettings {
  const preferences = loadPreferences(browserStorage());
  return {
    protocol: preferences.aiProtocol,
    baseUrl: preferences.aiBaseUrl,
    model: preferences.aiModel,
    embeddingModel: preferences.aiProtocol === "openai" ? preferences.aiEmbeddingModel : "",
  };
}

/** `1–5, 8`. */
export function pageRanges(pages: number[]): string {
  const ranges: string[] = [];
  let start = pages[0];
  let end = start;
  for (const page of [...pages.slice(1), Number.NaN]) {
    if (page === end + 1) {
      end = page;
      continue;
    }
    ranges.push(start === end ? String(start) : `${start}–${end}`);
    start = end = page;
  }
  return pages.length ? ranges.join(", ") : "";
}

