// @vitest-environment happy-dom
import { describe, expect, it, vi } from "vitest";
import type { CiteConnector, FormatResult } from "./connector";
import { MemoryDocument } from "./memoryDocument";
import { CitationSession, DEFAULT_STYLE } from "./session";

function setup(missing: string[] = []) {
  const document = new MemoryDocument([{ kind: "text", text: "Introduction " }]);
  const format = vi.fn<CiteConnector["format"]>(async (styleId, clusters, html): Promise<FormatResult> => {
    expect(html).toBe(true);
    const numbers = new Map<string, number>();
    const absent = new Set<string>();
    const citations = clusters.map(({ works }) => {
      const parts = works.flatMap(({ itemId, locator }) => {
        if (missing.includes(itemId)) { absent.add(itemId); return []; }
        if (!numbers.has(itemId)) numbers.set(itemId, numbers.size + 1);
        return [`${styleId === DEFAULT_STYLE ? numbers.get(itemId) : itemId}${locator ? `, p. ${locator}` : ""}`];
      });
      return parts.length ? `[${parts.join("; ")}]` : "";
    });
    return {
      citations,
      bibliography: [...numbers].map(([id, number]) => `[${number}] <i>${id}</i>`),
      missing: [...absent],
    };
  });
  const markers = vi.fn<CiteConnector["markers"]>(async () => []);
  return { document, format, markers, session: new CitationSession(document, { format, markers }) };
}

describe("citation session", () => {
  it("renumbers citations and references when inserting before existing citations", async () => {
    const { document, session, format } = setup();
    await session.insert([{ itemId: "first" }]);
    await session.insert([{ itemId: "second" }]);
    await session.insertBibliography();
    document.setSelection(1);
    await session.insert([{ itemId: "new-first" }]);
    expect((await document.getCitations()).map(({ text }) => text)).toEqual(["[1]", "[2]", "[3]"]);
    expect(format.mock.calls.at(-1)?.[1]).toEqual([
      { works: [{ itemId: "new-first" }] }, { works: [{ itemId: "first" }] }, { works: [{ itemId: "second" }] },
    ]);
    expect(await document.getBibliography()).toEqual(["[1] <i>new-first</i>", "[2] <i>first</i>", "[3] <i>second</i>"]);
    expect(await document.getStyleId()).toBe(DEFAULT_STYLE);
  });

  it("uses the saved document style and reformats when it changes", async () => {
    const { document, session, format } = setup();
    await session.insert([{ itemId: "Author 2026" }]);
    expect((await document.getCitations())[0].text).toBe("[1]");
    await session.setStyle("author-date");
    expect(await document.getStyleId()).toBe("author-date");
    expect((await document.getCitations())[0].text).toBe("[Author 2026]");
    expect(format.mock.calls.at(-1)?.[0]).toBe("author-date");
    await session.refresh();
    expect(format.mock.calls.at(-1)?.[0]).toBe("author-date");
  });

  it("keeps the snapshot when all works are missing and reports the missing ids", async () => {
    const { document, session } = setup(["deleted", "also-deleted"]);
    const citation = await document.insertCitation([{ itemId: "deleted" }, { itemId: "also-deleted" }]);
    await document.setCitationText(citation.id, "<sup>[7–8]</sup>");
    const write = vi.spyOn(document, "setCitationText");
    const result = await session.refresh();
    expect(result.missing).toEqual(["deleted", "also-deleted"]);
    expect((await document.getCitations())[0].text).toBe("<sup>[7–8]</sup>");
    expect(write).not.toHaveBeenCalled();
  });

  it("reformats partially missing clusters using the surviving works", async () => {
    const { document, session } = setup(["deleted"]);
    const result = await session.insert([{ itemId: "deleted" }, { itemId: "present", locator: "12" }]);
    expect(result.missing).toEqual(["deleted"]);
    expect((await document.getCitations())[0].text).toBe("[1, p. 12]");
  });

  it("only writes citation texts that changed", async () => {
    const { document, session } = setup();
    await session.insert([{ itemId: "one" }]);
    await session.insert([{ itemId: "two" }]);
    const write = vi.spyOn(document, "setCitationText");
    await session.refresh();
    expect(write).not.toHaveBeenCalled();
    await session.setStyle("author-date");
    expect(write).toHaveBeenCalledTimes(2);
  });

  it("updates the reference list only when present, including an empty existing list", async () => {
    const { document, session } = setup();
    const write = vi.spyOn(document, "setBibliography");
    const result = await session.insert([{ itemId: "one" }]);
    expect(result.references).toBe(0);
    expect(await document.getBibliography()).toBeNull();
    expect(write).not.toHaveBeenCalled();
    await document.setBibliography([]);
    write.mockClear();
    await session.refresh();
    expect(write).toHaveBeenCalledWith(["[1] <i>one</i>"]);
    write.mockClear();
    await session.refresh();
    expect(write).not.toHaveBeenCalled();
  });

  it("inserts one reference list at the selection and updates it rather than duplicating it", async () => {
    const { document, session } = setup();
    await session.insert([{ itemId: "one" }]);
    document.setSelection(0);
    await session.insertBibliography();
    await session.insertBibliography();
    expect(document.getRuns()[0].kind).toBe("bibliography");
    expect(document.getRuns().filter(({ kind }) => kind === "bibliography")).toHaveLength(1);
  });

  it("edits and removes clusters, updating the remaining numbering and reference list", async () => {
    const { document, session } = setup();
    await session.insert([{ itemId: "one" }]);
    await session.insert([{ itemId: "two" }]);
    await session.insertBibliography();
    const [first, second] = await document.getCitations();
    await session.edit(first.id, [{ itemId: "two", locator: "9" }]);
    expect((await document.getCitations()).map(({ text }) => text)).toEqual(["[1, p. 9]", "[1]"]);
    await session.remove(first.id);
    expect((await document.getCitations()).map(({ id, text }) => ({ id, text }))).toEqual([{ id: second.id, text: "[1]" }]);
    expect(await document.getBibliography()).toEqual(["[1] <i>two</i>"]);
    await session.remove(second.id);
    expect(await document.getBibliography()).toEqual([]);
  });

  it("unlinks all fields into plain text, decoding entities and keeping document order", async () => {
    const { document, session, format } = setup();
    const citation = await document.insertCitation([{ itemId: "one" }]);
    await document.setCitationText(citation.id, "<sup>[1]</sup> &amp; <b>two</b>");
    await document.setBibliography(["[1] <i>Title &lt;one&gt;</i>", "[2] <sub>Other</sub>"]);
    await document.setStyleId(DEFAULT_STYLE);
    await session.unlink();
    expect(await document.getCitations()).toEqual([]);
    expect(await document.getBibliography()).toBeNull();
    expect(await document.getStyleId()).toBeNull();
    expect(document.getRuns()).toEqual([
      { kind: "text", text: "Introduction " },
      { kind: "text", text: "[1] & two" },
      { kind: "text", text: "[1] Title <one>\n[2] Other" },
    ]);
    expect(format).not.toHaveBeenCalled();
    await session.refresh();
    expect(document.getRuns().every(({ kind }) => kind === "text")).toBe(true);
  });

  it("serializes overlapping operations and recovers the queue after an error", async () => {
    const { document, session, format } = setup();
    const first = session.insert([{ itemId: "one" }]);
    const second = session.insert([{ itemId: "two" }]);
    await Promise.all([first, second]);
    expect(format.mock.calls.map(([, clusters]) => clusters.length)).toEqual([1, 2]);
    format.mockRejectedValueOnce(new Error("Offline"));
    await expect(session.refresh()).rejects.toThrow("Offline");
    await session.setStyle("author-date");
    expect((await document.getCitations()).map(({ text }) => text)).toEqual(["[one]", "[two]"]);
  });

  it("owns copies of run metadata and validates selection boundaries", async () => {
    const { document } = setup();
    const citation = await document.insertCitation([{ itemId: "one" }]);
    citation.works[0].itemId = "changed";
    const snapshot = await document.getCitations();
    snapshot[0].text = "changed";
    expect((await document.getCitations())[0]).toMatchObject({ works: [{ itemId: "one" }], text: "" });
    expect(() => document.setSelection(-1)).toThrow(RangeError);
    expect(() => document.setSelection(99)).toThrow(RangeError);
  });

  it("turns every place a marker is written into a citation, leaving markers the library cannot name", async () => {
    const { format, markers } = setup();
    const marked = new MemoryDocument([{ kind: "text", text: "A [@a, p. 3] and [@b]; again [@a, p. 3]. " }]);
    const session = new CitationSession(marked, { format, markers });
    markers.mockResolvedValue([
      { text: "[@a, p. 3]", works: [{ itemId: "a", locator: "3" }], missing: [] },
      { text: "[@b]", works: [], missing: ["b"] },
      { text: "[@a, p. 3]", works: [{ itemId: "a", locator: "3" }], missing: [] },
    ]);
    const result = await session.convertMarkers();
    expect(markers).toHaveBeenCalledWith("A [@a, p. 3] and [@b]; again [@a, p. 3]. ");
    expect(result).toMatchObject({ converted: 2, unconverted: ["[@b]"], citations: 2 });
    expect(marked.getRuns().map((run) => run.kind === "text" ? run.text : run.kind)).toEqual([
      "A ", "citation", " and [@b]; again ", "citation", ". ",
    ]);
    expect((await marked.getCitations()).map(({ text }) => text)).toEqual(["[1, p. 3]", "[1, p. 3]"]);
  });
});
