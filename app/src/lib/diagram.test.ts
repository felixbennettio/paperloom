import { describe, expect, it } from "vitest";
import type { DiagramDrawn } from "./api";
import { aligned, boundsOf, boxBetween, connectionPoints, distanceToLine, freeSpot, handles, hitItem, resized, rotateKnob, shapeDragStart, snap, styleOf, within } from "./diagram";

const item = (patch: Partial<DiagramDrawn>): DiagramDrawn => ({
  id: "x",
  edge: false,
  parent: "",
  x: 0,
  y: 0,
  width: 100,
  height: 50,
  rotation: 0,
  path: "",
  inner: "",
  arrows: "",
  openArrows: "",
  fill: "#ffffff",
  stroke: "#000000",
  strokeWidth: 1,
  dashed: false,
  opacity: 1,
  text: "",
  textX: 0,
  textY: 0,
  textWidth: 0,
  textHeight: 0,
  fontSize: 12,
  fontColor: "#000000",
  bold: false,
  italic: false,
  underline: false,
  align: "center",
  verticalAlign: "middle",
  labelBackground: "",
  wrap: true,
  source: "",
  target: "",
  points: [],
  bends: [],
  container: false,
  style: "",
  image: "",
  imageAspect: true,
  ...patch,
});

describe("diagram geometry", () => {
  it("reads styles", () => {
    const style = styleOf("ellipse;fillColor=#fff; html=1;;");
    expect(style.get("ellipse")).toBe("ellipse");
    expect(style.get("fillColor")).toBe("#fff");
    expect(style.get("html")).toBe("1");
  });

  it("finds what the pointer is on, the topmost first", () => {
    const below = item({ id: "below" });
    const above = item({ id: "above", x: 50 });
    const edge = item({ id: "edge", edge: true, points: [{ x: 0, y: 200 }, { x: 100, y: 200 }] });
    const items = [below, above, edge];
    expect(hitItem(items, { x: 60, y: 10 }, 4)?.id).toBe("above");
    expect(hitItem(items, { x: 10, y: 10 }, 4)?.id).toBe("below");
    expect(hitItem(items, { x: 50, y: 203 }, 4)?.id).toBe("edge");
    expect(hitItem(items, { x: 500, y: 500 }, 4)).toBeNull();
    const container = item({ id: "box", container: true, width: 200, height: 200, style: "swimlane;" });
    expect(hitItem([container], { x: 100, y: 100 }, 4)).toBeNull();
    expect(hitItem([container], { x: 100, y: 10 }, 4)?.id).toBe("box");
    expect(distanceToLine({ x: 5, y: 5 }, [{ x: 0, y: 0 }, { x: 10, y: 0 }])).toBe(5);
  });

  it("resizes by handles", () => {
    const box = { x: 0, y: 0, width: 100, height: 50 };
    expect(handles(box).find((h) => h.handle === "se")).toEqual({ handle: "se", x: 100, y: 50 });
    expect(resized(box, "e", 20, 99)).toEqual({ x: 0, y: 0, width: 120, height: 50 });
    expect(resized(box, "nw", 10, 10)).toEqual({ x: 10, y: 10, width: 90, height: 40 });
    expect(resized(box, "w", 500, 0)).toEqual({ x: 90, y: 0, width: 10, height: 50 });
    expect(resized(box, "se", 100, 0, true)).toEqual({ x: 0, y: 0, width: 200, height: 100 });
  });

  it("starts connections near ports regardless of selection, including overlapping resize handles", () => {
    const box = { x: 20, y: 30, width: 100, height: 60 };
    for (const zoom of [0.5, 1, 2]) {
      for (const from of connectionPoints(box)) {
        for (const offset of [{ x: 0, y: 0 }, { x: 4, y: 4 }, { x: 7, y: 0 }]) {
          const p = { x: from.x + offset.x / zoom, y: from.y + offset.y / zoom };
          expect(shapeDragStart(p, box, null, zoom)).toEqual({ kind: "connect", from });
          expect(shapeDragStart(p, box, box, zoom)).toEqual({ kind: "connect", from });
        }
      }
      const from = connectionPoints(box)[1];
      expect(shapeDragStart({ x: from.x + 7.1 / zoom, y: from.y }, box, box, zoom)).toBeNull();
    }
  });

  it("keeps corner resize and rotate controls available with visible ports", () => {
    const box = { x: 20, y: 30, width: 100, height: 60 };
    for (const zoom of [0.5, 1, 2]) {
      for (const grip of handles(box).filter((h) => h.handle.length === 2)) {
        const p = { x: grip.x + 3 / zoom, y: grip.y + 3 / zoom };
        expect(shapeDragStart(p, box, box, zoom)).toEqual({ kind: "resize", handle: grip.handle });
        expect(shapeDragStart(p, box, null, zoom)).toBeNull();
      }
      const knob = rotateKnob(box, zoom);
      expect(shapeDragStart({ x: knob.x + 6 / zoom, y: knob.y }, box, box, zoom)).toEqual({ kind: "rotate" });
      expect(shapeDragStart(knob, box, null, zoom)).toBeNull();
    }
  });

  it("snaps to the grid and to other shapes", () => {
    expect(snap(14, 10)).toBe(10);
    expect(snap(16, 10)).toBe(20);
    expect(snap(16, 0)).toBe(16);
    const result = aligned({ x: 103, y: 300, width: 50, height: 50 }, [{ x: 100, y: 0, width: 50, height: 50 }], 5);
    expect(result).toEqual({ dx: -3, dy: 0, guides: [{ vertical: true, at: 100 }, { vertical: true, at: 125 }, { vertical: true, at: 150 }] });
    expect(aligned({ x: 120, y: 300, width: 10, height: 10 }, [{ x: 0, y: 0, width: 10, height: 10 }], 5).dx).toBe(0);
  });

  it("keeps a free spot unchanged", () => {
    const wanted = { x: 30, y: 40, width: 100, height: 50 };
    expect(freeSpot([], wanted)).toEqual({ x: 30, y: 40 });
    expect(freeSpot([{ ...wanted, x: 150 }], wanted)).toEqual({ x: 30, y: 40 });
  });

  it("places a shape to the right of an occupied centre", () => {
    const wanted = { x: 30, y: 40, width: 100, height: 50 };
    expect(freeSpot([wanted], wanted)).toEqual({ x: 150, y: 40 });
    expect(freeSpot([{ ...wanted, x: -80 }], wanted)).toEqual({ x: 150, y: 40 });
    expect(freeSpot([wanted], wanted, 10)).toEqual({ x: 140, y: 40 });
  });

  it("tries below, then below-right when the right is occupied", () => {
    const wanted = { x: 30, y: 40, width: 100, height: 50 };
    const boxes = [wanted, { ...wanted, x: 150 }];
    expect(freeSpot(boxes, wanted)).toEqual({ x: 30, y: 110 });
    boxes.push({ ...wanted, y: 110 });
    expect(freeSpot(boxes, wanted)).toEqual({ x: 150, y: 110 });
    boxes.push({ ...wanted, x: 150, y: 110 });
    expect(freeSpot(boxes, wanted)).toEqual({ x: -90, y: 40 });
  });

  it("searches through ring four and falls back when every spot is occupied", () => {
    const wanted = { x: 30, y: 40, width: 100, height: 50 };
    const boxes = [];
    for (let x = -4; x <= 4; x++) for (let y = -4; y <= 4; y++) {
      boxes.push({ ...wanted, x: wanted.x + x * 120, y: wanted.y + y * 70 });
    }
    expect(freeSpot(boxes, wanted)).toEqual({ x: 30, y: 40 });
    expect(freeSpot(boxes.slice(0, -1), wanted)).toEqual({ x: 510, y: 320 });
  });

  it("works with boxes", () => {
    expect(boxBetween({ x: 10, y: 0 }, { x: 0, y: 10 })).toEqual({ x: 0, y: 0, width: 10, height: 10 });
    expect(within({ x: 1, y: 1, width: 5, height: 5 }, { x: 0, y: 0, width: 10, height: 10 })).toBe(true);
    expect(within({ x: 8, y: 1, width: 5, height: 5 }, { x: 0, y: 0, width: 10, height: 10 })).toBe(false);
    expect(connectionPoints({ x: 0, y: 0, width: 10, height: 20 })).toEqual([
      { x: 5, y: 0 },
      { x: 10, y: 10 },
      { x: 5, y: 20 },
      { x: 0, y: 10 },
    ]);
    expect(boundsOf([])).toBeNull();
    expect(boundsOf([{ x: 0, y: 0, width: 1, height: 1 }, { x: 5, y: 5, width: 1, height: 1 }])).toEqual({ x: 0, y: 0, width: 6, height: 6 });
  });
});
