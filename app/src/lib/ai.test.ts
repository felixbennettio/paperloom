import { describe, expect, it } from "vitest";
import { pageRanges } from "./ai";

describe("pageRanges", () => {
  it("joins runs of pages", () => {
    expect(pageRanges([1, 2, 3, 5, 7, 8])).toBe("1–3, 5, 7–8");
    expect(pageRanges([4])).toBe("4");
    expect(pageRanges([])).toBe("");
  });
});
