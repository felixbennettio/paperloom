import { useCallback, useEffect, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type PointerEvent as ReactPointerEvent } from "react";
import { useI18n } from "../i18n";
import { browserStorage } from "../lib/preferences";
import { backend, type DrawingItem, type DrawingPage, type DrawingPoint } from "../lib/api";
import {
  blankItem,
  centre,
  degrees,
  diameter,
  distance,
  guideFor,
  hits,
  lassoed,
  loadFavouritePens,
  moved,
  onProtractor,
  onRuler,
  pictureSize,
  protractorKnob,
  recognize,
  rotated,
  RULER_WIDTH,
  rulerKnob,
  saveFavouritePens,
  scaled,
  stroke,
  unionBounds,
  withFavourite,
  type Box,
  type FavouritePen,
  type Protractor,
  type Ruler,
} from "../lib/drawing";
import { pictureOf, readPicture } from "../lib/picture";
import { Icon, type IconName } from "./Icon";
import { MenuButton } from "./MenuButton";

interface Props {
  path: string;
}

type Tool = "pen" | "highlighter" | "eraser" | "rub" | "lasso" | "line" | "arrow" | "rectangle" | "ellipse" | "diamond" | "text";

interface View {
  x: number;
  y: number;
  zoom: number;
}

type Guide = "ruler" | "protractor";

/** What a pointer held down is doing. */
type Gesture =
  /** A stroke; along a ruler or protractor, `guide` puts each point on it. */
  | { kind: "draw"; points: DrawingPoint[]; pressures: number[]; pen: boolean; guide: ((p: DrawingPoint) => DrawingPoint) | null }
  /** Moving a ruler or protractor by its body. */
  | { kind: "guide-move"; which: Guide; start: DrawingPoint; from: DrawingPoint }
  /** Turning one about its middle by its knob: angles from the middle. */
  | { kind: "guide-turn"; which: Guide; start: number; from: number }
  | { kind: "erase"; removed: Set<string> }
  /** Rubbing out the parts of strokes along a path. */
  | { kind: "rub"; path: DrawingPoint[] }
  | { kind: "lasso"; points: DrawingPoint[] }
  | { kind: "move"; start: DrawingPoint; at: DrawingPoint }
  /** Growing the selection from its top left corner by its bottom right. */
  | { kind: "scale"; origin: DrawingPoint; start: DrawingPoint; at: DrawingPoint }
  /** Turning the selection about its centre; angles from it to the pointer. */
  | { kind: "rotate"; pivot: DrawingPoint; start: number; at: number }
  | { kind: "shape"; start: DrawingPoint; end: DrawingPoint }
  | { kind: "pan"; start: DrawingPoint; origin: View };

/** A text being written, over the drawing. */
interface Writing {
  /** The text item it replaces, if any. */
  id: string | null;
  at: DrawingPoint;
  value: string;
}

const PEN_COLORS = ["#1e1e1e", "#1971c2", "#e03131", "#2f9e44", "#f08c00", "#9c36b5"];
const HIGHLIGHTER_COLORS = ["#ffd43b", "#8ce99a", "#faa2c1", "#a5d8ff"];
const WIDTHS: [string, number][] = [
  ["Thin", 0.35],
  ["Medium", 0.6],
  ["Thick", 1.2],
];
const HIGHLIGHTER_WIDTH = 4;
/** How far from its path the point eraser rubs, in screen pixels. */
const RUB = 9;
const PAPERS = ["blank", "lines", "grid", "dots"];
const SPACING = 32;
const FONT = "-apple-system, 'Segoe UI', 'PingFang SC', 'Microsoft YaHei', sans-serif";

const TOOLS: [Tool, IconName, string][] = [
  ["pen", "pen", "Pen"],
  ["highlighter", "highlighter", "Highlighter"],
  ["eraser", "eraser", "Eraser"],
  ["rub", "eraser", "Point Eraser"],
  ["lasso", "lasso", "Lasso"],
  ["text", "text", "Text"],
  ["line", "line", "Line"],
  ["arrow", "arrow", "Arrow"],
  ["rectangle", "shapes", "Rectangle"],
  ["ellipse", "shapes", "Ellipse"],
  ["diamond", "shapes", "Diamond"],
];

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** A millimetre on the page, at 96 CSS pixels to the inch. */
const MM = 96 / 25.4;
const GUIDE_FILL = "rgba(232, 236, 244, 0.85)";
const GUIDE_LINE = "#8b949e";
const GUIDE_TEXT = "#57606a";

/** The knob a ruler or protractor turns by, in its own frame. */
function drawKnob(ctx: CanvasRenderingContext2D, x: number, y: number, zoom: number) {
  ctx.beginPath();
  ctx.arc(x, y, 9, 0, 2 * Math.PI);
  ctx.fillStyle = "#ffffff";
  ctx.fill();
  ctx.strokeStyle = "#4f46e5";
  ctx.lineWidth = 1.5 / zoom;
  ctx.stroke();
  ctx.beginPath();
  ctx.arc(x, y, 4.5, -Math.PI * 0.9, Math.PI * 0.4);
  ctx.stroke();
}

/** A ruler marked in millimetres along both edges, with its angle. */
function drawRuler(ctx: CanvasRenderingContext2D, ruler: Ruler, zoom: number) {
  const [half, edge] = [ruler.length / 2, RULER_WIDTH / 2];
  ctx.save();
  ctx.translate(ruler.x, ruler.y);
  ctx.rotate(ruler.angle);
  ctx.fillStyle = GUIDE_FILL;
  ctx.strokeStyle = GUIDE_LINE;
  ctx.lineWidth = 1 / zoom;
  ctx.fillRect(-half, -edge, ruler.length, RULER_WIDTH);
  ctx.strokeRect(-half, -edge, ruler.length, RULER_WIDTH);
  ctx.beginPath();
  for (let i = 0; i * MM <= ruler.length; i++) {
    const x = -half + i * MM;
    const tick = i % 10 === 0 ? 14 : i % 5 === 0 ? 9 : 5;
    ctx.moveTo(x, -edge);
    ctx.lineTo(x, -edge + tick);
    ctx.moveTo(x, edge);
    ctx.lineTo(x, edge - tick);
  }
  ctx.stroke();
  ctx.fillStyle = GUIDE_TEXT;
  ctx.font = `10px ${FONT}`;
  ctx.textAlign = "center";
  ctx.textBaseline = "top";
  for (let cm = 0; cm * 10 * MM <= ruler.length; cm++) ctx.fillText(String(cm), -half + cm * 10 * MM, -edge + 16);
  ctx.font = `12px ${FONT}`;
  ctx.textBaseline = "middle";
  ctx.fillText(`${degrees(ruler.angle)}°`, 0, 6);
  drawKnob(ctx, half - 22, 0, zoom);
  ctx.restore();
}

/** A protractor marked in degrees, with its angle. */
function drawProtractor(ctx: CanvasRenderingContext2D, protractor: Protractor, zoom: number) {
  const r = protractor.radius;
  ctx.save();
  ctx.translate(protractor.x, protractor.y);
  ctx.rotate(protractor.angle);
  ctx.beginPath();
  ctx.moveTo(-r, 0);
  ctx.arc(0, 0, r, Math.PI, 2 * Math.PI);
  ctx.closePath();
  ctx.fillStyle = GUIDE_FILL;
  ctx.fill();
  ctx.strokeStyle = GUIDE_LINE;
  ctx.lineWidth = 1 / zoom;
  ctx.stroke();
  ctx.beginPath();
  for (let degree = 0; degree <= 180; degree++) {
    const a = Math.PI + (degree * Math.PI) / 180;
    const tick = degree % 10 === 0 ? 14 : degree % 5 === 0 ? 9 : 5;
    ctx.moveTo(Math.cos(a) * r, Math.sin(a) * r);
    ctx.lineTo(Math.cos(a) * (r - tick), Math.sin(a) * (r - tick));
  }
  // Its middle.
  ctx.moveTo(-6, 0);
  ctx.lineTo(6, 0);
  ctx.moveTo(0, -6);
  ctx.lineTo(0, 0);
  ctx.stroke();
  ctx.fillStyle = GUIDE_TEXT;
  ctx.font = `10px ${FONT}`;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  for (let degree = 0; degree <= 180; degree += 30) {
    const a = Math.PI + (degree * Math.PI) / 180;
    ctx.fillText(String(180 - degree), Math.cos(a) * (r - 26), Math.sin(a) * (r - 26));
  }
  ctx.font = `12px ${FONT}`;
  ctx.fillText(`${degrees(protractor.angle)}°`, 0, -r / 2.6);
  drawKnob(ctx, r - 22, -14, zoom);
  ctx.restore();
}

/** Pictures, decoded once, by their `data:` URL. */
const pictures = new Map<string, HTMLImageElement>();

/** A picture to draw, started decoding when first asked for; `loaded`
 * runs when it can be drawn. */
function picture(url: string, loaded: () => void): HTMLImageElement {
  let image = pictures.get(url);
  if (!image) {
    image = new Image();
    image.onload = loaded;
    image.src = url;
    pictures.set(url, image);
  }
  return image;
}

/** Draws one item in drawing space. */
function drawItem(ctx: CanvasRenderingContext2D, item: DrawingItem) {
  ctx.save();
  ctx.globalAlpha = Math.max(0, Math.min(1, item.opacity / 100));
  if (item.angle) {
    const c = centre(item);
    ctx.translate(c.x, c.y);
    ctx.rotate(item.angle);
    ctx.translate(-c.x, -c.y);
  }
  ctx.strokeStyle = item.strokeColor;
  ctx.fillStyle = item.strokeColor;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  const { x, y, width: w, height: h } = item;
  const fill = item.backgroundColor !== "transparent" ? item.backgroundColor : null;
  switch (item.kind) {
    case "freedraw": {
      const points = item.points.map((p) => ({ x: x + p.x, y: y + p.y }));
      if (points.length === 1) {
        ctx.beginPath();
        ctx.arc(points[0].x, points[0].y, diameter(item.strokeWidth, item.pressures[0] ?? 0.5) / 2, 0, 2 * Math.PI);
        ctx.fill();
      } else if (!item.pressures.length || item.highlighter || item.opacity < 100) {
        // One path, so a see-through stroke is even.
        ctx.lineWidth = diameter(item.strokeWidth, 0.5);
        ctx.beginPath();
        ctx.moveTo(points[0].x, points[0].y);
        for (let i = 1; i < points.length - 1; i++) {
          const mx = (points[i].x + points[i + 1].x) / 2;
          const my = (points[i].y + points[i + 1].y) / 2;
          ctx.quadraticCurveTo(points[i].x, points[i].y, mx, my);
        }
        ctx.lineTo(points[points.length - 1].x, points[points.length - 1].y);
        ctx.stroke();
      } else {
        for (let i = 1; i < points.length; i++) {
          const pressure = ((item.pressures[i - 1] ?? 0.5) + (item.pressures[i] ?? 0.5)) / 2;
          ctx.lineWidth = diameter(item.strokeWidth, pressure);
          ctx.beginPath();
          ctx.moveTo(points[i - 1].x, points[i - 1].y);
          ctx.lineTo(points[i].x, points[i].y);
          ctx.stroke();
        }
      }
      break;
    }
    case "line":
    case "arrow": {
      const points = item.points.map((p) => ({ x: x + p.x, y: y + p.y }));
      if (points.length < 2) break;
      ctx.lineWidth = item.strokeWidth;
      ctx.beginPath();
      ctx.moveTo(points[0].x, points[0].y);
      for (const p of points.slice(1)) ctx.lineTo(p.x, p.y);
      if (item.arrowhead) {
        const a = points[points.length - 2];
        const b = points[points.length - 1];
        const angle = Math.atan2(b.y - a.y, b.x - a.x);
        const size = 8 + 4 * item.strokeWidth;
        for (const turn of [-0.45, 0.45]) {
          ctx.moveTo(b.x - size * Math.cos(angle + turn), b.y - size * Math.sin(angle + turn));
          ctx.lineTo(b.x, b.y);
        }
      }
      ctx.stroke();
      break;
    }
    case "image": {
      const image = pictures.get(item.image ?? "");
      if (image?.complete && image.naturalWidth) {
        ctx.drawImage(image, x, y, w, h);
      } else {
        ctx.fillStyle = "#f1f3f5";
        ctx.fillRect(x, y, w, h);
      }
      break;
    }
    case "rectangle":
    case "ellipse":
    case "diamond": {
      ctx.lineWidth = item.strokeWidth;
      ctx.beginPath();
      if (item.kind === "rectangle") ctx.rect(x, y, w, h);
      else if (item.kind === "ellipse") ctx.ellipse(x + w / 2, y + h / 2, Math.abs(w / 2), Math.abs(h / 2), 0, 0, 2 * Math.PI);
      else {
        ctx.moveTo(x + w / 2, y);
        ctx.lineTo(x + w, y + h / 2);
        ctx.lineTo(x + w / 2, y + h);
        ctx.lineTo(x, y + h / 2);
        ctx.closePath();
      }
      if (fill) {
        ctx.fillStyle = fill;
        ctx.fill();
      }
      ctx.stroke();
      break;
    }
    case "text": {
      ctx.font = `${item.fontSize}px ${FONT}`;
      ctx.textBaseline = "top";
      item.text.split("\n").forEach((line, i) => ctx.fillText(line, x, y + i * item.fontSize * 1.25));
      break;
    }
  }
  ctx.restore();
}

/** The paper's lines, grid or dots, in screen space. */
function drawPaper(ctx: CanvasRenderingContext2D, paper: string, view: View, width: number, height: number) {
  const step = SPACING * view.zoom;
  if (paper === "blank" || step < 6) return;
  const startX = (((view.x % step) + step) % step) - step;
  const startY = (((view.y % step) + step) % step) - step;
  ctx.save();
  if (paper === "dots") {
    ctx.fillStyle = "#c3c7cf";
    for (let x = startX; x < width + step; x += step) {
      for (let y = startY; y < height + step; y += step) ctx.fillRect(x - 1, y - 1, 2, 2);
    }
  } else {
    ctx.strokeStyle = "#e3e6eb";
    ctx.lineWidth = 1;
    ctx.beginPath();
    for (let y = startY; y < height + step; y += step) {
      ctx.moveTo(0, Math.round(y) + 0.5);
      ctx.lineTo(width, Math.round(y) + 0.5);
    }
    if (paper === "grid") {
      for (let x = startX; x < width + step; x += step) {
        ctx.moveTo(Math.round(x) + 0.5, 0);
        ctx.lineTo(Math.round(x) + 0.5, height);
      }
    }
    ctx.stroke();
  }
  ctx.restore();
}

/** Where a selection's handles are: the corner that scales it and the knob
 * above it that turns it. */
function handles(box: Box, zoom: number): { corner: DrawingPoint; knob: DrawingPoint } {
  const pad = 4 / zoom;
  return {
    corner: { x: box.right + pad, y: box.bottom + pad },
    knob: { x: (box.left + box.right) / 2, y: box.top - pad - 18 / zoom },
  };
}

/** How much a scaling drag grows the selection. */
function scaleFactor(origin: DrawingPoint, start: DrawingPoint, at: DrawingPoint): number {
  const sx = (at.x - origin.x) / Math.max(1e-6, start.x - origin.x);
  const sy = (at.y - origin.y) / Math.max(1e-6, start.y - origin.y);
  return Math.max(0.05, (sx + sy) / 2);
}

/** Items with the selection where a move, scale or turn has it so far. */
function transformed(items: DrawingItem[], live: Gesture | null, selection: Set<string>): DrawingItem[] {
  switch (live?.kind) {
    case "move":
      return live.at.x === live.start.x && live.at.y === live.start.y
        ? items
        : moved(items, selection, live.at.x - live.start.x, live.at.y - live.start.y);
    case "scale": {
      const factor = scaleFactor(live.origin, live.start, live.at);
      return factor === 1 ? items : scaled(items, selection, live.origin, factor);
    }
    case "rotate":
      return live.at === live.start ? items : rotated(items, selection, live.pivot, live.at - live.start);
    default:
      return items;
  }
}

/** Highlighter strokes lie under the rest. */
function layered(items: DrawingItem[]): DrawingItem[] {
  return [...items.filter((i) => i.highlighter), ...items.filter((i) => !i.highlighter)];
}

/** A text item's size, as the canvas measures it. */
function measureText(ctx: CanvasRenderingContext2D | null, text: string, fontSize: number): { width: number; height: number } {
  const lines = text.split("\n");
  let width = 0;
  if (ctx) {
    ctx.save();
    ctx.font = `${fontSize}px ${FONT}`;
    width = Math.max(...lines.map((line) => ctx.measureText(line).width));
    ctx.restore();
  } else {
    width = Math.max(...lines.map((line) => line.length * fontSize * 0.6));
  }
  return { width: Math.ceil(width), height: Math.ceil(lines.length * fontSize * 1.25) };
}

/** A hand-drawn page kept as an Excalidraw scene in the vault: pens with
 * pressure, highlighters under the ink, erasers that take whole strokes or
 * rub out parts of them, a lasso to move, recolour or delete, lines,
 * arrows, shapes (or strokes turned into them) and text, on blank, lined,
 * squared or dotted paper. The
 * wheel pans, and zooms with Ctrl or ⌘; Space or the middle button drags
 * the page; with a pen in use, a finger pans. */
export function DrawingView({ path }: Props) {
  const { t } = useI18n();
  const [page, setPage] = useState<DrawingPage | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [view, setView] = useState<View>({ x: 40, y: 40, zoom: 1 });
  const [tool, setTool] = useState<Tool>("pen");
  const [penColor, setPenColor] = useState(PEN_COLORS[0]);
  const [highlighterColor, setHighlighterColor] = useState(HIGHLIGHTER_COLORS[0]);
  const [width, setWidth] = useState(WIDTHS[1][1]);
  const [shapes, setShapes] = useState(false);
  const [favourites, setFavourites] = useState<FavouritePen[]>(() => loadFavouritePens(browserStorage()));
  const [selection, setSelection] = useState<Set<string>>(new Set());
  const [writing, setWriting] = useState<Writing | null>(null);
  const [size, setSize] = useState({ width: 0, height: 0 });
  const [ruler, setRuler] = useState<Ruler | null>(null);
  const [protractor, setProtractor] = useState<Protractor | null>(null);
  const surface = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const latest = useRef<DrawingPage | null>(null);
  const past = useRef<DrawingItem[][]>([]);
  const future = useRef<DrawingItem[][]>([]);
  const timer = useRef<number | null>(null);
  const gesture = useRef<Gesture | null>(null);
  const frame = useRef<number | null>(null);
  const space = useRef(false);
  const penSeen = useRef(false);
  const fitting = useRef(false);
  /** The text last written, so leaving its field twice writes it once. */
  const written = useRef<Writing | null>(null);
  const pictureInput = useRef<HTMLInputElement>(null);

  const save = useCallback(() => {
    timer.current = null;
    const current = latest.current;
    if (current) backend.vaultWriteDrawing(path, current).catch((e: unknown) => setError(message(e)));
  }, [path]);

  /** Shows a change now, keeps it for undo, and saves it shortly after. */
  const change = useCallback(
    (next: DrawingPage, undoable = true) => {
      const before = latest.current;
      if (undoable && before) {
        past.current = [...past.current.slice(-199), before.items];
        future.current = [];
      }
      latest.current = next;
      setPage(next);
      if (timer.current !== null) window.clearTimeout(timer.current);
      timer.current = window.setTimeout(save, 600);
    },
    [save],
  );

  useEffect(() => {
    let current = true;
    backend.vaultReadDrawing(path).then(
      (loaded) => {
        if (!current) return;
        latest.current = loaded;
        past.current = [];
        future.current = [];
        fitting.current = true;
        setPage(loaded);
      },
      (e: unknown) => current && setError(message(e)),
    );
    return () => {
      current = false;
      if (timer.current !== null) {
        window.clearTimeout(timer.current);
        save();
      }
    };
  }, [path, save]);

  // The canvas follows the size of its area.
  useEffect(() => {
    const element = surface.current;
    if (!element) return;
    const observer = new ResizeObserver(() => setSize({ width: element.clientWidth, height: element.clientHeight }));
    observer.observe(element);
    setSize({ width: element.clientWidth, height: element.clientHeight });
    return () => observer.disconnect();
  }, [page !== null]);

  // A drawing just opened is shown whole.
  useEffect(() => {
    if (!page || !fitting.current || !size.width) return;
    fitting.current = false;
    const area = unionBounds(page.items);
    if (!area) return;
    const zoom = Math.min(1, Math.max(0.1, Math.min(size.width / (area.right - area.left + 80), size.height / (area.bottom - area.top + 80))));
    setView({
      zoom,
      x: size.width / 2 - ((area.left + area.right) / 2) * zoom,
      y: size.height / 2 - ((area.top + area.bottom) / 2) * zoom,
    });
  }, [page, size]);

  // The wheel pans, and zooms with Ctrl or ⌘ (pinch on a trackpad).
  useEffect(() => {
    const element = surface.current;
    if (!element) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const box = element.getBoundingClientRect();
      setView((v) => {
        if (!e.ctrlKey && !e.metaKey) return { ...v, x: v.x - e.deltaX, y: v.y - e.deltaY };
        const zoom = Math.min(8, Math.max(0.1, v.zoom * Math.exp(-e.deltaY * 0.0015)));
        const px = e.clientX - box.left;
        const py = e.clientY - box.top;
        return { zoom, x: px - ((px - v.x) / v.zoom) * zoom, y: py - ((py - v.y) / v.zoom) * zoom };
      });
    };
    element.addEventListener("wheel", onWheel, { passive: false });
    return () => element.removeEventListener("wheel", onWheel);
  }, [page !== null]);

  /** Draws the page, and over it what the pointer is doing. */
  const paint = useCallback(() => {
    frame.current = null;
    const element = canvas.current;
    const current = latest.current;
    const ctx = element?.getContext("2d");
    if (!element || !ctx || !current) return;
    const ratio = window.devicePixelRatio || 1;
    const w = Math.round(size.width * ratio);
    const h = Math.round(size.height * ratio);
    if (element.width !== w || element.height !== h) {
      element.width = w;
      element.height = h;
    }
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    ctx.fillStyle = "#ffffff";
    ctx.fillRect(0, 0, size.width, size.height);
    drawPaper(ctx, current.paper, view, size.width, size.height);
    ctx.setTransform(ratio * view.zoom, 0, 0, ratio * view.zoom, ratio * view.x, ratio * view.y);
    const live = gesture.current;
    const hidden = live?.kind === "erase" ? live.removed : null;
    const shown = transformed(current.items, live, selection);
    for (const item of layered(shown)) {
      if (hidden?.has(item.id) || writing?.id === item.id) continue;
      drawItem(ctx, item);
    }
    if (live?.kind === "draw" && live.points.length) {
      const highlighter = tool === "highlighter";
      drawItem(
        ctx,
        stroke(live.points, live.pen && !highlighter ? live.pressures : [], {
          strokeColor: highlighter ? highlighterColor : penColor,
          strokeWidth: highlighter ? HIGHLIGHTER_WIDTH : width,
          opacity: highlighter ? 40 : 100,
          highlighter,
        }),
      );
    } else if (live?.kind === "shape") {
      drawItem(ctx, shapeItem(live.start, live.end));
    } else if (live?.kind === "rub" && live.path.length) {
      ctx.save();
      ctx.strokeStyle = "rgba(79, 70, 229, 0.22)";
      ctx.lineWidth = (2 * RUB) / view.zoom;
      ctx.lineCap = "round";
      ctx.lineJoin = "round";
      ctx.beginPath();
      ctx.moveTo(live.path[0].x, live.path[0].y);
      for (const p of live.path) ctx.lineTo(p.x, p.y);
      ctx.stroke();
      ctx.restore();
    } else if (live?.kind === "lasso" && live.points.length > 1) {
      ctx.save();
      ctx.setLineDash([6 / view.zoom, 4 / view.zoom]);
      ctx.strokeStyle = "#4f46e5";
      ctx.lineWidth = 1 / view.zoom;
      ctx.beginPath();
      ctx.moveTo(live.points[0].x, live.points[0].y);
      for (const p of live.points.slice(1)) ctx.lineTo(p.x, p.y);
      ctx.stroke();
      ctx.restore();
    }
    const box = unionBounds(shown.filter((i) => selection.has(i.id)));
    if (box) {
      ctx.save();
      ctx.setLineDash([6 / view.zoom, 4 / view.zoom]);
      ctx.strokeStyle = "#4f46e5";
      ctx.lineWidth = 1.5 / view.zoom;
      const pad = 4 / view.zoom;
      ctx.strokeRect(box.left - pad, box.top - pad, box.right - box.left + 2 * pad, box.bottom - box.top + 2 * pad);
      if (tool === "lasso") {
        const { corner, knob } = handles(box, view.zoom);
        ctx.setLineDash([]);
        ctx.fillStyle = "#ffffff";
        const size = 9 / view.zoom;
        ctx.fillRect(corner.x - size / 2, corner.y - size / 2, size, size);
        ctx.strokeRect(corner.x - size / 2, corner.y - size / 2, size, size);
        ctx.beginPath();
        ctx.moveTo(knob.x, box.top - pad);
        ctx.lineTo(knob.x, knob.y);
        ctx.stroke();
        ctx.beginPath();
        ctx.arc(knob.x, knob.y, size / 2, 0, 2 * Math.PI);
        ctx.fill();
        ctx.stroke();
      }
      ctx.restore();
    }
    if (ruler) drawRuler(ctx, ruler, view.zoom);
    if (protractor) drawProtractor(ctx, protractor, view.zoom);
    // `shapeItem` reads the tool and colours below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [size, view, selection, writing, tool, penColor, highlighterColor, width, ruler, protractor]);

  const repaint = useCallback(() => {
    if (frame.current === null) frame.current = window.requestAnimationFrame(paint);
  }, [paint]);

  useEffect(() => {
    paint();
  }, [paint, page]);

  // Pictures are decoded once; each repaints the page when it is ready.
  useEffect(() => {
    for (const item of page?.items ?? []) if (item.image) picture(item.image, repaint);
  }, [page, repaint]);

  useEffect(
    () => () => {
      if (frame.current !== null) window.cancelAnimationFrame(frame.current);
    },
    [],
  );

  const toWorld = (clientX: number, clientY: number): DrawingPoint => {
    const box = surface.current?.getBoundingClientRect();
    return { x: (clientX - (box?.left ?? 0) - view.x) / view.zoom, y: (clientY - (box?.top ?? 0) - view.y) / view.zoom };
  };

  /** The item a shape tool makes from a drag. */
  function shapeItem(start: DrawingPoint, end: DrawingPoint): DrawingItem {
    const strokeWidth = Math.max(1, Math.round(diameter(width, 0.5) * 2) / 2);
    if (tool === "line" || tool === "arrow") {
      return blankItem(tool, {
        x: start.x,
        y: start.y,
        width: Math.abs(end.x - start.x),
        height: Math.abs(end.y - start.y),
        points: [
          { x: 0, y: 0 },
          { x: end.x - start.x, y: end.y - start.y },
        ],
        strokeColor: penColor,
        strokeWidth,
        arrowhead: tool === "arrow",
      });
    }
    return blankItem(tool, {
      x: Math.min(start.x, end.x),
      y: Math.min(start.y, end.y),
      width: Math.abs(end.x - start.x),
      height: Math.abs(end.y - start.y),
      strokeColor: penColor,
      strokeWidth,
    });
  }

  if (!page) {
    return <div className="drawing-view">{error ? <p className="hint error">{error}</p> : null}</div>;
  }

  const items = page.items;
  const setItems = (next: DrawingItem[]) => change({ ...(latest.current ?? page), items: next });

  const undo = () => {
    const previous = past.current.pop();
    if (!previous || !latest.current) return;
    future.current.push(latest.current.items);
    setSelection(new Set());
    change({ ...latest.current, items: previous }, false);
  };

  const redo = () => {
    const next = future.current.pop();
    if (!next || !latest.current) return;
    past.current.push(latest.current.items);
    setSelection(new Set());
    change({ ...latest.current, items: next }, false);
  };

  /** Saves the drawing, then the picture of it. */
  const exportAs = async (format: string) => {
    setNotice(null);
    setError(null);
    if (timer.current !== null) {
      window.clearTimeout(timer.current);
      timer.current = null;
    }
    try {
      if (latest.current) await backend.vaultWriteDrawing(path, latest.current);
      const png =
        format === "png" || format === "pdf"
          ? await pictureOf(await backend.vaultDrawingSvg(path), format === "pdf" ? "image/jpeg" : "image/png")
          : null;
      const written = await backend.drawingExport(path, format, png);
      if (written) setNotice(t("Saved {path}.", { path: written }));
    } catch (e) {
      setError(message(e));
    }
  };

  const removeSelection = () => {
    if (!selection.size) return;
    setItems(items.filter((i) => !selection.has(i.id)));
    setSelection(new Set());
  };

  const recolor = (color: string) => {
    if (tool === "highlighter") setHighlighterColor(color);
    else setPenColor(color);
    if (selection.size) setItems(items.map((i) => (selection.has(i.id) ? { ...i, strokeColor: color } : i)));
  };

  const commitWriting = () => {
    if (!writing || written.current === writing) return;
    written.current = writing;
    const text = writing.value.replace(/\s+$/, "");
    const others = items.filter((i) => i.id !== writing.id);
    setWriting(null);
    if (!text) {
      if (writing.id) setItems(others);
      return;
    }
    const fontSize = items.find((i) => i.id === writing.id)?.fontSize ?? 20;
    const measured = measureText(canvas.current?.getContext("2d") ?? null, text, fontSize);
    const old = items.find((i) => i.id === writing.id);
    const item = old
      ? { ...old, text, ...measured }
      : blankItem("text", { x: writing.at.x, y: writing.at.y, text, fontSize, strokeColor: penColor, ...measured });
    setItems(old ? items.map((i) => (i.id === old.id ? item : i)) : [...items, item]);
  };

  const begin = (e: ReactPointerEvent, next: Gesture) => {
    canvas.current?.setPointerCapture(e.pointerId);
    gesture.current = next;
    repaint();
  };

  const onPointerDown = (e: ReactPointerEvent<HTMLCanvasElement>) => {
    // Leaving a text field writes its text.
    surface.current?.focus();
    if (writing) {
      commitWriting();
      return;
    }
    if (e.pointerType === "pen") penSeen.current = true;
    const at = toWorld(e.clientX, e.clientY);
    const panning = e.button === 1 || (e.button === 0 && space.current) || (e.pointerType === "touch" && penSeen.current);
    if (panning) {
      begin(e, { kind: "pan", start: { x: e.clientX, y: e.clientY }, origin: view });
      return;
    }
    if (e.button !== 0 && e.button !== 5) return;
    // A ruler or protractor moves by its body and turns by its knob.
    const reach = 14 / view.zoom;
    const turning: [Guide, Ruler | Protractor] | null =
      protractor && distance(at, protractorKnob(protractor)) <= reach
        ? ["protractor", protractor]
        : ruler && distance(at, rulerKnob(ruler)) <= reach
          ? ["ruler", ruler]
          : null;
    if (turning) {
      const [which, guide] = turning;
      begin(e, { kind: "guide-turn", which, start: Math.atan2(at.y - guide.y, at.x - guide.x), from: guide.angle });
      return;
    }
    const holding: [Guide, Ruler | Protractor] | null =
      protractor && onProtractor(protractor, at) ? ["protractor", protractor] : ruler && onRuler(ruler, at) ? ["ruler", ruler] : null;
    if (holding) {
      const [which, guide] = holding;
      begin(e, { kind: "guide-move", which, start: at, from: { x: guide.x, y: guide.y } });
      return;
    }
    if (tool === "rub") {
      begin(e, { kind: "rub", path: [at] });
      return;
    }
    // The eraser end of a pen erases whatever the tool.
    const erasing = tool === "eraser" || e.button === 5 || (e.buttons & 32) !== 0;
    if (erasing) {
      const removed = new Set(items.filter((i) => hits(i, at, 6 / view.zoom)).map((i) => i.id));
      begin(e, { kind: "erase", removed });
      return;
    }
    switch (tool) {
      case "pen":
      case "highlighter": {
        setSelection(new Set());
        const guide = guideFor(ruler, protractor, at, 16 / view.zoom);
        begin(e, { kind: "draw", points: [guide ? guide(at) : at], pressures: [e.pressure || 0.5], pen: e.pointerType === "pen", guide });
        return;
      }
      case "lasso": {
        const box = unionBounds(items.filter((i) => selection.has(i.id)));
        const reach = 9 / view.zoom;
        const grips = box ? handles(box, view.zoom) : null;
        if (box && grips && distance(at, grips.corner) <= reach) {
          begin(e, { kind: "scale", origin: { x: box.left, y: box.top }, start: grips.corner, at });
        } else if (box && grips && distance(at, grips.knob) <= reach) {
          const pivot = { x: (box.left + box.right) / 2, y: (box.top + box.bottom) / 2 };
          const angle = Math.atan2(at.y - pivot.y, at.x - pivot.x);
          begin(e, { kind: "rotate", pivot, start: angle, at: angle });
        } else if (box && at.x >= box.left && at.x <= box.right && at.y >= box.top && at.y <= box.bottom) {
          begin(e, { kind: "move", start: at, at });
        } else {
          setSelection(new Set());
          begin(e, { kind: "lasso", points: [at] });
        }
        return;
      }
      case "text": {
        const existing = [...items].reverse().find((i) => i.kind === "text" && hits(i, at, 4 / view.zoom));
        setWriting(existing ? { id: existing.id, at: { x: existing.x, y: existing.y }, value: existing.text } : { id: null, at, value: "" });
        return;
      }
      default:
        setSelection(new Set());
        begin(e, { kind: "shape", start: at, end: at });
    }
  };

  const onPointerMove = (e: ReactPointerEvent<HTMLCanvasElement>) => {
    const live = gesture.current;
    if (!live) return;
    const events = e.nativeEvent.getCoalescedEvents?.() ?? [];
    const samples = events.length ? events : [e.nativeEvent];
    switch (live.kind) {
      case "pan":
        setView({ ...live.origin, x: live.origin.x + e.clientX - live.start.x, y: live.origin.y + e.clientY - live.start.y });
        return;
      case "draw":
        for (const sample of samples) {
          const raw = toWorld(sample.clientX, sample.clientY);
          const at = live.guide ? live.guide(raw) : raw;
          if (distance(at, live.points[live.points.length - 1]) < 0.6 / view.zoom) continue;
          live.points.push(at);
          live.pressures.push(sample.pressure || 0.5);
        }
        break;
      case "guide-move": {
        const at = toWorld(e.clientX, e.clientY);
        const place = { x: live.from.x + at.x - live.start.x, y: live.from.y + at.y - live.start.y };
        if (live.which === "ruler") setRuler((r) => r && { ...r, ...place });
        else setProtractor((p) => p && { ...p, ...place });
        return;
      }
      case "guide-turn": {
        const guide = live.which === "ruler" ? ruler : protractor;
        if (!guide) return;
        const at = toWorld(e.clientX, e.clientY);
        const angle = live.from + Math.atan2(at.y - guide.y, at.x - guide.x) - live.start;
        // Close to a multiple of 15°, it settles there.
        const step = Math.PI / 12;
        const settled = Math.abs(angle - Math.round(angle / step) * step) < Math.PI / 90 ? Math.round(angle / step) * step : angle;
        if (live.which === "ruler") setRuler((r) => r && { ...r, angle: settled });
        else setProtractor((p) => p && { ...p, angle: settled });
        return;
      }
      case "erase":
        for (const sample of samples) {
          const at = toWorld(sample.clientX, sample.clientY);
          for (const item of items) if (!live.removed.has(item.id) && hits(item, at, 6 / view.zoom)) live.removed.add(item.id);
        }
        break;
      case "rub":
        for (const sample of samples) live.path.push(toWorld(sample.clientX, sample.clientY));
        break;
      case "lasso":
        live.points.push(toWorld(e.clientX, e.clientY));
        break;
      case "move":
      case "scale":
        live.at = toWorld(e.clientX, e.clientY);
        break;
      case "rotate": {
        const at = toWorld(e.clientX, e.clientY);
        live.at = Math.atan2(at.y - live.pivot.y, at.x - live.pivot.x);
        break;
      }
      case "shape":
        live.end = toWorld(e.clientX, e.clientY);
        break;
    }
    repaint();
  };

  const onPointerUp = () => {
    const live = gesture.current;
    gesture.current = null;
    if (!live) return;
    switch (live.kind) {
      case "draw": {
        const highlighter = tool === "highlighter";
        const shape = shapes && !highlighter && !live.guide ? recognize(live.points) : null;
        if (shape?.kind === "line") {
          setItems([...items, shapeItemFor("line", shape.from, shape.to)]);
        } else if (shape) {
          setItems([
            ...items,
            shapeItemFor(shape.kind, { x: shape.box.left, y: shape.box.top }, { x: shape.box.right, y: shape.box.bottom }),
          ]);
        } else {
          setItems([
            ...items,
            stroke(live.points, live.pen && !highlighter ? live.pressures : [], {
              strokeColor: highlighter ? highlighterColor : penColor,
              strokeWidth: highlighter ? HIGHLIGHTER_WIDTH : width,
              opacity: highlighter ? 40 : 100,
              highlighter,
            }),
          ]);
        }
        break;
      }
      case "erase":
        if (live.removed.size) setItems(items.filter((i) => !live.removed.has(i.id)));
        else repaint();
        break;
      case "rub": {
        const before = items;
        backend.drawingErase(before, live.path, RUB / view.zoom).then(
          (left) => {
            const same = left.length === before.length && left.every((item, i) => item.points.length === before[i].points.length);
            if (same) repaint();
            else setItems(left);
          },
          (e: unknown) => setError(message(e)),
        );
        break;
      }
      case "lasso": {
        const chosen =
          live.points.length > 3
            ? items.filter((i) => lassoed(i, live.points))
            : [...items].reverse().filter((i) => hits(i, live.points[0], 4 / view.zoom)).slice(0, 1);
        setSelection(new Set(chosen.map((i) => i.id)));
        repaint();
        break;
      }
      case "move":
      case "scale":
      case "rotate": {
        const next = transformed(items, live, selection);
        if (next.some((item, i) => item !== items[i])) setItems(next);
        else repaint();
        break;
      }
      case "shape": {
        const made = shapeItem(live.start, live.end);
        if (distance(live.start, live.end) > 3 / view.zoom) setItems([...items, made]);
        else repaint();
        break;
      }
      case "pan":
      case "guide-move":
      case "guide-turn":
        break;
    }
  };

  /** The middle of what is in view, on the page. */
  const middle = (): DrawingPoint => ({ x: (size.width / 2 - view.x) / view.zoom, y: (size.height / 2 - view.y) / view.zoom });

  /** A shape made from a recognised stroke, in the pen's colour. */
  function shapeItemFor(kind: "line" | "rectangle" | "ellipse", from: DrawingPoint, to: DrawingPoint): DrawingItem {
    const strokeWidth = Math.max(1, Math.round(diameter(width, 0.5) * 2) / 2);
    if (kind === "line") {
      return blankItem("line", {
        x: from.x,
        y: from.y,
        width: Math.abs(to.x - from.x),
        height: Math.abs(to.y - from.y),
        points: [
          { x: 0, y: 0 },
          { x: to.x - from.x, y: to.y - from.y },
        ],
        strokeColor: penColor,
        strokeWidth,
      });
    }
    return blankItem(kind, { x: from.x, y: from.y, width: to.x - from.x, height: to.y - from.y, strokeColor: penColor, strokeWidth });
  }

  const onKeyDown = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    if (e.target instanceof HTMLTextAreaElement || e.target instanceof HTMLSelectElement) return;
    const command = e.metaKey || e.ctrlKey;
    if (command && e.key.toLowerCase() === "z") {
      e.preventDefault();
      if (e.shiftKey) redo();
      else undo();
    } else if (command && e.key.toLowerCase() === "y") {
      e.preventDefault();
      redo();
    } else if (command && e.key.toLowerCase() === "a") {
      e.preventDefault();
      setTool("lasso");
      setSelection(new Set(items.map((i) => i.id)));
    } else if (e.key === "Delete" || e.key === "Backspace") {
      e.preventDefault();
      removeSelection();
    } else if (e.key === "Escape") {
      setSelection(new Set());
    } else if (e.key === " ") {
      e.preventDefault();
      space.current = true;
    } else if (!command && !e.altKey) {
      const next = ({ p: "pen", h: "highlighter", e: "eraser", l: "lasso", t: "text", r: "rectangle", o: "ellipse", a: "arrow" } as Record<string, Tool>)[
        e.key.toLowerCase()
      ];
      if (next) setTool(next);
    }
  };

  /** Puts a picture from a file in the middle of the view, to move with the lasso. */
  const insertPicture = async (file: File) => {
    try {
      const { url, width: w, height: h } = await readPicture(file);
      const fit = pictureSize(w, h);
      const at = middle();
      setItems([
        ...items,
        blankItem("image", { x: at.x - fit.width / 2, y: at.y - fit.height / 2, width: fit.width, height: fit.height, image: url }),
      ]);
      setTool("lasso");
    } catch (e) {
      setError(message(e));
    }
  };

  const keepFavourites = (pens: FavouritePen[]) => {
    setFavourites(pens);
    saveFavouritePens(browserStorage(), pens);
  };

  const colors = tool === "highlighter" ? HIGHLIGHTER_COLORS : PEN_COLORS;
  const color = tool === "highlighter" ? highlighterColor : penColor;
  const cursor = tool === "text" ? "text" : tool === "lasso" ? "default" : "crosshair";

  return (
    <div className="drawing-view">
      <div className="drawing-toolbar" role="toolbar" aria-label={t("Drawing")}>
        <MenuButton
          label={t("Tool")}
          icon={<Icon name={TOOLS.find(([id]) => id === tool)?.[1] ?? "pen"} size={16} />}
          iconOnly
          items={TOOLS.map(([id, icon, label]) => ({
            label: t(label), icon: <Icon name={icon} size={16} />, checked: tool === id, onSelect: () => setTool(id),
          }))}
        />
        <span className="drawing-separator" />
        <MenuButton
          label={t("Color")}
          icon={<span className="drawing-swatch" style={{ background: color }} />}
          iconOnly
          items={colors.map((value) => ({
            label: value, icon: <span className="menu-swatch" style={{ background: value }} />, checked: value === color, onSelect: () => recolor(value),
          }))}
        />
        <MenuButton
          label={t("Line Width")}
          icon={<span className="drawing-width" style={{ height: Math.max(1, diameter(width, 0.5)) }} />}
          iconOnly
          items={WIDTHS.map(([label, value]) => ({ label: t(label), checked: value === width, onSelect: () => setWidth(value) }))}
        />
        <span className="drawing-separator" />
        <MenuButton
          label={t("Add Pen to Favourites")}
          icon={<Icon name="star" size={16} />}
          iconOnly
          items={[
            ...favourites.map((pen) => ({
              label: `${t(pen.highlighter ? "Highlighter" : "Pen")} ${pen.color} · ${pen.width}`,
              icon: <span className="menu-swatch" style={{ background: pen.color }} />,
              onSelect: () => {
                setTool(pen.highlighter ? "highlighter" : "pen");
                if (pen.highlighter) setHighlighterColor(pen.color);
                else {
                  setPenColor(pen.color);
                  setWidth(pen.width);
                }
              },
            })),
            { label: t("Add Pen to Favourites"), disabled: tool !== "pen" && tool !== "highlighter", onSelect: () => keepFavourites(withFavourite(favourites, {
              highlighter: tool === "highlighter", color: tool === "highlighter" ? highlighterColor : penColor, width: tool === "highlighter" ? HIGHLIGHTER_WIDTH : width,
            })) },
            ...favourites.map((pen, index) => ({
              label: `${t("Remove from Favourites")} ${pen.color} · ${pen.width}`,
              onSelect: () => keepFavourites(favourites.filter((_, i) => i !== index)),
            })),
          ]}
        />
        <span className="drawing-separator" />
        <input
          ref={pictureInput}
          type="file"
          accept="image/*"
          hidden
          onChange={(e) => {
            const file = e.target.files?.[0];
            e.target.value = "";
            if (file) void insertPicture(file);
          }}
        />
        <button
          className="icon-button"
          title={t("Insert Picture…")}
          aria-label={t("Insert Picture…")}
          onClick={() => pictureInput.current?.click()}
        >
          <Icon name="picture" size={16} />
        </button>
        <button className={shapes ? "icon-button selected" : "icon-button"} title={t("Ink to Shape")} aria-label={t("Ink to Shape")} aria-pressed={shapes} onClick={() => setShapes((value) => !value)}>
          <Icon name="shapes" size={16} />
        </button>
        <MenuButton label={t("More")} icon={<span aria-hidden="true">⋯</span>} iconOnly items={[
          { label: t("Ruler"), icon: <Icon name="ruler" size={16} />, checked: ruler !== null, onSelect: () => setRuler(ruler ? null : { ...middle(), angle: 0, length: 600 }) },
          { label: t("Protractor"), icon: <Icon name="protractor" size={16} />, checked: protractor !== null, onSelect: () => setProtractor(protractor ? null : { ...middle(), angle: 0, radius: 160 }) },
        ]} />
        <select value={page.paper || "blank"} aria-label={t("Paper")} onChange={(e) => change({ ...page, paper: e.target.value })}>
          {PAPERS.map((p) => (
            <option key={p} value={p}>
              {t(p.charAt(0).toUpperCase() + p.slice(1))}
            </option>
          ))}
        </select>
        <MenuButton label={t("Export…")} icon={<Icon name="doc" size={15} />} items={[
          { label: t("Picture (SVG)"), onSelect: () => void exportAs("svg") },
          { label: t("Picture (PNG)"), onSelect: () => void exportAs("png") },
          { label: "PDF", onSelect: () => void exportAs("pdf") },
        ]} />
        <span className="drawing-spacer" />
        <button className="icon-button" title={t("Undo")} aria-label={t("Undo")} disabled={!past.current.length} onClick={undo}>
          <Icon name="undo" size={16} />
        </button>
        <button className="icon-button" title={t("Redo")} aria-label={t("Redo")} disabled={!future.current.length} onClick={redo}>
          <Icon name="redo" size={16} />
        </button>
        <button className="icon-button" title={t("Delete")} aria-label={t("Delete")} disabled={!selection.size} onClick={removeSelection}>
          <Icon name="trash" size={16} />
        </button>
        <span className="mindmap-zoom">
          <button aria-label={t("Zoom Out")} onClick={() => setView((v) => ({ ...v, zoom: Math.max(0.1, v.zoom / 1.25) }))}>
            −
          </button>
          <span>{Math.round(view.zoom * 100)}%</span>
          <button aria-label={t("Zoom In")} onClick={() => setView((v) => ({ ...v, zoom: Math.min(8, v.zoom * 1.25) }))}>
            +
          </button>
        </span>
      </div>
      {error && <p className="hint error">{error}</p>}
      {notice && <p className="notice" role="status">{notice}</p>}
      <div
        ref={surface}
        className="drawing-surface"
        tabIndex={0}
        onKeyDown={onKeyDown}
        onKeyUp={(e) => {
          if (e.key === " ") space.current = false;
        }}
      >
        <canvas
          ref={canvas}
          style={{ width: size.width, height: size.height, cursor }}
          onPointerDown={onPointerDown}
          // Focus stays where the pointer put it (a new text field).
          onMouseDown={(e) => e.preventDefault()}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          onPointerCancel={onPointerUp}
        />
        {writing && (
          <textarea
            className="drawing-text"
            autoFocus
            value={writing.value}
            style={{
              left: view.x + writing.at.x * view.zoom,
              top: view.y + writing.at.y * view.zoom,
              fontSize: (items.find((i) => i.id === writing.id)?.fontSize ?? 20) * view.zoom,
              color: items.find((i) => i.id === writing.id)?.strokeColor ?? penColor,
            }}
            onChange={(e) => setWriting({ ...writing, value: e.target.value })}
            onBlur={commitWriting}
            onKeyDown={(e) => {
              if (e.key === "Escape") {
                e.preventDefault();
                commitWriting();
              }
            }}
          />
        )}
      </div>
    </div>
  );
}
