// @vitest-environment happy-dom
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { backend, type NoteGraph } from "../lib/api";
import { GraphView } from "./GraphView";

vi.mock("../lib/api", () => ({ backend: { vaultGraph: vi.fn() } }));

const graph: NoteGraph = {
  nodes: [
    { id: "Note.md", title: "Note", kind: "note", itemId: null, degree: 1, x: 0, y: 0 },
    { id: "paper", title: "Paper", kind: "item", itemId: "paper", degree: 1, x: 200, y: 100 },
  ],
  edges: [{ from: 0, to: 1 }],
};

describe("graph touch controls", () => {
  let host: HTMLDivElement;
  let root: Root;
  const onOpenNote = vi.fn();
  const onOpenItem = vi.fn();

  beforeEach(async () => {
    vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
    vi.mocked(backend.vaultGraph).mockResolvedValue(graph);
    onOpenNote.mockClear();
    onOpenItem.mockClear();
    host = document.createElement("div");
    document.body.append(host);
    root = createRoot(host);
    await act(async () => root.render(createElement(GraphView, { onOpenNote, onOpenItem })));
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("zooms around the centre and fits again without a wheel", async () => {
    const svg = host.querySelector("svg.graph")!;
    const initial = svg.getAttribute("viewBox")!;
    const readBox = () => svg.getAttribute("viewBox")!.split(" ").map(Number);
    const [x, y, w, h] = readBox();
    await act(async () => host.querySelector<HTMLButtonElement>('[aria-label="Zoom In"]')!.click());
    const [nextX, nextY, nextW, nextH] = readBox();
    expect(nextW).toBeCloseTo(w / 1.25);
    expect(nextH).toBeCloseTo(h / 1.25);
    expect(nextX + nextW / 2).toBeCloseTo(x + w / 2);
    expect(nextY + nextH / 2).toBeCloseTo(y + h / 2);
    await act(async () => host.querySelector<HTMLButtonElement>('[aria-label="Zoom Out"]')!.click());
    expect(readBox()[2]).toBeCloseTo(w);
    await act(async () => Array.from(host.querySelectorAll("button")).find((button) => button.textContent === "Fit")!.click());
    expect(svg.getAttribute("viewBox")).toBe(initial);
  });

  it("opens nodes through the larger hit area but does not open after a pan", async () => {
    const hits = host.querySelectorAll<SVGCircleElement>(".node-hit");
    await act(async () => hits[0].dispatchEvent(new MouseEvent("click", { bubbles: true })));
    await act(async () => hits[1].dispatchEvent(new MouseEvent("click", { bubbles: true })));
    expect(onOpenNote).toHaveBeenCalledWith("Note.md");
    expect(onOpenItem).toHaveBeenCalledWith("paper", "Paper");
    onOpenNote.mockClear();
    const svg = host.querySelector<SVGSVGElement>("svg.graph")!;
    svg.setPointerCapture = vi.fn();
    vi.spyOn(svg, "getBoundingClientRect").mockReturnValue(new DOMRect(0, 0, 390, 680));
    const initial = svg.getAttribute("viewBox");
    await act(async () => hits[0].dispatchEvent(new PointerEvent("pointerdown", { bubbles: true, button: 0, clientX: 100, clientY: 200 })));
    await act(async () => svg.dispatchEvent(new PointerEvent("pointermove", { bubbles: true, clientX: 140, clientY: 250 })));
    await act(async () => svg.dispatchEvent(new PointerEvent("pointerup", { bubbles: true })));
    await act(async () => hits[0].dispatchEvent(new MouseEvent("click", { bubbles: true })));
    expect(svg.getAttribute("viewBox")).not.toBe(initial);
    expect(onOpenNote).not.toHaveBeenCalled();
  });
});
