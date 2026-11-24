import { afterEach, describe, expect, it, vi } from "vitest";
import { backend } from "./api";
import { PluginHost, pluginHost, pluginFrameUrl, siteOf } from "./plugins";

describe("plugins", () => {
  it("serves frames from the app's own scheme", () => {
    expect(pluginFrameUrl("Mozilla/5.0 (Macintosh; Intel Mac OS X 15_0)")).toBe("plugin://localhost/host.html");
    expect(pluginFrameUrl("Mozilla/5.0 (Windows NT 10.0; Win64; x64)")).toBe("http://plugin.localhost/host.html");
    expect(pluginFrameUrl("Mozilla/5.0 (Linux; Android 15)")).toBe("http://plugin.localhost/host.html");
  });

  it("names the site a request goes to", () => {
    expect(siteOf("https://api.github.com/repos?x=1")).toBe("api.github.com");
    expect(siteOf("http://localhost:8080/x")).toBe("localhost:8080");
    expect(siteOf("file:///etc/passwd")).toBeNull();
    expect(siteOf("not a url")).toBeNull();
  });
});

describe("Zotero plugin host", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
    vi.useRealTimers();
  });

  async function start(withOtherPlugin = false) {
    vi.useFakeTimers();
    let receive: (event: MessageEvent) => void = () => {};
    vi.stubGlobal("window", {
      addEventListener: (_type: string, listener: typeof receive) => { receive = listener; },
      setTimeout: globalThis.setTimeout,
    });
    vi.stubGlobal("navigator", { userAgent: "Linux" });
    const source = { postMessage: vi.fn() };
    const frame = { contentWindow: source, setAttribute: vi.fn(), remove: vi.fn() };
    const otherSource = { postMessage: vi.fn() };
    const otherFrame = { ...frame, contentWindow: otherSource };
    vi.stubGlobal("document", { createElement: vi.fn().mockReturnValueOnce(frame).mockReturnValueOnce(otherFrame) });
    vi.spyOn(backend, "obsidianPlugins").mockResolvedValue([]);
    const plugin = { id: "columns", name: "Columns", version: "1", description: "", author: "", homepage: "", enabled: true };
    vi.spyOn(backend, "zoteroPlugins").mockResolvedValue(withOtherPlugin ? [plugin, { ...plugin, id: "other" }] : [plugin]);
    const host = new PluginHost();
    host.setLayer({ appendChild: vi.fn() } as unknown as HTMLElement);
    await host.reload();
    const send = (data: object, from: unknown = source) => receive({ source: from, data } as MessageEvent);
    send({ type: "column", key: "columns-score", label: "Score" });
    return { host, send, source, otherSource, frame, column: host.snapshot().columns[0] };
  }

  async function refresh(host: PluginHost) {
    const pending = host.itemsChanged();
    await vi.advanceTimersByTimeAsync(50);
    await pending;
  }

  it("notifies only successful opens, once per instance, and permits repeated external opens", async () => {
    const { host, send, source } = await start();
    send({ type: "ready" });
    vi.spyOn(backend, "zoteroItems").mockResolvedValue(JSON.stringify([{ key: "parent", attachments: [{ key: "pdf" }] }]));
    await refresh(host);
    const open = vi.spyOn(backend, "openAttachment").mockResolvedValue(undefined);
    const notifiers = () => source.postMessage.mock.calls.map(([message]) => message).filter(message => message.type === "notifier");
    const instance = {};
    await Promise.all([host.fileOpened("pdf", instance), host.fileOpened("pdf", instance)]);
    expect(notifiers()).toHaveLength(1);
    open.mockRejectedValueOnce(new Error("Open failed"));
    await expect(host.openAttachment("pdf")).rejects.toThrow("Open failed");
    expect(notifiers()).toHaveLength(1);
    await host.openAttachment("pdf");
    await host.openAttachment("pdf");
    expect(notifiers()).toHaveLength(3);
    expect(notifiers().every(message => JSON.stringify(message.events) === JSON.stringify([{ event: "open", objectType: "file", itemKeys: ["pdf"] }]))).toBe(true);
    const unknown = host.fileOpened("unknown", {});
    await vi.advanceTimersByTimeAsync(50);
    await unknown;
    expect(notifiers()).toHaveLength(3);
  });

  it("coalesces reader context, preserves views across page changes and pane moves, and reopens after close", async () => {
    const { host, send, source } = await start();
    send({ type: "ready" });
    vi.spyOn(backend, "zoteroItems").mockResolvedValue(JSON.stringify([{ key: "parent", attachments: [{ key: "pdf" }, { key: "other" }] }]));
    await refresh(host);
    const pdf = { attachmentKey: "pdf", parentItemKey: "parent" };
    const other = { attachmentKey: "other" };
    host.tabsChanged([pdf, other], "pdf");
    host.tabsChanged([other, pdf], "pdf");
    await vi.advanceTimersByTimeAsync(16);
    const tabMessages = () => source.postMessage.mock.calls.map(([message]) => message).filter(message => message.type === "tabs");
    const first = tabMessages().at(-1)!;
    expect(first.readers).toHaveLength(2);
    expect(first.readers.every((reader: { parentItemKey: string }) => reader.parentItemKey === "parent")).toBe(true);
    const viewID = first.selectedViewID;
    await host.readerOpened("pdf", () => false);
    expect(source.postMessage.mock.calls.filter(([message]) => message.type === "notifier")).toHaveLength(0);
    await host.readerOpened("pdf");
    await host.readerOpened("pdf");
    host.tabsChanged([other, pdf], "other");
    await vi.advanceTimersByTimeAsync(16);
    host.tabsChanged([pdf, other], "pdf");
    await vi.advanceTimersByTimeAsync(16);
    await host.readerOpened("pdf");
    expect(tabMessages().at(-1)!.selectedViewID).toBe(viewID);
    expect(source.postMessage.mock.calls.filter(([message]) => message.type === "notifier")).toHaveLength(1);
    host.selectTab = vi.fn().mockResolvedValue(undefined);
    await send({ type: "call", id: 100, method: "tabs.select", args: [viewID] });
    expect(host.selectTab).toHaveBeenCalledWith(viewID);
    await send({ type: "call", id: 101, method: "tabs.select", args: ["zotero-pane"] });
    expect(tabMessages().at(-1)!.selectedViewID).toBe("zotero-pane");
    await send({ type: "call", id: 102, method: "tabs.select", args: ["unknown"] });
    expect(source.postMessage.mock.calls.at(-1)![0]).toMatchObject({ type: "reply", id: 102, ok: false });
    host.tabsChanged([other], "other");
    await vi.advanceTimersByTimeAsync(16);
    await host.readerOpened("pdf"); // A late completion for a closed tab is ignored.
    host.tabsChanged([pdf, other], "pdf");
    await vi.advanceTimersByTimeAsync(16);
    await host.readerOpened("pdf");
    expect(tabMessages().at(-1)!.selectedViewID).not.toBe(viewID);
    expect(source.postMessage.mock.calls.filter(([message]) => message.type === "notifier")).toHaveLength(2);
  });

  it("ignores a PDF completion canceled while the initial attachment projection is loading", async () => {
    const { host, send, source } = await start();
    send({ type: "ready" });
    vi.spyOn(backend, "zoteroItems").mockResolvedValue(JSON.stringify([{ key: "parent", attachments: [{ key: "pdf" }] }]));
    host.tabsChanged([{ attachmentKey: "pdf" }], "pdf");
    let current = true;
    const completion = host.readerOpened("pdf", () => current);
    current = false;
    await vi.advanceTimersByTimeAsync(50);
    await completion;
    expect(source.postMessage.mock.calls.filter(([message]) => message.type === "notifier")).toHaveLength(0);
    await host.readerOpened("pdf");
    expect(source.postMessage.mock.calls.filter(([message]) => message.type === "notifier")).toHaveLength(1);
  });

  it("queues successful opens and tabs through startup until the runner is ready", async () => {
    const { host, send, source } = await start();
    vi.spyOn(backend, "zoteroItems").mockResolvedValue(JSON.stringify([{ key: "parent", attachments: [{ key: "pdf" }] }]));
    vi.spyOn(backend, "zoteroPluginCode").mockResolvedValue({ files: [], prefs: null });
    host.tabsChanged([{ attachmentKey: "pdf" }], "pdf");
    send({ type: "loaded" });
    await vi.advanceTimersByTimeAsync(50);
    await host.readerOpened("pdf");
    expect(source.postMessage.mock.calls).toHaveLength(1);
    expect(source.postMessage.mock.calls[0][0]).toMatchObject({ type: "start", tabs: { readers: [{ attachmentKey: "pdf", parentItemKey: "parent" }] } });
    send({ type: "ready" });
    const messages = source.postMessage.mock.calls.map(([message]) => message);
    expect(messages.at(-1)).toMatchObject({ type: "notifier", events: [{ event: "open", objectType: "file", itemKeys: ["pdf"] }] });
    expect(messages.findIndex(message => message.type === "tabs")).toBeLessThan(messages.length - 1);
  });

  it("diffs annotation lifecycle fields independently and serializes rapid native create/delete commits", async () => {
    const { host, send, source } = await start();
    send({ type: "ready" });
    const parents = [{ key: "paper", attachments: [{ key: "pdf" }] }, { key: "pdf", itemType: "attachment", parentItem: "paper" }];
    let items: object[] = parents;
    vi.spyOn(backend, "zoteroItems").mockImplementation(async () => JSON.stringify(items));
    const events = () => source.postMessage.mock.calls.flatMap(([message]) => message.events ?? []);
    const settleMutation = async <T,>(work: Promise<T>) => {
      await vi.runAllTimersAsync();
      return work;
    };
    await refresh(host);
    expect(events()).toEqual([]);
    let annotation = { key: "mark", itemType: "annotation", parentItem: "pdf", annotationType: "highlight",
      annotationColor: "#ffd400", annotationComment: "", annotationText: "quote", annotationPosition: '{"pageIndex":0,"rects":[[1,2,3,4]]}' };
    await settleMutation(host.annotationMutation(async () => { items = [...parents, annotation]; }));
    await refresh(host);
    expect(events()).toEqual([{ event: "add", objectType: "item", itemKeys: ["mark"] }]);
    await settleMutation(host.annotationMutation(async () => {
      annotation = { ...annotation, annotationComment: "Comment" }; items = [...parents, annotation];
    }));
    expect(events().at(-1)).toEqual({ event: "modify", objectType: "item", itemKeys: ["mark"], changed: { mark: ["annotationComment"] } });
    await settleMutation(host.annotationMutation(async () => {
      annotation = { ...annotation, annotationColor: "#abcdef" }; items = [...parents, annotation];
    }));
    expect(events().at(-1).changed).toEqual({ mark: ["annotationColor"] });
    await settleMutation(host.annotationMutation(async () => {
      annotation = { ...annotation, annotationText: "other", annotationPosition: '{"pageIndex":1,"rects":[[1,2,3,4]]}' }; items = [...parents, annotation];
    }));
    expect(events().at(-1).changed).toEqual({ mark: ["annotationText", "annotationPosition"] });
    const added = host.annotationMutation(async () => { items = [...items, { ...annotation, key: "rapid" }]; });
    const removed = host.annotationMutation(async () => { items = [...parents, annotation]; });
    await settleMutation(Promise.all([added, removed]));
    expect(events().slice(-2)).toEqual([
      { event: "add", objectType: "item", itemKeys: ["rapid"] },
      { event: "delete", objectType: "item", itemKeys: ["rapid"] },
    ]);
    const failed = host.annotationMutation(async () => { throw new Error("No commit"); });
    const handled = expect(failed).rejects.toThrow("No commit");
    await settleMutation(handled);
    await settleMutation(host.annotationMutation(async () => { items = []; }));
    expect(events().at(-1)).toEqual({ event: "delete", objectType: "item", itemKeys: ["paper", "pdf", "mark"] });
  });

  it("diffs the complete projection through baseline/add/modify/trash/restore/delete without refresh events", async () => {
    const { host, send, source } = await start();
    send({ type: "ready" });
    let items: object[] = [{ key: "one", title: "Original", tags: [] }];
    const fetch = vi.spyOn(backend, "zoteroItems").mockImplementation(async () => JSON.stringify(items));
    await refresh(host);
    expect(source.postMessage.mock.calls.at(-1)![0].events).toEqual([]);
    items = [...items, { key: "two", title: "New", tags: [] }];
    await refresh(host);
    expect(source.postMessage.mock.calls.at(-1)![0].events).toEqual([{ event: "add", objectType: "item", itemKeys: ["two"] }]);
    items[1] = { key: "two", title: "Changed", creators: [{ name: "Institute" }], tags: [{ tag: "x" }] };
    await refresh(host);
    expect(source.postMessage.mock.calls.at(-1)![0].events).toEqual([{ event: "modify", objectType: "item", itemKeys: ["two"], changed: { two: ["title", "tags", "creators"] } }]);
    await refresh(host);
    expect(source.postMessage.mock.calls.at(-1)![0].events).toEqual([]);
    items[1] = { ...items[1], deleted: true };
    await refresh(host);
    expect(source.postMessage.mock.calls.at(-1)![0].events[0].event).toBe("trash");
    items[1] = { ...items[1], deleted: false };
    await refresh(host);
    expect(source.postMessage.mock.calls.at(-1)![0].events[0]).toMatchObject({ event: "modify", itemKeys: ["two"], changed: { two: ["deleted"] } });
    items = items.slice(0, 1);
    await refresh(host);
    expect(source.postMessage.mock.calls.at(-1)![0].events).toEqual([{ event: "delete", objectType: "item", itemKeys: ["two"] }]);
    const seqs = source.postMessage.mock.calls.map(([message]) => message.seq);
    expect(seqs).toEqual([...seqs].sort((a, b) => a - b));
    expect(new Set(seqs).size).toBe(seqs.length);
    expect(fetch).toHaveBeenCalledTimes(7);
  });

  it("coalesces invalidations, keeps one fetch in flight and reruns after failed reads", async () => {
    const { host, send, source } = await start();
    send({ type: "ready" });
    const fetch = vi.spyOn(backend, "zoteroItems").mockResolvedValue('[{"key":"one"}]');
    await refresh(host);
    let resolve!: (value: string) => void;
    fetch.mockImplementationOnce(() => new Promise((done) => { resolve = done; }));
    const first = host.itemsChanged();
    const second = host.itemsChanged();
    await vi.advanceTimersByTimeAsync(50);
    expect(fetch).toHaveBeenCalledTimes(2);
    const trailing = host.itemsChanged();
    fetch.mockResolvedValue('[{"key":"one"},{"key":"two"}]');
    resolve('[{"key":"one"}]');
    await Promise.all([first, second, trailing]);
    expect(fetch).toHaveBeenCalledTimes(3);
    expect(source.postMessage.mock.calls.at(-1)![0].events[0]).toMatchObject({ event: "add", itemKeys: ["two"] });
    vi.spyOn(console, "error").mockImplementation(() => {});
    fetch.mockRejectedValueOnce(new Error("offline"));
    const count = source.postMessage.mock.calls.length;
    await refresh(host);
    expect(source.postMessage).toHaveBeenCalledTimes(count);
    fetch.mockResolvedValueOnce('{"invalid":true}');
    await refresh(host);
    expect(source.postMessage).toHaveBeenCalledTimes(count);
    fetch.mockResolvedValue('[{"key":"one"},{"key":"two","title":"Edited"}]');
    await refresh(host);
    expect(source.postMessage.mock.calls.at(-1)![0].events[0]).toMatchObject({ event: "modify", itemKeys: ["two"] });
  });

  it("batches lifecycle events in order and normalizes tag and property order", async () => {
    const { host, send, source } = await start();
    send({ type: "ready" });
    const fetch = vi.spyOn(backend, "zoteroItems").mockResolvedValue('[{"key":"old","tags":[{"tag":"b"},{"tag":"a"}]}]');
    await refresh(host);
    fetch.mockResolvedValue('[{"tags":[{"tag":"a"},{"tag":"b"}],"key":"old","deleted":false}]');
    await refresh(host);
    expect(source.postMessage.mock.calls.at(-1)![0].events).toEqual([]);
    fetch.mockResolvedValue(JSON.stringify(Array.from({ length: 1001 }, (_, index) => ({ key: `new-${index}` }))));
    await refresh(host);
    const events = source.postMessage.mock.calls.at(-1)![0].events;
    expect(events.map((event: { event: string; itemKeys: string[] }) => [event.event, event.itemKeys.length]))
      .toEqual([["add", 500], ["add", 500], ["add", 1], ["delete", 1]]);
  });

  it("queues startup changes until ready and includes persistent selection in start", async () => {
    const { host, send, source } = await start();
    vi.spyOn(backend, "zoteroPluginCode").mockResolvedValue({ files: [], prefs: null });
    const fetch = vi.spyOn(backend, "zoteroItems").mockResolvedValue('[{"key":"one"}]');
    host.selectionChanged(["one"]);
    send({ type: "loaded" });
    await vi.advanceTimersByTimeAsync(50);
    expect(source.postMessage.mock.calls[0][0]).toMatchObject({ type: "start", selection: { itemKeys: ["one"] } });
    fetch.mockResolvedValue('[{"key":"one"},{"key":"two"}]');
    await refresh(host);
    expect(source.postMessage).toHaveBeenCalledTimes(1);
    send({ type: "ready" });
    expect(source.postMessage.mock.calls.at(-1)![0].events).toEqual([{ event: "add", objectType: "item", itemKeys: ["two"] }]);
  });

  it("keeps one snapshot and a bounded event backlog for a starting plugin, and gives up after 30 seconds", async () => {
    const { host, send, source } = await start();
    vi.spyOn(backend, "zoteroPluginCode").mockResolvedValue({ files: [], prefs: null });
    const fetch = vi.spyOn(backend, "zoteroItems").mockResolvedValue('[{"key":"one"}]');
    send({ type: "loaded" });
    await vi.advanceTimersByTimeAsync(50);
    fetch.mockResolvedValue('[{"key":"one"},{"key":"two"}]');
    await refresh(host);
    fetch.mockResolvedValue('[{"key":"one"},{"key":"two"},{"key":"three"}]');
    await refresh(host);
    send({ type: "ready" });
    const queued = source.postMessage.mock.calls.slice(1).map(([message]) => message);
    expect(queued.filter((message) => message.type === "items")).toHaveLength(1);
    expect(queued[0].items.map((item: { key: string }) => item.key)).toEqual(["one", "two", "three"]);
    expect(queued[0].events).toEqual([
      { event: "add", objectType: "item", itemKeys: ["two"] },
      { event: "add", objectType: "item", itemKeys: ["three"] },
    ]);

    // A plugin that never becomes ready stops receiving context.
    const stalled = await start();
    vi.spyOn(backend, "zoteroPluginCode").mockResolvedValue({ files: [], prefs: null });
    stalled.send({ type: "loaded" });
    await vi.advanceTimersByTimeAsync(30_050);
    expect(stalled.host.snapshot().running[0].error).toMatch(/30 seconds/);
    const posted = stalled.source.postMessage.mock.calls.length;
    await refresh(stalled.host);
    stalled.send({ type: "ready" });
    expect(stalled.source.postMessage.mock.calls.length).toBe(posted);
  });

  it("sends a selection to a started plugin before it is ready", async () => {
    const { host, send, source } = await start();
    vi.spyOn(backend, "zoteroPluginCode").mockResolvedValue({ files: [], prefs: null });
    vi.spyOn(backend, "zoteroItems").mockResolvedValue('[{"key":"one"},{"key":"two"}]');
    send({ type: "loaded" });
    await vi.advanceTimersByTimeAsync(50);
    host.selectionChanged(["two"]);
    await vi.advanceTimersByTimeAsync(20);
    expect(source.postMessage.mock.calls.at(-1)![0]).toMatchObject({ type: "selection", itemKeys: ["two"] });
  });

  it("refreshes plugin saves, suppresses the origin's echo only and acknowledges applied selections", async () => {
    const { host, send, source, otherSource } = await start(true);
    send({ type: "ready" }); send({ type: "ready" }, otherSource);
    const fetch = vi.spyOn(backend, "zoteroItems").mockResolvedValue('[{"key":"one","title":"Original"}]');
    await refresh(host);
    vi.spyOn(backend, "zoteroUpdateItem").mockImplementation(async () => { fetch.mockResolvedValue('[{"key":"one","title":"Saved"}]'); });
    host.onItemsChanged = () => { void host.itemsChanged(); };
    send({ type: "call", id: 42, method: "items.update", args: ["one", '{"title":"Saved"}', null] });
    await vi.advanceTimersByTimeAsync(100);
    const own = source.postMessage.mock.calls.map(([message]) => message);
    const others = otherSource.postMessage.mock.calls.map(([message]) => message);
    expect(own.filter((message) => message.type === "items").every((message) => !message.events.length)).toBe(true);
    expect(others.flatMap((message) => message.events ?? [])).toEqual([{ event: "modify", objectType: "item", itemKeys: ["one"], changed: { one: ["title"] } }]);
    expect(own).toContainEqual({ type: "reply", id: 42, ok: true, value: null });
    let apply!: () => void;
    host.selectItems = vi.fn(() => new Promise<void>((resolve) => { apply = resolve; }));
    send({ type: "call", id: 43, method: "pane.selectItems", args: [["one"]] });
    await Promise.resolve();
    expect(host.selectItems).toHaveBeenCalledWith(["one"]);
    expect(source.postMessage.mock.calls.at(-1)![0].id).not.toBe(43);
    apply();
    await vi.advanceTimersByTimeAsync(16);
    expect(source.postMessage.mock.calls.map(([message]) => message)).toContainEqual({ type: "reply", id: 43, ok: true, value: true });
    expect(source.postMessage.mock.calls.map(([message]) => message)).toContainEqual({ type: "selection", seq: expect.any(Number), itemKeys: ["one"] });
  });

  it("refreshes subscribed readers after annotation commits, with precise fields and origin suppression", async () => {
    const { host, send, source, otherSource } = await start(true);
    send({ type: "ready" }); send({ type: "ready" }, otherSource);
    const parents = [{ key: "paper", attachments: [{ key: "pdf" }] }, { key: "pdf", itemType: "attachment", parentItem: "paper" }];
    let mark = { key: "mark", itemType: "annotation", parentItem: "pdf", annotationType: "highlight",
      annotationComment: "", annotationColor: "#ffd400", tags: [] as { tag: string; type: number }[] };
    vi.spyOn(backend, "zoteroItems").mockImplementation(async () => JSON.stringify([...parents, mark]));
    await refresh(host);
    const reader = vi.fn();
    const unrelated = vi.fn();
    const unsubscribe = host.subscribeAnnotations("pdf", reader);
    host.subscribeAnnotations("other", unrelated);
    vi.spyOn(backend, "zoteroUpdateItem").mockImplementation(async (_key, fields, tags) => {
      mark = { ...mark, ...JSON.parse(fields), ...(tags ? { tags: tags.map(tag => ({ tag, type: 0 })) } : {}) };
    });
    const events = (sender: typeof source) => sender.postMessage.mock.calls.flatMap(([message]) => message.events ?? []);
    for (const [index, fields, tags, changed] of [
      [1, { annotationComment: "  translated\n" }, null, ["annotationComment"]],
      [2, { annotationColor: "#abcdef" }, null, ["annotationColor"]],
      [3, {}, ["#auto"], ["tags"]],
    ] as const) {
      send({ type: "call", id: index, method: "items.update", args: ["mark", JSON.stringify(fields), tags] });
      await vi.runAllTimersAsync();
      expect(events(source)).toEqual([]);
      expect(events(otherSource).at(-1)).toEqual({ event: "modify", objectType: "item", itemKeys: ["mark"], changed: { mark: changed } });
      expect(reader).toHaveBeenCalledTimes(index);
    }
    await refresh(host);
    expect(reader).toHaveBeenCalledTimes(3);
    expect(unrelated).not.toHaveBeenCalled();
    send({ type: "call", id: 4, method: "items.update", args: ["mark", "{}", ["#auto"]] });
    await vi.runAllTimersAsync();
    expect(events(otherSource)).toHaveLength(3);
    expect(reader).toHaveBeenCalledTimes(3);
    send({ type: "call", id: 5, method: "items.update", args: ["mark", "{}", [1]] });
    await vi.runAllTimersAsync();
    expect(source.postMessage.mock.calls.map(([message]) => message)).toContainEqual({ type: "reply", id: 5, ok: false, error: "Invalid field tags." });
    expect(reader).toHaveBeenCalledTimes(3);
    unsubscribe();
    mark = { ...mark, annotationComment: "after close" };
    await refresh(host);
    expect(reader).toHaveBeenCalledTimes(3);
  });

  it("updates open reader comments and tags while preserving a focused draft", async () => {
    const { Window } = await import("happy-dom");
    const browser = new Window({ url: "http://localhost" });
    for (const [key, value] of Object.entries({ window: browser, document: browser.document,
      navigator: browser.navigator, Element: browser.Element, HTMLElement: browser.HTMLElement,
      Node: browser.Node, IS_REACT_ACT_ENVIRONMENT: true,
      ResizeObserver: class { observe() {} disconnect() {} },
    })) vi.stubGlobal(key, value);
    const doc = browser.document as unknown as Document;
    const pdf = { numPages: 0, getOutline: async () => [], destroy: vi.fn() };
    vi.doMock("pdfjs-dist", () => ({ GlobalWorkerOptions: {}, TextLayer: class {}, getDocument: () => ({ promise: Promise.resolve(pdf) }) }));
    let committed = () => {};
    vi.spyOn(pluginHost, "subscribeAnnotations").mockImplementation((_key, listener) => { committed = listener; return () => {}; });
    vi.spyOn(pluginHost, "readerOpened").mockResolvedValue(undefined);
    vi.spyOn(backend, "readAttachment").mockResolvedValue(new ArrayBuffer(0));
    vi.spyOn(backend, "pdfFormFields").mockResolvedValue([]);
    vi.spyOn(backend, "attachmentInfo").mockRejectedValue(new Error("No parent"));
    vi.spyOn(backend, "attachmentHasText").mockResolvedValue(true);
    let mark = { id: "mark", attachmentId: "pdf", kind: "highlight" as const, pageIndex: 0, rects: [],
      text: "quote", comment: "original", color: "#ffd400", dateAdded: 0, tags: [] as string[] };
    vi.spyOn(backend, "annotations").mockImplementation(async () => [mark]);
    const save = vi.spyOn(backend, "updateAnnotation");
    const { createElement, act } = await import("react");
    const { createRoot } = await import("react-dom/client");
    const { PdfView } = await import("../components/PdfView");
    const container = browser.document.createElement("div");
    browser.document.body.append(container);
    const root = createRoot(container as unknown as HTMLElement);
    try {
      await act(async () => { root.render(createElement(PdfView, { attachmentId: "pdf" })); });
      await act(async () => { doc.querySelector<HTMLButtonElement>('button[aria-label="More"]')!.click(); });
      const entry = [...doc.querySelectorAll<HTMLButtonElement>('button')].find(button => button.textContent.includes("Annotations (1)"))!;
      await act(async () => { entry.click(); });
      const input = doc.querySelector<HTMLTextAreaElement>('textarea[placeholder="Add a comment"]')!;
      expect(input.value).toBe("original");
      mark = { ...mark, comment: "plugin comment", tags: ["#auto"] };
      await act(async () => { committed(); });
      expect(input.value).toBe("plugin comment");
      expect(doc.querySelector(".pdf-annotations")!.textContent).toContain("#auto");
      await act(async () => { input.focus(); input.value = "unsaved draft"; });
      mark = { ...mark, comment: "concurrent plugin comment", color: "#abcdef" };
      await act(async () => { committed(); });
      expect(input.value).toBe("unsaved draft");
      expect(save).not.toHaveBeenCalled();
      expect(doc.querySelector<HTMLElement>(".pdf-annotations li")!.style.borderInlineStartColor).toBe("#abcdef");
    } finally {
      await act(async () => { root.unmount(); });
      await browser.happyDOM.close();
      vi.doUnmock("pdfjs-dist");
    }
  });

  it("preserves startup additions while plugin code is still loading", async () => {
    const { host, send, source } = await start();
    let load!: (code: Awaited<ReturnType<typeof backend.zoteroPluginCode>>) => void;
    vi.spyOn(backend, "zoteroPluginCode").mockImplementation(() => new Promise((resolve) => { load = resolve; }));
    const fetch = vi.spyOn(backend, "zoteroItems").mockResolvedValue('[{"key":"old"}]');
    send({ type: "loaded" });
    await vi.advanceTimersByTimeAsync(50);
    fetch.mockResolvedValue('[{"key":"old"},{"key":"new"}]');
    await refresh(host);
    expect(source.postMessage).not.toHaveBeenCalled();
    load({ files: [], prefs: null });
    await vi.advanceTimersByTimeAsync(1);
    expect(source.postMessage.mock.calls[0][0].items.map((item: { key: string }) => item.key)).toEqual(["old"]);
    send({ type: "ready" });
    expect(source.postMessage.mock.calls.at(-1)![0].events).toEqual([{ event: "add", objectType: "item", itemKeys: ["new"] }]);
  });

  it("discards a projection raced by a save and retains echo suppression after read failure", async () => {
    const { host, send, source, otherSource } = await start(true);
    send({ type: "ready" }); send({ type: "ready" }, otherSource);
    const fetch = vi.spyOn(backend, "zoteroItems").mockResolvedValue('[{"key":"one","title":"Old"}]');
    await refresh(host);
    let finish!: (value: string) => void;
    fetch.mockImplementationOnce(() => new Promise((resolve) => { finish = resolve; }));
    const pending = host.itemsChanged();
    await vi.advanceTimersByTimeAsync(50);
    vi.spyOn(backend, "zoteroUpdateItem").mockResolvedValue();
    send({ type: "call", id: 1, method: "items.update", args: ["one", '{"title":"Saved"}', null] });
    await vi.advanceTimersByTimeAsync(1);
    fetch.mockResolvedValue('[{"key":"one","title":"Saved"}]');
    finish('[{"key":"one","title":"Saved"}]');
    await pending;
    await vi.advanceTimersByTimeAsync(1);
    expect(source.postMessage.mock.calls.flatMap(([message]) => message.events ?? [])).toEqual([]);
    expect(otherSource.postMessage.mock.calls.flatMap(([message]) => message.events ?? [])).toHaveLength(1);
    vi.spyOn(console, "error").mockImplementation(() => {});
    fetch.mockRejectedValueOnce(new Error("offline"));
    send({ type: "call", id: 2, method: "items.update", args: ["one", '{"title":"Later"}', null] });
    await vi.advanceTimersByTimeAsync(50);
    fetch.mockResolvedValue('[{"key":"one","title":"Later"}]');
    await refresh(host);
    expect(source.postMessage.mock.calls.flatMap(([message]) => message.events ?? [])).toEqual([]);
    expect(otherSource.postMessage.mock.calls.flatMap(([message]) => message.events ?? [])).toHaveLength(2);
  });

  it("bounds and reports repeated alternating plugin writes", async () => {
    const { host, send, source, otherSource } = await start(true);
    send({ type: "ready" }); send({ type: "ready" }, otherSource);
    let title = "0";
    vi.spyOn(backend, "zoteroItems").mockImplementation(async () => JSON.stringify([{ key: "one", title }]));
    const write = vi.spyOn(backend, "zoteroUpdateItem").mockImplementation(async (_key, fields) => { title = JSON.parse(fields).title; });
    await refresh(host);
    for (let i = 1; i <= 21; i++) {
      send({ type: "call", id: i, method: "items.update", args: ["one", JSON.stringify({ title: String(i) }), null] }, i % 2 ? source : otherSource);
      await vi.advanceTimersByTimeAsync(50);
    }
    expect(write).toHaveBeenCalledTimes(20);
    expect(source.postMessage.mock.calls.at(-1)![0]).toMatchObject({ type: "reply", id: 21, ok: false, error: expect.stringContaining("alternating") });
  });

  it("tracks registrations and removes a plugin's columns", async () => {
    const { host, send, frame, column } = await start();
    expect(column).toEqual({ plugin: "zotero:columns", key: "columns-score", label: "Score" });
    send({ type: "column", key: column.key, label: "Rating" });
    expect(host.snapshot().columns).toEqual([{ ...column, label: "Rating" }]);
    send({ type: "column-removed", key: column.key }, {});
    expect(host.snapshot().columns).toHaveLength(1);
    send({ type: "column-removed", key: column.key });
    expect(host.snapshot().columns).toEqual([]);
    send({ type: "column", key: column.key, label: column.label });
    const pending = host.columnValues(column, ["one"]);
    host.stop(column.plugin);
    await expect(pending).resolves.toEqual({});
    expect(host.snapshot().columns).toEqual([]);
    expect(frame.remove).toHaveBeenCalledOnce();
    await expect(host.columnValues(column, ["one"])).resolves.toEqual({});
  });

  it("ignores invalid registrations", async () => {
    const { host, send, column } = await start();
    for (const options of [{}, { key: 42, label: "Score" }, { key: "", label: "Score" }, { key: "new", label: null }]) {
      send({ type: "column", ...options });
    }
    expect(host.snapshot().columns).toEqual([column]);
  });

  it("requests a batch and accepts text from the plugin's frame", async () => {
    const { host, send, source, column } = await start();
    const pending = host.columnValues(column, ["one", "two"]);
    const request = source.postMessage.mock.calls[0][0];
    expect(request).toEqual({ type: "column-values", id: expect.any(Number), key: column.key, itemKeys: ["one", "two"] });
    send({ type: "column-values", id: request.id, values: { one: "wrong" } }, {});
    send({ type: "column-values", id: request.id, values: { one: "<b>text</b>", two: "42", invalid: 42 } });
    await expect(pending).resolves.toEqual({ one: "<b>text</b>", two: "42" });
    await vi.advanceTimersByTimeAsync(5000);
  });

  it("isolates registrations and replies between running plugins", async () => {
    const { host, send, source, otherSource, column } = await start(true);
    send({ type: "column", key: column.key, label: "Other" }, otherSource);
    const otherColumn = { ...column, plugin: "zotero:other", label: "Other" };
    expect(host.snapshot().columns).toEqual([column, otherColumn]);
    const pending = host.columnValues(column, ["one"]);
    const request = source.postMessage.mock.calls[0][0];
    const settled = vi.fn();
    void pending.then(settled);
    send({ type: "column-values", id: request.id, values: { one: "wrong" } }, otherSource);
    await Promise.resolve();
    expect(settled).not.toHaveBeenCalled();
    send({ type: "column-values", id: request.id, values: { one: "right" } });
    await expect(pending).resolves.toEqual({ one: "right" });
    send({ type: "column-removed", key: column.key });
    expect(host.snapshot().columns).toEqual([otherColumn]);
    send({ type: "column", key: column.key, label: column.label });
    host.stop(column.plugin);
    expect(host.snapshot().columns).toEqual([otherColumn]);
  });

  it("treats malformed values as an empty batch", async () => {
    const { host, send, source, column } = await start();
    for (const values of [undefined, null, "text", 42, ["text"]]) {
      const pending = host.columnValues(column, ["one"]);
      const request = source.postMessage.mock.calls.at(-1)![0];
      send({ type: "column-values", id: request.id, values });
      await expect(pending).resolves.toEqual({});
    }
  });

  it("times out after five seconds and ignores late replies", async () => {
    const { host, send, source, column } = await start();
    const pending = host.columnValues(column, ["one"]);
    const request = source.postMessage.mock.calls[0][0];
    await vi.advanceTimersByTimeAsync(4999);
    const settled = vi.fn();
    void pending.then(settled);
    await Promise.resolve();
    expect(settled).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    await expect(pending).resolves.toEqual({});
    send({ type: "column-values", id: request.id, values: { one: "late" } });
    expect(settled).toHaveBeenCalledOnce();
  });

  it("tracks supported reader events by plugin and clears registrations", async () => {
    const { host, send, otherSource } = await start(true);
    send({ type: "reader-events", types: ["renderToolbar", "unknown", "renderToolbar", 42] });
    send({ type: "reader-events", types: ["createViewContextMenu"] }, otherSource);
    expect(host.snapshot().readerEvents).toEqual({ "zotero:columns": ["renderToolbar"], "zotero:other": ["createViewContextMenu"] });
    send({ type: "reader-events", types: [] }, {});
    send({ type: "reader-events", types: "renderToolbar" });
    expect(host.snapshot().readerEvents["zotero:columns"]).toEqual(["renderToolbar"]);
    send({ type: "reader-events", types: [] });
    expect(host.snapshot().readerEvents).toEqual({ "zotero:other": ["createViewContextMenu"] });
    await expect(host.readerRender("renderToolbar", "attachment", "parent", {})).resolves.toEqual([]);
    host.stop("zotero:other");
    expect(host.snapshot().readerEvents).toEqual({});
  });

  it("streams reader updates only from the owning frame and releases events on close", async () => {
    const { host, send, source, otherSource } = await start(true);
    send({ type: "reader-events", types: ["renderTextSelectionPopup"] });
    const update = vi.fn();
    const close = host.watchReader("renderTextSelectionPopup", "attachment", "parent", {}, update);
    const request = source.postMessage.mock.calls.at(-1)![0];
    expect(request.live).toBe(true);
    send({ type: "reader-rendered", id: request.id, html: "wrong" }, otherSource);
    expect(update).not.toHaveBeenCalled();
    send({ type: "reader-rendered", id: request.id, html: "waiting" });
    send({ type: "reader-rendered", id: request.id, html: "translated" });
    expect(update.mock.calls.at(-1)![0]).toEqual([{ plugin: "zotero:columns", render: request.id, html: "translated" }]);
    host.readerEvent("zotero:other", request.id, "1", { event: "click" });
    expect(otherSource.postMessage).not.toHaveBeenCalled();
    host.readerEvent("zotero:columns", request.id, "1", { event: "click" });
    expect(source.postMessage.mock.calls.at(-1)![0]).toEqual({ type: "reader-event", id: request.id, node: "1", event: "click" });
    close();
    expect(source.postMessage.mock.calls.at(-1)![0]).toEqual({ type: "reader-release", id: request.id });
    send({ type: "reader-rendered", id: request.id, html: "late" });
    host.readerEvent("zotero:columns", request.id, "1", { event: "click" });
    expect(update).toHaveBeenCalledTimes(2);
    expect(source.postMessage.mock.calls.at(-1)![0].type).toBe("reader-release");
  });

  it("requests reader HTML from every listener and isolates replies by frame", async () => {
    const { host, send, source, otherSource } = await start(true);
    for (const from of [source, otherSource]) send({ type: "reader-events", types: ["renderToolbar"] }, from);
    const pending = host.readerRender("renderToolbar", "attachment", "parent", {});
    const request = source.postMessage.mock.calls[0][0];
    const otherRequest = otherSource.postMessage.mock.calls[0][0];
    expect(request).toEqual({ type: "reader-render", id: expect.any(Number), event: "renderToolbar", attachmentKey: "attachment", itemKey: "parent", params: {} });
    expect(otherRequest.id).not.toBe(request.id);
    send({ type: "reader-rendered", id: request.id, html: "wrong" }, otherSource);
    send({ type: "reader-rendered", id: request.id, html: "<b>toolbar</b>" });
    send({ type: "reader-rendered", id: otherRequest.id, html: "<i>other</i>" }, otherSource);
    await expect(pending).resolves.toEqual([
      { plugin: "zotero:columns", html: "<b>toolbar</b>" }, { plugin: "zotero:other", html: "<i>other</i>" },
    ]);
  });

  it("validates menu items and routes commands to their plugin", async () => {
    const { host, send, source, otherSource } = await start(true);
    send({ type: "reader-events", types: ["createAnnotationContextMenu"] });
    const params = { ids: ["mark"], currentID: "mark" };
    const pending = host.readerMenu("createAnnotationContextMenu", "attachment", "parent", params);
    const request = source.postMessage.mock.calls[0][0];
    expect(request).toEqual({ type: "reader-menu", id: expect.any(Number), event: "createAnnotationContextMenu", attachmentKey: "attachment", itemKey: "parent", params });
    const settled = vi.fn();
    void pending.then(settled);
    send({ type: "reader-menu", id: request.id, items: [] }, otherSource);
    await Promise.resolve();
    expect(settled).not.toHaveBeenCalled();
    send({ type: "reader-menu", id: request.id, items: [
      { index: 0, label: "Run", disabled: false, onCommand: "ignored" }, { index: 1, label: "Unavailable", disabled: true },
      null, {}, { index: -1, label: "Bad" }, { index: 1.5, label: "Bad" }, { index: 2, label: 42 },
    ] });
    await expect(pending).resolves.toEqual([{ plugin: "zotero:columns", menu: request.id, items: [
      { index: 0, label: "Run", disabled: false }, { index: 1, label: "Unavailable", disabled: true },
    ] }]);
    host.readerCommand("zotero:columns", request.id, 0);
    expect(source.postMessage.mock.calls.at(-1)?.[0]).toEqual({ type: "reader-command", menu: request.id, index: 0 });
    expect(otherSource.postMessage).not.toHaveBeenCalled();
    host.readerCommand("missing", request.id, 0);
    expect(source.postMessage).toHaveBeenCalledTimes(2);
  });

  it("uses empty results for malformed reader replies", async () => {
    const { host, send, source } = await start();
    send({ type: "reader-events", types: ["renderToolbar", "createViewContextMenu"] });
    const render = host.readerRender("renderToolbar", "attachment", null, {});
    send({ type: "reader-rendered", id: source.postMessage.mock.calls.at(-1)![0].id, html: 42 });
    await expect(render).resolves.toEqual([{ plugin: "zotero:columns", html: "" }]);
    for (const items of [undefined, null, "text", 42, {}]) {
      const pending = host.readerMenu("createViewContextMenu", "attachment", null, {});
      const request = source.postMessage.mock.calls.at(-1)![0];
      send({ type: "reader-menu", id: request.id, items });
      await expect(pending).resolves.toEqual([{ plugin: "zotero:columns", menu: request.id, items: [] }]);
    }
  });

  it("times out reader requests together after five seconds and ignores late replies", async () => {
    const { host, send, source } = await start();
    send({ type: "reader-events", types: ["renderToolbar", "createViewContextMenu"] });
    const render = host.readerRender("renderToolbar", "attachment", null, {});
    const menu = host.readerMenu("createViewContextMenu", "attachment", null, {});
    const renderRequest = source.postMessage.mock.calls[0][0];
    const menuRequest = source.postMessage.mock.calls[1][0];
    const settled = vi.fn();
    void Promise.all([render, menu]).then(settled);
    await vi.advanceTimersByTimeAsync(4999);
    expect(settled).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    await expect(render).resolves.toEqual([{ plugin: "zotero:columns", html: "" }]);
    await expect(menu).resolves.toEqual([{ plugin: "zotero:columns", menu: menuRequest.id, items: [] }]);
    send({ type: "reader-rendered", id: renderRequest.id, html: "late" });
    send({ type: "reader-menu", id: menuRequest.id, items: [{ index: 0, label: "Late" }] });
    expect(settled).toHaveBeenCalledOnce();
  });

  it("settles pending reader requests immediately when a plugin stops", async () => {
    const { host, send, source, otherSource } = await start(true);
    send({ type: "reader-events", types: ["renderToolbar", "createViewContextMenu"] });
    send({ type: "reader-events", types: ["renderToolbar"] }, otherSource);
    const render = host.readerRender("renderToolbar", "attachment", "parent", {});
    const menu = host.readerMenu("createViewContextMenu", "attachment", "parent", {});
    const menuRequest = source.postMessage.mock.calls[1][0];
    host.stop("zotero:columns");
    await expect(menu).resolves.toEqual([{ plugin: "zotero:columns", menu: menuRequest.id, items: [] }]);
    send({ type: "reader-rendered", id: otherSource.postMessage.mock.calls[0][0].id, html: "other" }, otherSource);
    await expect(render).resolves.toEqual([{ plugin: "zotero:columns", html: "" }, { plugin: "zotero:other", html: "other" }]);
    expect(host.snapshot().readerEvents).toEqual({ "zotero:other": ["renderToolbar"] });
    await expect(host.readerMenu("createViewContextMenu", "attachment", "parent", {})).resolves.toEqual([]);
    const calls = source.postMessage.mock.calls.length;
    host.readerCommand("zotero:columns", menuRequest.id, 0);
    expect(source.postMessage).toHaveBeenCalledTimes(calls);
  });

  it("asks every Zotero plugin for item menus and routes commands with the selection", async () => {
    const { host, send, source, otherSource } = await start(true);
    const pending = host.itemMenu(["one"]);
    const request = source.postMessage.mock.calls[0][0];
    const otherRequest = otherSource.postMessage.mock.calls[0][0];
    expect(request).toEqual({ type: "item-menu", id: expect.any(Number), itemKeys: ["one"] });
    expect(otherRequest).toEqual({ ...request, id: expect.any(Number) });
    expect(otherRequest.id).not.toBe(request.id);
    const items = [{ label: "Read Status", disabled: false, children: [
      { index: 0, label: "Read", disabled: false }, { separator: true }, { index: 1, label: "Unread", disabled: true },
    ] }];
    const settled = vi.fn();
    void pending.then(settled);
    send({ type: "item-menu", id: request.id, items }, otherSource);
    send({ type: "item-menu", id: otherRequest.id, items: [] }, otherSource);
    await Promise.resolve();
    expect(settled).not.toHaveBeenCalled();
    send({ type: "item-menu", id: request.id, items });
    await expect(pending).resolves.toEqual([
      { plugin: "zotero:columns", menu: request.id, items }, { plugin: "zotero:other", menu: otherRequest.id, items: [] },
    ]);
    host.itemCommand("zotero:columns", request.id, 0, ["Read Status", "Read"], ["two"]);
    expect(source.postMessage.mock.calls.at(-1)).toEqual([{ type: "item-command", menu: request.id, index: 0, path: ["Read Status", "Read"], itemKeys: ["two"] }, "*"]);
    host.itemCommand("zotero:columns", request.id, 1, ["Run"], ["one"]);
    expect(source.postMessage.mock.calls.at(-1)).toEqual([{ type: "item-command", menu: request.id, index: 1, path: ["Run"], itemKeys: ["one"] }, "*"]);
    host.itemCommand("missing", request.id, 0, ["Read Status", "Read"], ["one"]);
    expect(source.postMessage).toHaveBeenCalledTimes(3);
  });

  it("strictly validates item menu replies and limits their total size and depth", async () => {
    const { host, send, source } = await start();
    const item = { index: 0, label: "Read", disabled: false };
    const submenu = { label: "Read Status", disabled: false, children: [item] };
    for (const items of [
      undefined, null, "text", {}, [null], [42], [{ ...item, label: 42 }], [{ ...item, label: "" }],
      [{ ...item, disabled: "false" }], [{ ...item, index: 0.5 }], [{ ...item, index: -1 }],
      [{ ...item, index: "0" }], [item, item], [{ ...submenu, children: [submenu] }],
      [{ ...submenu, children: "text" }], [{ separator: true, label: "mixed" }],
      Array.from({ length: 51 }, (_, index) => ({ ...item, index })),
      [{ ...submenu, children: Array.from({ length: 50 }, (_, index) => ({ ...item, index })) }],
    ]) {
      const pending = host.itemMenu(["one"]);
      const request = source.postMessage.mock.calls.at(-1)![0];
      send({ type: "item-menu", id: request.id, items });
      await expect(pending).resolves.toEqual([{ plugin: "zotero:columns", menu: request.id, items: [] }]);
    }
    const items = Array.from({ length: 50 }, (_, index) => ({ ...item, index }));
    const pending = host.itemMenu([]);
    const request = source.postMessage.mock.calls.at(-1)![0];
    send({ type: "item-menu", id: request.id, items });
    await expect(pending).resolves.toEqual([{ plugin: "zotero:columns", menu: request.id, items }]);
  });

  it("times out item menus after two seconds and ignores late replies", async () => {
    const { host, send, source } = await start();
    const pending = host.itemMenu(["one"]);
    const request = source.postMessage.mock.calls[0][0];
    const settled = vi.fn();
    void pending.then(settled);
    await vi.advanceTimersByTimeAsync(1999);
    expect(settled).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    await expect(pending).resolves.toEqual([{ plugin: "zotero:columns", menu: request.id, items: [] }]);
    send({ type: "item-menu", id: request.id, items: [{ index: 0, label: "Late", disabled: false }] });
    expect(settled).toHaveBeenCalledOnce();
  });

  it("settles item menus when a plugin stops while other plugins can still answer", async () => {
    const { host, send, source, otherSource } = await start(true);
    const pending = host.itemMenu(["one"]);
    const request = source.postMessage.mock.calls[0][0];
    const otherRequest = otherSource.postMessage.mock.calls[0][0];
    host.stop("zotero:columns");
    const items = [{ index: 0, label: "Read", disabled: false }];
    send({ type: "item-menu", id: otherRequest.id, items }, otherSource);
    await expect(pending).resolves.toEqual([
      { plugin: "zotero:columns", menu: request.id, items: [] }, { plugin: "zotero:other", menu: otherRequest.id, items },
    ]);
    const calls = source.postMessage.mock.calls.length;
    host.itemCommand("zotero:columns", request.id, 0, ["Read"], ["one"]);
    expect(source.postMessage).toHaveBeenCalledTimes(calls);
    host.stop("zotero:other");
    await expect(host.itemMenu(["one"])).resolves.toEqual([]);
  });
});

// Exercise the same transaction builder used by the live CM editor.
import { EditorSelection, EditorState, Transaction } from "@codemirror/state";
import { history, undo, undoDepth } from "@codemirror/commands";
import { pluginTransaction } from "../components/NoteEditor";
import type { EditorProposal, EditorSnapshot } from "./plugins";

function editorFixture() {
  const value = "A😀e\u0301Z\nsecond";
  const snapshot: EditorSnapshot = { protocol: 1, session: "session-a", generation: 1, revision: 3, contextRevision: 5,
    value, ranges: [{ anchor: 5, head: 1 }, { anchor: 8, head: 10 }], mainIndex: 1,
    mode: "source", focused: true, composing: false };
  const state = EditorState.create({ doc: value, selection: EditorSelection.create(snapshot.ranges.map(r => EditorSelection.range(r.anchor, r.head)), 1),
    extensions: [history(), EditorState.allowMultipleSelections.of(true)] });
  const proposal: EditorProposal = { operation: "op-1", session: snapshot.session, generation: 1, baseRevision: 3, baseContextRevision: 5,
    edits: [{ from: 1, to: 5, insert: "<mark>😀e\u0301</mark>" }, { from: 20, to: 20, insert: "!" }],
    ranges: [{ anchor: 18, head: 7 }, { anchor: 22, head: 24 }], mainIndex: 1 };
  return { snapshot, state, proposal };
}

describe("plugin editor transactions", () => {
  it("applies several UTF-16 edits in one isolated undo step, preserving directional selections", () => {
    const { snapshot, state, proposal } = editorFixture();
    const tr = state.update(pluginTransaction(state, snapshot, proposal)!);
    expect(tr.annotation(Transaction.userEvent)).toBe("plugin");
    expect(tr.newDoc.toString()).toBe("A<mark>😀e\u0301</mark>Z\n!second");
    expect(tr.newSelection.toJSON()).toEqual({ ranges: proposal.ranges, main: 1 });
    expect(undoDepth(tr.state)).toBe(1);
    let reverted = tr.state;
    expect(undo({ state: reverted, dispatch: transaction => { reverted = transaction.state; } })).toBe(true);
    expect(reverted.doc.toString()).toBe(snapshot.value);
    expect(reverted.selection.toJSON()).toEqual(state.selection.toJSON());
    // A prior user edit must remain a separate undo entry.
    const typed = state.update({ changes: { from: 0, insert: "x" }, annotations: Transaction.userEvent.of("input.type") }).state;
    const fresh = { ...snapshot, value: typed.doc.toString(), revision: 4, contextRevision: 6 };
    const single = { ...proposal, baseRevision: 4, baseContextRevision: 6, edits: [{ from: 0, to: 1, insert: "y" }], ranges: [{ anchor: 1, head: 1 }], mainIndex: 0 };
    const changed = typed.update(pluginTransaction(typed, fresh, single)!).state;
    expect(undoDepth(changed)).toBe(2);
  });

  it("refuses stale revisions, selections, generations, note sessions and composition", () => {
    const { snapshot, state, proposal } = editorFixture();
    for (const changed of [{ revision: 4 }, { contextRevision: 6 }, { session: "other" }, { generation: 2 }, { composing: true }]) {
      expect(pluginTransaction(state, { ...snapshot, ...changed }, proposal)).toBeNull();
    }
    expect(state.doc.toString()).toBe(snapshot.value);
  });

  it("validates the entire proposal before applying anything", () => {
    const { snapshot, state, proposal } = editorFixture();
    for (const invalid of [
      { edits: [...proposal.edits, { from: 999, to: 999, insert: "bad" }] },
      { ranges: [{ anchor: -1, head: 2 }] }, { mainIndex: 5 },
      { edits: [{ from: 1.5, to: 2, insert: "bad" }] }, { edits: null },
    ]) expect(pluginTransaction(state, snapshot, { ...proposal, ...invalid } as EditorProposal)).toBeNull();
    expect(state.doc.toString()).toBe(snapshot.value);
  });
});

describe("Obsidian editor host protocol", () => {
  afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); });

  it("binds senders, rejects stale/note-switch edits, and sends update then change then ack on acceptance", async () => {
    let receive: (event: MessageEvent) => Promise<void> = async () => {};
    vi.stubGlobal("window", { addEventListener: (_type: string, fn: typeof receive) => { receive = fn; } });
    const source = { postMessage: vi.fn() };
    const frame = { contentWindow: source, setAttribute: vi.fn(), remove: vi.fn() };
    vi.stubGlobal("document", { createElement: () => frame });
    vi.spyOn(backend, "obsidianPlugins").mockResolvedValue([{ id: "test", name: "Test", version: "1", enabled: true } as never]);
    vi.spyOn(backend, "zoteroPlugins").mockResolvedValue([]);
    const host = new PluginHost();
    host.setLayer({ appendChild: vi.fn() } as unknown as HTMLElement);
    await host.reload();
    const send = (data: object, sender: unknown = source) => receive({ source: sender, data } as MessageEvent);
    const messages = () => source.postMessage.mock.calls.map(([m]) => m);
    let { snapshot, state, proposal } = editorFixture();
    const apply = vi.fn((p: EditorProposal) => {
      const spec = pluginTransaction(state, snapshot, p);
      if (!spec) return false;
      state = state.update(spec).state;
      snapshot = { ...snapshot, value: state.doc.toString(), revision: snapshot.revision + 1, contextRevision: snapshot.contextRevision + 1,
        ranges: state.selection.ranges.map(({anchor, head}) => ({anchor, head})), mainIndex: state.selection.mainIndex };
      host.editorUpdated("plugin", "plugin");
      return true;
    });
    const note = { path: "one.md", state: () => snapshot, apply, setValue: vi.fn() };
    host.setActiveNote(note);
    expect(messages().at(-1)).toMatchObject({ type: "editor/open", editor: { ranges: snapshot.ranges, mainIndex: 1 } });
    await send({ type: "ready" });
    expect(messages().at(-1)).toMatchObject({ type: "editor/snapshot", editor: { session: "session-a" } });
    const enabled = host.check({ id: "test:check", name: "Check", plugin: "test", editor: true });
    const check = messages().at(-1);
    await send({ ...check, enabled: true }, {});
    await send({ type: "command/check", id: "test:check", request: check.request, enabled: true });
    expect(await enabled).toBe(true);
    await send({ type: "editor/propose", ...proposal }, {});
    expect(apply).not.toHaveBeenCalled();
    source.postMessage.mockClear();
    await send({ type: "editor/propose", ...proposal });
    expect(messages().map(m => m.type)).toEqual(["editor/update", "event", "editor/ack"]);
    expect(messages()[1]).toMatchObject({ name: "editor-change", editor: { revision: 4 } });
    state = state.update({ changes: { from: 0, insert: "typed" } }).state;
    snapshot = { ...snapshot, value: state.doc.toString(), revision: 5, contextRevision: 7 };
    host.editorUpdated("user", "input.type");
    source.postMessage.mockClear();
    await send({ type: "editor/propose", ...proposal });
    expect(messages().map(m => m.type)).toEqual(["editor/reject"]);
    expect(messages()[0].editor.revision).toBe(5);
    expect(state.doc.toString()).toMatch(/^typedA<mark>/);
    snapshot = { ...snapshot, session: "session-b", generation: 2 };
    host.setActiveNote({ ...note, path: "two.md" });
    expect(messages()).toContainEqual(expect.objectContaining({ type: "editor/close", session: "session-a" }));
    await send({ type: "editor/propose", ...proposal });
    expect(messages().at(-1)).toMatchObject({ type: "editor/reject", editor: { path: "two.md" } });
    host.setActiveNote(null);
    await send({ type: "editor/propose", ...proposal });
    expect(messages().at(-1)).toMatchObject({ type: "editor/reject", editor: null });
  });
});


describe("Obsidian suggestion/menu host", () => {
  afterEach(() => { vi.restoreAllMocks(); vi.unstubAllGlobals(); vi.useRealTimers(); });
  async function start() {
    vi.useFakeTimers();
    let receive: (event: MessageEvent) => Promise<void> = async () => {};
    vi.stubGlobal("window", { addEventListener: (_: string, fn: typeof receive) => { receive = fn; } });
    const sources = [0, 1].map(() => ({ postMessage: vi.fn() }));
    const frames = sources.map(contentWindow => ({ contentWindow, setAttribute: vi.fn(), remove: vi.fn() }));
    let frame = 0;
    vi.stubGlobal("document", { createElement: () => frames[frame++] });
    vi.spyOn(backend, "obsidianPlugins").mockResolvedValue(["first", "second"].map(id => ({ id, name: id, version: "1", enabled: true })) as never);
    vi.spyOn(backend, "zoteroPlugins").mockResolvedValue([]);
    const host = new PluginHost(); host.setLayer({ appendChild: vi.fn() } as unknown as HTMLElement); await host.reload();
    const send = (data: object, i = 0) => receive({ source: sources[i], data } as unknown as MessageEvent);
    const messages = (i = 0) => sources[i].postMessage.mock.calls.map(([m]) => m);
    let editor = { ...editorFixture().snapshot, ranges: [{ anchor: 4, head: 4 }], mainIndex: 0, focused: true, composing: false };
    host.setActiveNote({ path: "one.md", state: () => editor, apply: vi.fn(), setValue: vi.fn() });
    for (const i of [0, 1]) { await send({type: 'suggest/register'}, i); await send({type: 'ready'}, i); }
    const query = () => messages().filter(m => m.type === 'suggest/query').at(-1)!;
    const result = (priority = 0) => ({ ...editor, request: query().request, type: 'suggest/result', priority, start: 0, end: 4,
      items: [{ id: 'choice', label: '<img src=x>', secondary: 'text only' }], selected: 0, keys: [{key: 'Enter', modifiers: ['Shift']}] });
    return { host, send, messages, query, result, update: () => { editor = {...editor, revision: editor.revision + 1, contextRevision: editor.contextRevision + 1}; host.editorUpdated(); },
      move: () => { editor = {...editor, contextRevision: editor.contextRevision + 1, ranges: [{ anchor: 2, head: 2 }]}; host.editorUpdated(); } };
  }

  it("opens suggestions while typing, not when only the cursor moves", async () => {
    const r = await start();
    r.host.closeSuggestions();
    const queries = () => r.messages().filter(m => m.type === 'suggest/query').length;
    const before = queries();
    r.move();
    expect(queries()).toBe(before);
    r.update();
    expect(queries()).toBe(before + 1);
    // With a list open, moving the cursor asks again so it can follow or close.
    await r.send(r.result());
    r.move();
    expect(queries()).toBe(before + 2);
  });

  it("arbitrates by priority then load order, binds senders and forwards modifier keys", async () => {
    const r = await start();
    await r.send(r.result(0), 1);
    expect(r.host.suggestionSnapshot()).toBeNull();
    await r.send(r.result(0), 0);
    let suggestion = r.host.suggestionSnapshot()!;
    expect(suggestion.plugin).toBe('first');
    expect(suggestion.items).toEqual([{id: 'choice', label: '<img src=x>', secondary: 'text only'}]);
    r.host.chooseSuggestion(suggestion, 'choice', {key: 'Enter', shiftKey: true, ctrlKey: true, altKey: true, metaKey: true});
    expect(r.messages().at(-1)).toMatchObject({type: 'suggest/choose', id: 'choice', shiftKey: true, ctrlKey: true, altKey: true, metaKey: true});
    r.update();
    await r.send(r.result(20), 1); await r.send(r.result(5), 0);
    suggestion = r.host.suggestionSnapshot()!;
    expect(suggestion.plugin).toBe('second');
    r.host.closeSuggestions();
    expect(r.host.suggestionSnapshot()).toBeNull();
    expect(r.messages(1).at(-1)).toMatchObject({type: 'suggest/close'});
    const count = r.messages(1).length;
    r.host.chooseSuggestion(suggestion, 'choice', {key: 'Tab'});
    expect(r.messages(1)).toHaveLength(count);
  });

  it("drops stale context results, malformed payloads, timeouts, and late choices", async () => {
    const r = await start();
    const old = r.result(); r.update();
    await r.send(old); await r.send(old, 1);
    expect(r.host.suggestionSnapshot()).toBeNull();
    await r.send({...r.result(), contextRevision: -1});
    await r.send({...r.result(), items: [{id: 'x', label: {html: 'unsafe'}}]}, 1);
    expect(r.host.suggestionSnapshot()).toBeNull();
    await r.send(r.result());
    expect(r.host.suggestionSnapshot()?.plugin).toBe('first');
    const suggestion = r.host.suggestionSnapshot()!;
    r.update();
    r.host.chooseSuggestion(suggestion, 'choice', {key: 'Enter'});
    expect(r.messages().at(-1).type).toBe('suggest/query');
    await vi.advanceTimersByTimeAsync(1000);
    await r.send(r.result(100));
    expect(r.host.suggestionSnapshot()).toBeNull();
    r.host.setActiveNote(null);
    await r.send(old);
    expect(r.host.suggestionSnapshot()).toBeNull();
  });

  it("validates editor menus, routes choices to their owner and drops menus after context changes", async () => {
    const r = await start();
    const pending = r.host.editorMenu();
    const requests = [0, 1].map(i => r.messages(i).filter(m => m.type === 'menu/request').at(-1)!);
    const reply = (i: number) => ({...requests[i].editor, request: requests[i].request, type: 'menu/result', items: [
      {index: 0, label: 'Plugin action', disabled: false}, {index: 1, label: 'Disabled', disabled: true}]});
    await r.send(reply(0), 1); // A different registered frame cannot answer for it.
    await r.send(reply(1), 1); await r.send(reply(0), 0);
    const menus = await pending;
    expect(menus.map(m => m.plugin)).toEqual(['first', 'second']);
    r.host.chooseEditorMenu(menus[0], 1);
    expect(r.messages().filter(m => m.type === 'menu/choose')).toHaveLength(0);
    r.host.chooseEditorMenu(menus[1], 0, {shiftKey: true});
    expect(r.messages(1).filter(m => m.type === 'menu/choose')).toMatchObject([{request: requests[1].request, index: 0, shiftKey: true}]);
    const next = r.host.editorMenu(); r.update();
    expect(await next).toEqual([]);
    const before = r.messages().length;
    r.host.chooseEditorMenu(menus[0], 0);
    expect(r.messages()).toHaveLength(before);
  });
});
