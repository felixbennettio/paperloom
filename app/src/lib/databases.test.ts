import { describe, expect, it } from "vitest";
import { isCatalogueAddress, loadCatalogues, newCatalogueId, saveCatalogues } from "./databases";

function memory(): Storage {
  const items = new Map<string, string>();
  return {
    get length() {
      return items.size;
    },
    clear: () => items.clear(),
    getItem: (key) => items.get(key) ?? null,
    key: (i) => [...items.keys()][i] ?? null,
    removeItem: (key) => void items.delete(key),
    setItem: (key, value) => void items.set(key, value),
  };
}

describe("library catalogues", () => {
  it("keeps the ones the person added", () => {
    const storage = memory();
    const dnb = { id: newCatalogueId(), name: "DNB", url: "https://services.dnb.de/sru/dnb?version=1.1", index: "", schema: "MARC21-xml" };
    saveCatalogues([dnb], storage);
    expect(loadCatalogues(storage)).toEqual([dnb]);
  });

  it("drops what is broken", () => {
    const storage = memory();
    expect(loadCatalogues(storage)).toEqual([]);
    storage.setItem("paperloom.catalogues", "{not json");
    expect(loadCatalogues(storage)).toEqual([]);
    storage.setItem(
      "paperloom.catalogues",
      JSON.stringify([null, { id: "a", name: "A", url: "file:///etc" }, { id: "b", name: "B", url: "http://x.org/sru", index: 3 }]),
    );
    expect(loadCatalogues(storage)).toEqual([{ id: "b", name: "B", url: "http://x.org/sru", index: "", schema: "" }]);
    expect(loadCatalogues(null)).toEqual([]);
  });

  it("makes up distinct IDs and checks addresses", () => {
    expect(newCatalogueId()).not.toBe(newCatalogueId());
    expect(newCatalogueId()).toMatch(/^sru-/);
    expect(isCatalogueAddress(" https://a.org/sru ")).toBe(true);
    expect(isCatalogueAddress("z3950://opac.example.edu:9991/UCS01")).toBe(true);
    expect(isCatalogueAddress("ftp://a.org")).toBe(false);
  });
});
