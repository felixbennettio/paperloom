// Geometry for canvases: where arrows leave and reach cards, and how they
// curve. Pure functions, so they are easy to test.

import type { CanvasBoard, CanvasCard } from "./api";

export type Side = "top" | "right" | "bottom" | "left";
export interface Point {
  x: number;
  y: number;
}

/** Obsidian's six card colours, by number. */
export const CARD_COLORS: Record<string, string> = {
  "1": "#e5484d",
  "2": "#f76b15",
  "3": "#e2a336",
  "4": "#30a46c",
  "5": "#00a2c7",
  "6": "#8e4ec6",
};

/** A card's colour as CSS, or null for none (or one this app cannot read). */
export function cardColor(color: string | null): string | null {
  if (!color) return null;
  return CARD_COLORS[color] ?? (/^#[0-9a-f]{6}$/i.test(color) ? color : null);
}

const NORMALS: Record<Side, Point> = {
  top: { x: 0, y: -1 },
  right: { x: 1, y: 0 },
  bottom: { x: 0, y: 1 },
  left: { x: -1, y: 0 },
};

export function isSide(value: string | null): value is Side {
  return value === "top" || value === "right" || value === "bottom" || value === "left";
}

/** The middle of a card's side. */
export function anchor(card: CanvasCard, side: Side): Point {
  switch (side) {
    case "top":
      return { x: card.x + card.width / 2, y: card.y };
    case "right":
      return { x: card.x + card.width, y: card.y + card.height / 2 };
    case "bottom":
      return { x: card.x + card.width / 2, y: card.y + card.height };
    case "left":
      return { x: card.x, y: card.y + card.height / 2 };
  }
}

/** The side of `card` that faces a point. */
export function facing(card: CanvasCard, point: Point): Side {
  const dx = point.x - (card.x + card.width / 2);
  const dy = point.y - (card.y + card.height / 2);
  if (Math.abs(dx) * card.height >= Math.abs(dy) * card.width) return dx >= 0 ? "right" : "left";
  return dy >= 0 ? "bottom" : "top";
}

export function center(card: CanvasCard): Point {
  return { x: card.x + card.width / 2, y: card.y + card.height / 2 };
}

/** An arrow's curve: leaving `from` out of its side and reaching `to`
 * from outside its side. The control points, for the head and the label. */
export function curve(from: Point, fromSide: Side, to: Point, toSide: Side): [Point, Point, Point, Point] {
  const reach = Math.max(40, Math.hypot(to.x - from.x, to.y - from.y) / 3);
  const a = NORMALS[fromSide];
  const b = NORMALS[toSide];
  return [
    from,
    { x: from.x + a.x * reach, y: from.y + a.y * reach },
    { x: to.x + b.x * reach, y: to.y + b.y * reach },
    to,
  ];
}

export function curvePath([p0, p1, p2, p3]: [Point, Point, Point, Point]): string {
  return `M ${p0.x} ${p0.y} C ${p1.x} ${p1.y}, ${p2.x} ${p2.y}, ${p3.x} ${p3.y}`;
}

/** The point halfway along a curve. */
export function midpoint([p0, p1, p2, p3]: [Point, Point, Point, Point]): Point {
  return {
    x: (p0.x + 3 * p1.x + 3 * p2.x + p3.x) / 8,
    y: (p0.y + 3 * p1.y + 3 * p2.y + p3.y) / 8,
  };
}

/** A triangle at `tip`, pointing away from `from`. */
export function arrowHead(from: Point, tip: Point, size = 10): string {
  const angle = Math.atan2(tip.y - from.y, tip.x - from.x);
  const corner = (turn: number) => ({
    x: tip.x - size * Math.cos(angle + turn),
    y: tip.y - size * Math.sin(angle + turn),
  });
  const [a, b] = [corner(0.45), corner(-0.45)];
  return `M ${tip.x} ${tip.y} L ${a.x} ${a.y} L ${b.x} ${b.y} Z`;
}

/** Whether `inner` lies wholly inside `outer`. */
export function contains(outer: CanvasCard, inner: CanvasCard): boolean {
  return (
    inner.x >= outer.x &&
    inner.y >= outer.y &&
    inner.x + inner.width <= outer.x + outer.width &&
    inner.y + inner.height <= outer.y + outer.height
  );
}

/** The cards that move with a card: a group takes what lies inside it. */
export function moving(board: CanvasBoard, id: string): string[] {
  const card = board.cards.find((c) => c.id === id);
  if (!card) return [];
  if (card.kind !== "group") return [id];
  return [id, ...board.cards.filter((c) => c.id !== id && contains(card, c)).map((c) => c.id)];
}

/** The board without some cards and the arrows that touch them. */
export function without(board: CanvasBoard, ids: string[]): CanvasBoard {
  const gone = new Set(ids);
  return {
    cards: board.cards.filter((c) => !gone.has(c.id)),
    arrows: board.arrows.filter((a) => !gone.has(a.id) && !gone.has(a.fromNode) && !gone.has(a.toNode)),
  };
}

/** The topmost card at a point (groups last, so cards inside them win). */
export function cardAt(board: CanvasBoard, point: Point, except?: string): CanvasCard | null {
  const inside = board.cards.filter(
    (c) =>
      c.id !== except && point.x >= c.x && point.x <= c.x + c.width && point.y >= c.y && point.y <= c.y + c.height,
  );
  return inside.find((c) => c.kind !== "group") ?? inside[0] ?? null;
}

/** The smallest rectangle around every card, or null for an empty board. */
export function bounds(board: CanvasBoard): { x: number; y: number; width: number; height: number } | null {
  if (board.cards.length === 0) return null;
  const left = Math.min(...board.cards.map((c) => c.x));
  const top = Math.min(...board.cards.map((c) => c.y));
  const right = Math.max(...board.cards.map((c) => c.x + c.width));
  const bottom = Math.max(...board.cards.map((c) => c.y + c.height));
  return { x: left, y: top, width: right - left, height: bottom - top };
}

/** A new card or arrow ID: 16 hex digits, as Obsidian makes them. */
export function newId(): string {
  const bytes = new Uint8Array(8);
  crypto.getRandomValues(bytes);
  return Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
}
