// Saved searches: what the editor offers for each field, and how a
// condition changes when its field does.

import { ITEM_TYPES, type SavedSearch, type SearchCondition, type SearchField } from "./api";

/** The English names of the fields; the editor translates them. */
export const FIELD_NAMES: Record<string, string> = {
  anyField: "Any Field",
  title: "Title",
  creator: "Creator",
  publication: "Publication",
  year: "Year",
  tag: "Tag",
  collection: "Collection",
  itemType: "Item Type",
  keyword: "Keyword",
  abstract: "Abstract",
  doi: "DOI",
  publisher: "Publisher",
  readStatus: "Reading Status",
  starred: "Starred",
  hasPdf: "Has PDF",
  dateAdded: "Date Added",
};

/** The English names of the tests. */
export const OP_NAMES: Record<string, string> = {
  contains: "contains",
  doesNotContain: "does not contain",
  is: "is",
  isNot: "is not",
  isBefore: "is before",
  isAfter: "is after",
  inTheLast: "is in the last",
  notInTheLast: "is not in the last",
};

/** What the choices of a field are, for those that offer a list. */
export interface Choices {
  tags: string[];
  /** Collection IDs. */
  collections: string[];
}

/** A value that fits a field: the first choice, or a usual one. */
export function defaultValue(field: SearchField, choices: Choices): string {
  switch (field.kind) {
    case "flag":
      return "true";
    case "readStatus":
      return "unread";
    case "itemType":
      return ITEM_TYPES[0];
    case "days":
      return "30";
    case "tag":
      return choices.tags[0] ?? "";
    case "collection":
      return choices.collections[0] ?? "";
    case "year":
    case "text":
      return "";
  }
}

/** A new condition on a field: its usual test and a value that fits. */
export function conditionFor(field: SearchField, choices: Choices): SearchCondition {
  return { field: field.id, op: field.ops[0] ?? "is", value: defaultValue(field, choices) };
}

/** The condition after its field changes: it keeps its test, and words,
 * where the new field takes them. */
export function withField(condition: SearchCondition, field: SearchField, choices: Choices, fields: SearchField[]): SearchCondition {
  const old = fields.find((f) => f.id === condition.field);
  const op = field.ops.includes(condition.op) ? condition.op : (field.ops[0] ?? "is");
  const keepValue = old !== undefined && old.kind === field.kind;
  return { field: field.id, op, value: keepValue ? condition.value : defaultValue(field, choices) };
}

/** A search not saved yet: one condition on the first field. */
export function newSearch(fields: SearchField[], choices: Choices): SavedSearch {
  return { id: "", name: "", matchAll: true, conditions: fields.length > 0 ? [conditionFor(fields[0], choices)] : [] };
}
