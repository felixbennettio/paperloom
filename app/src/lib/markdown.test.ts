import { describe, expect, it } from "vitest";
import { colorText, toggleLinePrefix, toggleWrap, wikiLink, type Edit } from "./markdown";

const apply = (doc: string, e: Edit) => {
  const text = doc.slice(0, e.from) + e.insert + doc.slice(e.to);
  return { text, selected: text.slice(e.selectFrom, e.selectTo) };
};

describe("toggleWrap", () => {
  it("wraps and unwraps", () => {
    const doc = "make this bold";
    const once = apply(doc, toggleWrap(doc, 5, 9, "**"));
    expect(once).toEqual({ text: "make **this** bold", selected: "this" });
    expect(apply(once.text, toggleWrap(once.text, 7, 11, "**"))).toEqual({ text: doc, selected: "this" });
    expect(apply(once.text, toggleWrap(once.text, 5, 13, "**"))).toEqual({ text: doc, selected: "this" });
  });
});

describe("toggleLinePrefix", () => {
  it("prefixes every touched line and takes it away again", () => {
    const doc = "one\ntwo\nthree";
    const listed = apply(doc, toggleLinePrefix(doc, 1, 5, "- "));
    expect(listed.text).toBe("- one\n- two\nthree");
    expect(apply(listed.text, toggleLinePrefix(listed.text, 0, 9, "- ")).text).toBe(doc);
  });

  it("swaps heading levels", () => {
    expect(apply("# Title", toggleLinePrefix("# Title", 3, 3, "## ")).text).toBe("## Title");
  });
});

describe("colors and links", () => {
  it("writes inline HTML colors", () => {
    expect(apply("red text", colorText("red text", 0, 3, "#e03e3e", false)).text).toBe('<span style="color: #e03e3e">red</span> text');
    expect(apply("hi", colorText("hi", 0, 2, "#fff3a3", true)).text).toBe('<mark style="background: #fff3a3">hi</mark>');
  });

  it("makes wikilinks", () => {
    expect(apply("see Ideas", wikiLink("see Ideas", 4, 9))).toEqual({ text: "see [[Ideas]]", selected: "Ideas" });
  });
});
