// Geometry for drawings: how wide strokes are, what an item covers, what a
// pointer or a lasso touches, and strokes recognised as shapes.

import type { DrawingItem, DrawingPoint } from "./api";

/** Excalidraw draws a stroke this many times its `strokeWidth` wide. */
export const STROKE_SCALE = 4.25;

/** How wide a stroke is at a pressure, as Excalidraw draws it. */
export function diameter(strokeWidth: number, pressure: number): number {
  return strokeWidth * STROKE_SCALE * (0.4 + 1.2 * Math.min(1, Math.max(0, pressure)));
}

export interface Box {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

export function distance(a: DrawingPoint, b: DrawingPoint): number {
  return Math.hypot(a.x - b.x, a.y - b.y);
}

/** How far a point is from the segment `a`–`b`. */
export function distanceToSegment(p: DrawingPoint, a: DrawingPoint, b: DrawingPoint): number {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const length = dx * dx + dy * dy;
  const t = length === 0 ? 0 : Math.max(0, Math.min(1, ((p.x - a.x) * dx + (p.y - a.y) * dy) / length));
  return Math.hypot(p.x - (a.x + t * dx), p.y - (a.y + t * dy));
}

/** Whether a point lies inside a polygon (its last point joins its first). */
export function insidePolygon(p: DrawingPoint, polygon: DrawingPoint[]): boolean {
  let inside = false;
  for (let i = 0, j = polygon.length - 1; i < polygon.length; j = i++) {
    const a = polygon[i];
    const b = polygon[j];
    if (a.y > p.y !== b.y > p.y && p.x < ((b.x - a.x) * (p.y - a.y)) / (b.y - a.y) + a.x) inside = !inside;
  }
  return inside;
}

function rotate(p: DrawingPoint, centre: DrawingPoint, angle: number): DrawingPoint {
  if (!angle) return p;
  const cos = Math.cos(angle);
  const sin = Math.sin(angle);
  const dx = p.x - centre.x;
  const dy = p.y - centre.y;
  return { x: centre.x + dx * cos - dy * sin, y: centre.y + dx * sin + dy * cos };
}

export function centre(item: DrawingItem): DrawingPoint {
  return { x: item.x + item.width / 2, y: item.y + item.height / 2 };
}

/** An item's points in drawing space: a stroke's or line's points, or a
 * shape's outline (closed), turned by its angle. */
export function outline(item: DrawingItem): DrawingPoint[] {
  const { x, y, width: w, height: h } = item;
  let points: DrawingPoint[];
  switch (item.kind) {
    case "freedraw":
    case "line":
    case "arrow":
      points = item.points.map((p) => ({ x: x + p.x, y: y + p.y }));
      break;
    case "ellipse":
      points = Array.from({ length: 33 }, (_, i) => {
        const a = (i / 32) * 2 * Math.PI;
        return { x: x + w / 2 + (w / 2) * Math.cos(a), y: y + h / 2 + (h / 2) * Math.sin(a) };
      });
      break;
    case "diamond":
      points = [
        { x: x + w / 2, y },
        { x: x + w, y: y + h / 2 },
        { x: x + w / 2, y: y + h },
        { x, y: y + h / 2 },
        { x: x + w / 2, y },
      ];
      break;
    default:
      points = [
        { x, y },
        { x: x + w, y },
        { x: x + w, y: y + h },
        { x, y: y + h },
        { x, y },
      ];
  }
  const c = centre(item);
  return points.map((p) => rotate(p, c, item.angle));
}

/** The area an item covers, its stroke included. */
export function itemBounds(item: DrawingItem): Box {
  const points = outline(item);
  const reach = item.kind === "freedraw" ? diameter(item.strokeWidth, 1) / 2 : item.strokeWidth / 2 + 1;
  const box = { left: Infinity, top: Infinity, right: -Infinity, bottom: -Infinity };
  for (const p of points) {
    box.left = Math.min(box.left, p.x - reach);
    box.top = Math.min(box.top, p.y - reach);
    box.right = Math.max(box.right, p.x + reach);
    box.bottom = Math.max(box.bottom, p.y + reach);
  }
  return points.length ? box : { left: item.x, top: item.y, right: item.x, bottom: item.y };
}

/** The area several items cover; null for none. */
export function unionBounds(items: DrawingItem[]): Box | null {
  if (!items.length) return null;
  return items.map(itemBounds).reduce((a, b) => ({
    left: Math.min(a.left, b.left),
    top: Math.min(a.top, b.top),
    right: Math.max(a.right, b.right),
    bottom: Math.max(a.bottom, b.bottom),
  }));
}

function filled(item: DrawingItem): boolean {
  return (
    item.kind === "text" ||
    item.kind === "image" ||
    (item.backgroundColor !== "transparent" && ["rectangle", "ellipse", "diamond"].includes(item.kind))
  );
}

/** Whether a point touches an item, within `slop` drawing units. */
export function hits(item: DrawingItem, p: DrawingPoint, slop: number): boolean {
  const points = outline(item);
  const reach = slop + (item.kind === "freedraw" ? diameter(item.strokeWidth, 0.5) / 2 : item.strokeWidth / 2);
  if (points.length === 1) return distance(points[0], p) <= reach;
  for (let i = 1; i < points.length; i++) {
    if (distanceToSegment(p, points[i - 1], points[i]) <= reach) return true;
  }
  return filled(item) && insidePolygon(p, points);
}

/** Whether most of an item lies inside a lasso. */
export function lassoed(item: DrawingItem, lasso: DrawingPoint[]): boolean {
  const points = outline(item);
  if (!points.length || lasso.length < 3) return false;
  return points.filter((p) => insidePolygon(p, lasso)).length / points.length >= 0.8;
}

/** How long a path is. */
export function pathLength(points: DrawingPoint[]): number {
  let length = 0;
  for (let i = 1; i < points.length; i++) length += distance(points[i - 1], points[i]);
  return length;
}

export type Shape =
  | { kind: "line"; from: DrawingPoint; to: DrawingPoint }
  | { kind: "rectangle" | "ellipse"; box: Box };

/** A hand-drawn stroke as the shape it was meant to be: a straight line, a
 * rectangle or an ellipse; null when it is none of them. */
export function recognize(points: DrawingPoint[]): Shape | null {
  if (points.length < 3) return null;
  const length = pathLength(points);
  const first = points[0];
  const last = points[points.length - 1];
  const chord = distance(first, last);
  if (length < 24) return null;
  if (chord / length > 0.95) return { kind: "line", from: first, to: last };
  if (chord > 0.2 * length || length < 60) return null;
  const box = { left: Infinity, top: Infinity, right: -Infinity, bottom: -Infinity };
  for (const p of points) {
    box.left = Math.min(box.left, p.x);
    box.top = Math.min(box.top, p.y);
    box.right = Math.max(box.right, p.x);
    box.bottom = Math.max(box.bottom, p.y);
  }
  const w = box.right - box.left;
  const h = box.bottom - box.top;
  if (w < 12 || h < 12) return null;
  const cx = (box.left + box.right) / 2;
  const cy = (box.top + box.bottom) / 2;
  let ellipse = 0;
  let rectangle = 0;
  for (const p of points) {
    ellipse += Math.abs(Math.hypot((p.x - cx) / (w / 2), (p.y - cy) / (h / 2)) - 1);
    rectangle += Math.min(p.x - box.left, box.right - p.x, p.y - box.top, box.bottom - p.y) / Math.max(w, h);
  }
  ellipse /= points.length;
  rectangle /= points.length;
  if (rectangle < 0.05 && rectangle * 2 < ellipse) return { kind: "rectangle", box };
  if (ellipse < 0.1) return { kind: "ellipse", box };
  return null;
}

/** Items moved by `dx`, `dy`. */
export function moved(items: DrawingItem[], ids: Set<string>, dx: number, dy: number): DrawingItem[] {
  return items.map((item) => (ids.has(item.id) ? { ...item, x: item.x + dx, y: item.y + dy } : item));
}

/** A fresh id, as Excalidraw's are random. */
export function drawingId(): string {
  const bytes = new Uint8Array(10);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => b.toString(36).padStart(2, "0")).join("").slice(0, 20);
}

/** An item with the fields every kind needs. */
export function blankItem(kind: string, patch: Partial<DrawingItem>): DrawingItem {
  return {
    id: drawingId(),
    kind,
    x: 0,
    y: 0,
    width: 0,
    height: 0,
    angle: 0,
    strokeColor: "#1e1e1e",
    backgroundColor: "transparent",
    strokeWidth: 2,
    opacity: 100,
    points: [],
    pressures: [],
    text: "",
    fontSize: 20,
    highlighter: false,
    arrowhead: false,
    ...patch,
  };
}

/** A stroke's points made relative to its corner, with its size set. */
export function stroke(points: DrawingPoint[], pressures: number[], patch: Partial<DrawingItem>): DrawingItem {
  const left = Math.min(...points.map((p) => p.x));
  const top = Math.min(...points.map((p) => p.y));
  const right = Math.max(...points.map((p) => p.x));
  const bottom = Math.max(...points.map((p) => p.y));
  return blankItem("freedraw", {
    ...patch,
    x: left,
    y: top,
    width: right - left,
    height: bottom - top,
    points: points.map((p) => ({ x: p.x - left, y: p.y - top })),
    pressures,
  });
}

/** Items grown or shrunk by `factor` from `origin`, their strokes and text
 * with them. */
export function scaled(items: DrawingItem[], ids: Set<string>, origin: DrawingPoint, factor: number): DrawingItem[] {
  return items.map((item) =>
    ids.has(item.id)
      ? {
          ...item,
          x: origin.x + (item.x - origin.x) * factor,
          y: origin.y + (item.y - origin.y) * factor,
          width: item.width * factor,
          height: item.height * factor,
          points: item.points.map((p) => ({ x: p.x * factor, y: p.y * factor })),
          strokeWidth: item.strokeWidth * factor,
          fontSize: item.fontSize * factor,
        }
      : item,
  );
}

/** Items turned by `angle` (radians, clockwise) about `pivot`. */
export function rotated(items: DrawingItem[], ids: Set<string>, pivot: DrawingPoint, angle: number): DrawingItem[] {
  const cos = Math.cos(angle);
  const sin = Math.sin(angle);
  return items.map((item) => {
    if (!ids.has(item.id)) return item;
    const c = centre(item);
    const dx = c.x - pivot.x;
    const dy = c.y - pivot.y;
    const turned = { x: pivot.x + dx * cos - dy * sin, y: pivot.y + dx * sin + dy * cos };
    return { ...item, x: turned.x - item.width / 2, y: turned.y - item.height / 2, angle: item.angle + angle };
  });
}

/** A ruler laid on the page: its middle, its angle in radians and its
 * length. It is `RULER_WIDTH` wide, and both long sides draw. */
export interface Ruler {
  x: number;
  y: number;
  angle: number;
  length: number;
}

/** A protractor: the middle of its straight side, that side's angle, and
 * its radius. Its arc is on the side above the straight one when it is
 * level. */
export interface Protractor {
  x: number;
  y: number;
  angle: number;
  radius: number;
}

export const RULER_WIDTH = 64;

/** A point in a guide's own frame: along it, and across it (negative is
 * above it when it is level). */
export function local(guide: { x: number; y: number; angle: number }, p: DrawingPoint): { along: number; across: number } {
  const [dx, dy] = [p.x - guide.x, p.y - guide.y];
  const [c, s] = [Math.cos(guide.angle), Math.sin(guide.angle)];
  return { along: dx * c + dy * s, across: -dx * s + dy * c };
}

/** The page point at a place in a guide's frame. */
export function fromLocal(guide: { x: number; y: number; angle: number }, along: number, across: number): DrawingPoint {
  const [c, s] = [Math.cos(guide.angle), Math.sin(guide.angle)];
  return { x: guide.x + along * c - across * s, y: guide.y + along * s + across * c };
}

export function onRuler(ruler: Ruler, p: DrawingPoint): boolean {
  const { along, across } = local(ruler, p);
  return Math.abs(along) <= ruler.length / 2 && Math.abs(across) <= RULER_WIDTH / 2;
}

export function onProtractor(protractor: Protractor, p: DrawingPoint): boolean {
  const { along, across } = local(protractor, p);
  return across <= 0 && Math.hypot(along, across) <= protractor.radius;
}

/** Where a ruler's and a protractor's turning knobs are. */
export function rulerKnob(ruler: Ruler): DrawingPoint {
  return fromLocal(ruler, ruler.length / 2 - 22, 0);
}

export function protractorKnob(protractor: Protractor): DrawingPoint {
  return fromLocal(protractor, protractor.radius - 22, -14);
}

/** How a stroke that starts at `p` keeps to a guide: each point put on the
 * ruler's nearer edge, or on the protractor's arc; null when `p` is not
 * within `reach` of either. */
export function guideFor(
  ruler: Ruler | null,
  protractor: Protractor | null,
  p: DrawingPoint,
  reach: number,
): ((q: DrawingPoint) => DrawingPoint) | null {
  if (ruler) {
    const { along, across } = local(ruler, p);
    if (Math.abs(along) <= ruler.length / 2 && Math.abs(across) - RULER_WIDTH / 2 <= reach) {
      const edge = (across < 0 ? -1 : 1) * (RULER_WIDTH / 2);
      return (q) => fromLocal(ruler, local(ruler, q).along, edge);
    }
  }
  if (protractor) {
    const { along, across } = local(protractor, p);
    if (across <= reach && Math.abs(Math.hypot(along, across) - protractor.radius) <= reach) {
      return (q) => {
        const at = local(protractor, q);
        const length = Math.hypot(at.along, at.across) || 1;
        return fromLocal(protractor, (at.along / length) * protractor.radius, (at.across / length) * protractor.radius);
      };
    }
  }
  return null;
}

/** An angle as whole degrees from 0 to 359. */
export function degrees(angle: number): number {
  return ((Math.round((angle * 180) / Math.PI) % 360) + 360) % 360;
}

/** A pen kept among the favourites, as OneNote keeps them. */
export interface FavouritePen {
  highlighter: boolean;
  color: string;
  /** A pen's stroke width; highlighters have their own. */
  width: number;
}

const FAVOURITES = "paperloom.favouritePens";

/** The favourite pens kept on this device; none when there are none, or
 * the storage cannot be read. */
export function loadFavouritePens(storage: Pick<Storage, "getItem"> | null): FavouritePen[] {
  try {
    const kept: unknown = JSON.parse(storage?.getItem(FAVOURITES) ?? "[]");
    if (!Array.isArray(kept)) return [];
    return kept.filter(
      (p): p is FavouritePen =>
        typeof p === "object" &&
        p !== null &&
        typeof p.highlighter === "boolean" &&
        typeof p.color === "string" &&
        typeof p.width === "number",
    );
  } catch {
    return [];
  }
}

export function saveFavouritePens(storage: Pick<Storage, "setItem"> | null, pens: FavouritePen[]): void {
  try {
    storage?.setItem(FAVOURITES, JSON.stringify(pens));
  } catch {
    // A private window: the favourites last as long as the page.
  }
}

/** The favourites with a pen added at the end: each pen once, at most eight. */
export function withFavourite(pens: FavouritePen[], pen: FavouritePen): FavouritePen[] {
  const same = (p: FavouritePen) => p.highlighter === pen.highlighter && p.color === pen.color && (pen.highlighter || p.width === pen.width);
  return [...pens.filter((p) => !same(p)), pen].slice(-8);
}

/** The size a picture takes on a page: its own, made to fit within a box. */
export function pictureSize(width: number, height: number, room = { width: 480, height: 360 }): { width: number; height: number } {
  const scale = Math.min(1, room.width / Math.max(1, width), room.height / Math.max(1, height));
  return { width: Math.round(width * scale), height: Math.round(height * scale) };
}
