import { describe, expect, it } from "vitest";
import type { CanvasBoard, CanvasCard } from "./api";
import { anchor, arrowHead, bounds, cardAt, cardColor, contains, curve, facing, midpoint, moving, newId, without } from "./canvas";

function card(id: string, x: number, y: number, width = 100, height = 50, kind = "text"): CanvasCard {
  return { id, kind, x, y, width, height, color: null, text: id, file: null, subpath: null, url: null, label: null };
}

describe("canvas", () => {
  it("anchors arrows on the side facing the other card", () => {
    const a = card("a", 0, 0);
    expect(anchor(a, "right")).toEqual({ x: 100, y: 25 });
    expect(anchor(a, "top")).toEqual({ x: 50, y: 0 });
    expect(facing(a, { x: 400, y: 30 })).toBe("right");
    expect(facing(a, { x: -400, y: 30 })).toBe("left");
    expect(facing(a, { x: 50, y: 400 })).toBe("bottom");
    expect(facing(a, { x: 50, y: -400 })).toBe("top");
  });

  it("curves out of one side and into the other", () => {
    const points = curve({ x: 0, y: 0 }, "right", { x: 300, y: 0 }, "left");
    expect(points[1]).toEqual({ x: 100, y: 0 });
    expect(points[2]).toEqual({ x: 200, y: 0 });
    expect(midpoint(points)).toEqual({ x: 150, y: 0 });
    expect(arrowHead({ x: 0, y: 0 }, { x: 10, y: 0 })).toMatch(/^M 10 0 L /);
  });

  it("moves groups with what is inside them, and removes arrows with their cards", () => {
    const board: CanvasBoard = {
      cards: [card("g", 0, 0, 500, 500, "group"), card("in", 10, 10), card("out", 600, 0)],
      arrows: [
        { id: "e", fromNode: "in", fromSide: null, fromEnd: null, toNode: "out", toSide: null, toEnd: null, color: null, label: null },
      ],
    };
    expect(contains(board.cards[0], board.cards[1])).toBe(true);
    expect(moving(board, "g")).toEqual(["g", "in"]);
    expect(moving(board, "out")).toEqual(["out"]);
    expect(without(board, ["in"]).arrows).toEqual([]);
    expect(cardAt(board, { x: 20, y: 20 })?.id).toBe("in");
    expect(cardAt(board, { x: 300, y: 300 })?.id).toBe("g");
    expect(cardAt(board, { x: 20, y: 20 }, "in")?.id).toBe("g");
    expect(bounds(board)).toEqual({ x: 0, y: 0, width: 700, height: 500 });
    expect(bounds({ cards: [], arrows: [] })).toBeNull();
  });

  it("reads colours and makes IDs", () => {
    expect(cardColor("4")).toBe("#30a46c");
    expect(cardColor("#AABBCC")).toBe("#AABBCC");
    expect(cardColor("red")).toBeNull();
    expect(cardColor(null)).toBeNull();
    expect(newId()).toMatch(/^[0-9a-f]{16}$/);
  });
});
