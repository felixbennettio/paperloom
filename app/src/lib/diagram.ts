// Geometry for the diagram editor: what the pointer is on, the handles a
// shape is resized by, snapping to the grid and to other shapes, and
// reading a cell's style.

import type { DiagramDrawn } from "./api";

export interface Box {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface Point {
  x: number;
  y: number;
}

/** A cell's style, `key=value;` pairs; a bare word (`ellipse`) has itself
 * as its value. */
export function styleOf(style: string): Map<string, string> {
  const map = new Map<string, string>();
  for (const part of style.split(";")) {
    const entry = part.trim();
    if (!entry) continue;
    const at = entry.indexOf("=");
    if (at < 0) map.set(entry, entry);
    else map.set(entry.slice(0, at).trim(), entry.slice(at + 1).trim());
  }
  return map;
}

/** How far a point is from a line through points. */
export function distanceToLine(p: Point, points: Point[]): number {
  if (points.length === 1) return Math.hypot(p.x - points[0].x, p.y - points[0].y);
  let best = Infinity;
  for (let i = 1; i < points.length; i++) {
    const [a, b] = [points[i - 1], points[i]];
    const [dx, dy] = [b.x - a.x, b.y - a.y];
    const length = dx * dx + dy * dy;
    const t = length ? Math.max(0, Math.min(1, ((p.x - a.x) * dx + (p.y - a.y) * dy) / length)) : 0;
    best = Math.min(best, Math.hypot(p.x - (a.x + t * dx), p.y - (a.y + t * dy)));
  }
  return best;
}

/** The topmost shape or edge under a point; edges within `slop` of their
 * line, shapes in their box (or their label's). */
export function hitItem(items: DiagramDrawn[], p: Point, slop: number): DiagramDrawn | null {
  for (let i = items.length - 1; i >= 0; i--) {
    const item = items[i];
    if (item.edge) {
      if (item.points.length && distanceToLine(p, item.points) <= slop) return item;
      continue;
    }
    const inBox = p.x >= item.x && p.x <= item.x + item.width && p.y >= item.y && p.y <= item.y + item.height;
    const onLabel =
      item.text !== "" && p.x >= item.textX && p.x <= item.textX + item.textWidth && p.y >= item.textY && p.y <= item.textY + item.textHeight;
    // A container is picked by its edge or header, so what is in it can be.
    if (inBox && item.container && !item.edge) {
      const inner = { x: item.x + slop, y: item.y + 26, width: item.width - 2 * slop, height: item.height - 26 - slop };
      const inMiddle = p.x > inner.x && p.x < inner.x + inner.width && p.y > inner.y && p.y < inner.y + inner.height;
      if (inMiddle && !item.style.startsWith("group")) continue;
    }
    if (inBox || onLabel) return item;
  }
  return null;
}

export type Handle = "nw" | "n" | "ne" | "e" | "se" | "s" | "sw" | "w";

/** Where a box's resize handles are. */
export function handles(box: Box): { handle: Handle; x: number; y: number }[] {
  const [l, c, r] = [box.x, box.x + box.width / 2, box.x + box.width];
  const [t, m, b] = [box.y, box.y + box.height / 2, box.y + box.height];
  return [
    { handle: "nw", x: l, y: t },
    { handle: "n", x: c, y: t },
    { handle: "ne", x: r, y: t },
    { handle: "e", x: r, y: m },
    { handle: "se", x: r, y: b },
    { handle: "s", x: c, y: b },
    { handle: "sw", x: l, y: b },
    { handle: "w", x: l, y: m },
  ];
}

/** A box dragged by a handle; never smaller than `least` either way. With
 * `keepRatio`, corners keep its proportions. */
export function resized(box: Box, handle: Handle, dx: number, dy: number, keepRatio = false, least = 10): Box {
  let { x, y, width, height } = box;
  if (handle.includes("w")) {
    const w = Math.max(least, width - dx);
    x += width - w;
    width = w;
  }
  if (handle.includes("e")) width = Math.max(least, width + dx);
  if (handle.includes("n")) {
    const h = Math.max(least, height - dy);
    y += height - h;
    height = h;
  }
  if (handle.includes("s")) height = Math.max(least, height + dy);
  if (keepRatio && handle.length === 2 && box.width > 0 && box.height > 0) {
    const scale = Math.max(width / box.width, height / box.height);
    const [w, h] = [box.width * scale, box.height * scale];
    if (handle.includes("w")) x = box.x + box.width - w;
    if (handle.includes("n")) y = box.y + box.height - h;
    [width, height] = [w, h];
  }
  return { x, y, width, height };
}

export function snap(value: number, grid: number): number {
  return grid > 0 ? Math.round(value / grid) * grid : value;
}

/** A free position within four lattice rings, preferring right and down. */
export function freeSpot(boxes: Box[], wanted: Box, gap = 20): Point {
  const free = (p: Point) => !boxes.some((b) =>
    p.x < b.x + b.width + gap && p.x + wanted.width > b.x - gap &&
    p.y < b.y + b.height + gap && p.y + wanted.height > b.y - gap);
  const origin = { x: wanted.x, y: wanted.y };
  if (free(origin)) return origin;
  const forward = (p: Point) => p.x >= 0 && p.y >= 0 ? 0 : 1;
  const angle = (p: Point) => (Math.atan2(p.y, p.x) + 2 * Math.PI) % (2 * Math.PI);
  for (let ring = 1; ring <= 4; ring++) {
    const offsets: Point[] = [];
    for (let x = -ring; x <= ring; x++) for (let y = -ring; y <= ring; y++) {
      if (Math.max(Math.abs(x), Math.abs(y)) === ring) offsets.push({ x, y });
    }
    offsets.sort((a, b) => forward(a) - forward(b) ||
      a.x * a.x + a.y * a.y - b.x * b.x - b.y * b.y || angle(a) - angle(b));
    for (const offset of offsets) {
      const p = { x: wanted.x + offset.x * (wanted.width + gap), y: wanted.y + offset.y * (wanted.height + gap) };
      if (free(p)) return p;
    }
  }
  return origin;
}

/** A guide shown while snapping: a line across the page. */
export interface Guide {
  vertical: boolean;
  at: number;
}

/** How far to nudge a moving box so an edge or middle lines up with
 * another box's, within `tolerance`; and the guides to show. */
export function aligned(moving: Box, others: Box[], tolerance: number): { dx: number; dy: number; guides: Guide[] } {
  const xs = (b: Box) => [b.x, b.x + b.width / 2, b.x + b.width];
  const ys = (b: Box) => [b.y, b.y + b.height / 2, b.y + b.height];
  let [nearestX, nearestY] = [Infinity, Infinity];
  for (const other of others) {
    for (const a of xs(moving)) for (const b of xs(other)) if (Math.abs(b - a) < Math.abs(nearestX)) nearestX = b - a;
    for (const a of ys(moving)) for (const b of ys(other)) if (Math.abs(b - a) < Math.abs(nearestY)) nearestY = b - a;
  }
  const [snapX, snapY] = [Math.abs(nearestX) <= tolerance, Math.abs(nearestY) <= tolerance];
  const [dx, dy] = [snapX ? nearestX : 0, snapY ? nearestY : 0];
  const placed = { ...moving, x: moving.x + dx, y: moving.y + dy };
  const near = (a: number, b: number) => Math.abs(a - b) < 0.01;
  const guides: Guide[] = [];
  const add = (vertical: boolean, at: number) => {
    if (!guides.some((g) => g.vertical === vertical && near(g.at, at))) guides.push({ vertical, at });
  };
  for (const other of others) {
    if (snapX) for (const a of xs(placed)) if (xs(other).some((b) => near(a, b))) add(true, a);
    if (snapY) for (const a of ys(placed)) if (ys(other).some((b) => near(a, b))) add(false, a);
  }
  return { dx, dy, guides };
}

/** Whether a box lies wholly inside another (a marquee). */
export function within(box: Box, area: Box): boolean {
  return box.x >= area.x && box.y >= area.y && box.x + box.width <= area.x + area.width && box.y + box.height <= area.y + area.height;
}

/** A box from two corners. */
export function boxBetween(a: Point, b: Point): Box {
  return { x: Math.min(a.x, b.x), y: Math.min(a.y, b.y), width: Math.abs(b.x - a.x), height: Math.abs(b.y - a.y) };
}

/** The middles of a box's sides, where new edges start. */
export function connectionPoints(box: Box): Point[] {
  return [
    { x: box.x + box.width / 2, y: box.y },
    { x: box.x + box.width, y: box.y + box.height / 2 },
    { x: box.x + box.width / 2, y: box.y + box.height },
    { x: box.x, y: box.y + box.height / 2 },
  ];
}

/** Where a shape's turning knob is: above the middle of its top. */
export function rotateKnob(item: { x: number; y: number; width: number }, zoom: number): Point {
  return { x: item.x + item.width / 2, y: item.y - 24 / zoom };
}

export type ShapeDragStart =
  | { kind: "connect"; from: Point }
  | { kind: "rotate" }
  | { kind: "resize"; handle: Handle };

/** Resolve shape controls in screen pixels; ports win over overlapping handles. */
export function shapeDragStart(p: Point, portOwner: Box | null, selected: Box | null, zoom: number): ShapeDragStart | null {
  const reach = 7 / zoom;
  const near = (q: Point) => Math.hypot(q.x - p.x, q.y - p.y) <= reach;
  const from = portOwner && connectionPoints(portOwner).find(near);
  if (from) return { kind: "connect", from };
  if (!selected) return null;
  if (near(rotateKnob(selected, zoom))) return { kind: "rotate" };
  const grip = handles(selected).find(near);
  return grip ? { kind: "resize", handle: grip.handle } : null;
}

/** The box around boxes; null for none. */
export function boundsOf(boxes: Box[]): Box | null {
  if (!boxes.length) return null;
  const left = Math.min(...boxes.map((b) => b.x));
  const top = Math.min(...boxes.map((b) => b.y));
  const right = Math.max(...boxes.map((b) => b.x + b.width));
  const bottom = Math.max(...boxes.map((b) => b.y + b.height));
  return { x: left, y: top, width: right - left, height: bottom - top };
}
