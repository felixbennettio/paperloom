import { describe, expect, it, vi } from "vitest";
import { memoryBackend, normalizeDoiLocally, recognizeIdentifierLocally } from "./api";

const newItem = (title: string, doi: string | null = null) => ({
  itemType: "",
  title,
  authors: ["Curie, Marie", "Pierre Curie", "Henri Becquerel"],
  containerTitle: null,
  year: 1903,
  doi,
});

describe("DOI normalisation (browser stand-in)", () => {
  it("accepts the usual spellings", () => {
    for (const input of [
      "10.1038/nature12373",
      "DOI: 10.1038/NATURE12373",
      "https://doi.org/10.1038/nature12373",
      "doi.org/10.1038%2Fnature12373",
      "10.1038/nature12373.",
    ]) {
      expect(normalizeDoiLocally(input)).toBe("10.1038/nature12373");
    }
  });

  it("rejects non-DOIs", () => {
    for (const input of ["", "hello", "10.1/x", "11.1038/x", "10.1038/"]) {
      expect(normalizeDoiLocally(input)).toBeNull();
    }
  });
});

describe("memory backend", () => {
  it("provides Seafile credential stand-ins and requires the app for sync", async () => {
    const api = memoryBackend();
    await api.seafilePasswordSet("secret");
    expect(await api.seafilePasswordKnown()).toBe(false);
    await expect(api.syncNow({
      kind: "seafile", server: "https://cloud.example.com", account: "me@example.com", library: "中文资料库", folder: "",
    })).rejects.toThrow("Syncing needs the Paperloom app.");
  });

  it("adds, lists, files and trashes items", async () => {
    const api = memoryBackend();
    const thesis = await api.createCollection("Thesis", null);
    const a = await api.addItem(newItem("Radioactivity", "doi:10.1000/X"), thesis.id);
    const b = await api.addItem(newItem("Other"), null);
    expect(a.authors).toBe("Curie et al.");
    expect(a.doi).toBe("10.1000/x");
    expect((await api.listItems({})).map((i) => i.id)).toEqual([b.id, a.id]);
    expect((await api.listItems({ collectionId: thesis.id })).map((i) => i.id)).toEqual([a.id]);
    await expect(api.addItem(newItem("Copy", "10.1000/x"), null)).rejects.toThrow(/already/);
    await expect(api.addItem(newItem("  "), null)).rejects.toThrow(/title/);
    await api.trashItem(a.id);
    expect((await api.listItems({})).map((i) => i.id)).toEqual([b.id]);
    expect((await api.listItems({ trashed: true })).map((i) => i.id)).toEqual([a.id]);
    await api.restoreItem(a.id);
    await api.trashItem(b.id);
    expect(await api.emptyTrash()).toBe(1);
    expect((await api.listItems({})).map((i) => i.id)).toEqual([a.id]);
  });

  it("stars, tags and edits items", async () => {
    const api = memoryBackend();
    const a = await api.addItem(newItem("Radioactivity"), null);
    const b = await api.addItem(newItem("Other"), null);
    await api.setStarred(a.id, true);
    await api.setReadStatus(b.id, "reading");
    await api.setItemTags(a.id, ["physics", " todo ", "physics"]);
    await api.setItemTags(b.id, ["Todo"]);
    await api.setTagColor("todo", "#FF6666");
    expect((await api.listItems({ starredOnly: true })).map((i) => i.title)).toEqual(["Radioactivity"]);
    expect((await api.listItems({ readStatus: "reading" })).map((i) => i.title)).toEqual(["Other"]);
    expect((await api.listItems({ tag: "todo" })).map((i) => i.title)).toEqual(["Radioactivity"]);
    expect(await api.tags()).toEqual([
      { name: "physics", color: null, count: 1 },
      { name: "todo", color: "#ff6666", count: 1 },
      { name: "Todo", color: null, count: 1 },
    ]);
    await api.renameTag("Todo", "todo");
    expect((await api.listItems({ tag: "todo" })).length).toBe(2);
    const detail = (await api.itemDetail(a.id))!;
    const saved = await api.updateItem({ ...detail, title: " Radium ", starred: false });
    expect([saved.title, saved.starred]).toEqual(["Radium", true]);
    await expect(api.updateItem({ ...detail, title: "" })).rejects.toThrow(/title/);
  });

  it("relates items both ways", async () => {
    const api = memoryBackend();
    const a = await api.addItem(newItem("Theory"), null);
    const b = await api.addItem(newItem("Experiment"), null);
    await api.relateItems(a.id, b.id);
    expect((await api.relatedItems(b.id)).map((i) => i.title)).toEqual(["Theory"]);
    await expect(api.relateItems(a.id, a.id)).rejects.toThrow();
    await api.unrelateItems(b.id, a.id);
    expect(await api.relatedItems(a.id)).toEqual([]);
  });

  it("labels Chinese authors", async () => {
    const api = memoryBackend();
    const item = await api.addItem({ ...newItem("研究"), authors: ["张三", "李四", "王五"] }, null);
    expect(item.authors).toBe("张三 等");
  });
});

describe("identifier recognition (browser stand-in)", () => {
  it("tells the kinds apart", () => {
    expect(recognizeIdentifierLocally("10.1038/nature12373")).toBe("doi");
    expect(recognizeIdentifierLocally("arXiv:1706.03762v7")).toBe("arxiv");
    expect(recognizeIdentifierLocally("https://arxiv.org/abs/1706.03762")).toBe("arxiv");
    expect(recognizeIdentifierLocally("PMID: 23903748")).toBe("pmid");
    expect(recognizeIdentifierLocally("ISBN 978-0-262-03384-8")).toBe("isbn");
    expect(recognizeIdentifierLocally("deep learning")).toBeNull();
  });

  it("explains that lookups need the app", async () => {
    await expect(memoryBackend().addByIdentifier("10.1038/nature12373", null)).rejects.toThrow(/app/);
  });
});


describe("browser demo library", () => {
  it("provides sample content only when the demo parameter is present", async () => {
    vi.stubGlobal("window", { location: { search: "?demo" } });
    try {
      const api = memoryBackend();
      const items = await api.listItems({});
      const collections = await api.listCollections();
      expect(items).toHaveLength(6);
      expect(new Set(items.map((item) => item.itemType)).size).toBeGreaterThan(3);
      expect(items.some((item) => /[\u4e00-\u9fff]/.test(item.title))).toBe(true);
      expect(items.some((item) => item.title.length > 140)).toBe(true);
      expect(items.filter((item) => item.starred)).toHaveLength(1);
      expect(collections).toHaveLength(1);
      expect(await api.listItems({ collectionId: collections[0].id })).toHaveLength(6);
      expect((await api.tags()).some((tag) => tag.color !== null)).toBe(true);
      vi.stubGlobal("window", { location: { search: "?other=demo" } });
      expect(await memoryBackend().listItems({})).toEqual([]);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("never seeds the stand-in inside Tauri", async () => {
    vi.stubGlobal("window", { location: { search: "?demo" }, __TAURI_INTERNALS__: {} });
    try {
      const api = memoryBackend();
      expect(await api.listItems({})).toEqual([]);
      expect(await api.listCollections()).toEqual([]);
    } finally {
      vi.unstubAllGlobals();
    }
  });
});
