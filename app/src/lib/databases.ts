import type { SruCatalogue } from "./api";
import { browserStorage } from "./preferences";

/** Sent on `window` when a database key is kept or forgotten. */
export const DATABASE_KEYS_CHANGED = "paperloom:database-keys";

/** Where to ask each database for an API key. */
export const KEY_PAGES: Record<string, string> = {
  wos: "https://developer.clarivate.com/apis/wos-starter",
  scopus: "https://dev.elsevier.com/",
  sciencedirect: "https://dev.elsevier.com/",
  ieee: "https://developer.ieee.org/",
  springer: "https://dev.springernature.com/",
};

/** Sent on `window` when the person adds or removes a library catalogue. */
export const CATALOGUES_CHANGED = "paperloom:catalogues";

const CATALOGUES_KEY = "paperloom.catalogues";

/** The library catalogues the person added, kept on this device. */
export function loadCatalogues(storage: Pick<Storage, "getItem"> | null = browserStorage()): SruCatalogue[] {
  let saved: unknown;
  try {
    saved = JSON.parse(storage?.getItem(CATALOGUES_KEY) ?? "[]");
  } catch {
    return [];
  }
  if (!Array.isArray(saved)) return [];
  const text = (value: unknown) => (typeof value === "string" ? value : "");
  return saved.flatMap((entry: unknown) => {
    const c = (entry ?? {}) as Record<string, unknown>;
    const catalogue = { id: text(c.id), name: text(c.name), url: text(c.url), index: text(c.index), schema: text(c.schema) };
    return catalogue.id && catalogue.name.trim() && isCatalogueAddress(catalogue.url) ? [catalogue] : [];
  });
}

export function saveCatalogues(
  catalogues: SruCatalogue[],
  storage: Pick<Storage, "setItem"> | null = browserStorage(),
): void {
  try {
    storage?.setItem(CATALOGUES_KEY, JSON.stringify(catalogues));
  } catch {
    // Storage full or unavailable: the catalogues last for this session.
  }
  if (typeof window !== "undefined") window.dispatchEvent(new Event(CATALOGUES_CHANGED));
}

/** An SRU (web) or Z39.50 (`z3950://host:port/database`) address. */
export function isCatalogueAddress(text: string): boolean {
  return /^(https?|z3950|z39\.50|tcp):\/\/\S+$/i.test(text.trim());
}

/** An ID for a catalogue being added, unlike the built-in sources' and
 * the presets'. */
export function newCatalogueId(): string {
  return `sru-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}
