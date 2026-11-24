import { describe, expect, it } from "vitest";
import { findUrls, looksLikeReferences, planSharedText } from "./share";

describe("shared text", () => {
  it("recognises reference data", () => {
    expect(looksLikeReferences("TY  - JOUR\nTI  - A title\nER  - ")).toBe(true);
    expect(looksLikeReferences("@article{key,\n title={A}\n}")).toBe(true);
    expect(looksLikeReferences("%0 Journal Article\n%T A title")).toBe(true);
    expect(looksLikeReferences("PMID- 12345678\nTI  - A title")).toBe(true);
    expect(looksLikeReferences("https://doi.org/10.1000/xyz")).toBe(false);
    expect(looksLikeReferences("Read this: @someone{ not bibtex")).toBe(false);
  });

  it("finds web addresses without the punctuation around them", () => {
    expect(findUrls("See https://example.org/a.")).toEqual(["https://example.org/a"]);
    expect(findUrls("(https://example.org/wiki/A_(b))")).toEqual(["https://example.org/wiki/A_(b)"]);
    expect(findUrls("论文：https://arxiv.org/abs/2101.00001。")).toEqual(["https://arxiv.org/abs/2101.00001"]);
    expect(findUrls("a https://x.org b http://y.org https://x.org")).toEqual(["https://x.org", "http://y.org"]);
    expect(findUrls("no address here")).toEqual([]);
  });

  it("plans what to do with shared text", () => {
    expect(planSharedText({ text: "  ", subject: null })).toEqual({ kind: "nothing" });
    expect(planSharedText({ text: "TY  - JOUR\nER  - ", subject: null })).toEqual({ kind: "references", text: "TY  - JOUR\nER  - " });
    expect(planSharedText({ text: "10.1038/nature12373", subject: null })).toEqual({
      kind: "lookup",
      candidates: ["10.1038/nature12373"],
      page: null,
    });
    expect(planSharedText({ text: "A paper\nhttps://www.nature.com/articles/nature12373", subject: "A paper" })).toEqual({
      kind: "lookup",
      candidates: ["A paper\nhttps://www.nature.com/articles/nature12373", "https://www.nature.com/articles/nature12373"],
      page: "https://www.nature.com/articles/nature12373",
    });
    expect(planSharedText({ text: "https://doi.org/10.1038/nature12373", subject: null })).toEqual({
      kind: "lookup",
      candidates: ["https://doi.org/10.1038/nature12373"],
      page: "https://doi.org/10.1038/nature12373",
    });
  });
});
