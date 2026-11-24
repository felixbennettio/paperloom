import { describe, expect, it } from "vitest";
import {
  blankItem,
  degrees,
  diameter,
  distanceToSegment,
  fromLocal,
  guideFor,
  hits,
  insidePolygon,
  itemBounds,
  lassoed,
  loadFavouritePens,
  local,
  moved,
  onProtractor,
  onRuler,
  outline,
  pictureSize,
  recognize,
  rotated,
  scaled,
  stroke,
  unionBounds,
  withFavourite,
  type Protractor,
  type Ruler,
} from "./drawing";

const square = (size: number, steps: number) => {
  const points: { x: number; y: number }[] = [];
  for (let i = 0; i < steps; i++) points.push({ x: (i / steps) * size, y: 0 });
  for (let i = 0; i < steps; i++) points.push({ x: size, y: (i / steps) * size });
  for (let i = 0; i < steps; i++) points.push({ x: size - (i / steps) * size, y: size });
  for (let i = 0; i <= steps; i++) points.push({ x: 0, y: size - (i / steps) * size });
  return points;
};

const circle = (radius: number, steps: number) =>
  Array.from({ length: steps + 1 }, (_, i) => ({
    x: radius + radius * Math.cos((i / steps) * 2 * Math.PI),
    y: radius + radius * Math.sin((i / steps) * 2 * Math.PI),
  }));

describe("drawing", () => {
  it("widens strokes with pressure as Excalidraw does", () => {
    expect(diameter(1, 0.5)).toBeCloseTo(4.25);
    expect(diameter(1, 0)).toBeLessThan(diameter(1, 1));
    expect(diameter(2, 9)).toBe(diameter(2, 1));
  });

  it("measures distances to segments and finds points in polygons", () => {
    expect(distanceToSegment({ x: 5, y: 3 }, { x: 0, y: 0 }, { x: 10, y: 0 })).toBe(3);
    expect(distanceToSegment({ x: -4, y: 3 }, { x: 0, y: 0 }, { x: 10, y: 0 })).toBe(5);
    expect(distanceToSegment({ x: 1, y: 1 }, { x: 0, y: 0 }, { x: 0, y: 0 })).toBeCloseTo(Math.SQRT2);
    const triangle = [
      { x: 0, y: 0 },
      { x: 10, y: 0 },
      { x: 0, y: 10 },
    ];
    expect(insidePolygon({ x: 2, y: 2 }, triangle)).toBe(true);
    expect(insidePolygon({ x: 8, y: 8 }, triangle)).toBe(false);
  });

  it("outlines shapes, turned by their angle", () => {
    const box = blankItem("rectangle", { x: 0, y: 0, width: 20, height: 10, angle: Math.PI / 2 });
    const corners = outline(box);
    expect(corners).toHaveLength(5);
    // Turned a quarter, the 20 × 10 box stands 10 wide and 20 tall about its centre.
    expect(corners[0].x).toBeCloseTo(15);
    expect(corners[0].y).toBeCloseTo(-5);
    const bounds = itemBounds(box);
    expect(bounds.right - bounds.left).toBeCloseTo(10 + 2 * 2);
    expect(outline(blankItem("ellipse", { width: 10, height: 10 }))).toHaveLength(33);
  });

  it("hits strokes near their line and filled shapes inside", () => {
    const line = stroke(
      [
        { x: 10, y: 10 },
        { x: 50, y: 10 },
      ],
      [],
      { strokeWidth: 1 },
    );
    expect(line.x).toBe(10);
    expect(line.points[1]).toEqual({ x: 40, y: 0 });
    expect(hits(line, { x: 30, y: 13 }, 2)).toBe(true);
    expect(hits(line, { x: 30, y: 30 }, 2)).toBe(false);
    const empty = blankItem("rectangle", { x: 0, y: 0, width: 40, height: 40 });
    expect(hits(empty, { x: 20, y: 20 }, 2)).toBe(false);
    expect(hits(empty, { x: 0, y: 20 }, 2)).toBe(true);
    expect(hits({ ...empty, backgroundColor: "#ffec99" }, { x: 20, y: 20 }, 2)).toBe(true);
    const dot = blankItem("freedraw", { x: 5, y: 5, points: [{ x: 0, y: 0 }] });
    expect(hits(dot, { x: 6, y: 6 }, 1)).toBe(true);
  });

  it("lassoes what lies mostly inside", () => {
    const lasso = square(100, 4).map((p) => ({ x: p.x - 10, y: p.y - 10 }));
    expect(lassoed(blankItem("rectangle", { x: 10, y: 10, width: 30, height: 30 }), lasso)).toBe(true);
    expect(lassoed(blankItem("rectangle", { x: 60, y: 60, width: 80, height: 80 }), lasso)).toBe(false);
    expect(lassoed(blankItem("rectangle", { width: 5, height: 5 }), lasso.slice(0, 2))).toBe(false);
  });

  it("recognises lines, rectangles and ellipses", () => {
    const straight = Array.from({ length: 12 }, (_, i) => ({ x: i * 10, y: i * 5 + (i % 2) * 0.5 }));
    expect(recognize(straight)).toEqual({ kind: "line", from: straight[0], to: straight[11] });
    const box = recognize(square(100, 10));
    expect(box?.kind).toBe("rectangle");
    const round = recognize(circle(50, 40));
    expect(round?.kind).toBe("ellipse");
    if (round?.kind === "ellipse") expect(round.box.right - round.box.left).toBeCloseTo(100);
    const scribble = Array.from({ length: 12 }, (_, i) => ({ x: i * 10, y: (i % 2) * 40 }));
    expect(recognize(scribble)).toBeNull();
    expect(recognize([{ x: 0, y: 0 }])).toBeNull();
  });

  it("moves and bounds groups of items", () => {
    const items = [blankItem("rectangle", { id: "a", width: 10, height: 10 }), blankItem("ellipse", { id: "b", x: 50, width: 10, height: 10 })];
    const next = moved(items, new Set(["b"]), 5, -5);
    expect(next[0]).toBe(items[0]);
    expect(next[1]).toMatchObject({ x: 55, y: -5 });
    expect(unionBounds(next)?.right).toBeCloseTo(67);
    expect(unionBounds([])).toBeNull();
  });

  it("scales and turns a selection", () => {
    const line = stroke(
      [
        { x: 10, y: 10 },
        { x: 30, y: 20 },
      ],
      [0.5, 0.5],
      { id: "s", strokeWidth: 1 },
    );
    const label = blankItem("text", { id: "t", x: 10, y: 30, width: 40, height: 20, fontSize: 16 });
    const [big, text] = scaled([line, label], new Set(["s", "t"]), { x: 10, y: 10 }, 2);
    expect(big).toMatchObject({ x: 10, y: 10, width: 40, height: 20, strokeWidth: 2 });
    expect(big.points[1]).toEqual({ x: 40, y: 20 });
    expect(text).toMatchObject({ x: 10, y: 50, fontSize: 32 });
    const box = blankItem("rectangle", { id: "r", x: 0, y: 0, width: 20, height: 10 });
    const [turned] = rotated([box], new Set(["r"]), { x: 20, y: 5 }, Math.PI);
    // Half a turn about a point beside it: the box lands on the other side, upside down.
    expect(turned.x).toBeCloseTo(20);
    expect(turned.y).toBeCloseTo(0);
    expect(turned.angle).toBeCloseTo(Math.PI);
    expect(rotated([box], new Set(), { x: 0, y: 0 }, 1)[0]).toBe(box);
  });
});

describe("ruler and protractor", () => {
  const ruler: Ruler = { x: 100, y: 100, angle: Math.PI / 2, length: 400 };
  const protractor: Protractor = { x: 0, y: 0, angle: 0, radius: 150 };

  it("works in the guide's own frame", () => {
    const at = local(ruler, { x: 100, y: 150 });
    expect(at.along).toBeCloseTo(50);
    expect(at.across).toBeCloseTo(0);
    const back = fromLocal(ruler, 50, 0);
    expect(back.x).toBeCloseTo(100);
    expect(back.y).toBeCloseTo(150);
    expect(onRuler(ruler, { x: 120, y: 290 })).toBe(true);
    expect(onRuler(ruler, { x: 140, y: 100 })).toBe(false);
    expect(onProtractor(protractor, { x: 10, y: -100 })).toBe(true);
    expect(onProtractor(protractor, { x: 10, y: 100 })).toBe(false);
    expect(degrees(-Math.PI / 2)).toBe(270);
  });

  it("keeps a stroke to the nearer edge of the ruler", () => {
    // Standing upright: its edges are at x = 68 and x = 132.
    const guide = guideFor(ruler, null, { x: 140, y: 120 }, 12);
    expect(guide).not.toBeNull();
    const kept = guide!({ x: 150, y: 200 });
    expect(kept.x).toBeCloseTo(132);
    expect(kept.y).toBeCloseTo(200);
    expect(guideFor(ruler, null, { x: 160, y: 120 }, 12)).toBeNull();
    expect(guideFor(ruler, null, { x: 132, y: 400 }, 12)).toBeNull();
  });

  it("keeps a stroke to the protractor's arc", () => {
    const guide = guideFor(null, protractor, { x: 0, y: -155 }, 12);
    expect(guide).not.toBeNull();
    const kept = guide!({ x: 100, y: -100 });
    expect(Math.hypot(kept.x, kept.y)).toBeCloseTo(150);
    expect(kept.x).toBeCloseTo(kept.y * -1);
    expect(guideFor(null, protractor, { x: 0, y: -100 }, 12)).toBeNull();
  });
});

describe("favourite pens", () => {
  it("are kept once each, eight at most, newest last", () => {
    const pen = { highlighter: false, color: "#1971c2", width: 0.6 };
    let pens = withFavourite([], pen);
    pens = withFavourite(pens, { ...pen, width: 1.2 });
    pens = withFavourite(pens, pen);
    expect(pens).toEqual([{ ...pen, width: 1.2 }, pen]);
    for (let i = 0; i < 10; i++) pens = withFavourite(pens, { highlighter: true, color: `#00000${i}`, width: 4 });
    expect(pens).toHaveLength(8);
    expect(pens[7].color).toBe("#000009");
  });

  it("read back only well-formed pens", () => {
    const kept = JSON.stringify([{ highlighter: true, color: "#ffd43b", width: 4 }, { color: 3 }, "pen"]);
    expect(loadFavouritePens({ getItem: () => kept })).toEqual([{ highlighter: true, color: "#ffd43b", width: 4 }]);
    expect(loadFavouritePens({ getItem: () => "not json" })).toEqual([]);
    expect(loadFavouritePens(null)).toEqual([]);
  });
});

describe("pictures", () => {
  it("fit the page and can be picked up anywhere on them", () => {
    expect(pictureSize(1920, 1080)).toEqual({ width: 480, height: 270 });
    expect(pictureSize(100, 50)).toEqual({ width: 100, height: 50 });
    const picture = blankItem("image", { x: 0, y: 0, width: 100, height: 80, image: "data:image/png;base64,AAAA" });
    expect(hits(picture, { x: 50, y: 40 }, 1)).toBe(true);
  });
});
