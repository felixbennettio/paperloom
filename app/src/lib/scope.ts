// What the item list shows, and how it is ordered.

import type { ItemFilter, ItemSummary } from "./api";

export type Scope =
  | { kind: "all" }
  | { kind: "starred" }
  | { kind: "unread" }
  | { kind: "trash" }
  /** Items that look like the same work, to merge. */
  | { kind: "duplicates" }
  /** The items a saved search finds. */
  | { kind: "search"; id: string }
  /** What a feed announced (not the library's items). */
  | { kind: "feed"; id: string }
  | { kind: "collection"; id: string }
  | { kind: "tag"; name: string };

export const ALL: Scope = { kind: "all" };

export function filterFor(scope: Scope): ItemFilter {
  switch (scope.kind) {
    case "all":
      return {};
    case "starred":
      return { starredOnly: true };
    case "unread":
      return { readStatus: "unread" };
    case "trash":
      return { trashed: true };
    case "duplicates":
    case "search":
    case "feed":
      return {};
    case "collection":
      return { collectionId: scope.id };
    case "tag":
      return { tag: scope.name };
  }
}

export function sameScope(a: Scope, b: Scope): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

export type SortKey = "dateAdded" | "title" | "authors" | "year" | "citations";

export interface Sort {
  key: SortKey;
  descending: boolean;
}

/** Items in a sort order; titles and names compare the way people read them. */
export function sortItems(items: ItemSummary[], { key, descending }: Sort): ItemSummary[] {
  const text = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });
  const compare = (a: ItemSummary, b: ItemSummary): number => {
    switch (key) {
      case "dateAdded":
        return a.dateAdded - b.dateAdded;
      case "title":
        return text.compare(a.title, b.title);
      case "authors":
        return text.compare(a.authors, b.authors);
      case "year":
        return (a.year ?? -Infinity) - (b.year ?? -Infinity);
      case "citations":
        return (a.citedBy ?? -1) - (b.citedBy ?? -1);
    }
  };
  return [...items].sort((a, b) => (descending ? -compare(a, b) : compare(a, b)));
}

/** Clicking a column header: sort by it, or flip the order if it already is. */
export function toggleSort(sort: Sort, key: SortKey): Sort {
  if (sort.key === key) return { key, descending: !sort.descending };
  return { key, descending: key === "dateAdded" || key === "year" || key === "citations" };
}
