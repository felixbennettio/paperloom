import { describe, expect, it } from "vitest";
import type { ItemSummary } from "./api";
import { filterFor, sortItems, toggleSort } from "./scope";

const item = (title: string, year: number | null, dateAdded: number): ItemSummary => ({
  id: title,
  itemType: "journal-article",
  title,
  authors: "",
  containerTitle: null,
  year,
  doi: null,
  starred: false,
  readStatus: "unread",
  tags: [],
  hasPdf: false,
  dateAdded,
});

describe("scopes", () => {
  it("become filters", () => {
    expect(filterFor({ kind: "all" })).toEqual({});
    expect(filterFor({ kind: "trash" })).toEqual({ trashed: true });
    expect(filterFor({ kind: "tag", name: "ml" })).toEqual({ tag: "ml" });
    expect(filterFor({ kind: "collection", id: "c1" })).toEqual({ collectionId: "c1" });
  });
});

describe("sorting", () => {
  const items = [item("Paper 10", 2020, 1), item("paper 9", null, 3), item("Älter", 2001, 2)];

  it("orders titles naturally and years newest first", () => {
    expect(sortItems(items, { key: "title", descending: false }).map((i) => i.title)).toEqual(["Älter", "paper 9", "Paper 10"]);
    expect(sortItems(items, { key: "year", descending: true }).map((i) => i.year)).toEqual([2020, 2001, null]);
    expect(sortItems(items, { key: "dateAdded", descending: true }).map((i) => i.dateAdded)).toEqual([3, 2, 1]);
  });

  it("flips on a second click", () => {
    const byTitle = toggleSort({ key: "dateAdded", descending: true }, "title");
    expect(byTitle).toEqual({ key: "title", descending: false });
    expect(toggleSort(byTitle, "title")).toEqual({ key: "title", descending: true });
  });
});
