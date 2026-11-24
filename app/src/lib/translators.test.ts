import { afterEach, describe, expect, it, vi } from "vitest";
import { backend, memoryBackend } from "./api";
import { TranslatorHost } from "./translators";

const translator = { id: "test", label: "Test", target: "", priority: 100 };
const hosts: TranslatorHost[] = [];
afterEach(() => {
  for (const host of hosts.splice(0)) host.dispose();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
  vi.useRealTimers();
});

async function setup() {
  vi.useFakeTimers();
  let receive: (event: MessageEvent) => void = () => {};
  const source = { postMessage: vi.fn() };
  const frame = { contentWindow: source, setAttribute: vi.fn(), remove: vi.fn(), hidden: false, src: "", title: "" };
  vi.stubGlobal("window", { addEventListener: (_: string, fn: typeof receive) => { receive = fn; }, removeEventListener: vi.fn() });
  vi.stubGlobal("navigator", { userAgent: "Linux" });
  vi.stubGlobal("document", { createElement: () => frame, body: { appendChild: vi.fn() } });
  vi.spyOn(backend, "webTranslators").mockResolvedValue([translator]);
  const host = new TranslatorHost();
  hosts.push(host);
  const send = (data: object, from: unknown = source) => receive({ source: from, data } as MessageEvent);
  const detecting = host.detect("https://example.org/", "<title>Page</title>");
  await vi.advanceTimersByTimeAsync(0);
  send({ type: "loaded" });
  await vi.advanceTimersByTimeAsync(0);
  const detect = source.postMessage.mock.calls.at(-1)![0];
  return { host, send, source, frame, detecting, detect };
}

async function detected() {
  const state = await setup();
  state.send({ type: "detected", id: state.detect.id, translator: "test", label: "Test", itemType: "multiple" });
  await state.detecting;
  return state;
}

describe("translator host", () => {
  it("uses a hidden sandbox and trusts only replies from its frame with matching ids", async () => {
    const { host, send, source, frame, detecting, detect } = await setup();
    expect(frame.hidden).toBe(true);
    expect(frame.setAttribute).toHaveBeenCalledWith("sandbox", "allow-scripts");
    expect(detect).toEqual({ type: "translate", id: expect.any(Number), url: "https://example.org/", html: "<title>Page</title>", translators: [translator] });
    const resolved = vi.fn();
    void detecting.then(resolved);
    send({ type: "detected", id: detect.id, translator: "wrong", label: "Wrong", itemType: "book" }, {});
    send({ type: "detected", id: detect.id + 1, translator: "wrong" });
    send({ type: "translated", id: detect.id, items: [] });
    await vi.advanceTimersByTimeAsync(0);
    expect(resolved).not.toHaveBeenCalled();
    send({ type: "detected", id: detect.id, translator: "test", label: "Test", itemType: "book" });
    await expect(detecting).resolves.toEqual({ id: "test", label: "Test", itemType: "book" });
    const saving = host.save();
    const request = source.postMessage.mock.calls.at(-1)![0];
    send({ type: "translated", id: request.id, items: [{ title: "A book" }] });
    await expect(saving).resolves.toEqual([{ title: "A book" }]);
  });

  it("answers code, network and selection calls, filtering keys and rejecting unknown methods", async () => {
    const { host, send, source } = await detected();
    vi.spyOn(backend, "translatorCode").mockResolvedValue("code");
    const network = vi.spyOn(backend, "translatorRequest").mockResolvedValue({ status: 201, text: "answer", url: "https://example.org/final" });
    const picker = vi.fn().mockResolvedValue(["b", "unlisted"]);
    host.setPicker(picker);
    const saving = host.save();
    const request = source.postMessage.mock.calls.at(-1)![0];
    for (const data of [
      { id: 10, method: "translatorCode", args: ["test"] },
      { id: 11, method: "request", args: [{ url: "https://example.org/", method: "POST", headers: { Accept: "text/plain" }, body: "x=1" }] },
      { id: 12, method: "selectItems", args: [{ a: "One", b: "Two" }] },
      { id: 13, method: "unsupported", args: [] },
      { id: 14, method: "request", args: [{ url: "file:///etc/passwd" }] },
    ]) send({ type: "call", ...data });
    await vi.advanceTimersByTimeAsync(0);
    const replies = source.postMessage.mock.calls.map(([data]) => data).filter((data) => data.type === "reply");
    expect(replies).toEqual(expect.arrayContaining([
      { type: "reply", id: 10, value: "code" },
      { type: "reply", id: 11, value: { status: 201, text: "answer", url: "https://example.org/final" } },
      { type: "reply", id: 12, value: ["b"] },
      { type: "reply", id: 13, error: expect.any(String) },
      { type: "reply", id: 14, error: expect.any(String) },
    ]));
    expect(network).toHaveBeenCalledOnce();
    expect(picker).toHaveBeenCalledWith({ a: "One", b: "Two" }, expect.any(AbortSignal));
    send({ type: "translated", id: request.id, items: [] });
    await saving;
  });

  it("propagates host failures and translator errors", async () => {
    const { host, send, source } = await detected();
    vi.spyOn(backend, "translatorCode").mockRejectedValue(new Error("Missing file"));
    send({ type: "call", id: 8, method: "translatorCode", args: ["gone"] });
    await vi.advanceTimersByTimeAsync(0);
    expect(source.postMessage.mock.calls.at(-1)![0]).toEqual({ type: "reply", id: 8, error: "Missing file" });
    const saving = host.save();
    const request = source.postMessage.mock.calls.at(-1)![0];
    send({ type: "translate-error", id: request.id, message: "Cannot translate" });
    await expect(saving).rejects.toThrow("Cannot translate");
  });

  it("times out, aborts selection, removes the frame, and ignores late replies", async () => {
    const { host, send, source, frame } = await detected();
    let signal: AbortSignal | undefined;
    host.setPicker((_, abort) => { signal = abort; return new Promise(() => {}); });
    const saving = host.save();
    const rejected = expect(saving).rejects.toThrow("60 seconds");
    send({ type: "call", id: 8, method: "selectItems", args: [{ a: "One" }] });
    await vi.advanceTimersByTimeAsync(60000);
    await rejected;
    expect(signal?.aborted).toBe(true);
    expect(frame.remove).toHaveBeenCalled();
    const before = source.postMessage.mock.calls.length;
    send({ type: "translated", id: 2, items: [{ title: "Late" }] });
    expect(source.postMessage.mock.calls.length).toBe(before);
  });

  it("handles no detection and avoids creating a frame when no translators are installed", async () => {
    const { host, send, detecting, detect } = await setup();
    send({ type: "detected", id: detect.id, translator: null });
    await expect(detecting).resolves.toBeNull();
    await expect(host.save()).rejects.toThrow("No translator");
    vi.spyOn(backend, "webTranslators").mockResolvedValue([]);
    await expect(host.detect("https://example.org/", "")).resolves.toBeNull();
  });

  it("rejects oversized HTML and invalid completed items", async () => {
    const { host, send, source } = await detected();
    const saving = host.save();
    const request = source.postMessage.mock.calls.at(-1)![0];
    send({ type: "translated", id: request.id, items: ["invalid"] });
    await expect(saving).rejects.toThrow("invalid items");
    await expect(host.detect("https://example.org/", "字".repeat(2 * 1024 * 1024))).rejects.toThrow("5 MB");
  });
});

describe("translator preview backend", () => {
  it("updates installation status and keeps converted results addable", async () => {
    const preview = memoryBackend();
    expect(await preview.translatorsStatus()).toEqual({ count: 0, downloaded: null });
    await preview.downloadTranslators();
    const events: { kind: string }[] = [];
    const stop = await preview.onBrowserEvent((event) => events.push(event));
    await preview.browserOpen("example.org");
    expect(events.map((event) => event.kind)).toEqual(["navigating", "showing", "html"]);
    stop();
    expect((await preview.webTranslators()).length).toBe(1);
    const page = await preview.readTranslatedItems(JSON.stringify([{ itemType: "journalArticle", title: "文献", creators: [{ firstName: "三", lastName: "张" }], tags: ["研究"], attachments: [{ url: "https://example.org/full.pdf", mimeType: "application/pdf" }] }]), "https://example.org/");
    expect(page.hits[0].detail.authors).toEqual(["张三"]);
    expect(page.hits[0].pdfUrl).toBe("https://example.org/full.pdf");
    const added = await preview.addSearchResults([page.hits[0].key], null);
    expect(added.added).toBe(1);
    expect((await preview.itemDetail(added.addedIds[0]))?.tags).toEqual(["研究"]);
    await preview.removeTranslators();
    expect(await preview.webTranslators()).toEqual([]);
  });
});
