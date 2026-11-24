// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { AddinApp } from "./AddinApp";
import { ConnectorError, type CiteConnector, type SearchItem } from "./connector";
import { MemoryDocument } from "./memoryDocument";
import { DEFAULT_STYLE } from "./session";

const item: SearchItem = { id: "one", key: "one2026", title: "First work", cite: "Author 2026", year: 2026 };

describe("shared task pane", () => {
  let container: HTMLDivElement;
  let root: Root;
  let model: MemoryDocument;
  let client: CiteConnector;
  let paired: boolean;

  beforeEach(() => {
    vi.useFakeTimers();
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    container = document.createElement("div");
    document.body.append(container);
    root = createRoot(container);
    model = new MemoryDocument();
    paired = true;
    client = {
      hasToken: () => paired,
      ping: vi.fn<CiteConnector["ping"]>().mockResolvedValue({ app: "Paperloom", version: "0.1.0", cite: 1 }),
      pair: vi.fn<CiteConnector["pair"]>(async () => { paired = true; return "token"; }),
      styles: vi.fn<CiteConnector["styles"]>().mockResolvedValue([{ id: DEFAULT_STYLE, name: "Numeric" }]),
      search: vi.fn<CiteConnector["search"]>().mockResolvedValue([item]),
      format: vi.fn<CiteConnector["format"]>(async (_style, clusters) => ({
        citations: clusters.map((_, index) => `[${index + 1}]`),
        bibliography: clusters.length ? ["[1] <i>First work</i>"] : [],
        missing: [],
      })),
      markers: vi.fn<CiteConnector["markers"]>().mockResolvedValue([]),
    };
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    container.remove();
    vi.useRealTimers();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  const render = async () => {
    await act(async () => root.render(createElement(AddinApp, { client, document: model, clientName: "Paperloom", preview: model })));
  };

  const button = (label: string) => {
    const found = [...container.querySelectorAll<HTMLButtonElement>("button")].find((candidate) => candidate.textContent === label);
    if (!found) throw new Error(`Button missing: ${label}`);
    return found;
  };

  const searchInput = () => container.querySelector<HTMLInputElement>("#addin-search")!;

  const input = async (element: HTMLInputElement, value: string) => {
    await act(async () => {
      // Use the native setter so React sees a user edit rather than its own value.
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!.call(element, value);
      element.dispatchEvent(new Event("input", { bubbles: true }));
    });
  };

  const tick = async () => { await act(async () => { await vi.advanceTimersByTimeAsync(300); }); };

  it("shows checking, then lets the person retry when Paperloom is not running", async () => {
    let rejectPing!: (error: Error) => void;
    vi.mocked(client.ping).mockReturnValueOnce(new Promise((_resolve, reject) => { rejectPing = reject; }));
    await render();
    expect(container.textContent).toContain("Checking Paperloom…");
    await act(async () => rejectPing(new ConnectorError("not-running", "Offline")));
    expect(container.textContent).toContain("Paperloom is not running on this computer");
    await act(async () => button("Try Again").click());
    expect(searchInput()).not.toBeNull();
  });

  it.each(["declined", "busy"] as const)("shows the %s connection error and allows pairing again", async (code) => {
    paired = false;
    vi.mocked(client.pair).mockRejectedValueOnce(new ConnectorError(code, "Pairing failed"));
    await render();
    expect(container.textContent).toContain("Paperloom will ask you to allow this task pane.");
    await act(async () => button("Connect").click());
    expect(container.textContent).toContain(code === "declined" ? "The connection was declined" : "Another connection request is waiting");
    await act(async () => button("Connect").click());
    expect(client.pair).toHaveBeenLastCalledWith("Paperloom");
    expect(searchInput()).not.toBeNull();
  });

  it("debounces queries, ignores old responses, and keeps chosen works across searches", async () => {
    let resolveOld!: (items: SearchItem[]) => void;
    vi.mocked(client.search).mockReturnValueOnce(new Promise((resolve) => { resolveOld = resolve; }));
    await render();
    await tick();
    await input(searchInput(), "new");
    expect(client.search).toHaveBeenCalledTimes(1);
    await tick();
    expect(client.search).toHaveBeenLastCalledWith("new", 20);
    await act(async () => container.querySelector<HTMLInputElement>('input[type="checkbox"]')!.click());
    await input(searchInput(), "no match");
    vi.mocked(client.search).mockResolvedValueOnce([]);
    await tick();
    await act(async () => resolveOld([{ ...item, id: "stale", title: "Stale result" }]));
    expect(container.textContent).not.toContain("Stale result");
    expect(container.textContent).toContain("Selected works: 1");
    expect(container.textContent).toContain("First work");
    await input(container.querySelector<HTMLInputElement>("#addin-page")!, "12");
    await act(async () => button("Insert Citation").click());
    expect((await model.getCitations())[0].works).toEqual([{ itemId: "one", locator: "12" }]);
  });

  it("returns to Connect after a rejected token during search", async () => {
    vi.mocked(client.search).mockRejectedValueOnce(new ConnectorError("not-connected", "Token removed"));
    await render();
    await tick();
    expect(container.textContent).toContain("Not connected to Paperloom");
    expect(button("Connect")).toBeDefined();
  });

  it("converts citation markers and names the ones it leaves", async () => {
    model = new MemoryDocument([{ kind: "text", text: "Heat [@kucsko] and [@nobody]." }]);
    vi.mocked(client.markers).mockResolvedValue([
      { text: "[@kucsko]", works: [{ itemId: "one" }], missing: [] },
      { text: "[@nobody]", works: [], missing: ["nobody"] },
    ]);
    await render();
    await act(async () => button("Convert Citation Markers").click());
    expect(client.markers).toHaveBeenCalledWith("Heat [@kucsko] and [@nobody].");
    expect((await model.getCitations()).map(({ works, text }) => ({ works, text }))).toEqual([{ works: [{ itemId: "one" }], text: "[1]" }]);
    expect(container.textContent).toContain("Citation markers converted: 1.");
    expect(container.textContent).toContain("the library lacks a work they name: [@nobody]");
    expect(model.getRuns().map((run) => run.kind)).toEqual(["text", "citation", "text"]);
  });

  it("reports missing works by count and confirms before unlinking", async () => {
    const citation = await model.insertCitation([{ itemId: "missing" }]);
    await model.setCitationText(citation.id, "<sup>[7]</sup>");
    await model.setBibliography(["[7] <i>Saved title</i>"]);
    vi.mocked(client.format).mockResolvedValue({ citations: [""], bibliography: [], missing: ["missing"] });
    await render();
    await act(async () => button("Update Citations").click());
    expect(container.textContent).toContain("Works missing from the library: 1.");
    await act(async () => button("Convert to Plain Text").click());
    expect(await model.getCitations()).toHaveLength(1);
    expect(document.activeElement).toBe(button("Cancel"));
    await act(async () => button("Cancel").click());
    expect(await model.getCitations()).toHaveLength(1);
    await act(async () => button("Convert to Plain Text").click());
    await act(async () => {
      const confirmation = container.querySelector<HTMLElement>('[role="group"]')!;
      [...confirmation.querySelectorAll("button")].find((candidate) => candidate.textContent === "Convert to Plain Text")!.click();
    });
    expect(await model.getCitations()).toEqual([]);
    expect(container.textContent).toContain("Citations and the reference list are now plain text.");
    expect(model.getRuns().some((run) => run.kind === "text" && run.text === "[7]")).toBe(true);
  });
});
