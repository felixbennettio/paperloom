import { describe, expect, it } from "vitest";
import type { SearchField } from "./api";
import { conditionFor, newSearch, withField } from "./searches";

const fields: SearchField[] = [
  { id: "anyField", kind: "text", ops: ["contains", "doesNotContain", "is", "isNot"] },
  { id: "title", kind: "text", ops: ["contains", "doesNotContain", "is", "isNot"] },
  { id: "year", kind: "year", ops: ["is", "isNot", "isBefore", "isAfter"] },
  { id: "tag", kind: "tag", ops: ["contains", "doesNotContain", "is", "isNot"] },
  { id: "hasPdf", kind: "flag", ops: ["is", "isNot"] },
  { id: "dateAdded", kind: "days", ops: ["inTheLast", "notInTheLast"] },
];
const choices = { tags: ["Review"], collections: [] };
const field = (id: string) => fields.find((f) => f.id === id)!;

describe("saved searches", () => {
  it("start with a condition that fits the field", () => {
    expect(newSearch(fields, choices)).toEqual({
      id: "",
      name: "",
      matchAll: true,
      conditions: [{ field: "anyField", op: "contains", value: "" }],
    });
    expect(conditionFor(field("tag"), choices)).toEqual({ field: "tag", op: "contains", value: "Review" });
    expect(conditionFor(field("dateAdded"), choices).value).toBe("30");
    expect(newSearch([], choices).conditions).toEqual([]);
  });

  it("keep the test and words when the field changes alike", () => {
    const words = { field: "anyField", op: "doesNotContain", value: "quantum" };
    expect(withField(words, field("title"), choices, fields)).toEqual({ field: "title", op: "doesNotContain", value: "quantum" });
    expect(withField(words, field("year"), choices, fields)).toEqual({ field: "year", op: "is", value: "" });
    expect(withField(words, field("hasPdf"), choices, fields)).toEqual({ field: "hasPdf", op: "is", value: "true" });
  });
});
