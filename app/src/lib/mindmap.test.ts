import { describe, expect, it } from "vitest";
import { fitView, onStep, presentationSteps, revealTopic, stepBounds, withMapTheme } from "./mindmap";

describe("map themes", () => {
  it("are written beside the map's own property", () => {
    const map = "---\nmindmap: logic\n---\n# Centre\n";
    const themed = withMapTheme(map, "ocean");
    expect(themed).toBe("---\nmindmap: logic\nmindmap-theme: ocean\n---\n# Centre\n");
    expect(withMapTheme(themed, "dark")).toBe("---\nmindmap: logic\nmindmap-theme: dark\n---\n# Centre\n");
    expect(withMapTheme("# Plain note\n", "dark")).toBe("# Plain note\n");
  });
});

describe("presentations", () => {
  const topic = (id: string, depth: number, x: number, y: number) => ({ id, depth, x, y, width: 100, height: 40 });
  const topics = [topic("0", 0, 200, 200), topic("0.0", 1, 400, 100), topic("0.0.0", 2, 600, 80), topic("0.1", 1, 400, 300)];

  it("show the whole map, then each main branch", () => {
    expect(presentationSteps(topics)).toEqual(["0", "0.0", "0.1"]);
    expect(onStep("0.0.0", "0.0")).toBe(true);
    expect(onStep("0.1", "0.0")).toBe(false);
    expect(onStep("0", "0.1")).toBe(true);
    expect(stepBounds(topics, "0.0", 900, 500)).toEqual({ left: 400, top: 80, right: 700, bottom: 140 });
    expect(stepBounds(topics, "0", 900, 500)).toEqual({ left: 0, top: 0, right: 900, bottom: 500 });
  });

  it("fit a step in the view", () => {
    const view = fitView({ left: 400, top: 80, right: 700, bottom: 140 }, { width: 800, height: 600 });
    expect(view.zoom).toBe(2);
    expect(view.x + 400 * view.zoom).toBeCloseTo((800 - 300 * 2) / 2);
  });
});

describe("topic visibility", () => {
  const frame = { width: 800, height: 600 };
  const view = { x: 50, y: -30, zoom: 2 };

  it("leaves a visible topic and its view unchanged", () => {
    const topic = { x: 100, y: 100, width: 100, height: 40 };
    expect(revealTopic(topic, frame, view)).toBe(view);
  });

  it("leaves a topic at the margin unchanged", () => {
    const view = { x: 0, y: 0, zoom: 1 };
    const topic = { x: 24, y: 24, width: 752, height: 552 };
    expect(revealTopic(topic, frame, view)).toBe(view);
  });

  it.each([
    { edge: "left", x: -30, y: 100, panX: 84, panY: -30 },
    { edge: "right", x: 350, y: 100, panX: -124, panY: -30 },
    { edge: "top", x: 100, y: 0, panX: 50, panY: 24 },
    { edge: "bottom", x: 100, y: 280, panX: 50, panY: -64 },
  ])("pans only enough to clear the $edge edge", ({ x, y, panX, panY }) => {
    const topic = { x, y, width: 100, height: 40 };
    const moved = revealTopic(topic, frame, view);
    expect(moved).toEqual({ x: panX, y: panY, zoom: view.zoom });
    expect(revealTopic(topic, frame, moved)).toBe(moved);
  });

  it("reveals both axes when zoomed out with a custom margin", () => {
    const topic = { x: 900, y: 700, width: 100, height: 40 };
    const view = { x: -10, y: -20, zoom: 0.5 };
    expect(revealTopic(topic, { width: 400, height: 300 }, view, 16)).toEqual({ x: -116, y: -86, zoom: 0.5 });
  });

  it("keeps an oversized topic still while it fills the frame", () => {
    const topic = { x: 0, y: 0, width: 500, height: 300 };
    const frame = { width: 300, height: 200 };
    const view = { x: -80, y: -50, zoom: 1 };
    expect(revealTopic(topic, frame, view)).toBe(view);
    const moved = revealTopic(topic, frame, { ...view, x: 400, y: 200 });
    expect(moved).toEqual({ x: 24, y: 24, zoom: 1 });
    expect(revealTopic(topic, frame, moved)).toBe(moved);
  });

  it("waits for a frame with a visible size", () => {
    const topic = { x: 900, y: 700, width: 100, height: 40 };
    expect(revealTopic(topic, { width: 0, height: 600 }, view)).toBe(view);
    expect(revealTopic(topic, { width: 800, height: 0 }, view)).toBe(view);
  });
});
