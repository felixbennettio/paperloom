import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { useI18n } from "../i18n";
import { MenuButton } from "./MenuButton";
import { backend, type DiagramChange, type DiagramDrawn, type DiagramPageView, type DiagramShape } from "../lib/api";
import {
  aligned,
  boundsOf,
  boxBetween,
  connectionPoints,
  freeSpot,
  handles,
  hitItem,
  resized,
  rotateKnob,
  shapeDragStart,
  snap,
  styleOf,
  within,
  type Box,
  type Guide,
  type Handle,
  type Point,
} from "../lib/diagram";
import { pictureSize } from "../lib/drawing";
import { pictureOf, readPicture } from "../lib/picture";
import { Icon } from "./Icon";

interface Props {
  path: string;
}

interface Camera {
  x: number;
  y: number;
  zoom: number;
}

/** The pointer's angle about a centre, in degrees, 0 straight up. */
function pointerAngle(centre: Point, p: Point): number {
  return (Math.atan2(p.y - centre.y, p.x - centre.x) * 180) / Math.PI + 90;
}

/** A rotation turned by the pointer, in whole degrees from 0 to 359;
 * with Shift, in steps of 15°. */
function turned(base: number, start: number, now: number, steps: boolean): number {
  const raw = base + now - start;
  const settled = steps ? Math.round(raw / 15) * 15 : Math.round(raw);
  return ((settled % 360) + 360) % 360;
}

/** What a pointer held down is doing. */
type Drag =
  | { kind: "move"; start: Point; dx: number; dy: number; guides: Guide[] }
  | { kind: "resize"; id: string; handle: Handle; start: Point; box: Box; now: Box }
  /** Turning a shape about its middle: its rotation before, and the pointer's angles. */
  | { kind: "rotate"; id: string; centre: Point; base: number; start: number; now: number }
  | { kind: "marquee"; start: Point; now: Point }
  | { kind: "connect"; source: string; from: Point; now: Point }
  | { kind: "end"; id: string; source: boolean; now: Point }
  | { kind: "bend"; id: string; index: number; points: Point[] }
  | { kind: "pan"; start: Point; origin: Camera };

const FILLS = ["", "#ffffff", "#dae8fc", "#d5e8d4", "#fff2cc", "#ffe6cc", "#f8cecc", "#e1d5e7", "#f5f5f5"];
const STROKES = ["#000000", "#6c8ebf", "#82b366", "#d6b656", "#d79b00", "#b85450", "#9673a6", ""];
const ARROWS = ["classic", "block", "open", "oval", "diamond", "none"];
const ARROW_NAMES: Record<string, string> = {
  classic: "Classic",
  block: "Block",
  open: "Open Arrowhead",
  oval: "Oval",
  diamond: "Diamond",
  none: "None",
};
const GROUPS = ["General", "Arrows", "Flowchart", "UML", "Entity Relation", "BPMN"];
const DRAG_TYPE = "application/x-paperloom-shape";

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** A shape's drawing fitted into a small square, for the palette. */
function ShapeIcon({ shape }: { shape: DiagramShape }) {
  const pad = 2;
  return (
    <svg viewBox={`${-pad} ${-pad} ${shape.width + 2 * pad} ${shape.height + 2 * pad}`} width={30} height={30} aria-hidden="true">
      {shape.path ? (
        <path d={shape.path} fill="var(--surface-strong)" stroke="currentColor" strokeWidth={Math.max(shape.width, shape.height) / 30} />
      ) : (
        <text x={shape.width / 2} y={shape.height / 2 + 6} textAnchor="middle" fontSize={18} fill="currentColor">
          T
        </text>
      )}
    </svg>
  );
}

/** One shape or edge as the core laid it out. */
function Drawn({ item, offset }: { item: DiagramDrawn; offset: Point | null }) {
  const turn = item.rotation ? `rotate(${item.rotation} ${item.x + item.width / 2} ${item.y + item.height / 2})` : "";
  const shift = offset ? `translate(${offset.x} ${offset.y}) ` : "";
  const stroke = item.stroke || "none";
  const dash = item.dashed ? "6 4" : undefined;
  return (
    <g className={item.edge ? undefined : "diagram-shape"} transform={shift + turn || undefined} opacity={item.opacity < 1 ? item.opacity : undefined}>
      {item.image && (
        <image
          href={item.image}
          x={item.x}
          y={item.y}
          width={item.width}
          height={item.height}
          preserveAspectRatio={item.imageAspect ? "xMidYMid meet" : "none"}
        />
      )}
      {item.path && (
        <path
          d={item.path}
          fill={item.edge ? "none" : item.fill || "none"}
          stroke={stroke}
          strokeWidth={item.strokeWidth}
          strokeDasharray={dash}
          strokeLinejoin="round"
        />
      )}
      {item.inner && <path d={item.inner} fill="none" stroke={stroke} strokeWidth={item.strokeWidth} />}
      {item.arrows && <path d={item.arrows} fill={stroke} stroke={stroke} strokeWidth={item.strokeWidth} />}
      {item.openArrows && <path d={item.openArrows} fill="none" stroke={stroke} strokeWidth={item.strokeWidth} />}
      {item.text && (
        <foreignObject x={item.textX} y={item.textY} width={Math.max(item.textWidth, 1)} height={Math.max(item.textHeight, 1)}>
          <div
            className="diagram-label"
            style={{
              justifyContent: item.align === "left" ? "flex-start" : item.align === "right" ? "flex-end" : "center",
              alignItems: item.verticalAlign === "top" ? "flex-start" : item.verticalAlign === "bottom" ? "flex-end" : "center",
              textAlign: item.align === "left" || item.align === "right" ? item.align : "center",
              whiteSpace: item.wrap ? "pre-wrap" : "pre",
              fontSize: item.fontSize,
              color: item.fontColor || "#000000",
              fontWeight: item.bold ? 700 : 400,
              fontStyle: item.italic ? "italic" : "normal",
              textDecoration: item.underline ? "underline" : "none",
            }}
          >
            <span style={{ background: item.labelBackground || undefined }}>{item.text}</span>
          </div>
        </foreignObject>
      )}
    </g>
  );
}

/** A diagram drawn and edited here, kept in the vault as a `.drawio.svg`
 * (a picture every app shows, the diagram inside) or a `.drawio` file in
 * draw.io's format. Shapes come from the palette (dragged or clicked);
 * the blue points on a shape start an edge; double-click writes a label;
 * Delete, ⌘D (duplicate), ⌘G (group), ⇧⌘G (ungroup), ⌘Z, the arrow keys
 * (nudge), Space-drag or the wheel (pan) and ⌘-wheel (zoom) work as
 * usual. */
export function DiagramView({ path }: Props) {
  const { t } = useI18n();
  const [xml, setXml] = useState<string | null>(null);
  const [page, setPage] = useState(0);
  const [view, setView] = useState<DiagramPageView | null>(null);
  const [shapes, setShapes] = useState<DiagramShape[]>([]);
  const [selection, setSelection] = useState<string[]>([]);
  const [camera, setCamera] = useState<Camera>({ x: 40, y: 40, zoom: 1 });
  const [drag, setDrag] = useState<Drag | null>(null);
  const [hover, setHover] = useState<string | null>(null);
  const [editing, setEditing] = useState<{ id: string; text: string } | null>(null);
  const [snapping, setSnapping] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const svg = useRef<SVGSVGElement>(null);
  const root = useRef<HTMLDivElement>(null);
  const past = useRef<string[]>([]);
  const future = useRef<string[]>([]);
  const timer = useRef<number | null>(null);
  const pictureInput = useRef<HTMLInputElement>(null);
  const latest = useRef<string | null>(null);
  const space = useRef(false);
  const fitted = useRef(false);
  const name = path.replace(/^.*\//, "").replace(/\.drawio(\.svg)?$/i, "");

  const save = useCallback(() => {
    timer.current = null;
    if (latest.current !== null) backend.vaultWriteDiagram(path, latest.current, null).catch((e: unknown) => setError(message(e)));
  }, [path]);

  useEffect(() => {
    let current = true;
    fitted.current = false;
    backend.vaultReadDiagram(path).then(
      (loaded) => {
        if (!current) return;
        latest.current = loaded;
        past.current = [];
        future.current = [];
        setXml(loaded);
        setPage(0);
      },
      (e: unknown) => current && setError(message(e)),
    );
    backend.diagramShapes().then(setShapes, () => setShapes([]));
    return () => {
      current = false;
      if (timer.current !== null) {
        window.clearTimeout(timer.current);
        save();
      }
    };
  }, [path, save]);

  useEffect(() => {
    if (xml === null) return;
    let current = true;
    backend.diagramView(xml, page).then(
      (laid) => {
        if (!current) return;
        setView(laid);
        if (laid.page !== page) setPage(laid.page);
        const box = svg.current?.getBoundingClientRect();
        if (!fitted.current && box && box.width > 0) {
          fitted.current = true;
          if (laid.items.length) {
            const zoom = Math.min(1, Math.max(0.2, Math.min(box.width / (laid.width + 80), box.height / (laid.height + 80))));
            setCamera({ zoom, x: box.width / 2 - (laid.x + laid.width / 2) * zoom, y: box.height / 2 - (laid.y + laid.height / 2) * zoom });
          }
        }
      },
      (e: unknown) => current && setError(message(e)),
    );
    return () => {
      current = false;
    };
  }, [xml, page]);

  // The wheel pans, and zooms with Ctrl or ⌘ (pinch on a trackpad).
  useEffect(() => {
    const element = svg.current;
    if (!element) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const box = element.getBoundingClientRect();
      setCamera((c) => {
        if (!e.ctrlKey && !e.metaKey) return { ...c, x: c.x - e.deltaX, y: c.y - e.deltaY };
        const zoom = Math.min(4, Math.max(0.1, c.zoom * Math.exp(-e.deltaY * 0.0015)));
        const [px, py] = [e.clientX - box.left, e.clientY - box.top];
        return { zoom, x: px - ((px - c.x) / c.zoom) * zoom, y: py - ((py - c.y) / c.zoom) * zoom };
      });
    };
    element.addEventListener("wheel", onWheel, { passive: false });
    return () => element.removeEventListener("wheel", onWheel);
  }, [xml !== null]);

  const items = useMemo(() => view?.items ?? [], [view]);
  const byId = useMemo(() => new Map(items.map((item) => [item.id, item])), [items]);
  const grid = snapping ? view?.grid || 10 : 0;

  /** Keeps a new version: shown now, kept for undo, saved shortly after. */
  const keep = (next: string) => {
    if (latest.current !== null) past.current = [...past.current.slice(-99), latest.current];
    future.current = [];
    latest.current = next;
    setXml(next);
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(save, 600);
  };

  /** Makes changes one after another, kept as one step. */
  const apply = async (...changes: DiagramChange[]) => {
    if (latest.current === null) return;
    try {
      let text = latest.current;
      let shown = page;
      let picked = selection;
      for (const change of changes) {
        const result = await backend.diagramChange(text, shown, change);
        [text, shown, picked] = [result.xml, result.page, result.selection.length ? result.selection : picked];
      }
      keep(text);
      setPage(shown);
      setSelection(picked);
      setError(null);
    } catch (e) {
      setError(message(e));
    }
  };

  const undo = () => {
    const previous = past.current.pop();
    if (previous === undefined || latest.current === null) return;
    future.current.push(latest.current);
    latest.current = previous;
    setXml(previous);
    setSelection([]);
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(save, 600);
  };

  const redo = () => {
    const next = future.current.pop();
    if (next === undefined || latest.current === null) return;
    past.current.push(latest.current);
    latest.current = next;
    setXml(next);
    setSelection([]);
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(save, 600);
  };

  const toWorld = (clientX: number, clientY: number): Point => {
    const box = svg.current?.getBoundingClientRect();
    return { x: (clientX - (box?.left ?? 0) - camera.x) / camera.zoom, y: (clientY - (box?.top ?? 0) - camera.y) / camera.zoom };
  };

  /** The middle of what is in view, on the page. */
  const middle = (): Point => {
    const box = svg.current?.getBoundingClientRect();
    return toWorld((box?.left ?? 0) + (box?.width ?? 0) / 2, (box?.top ?? 0) + (box?.height ?? 0) / 2);
  };

  /** A free centre near the middle of the view. */
  const freeMiddle = (width: number, height: number): Point => {
    const at = middle();
    const p = freeSpot(items.filter((item) => !item.edge), { x: at.x - width / 2, y: at.y - height / 2, width, height });
    return { x: p.x + width / 2, y: p.y + height / 2 };
  };

  /** A picture from a file in the middle of the view, no bigger than fits. */
  const insertPicture = async (file: File) => {
    try {
      const { url, width, height } = await readPicture(file);
      const size = pictureSize(width, height, { width: 320, height: 240 });
      const at = freeMiddle(size.width, size.height);
      void apply({
        kind: "addImage",
        image: url,
        x: snap(at.x - size.width / 2, grid),
        y: snap(at.y - size.height / 2, grid),
        width: size.width,
        height: size.height,
      });
    } catch (e) {
      setError(message(e));
    }
  };

  const addShape = (shape: DiagramShape, at: Point) =>
    void apply({ kind: "addShape", shape: shape.id, x: snap(at.x - shape.width / 2, grid), y: snap(at.y - shape.height / 2, grid) });

  const selected = selection.map((id) => byId.get(id)).filter((item): item is DiagramDrawn => item !== undefined);
  const selectedShapes = selected.filter((item) => !item.edge);
  const single = selected.length === 1 ? selected[0] : null;
  const hovered = hover ? byId.get(hover) : undefined;
  const portOwner = hovered && !hovered.edge ? hovered : single && !single.edge ? single : null;
  const reach = 7 / camera.zoom;

  /** The shapes moving with the selection: it and what it holds. */
  const moving = useMemo(() => {
    const ids = new Set(selection);
    let grew = true;
    while (grew) {
      grew = false;
      for (const item of items) {
        if (!ids.has(item.id) && item.parent && ids.has(item.parent)) {
          ids.add(item.id);
          grew = true;
        }
      }
    }
    return ids;
  }, [selection, items]);

  const onPointerDown = (e: ReactPointerEvent<SVGSVGElement>) => {
    root.current?.focus();
    if (editing) commitText();
    const p = toWorld(e.clientX, e.clientY);
    if (e.button === 1 || (e.button === 0 && space.current)) {
      e.currentTarget.setPointerCapture(e.pointerId);
      setDrag({ kind: "pan", start: { x: e.clientX, y: e.clientY }, origin: camera });
      return;
    }
    if (e.button !== 0 || !view) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    // A visible connection port wins over a resize handle at the same point.
    const port = e.target instanceof SVGElement ? e.target.closest<SVGElement>(".diagram-connect") : null;
    const source = port?.dataset.source ? byId.get(port.dataset.source) : undefined;
    const from = source && connectionPoints(source)[Number(port?.dataset.index)];
    if (source && from) {
      setDrag({ kind: "connect", source: source.id, from, now: p });
      return;
    }
    const control = shapeDragStart(p, portOwner, single && !single.edge ? single : null, camera.zoom);
    if (control?.kind === "connect" && portOwner) {
      setDrag({ kind: "connect", source: portOwner.id, from: control.from, now: p });
      return;
    }
    if (single && !single.edge) {
      if (control?.kind === "rotate") {
        const centre = { x: single.x + single.width / 2, y: single.y + single.height / 2 };
        const angle = pointerAngle(centre, p);
        setDrag({ kind: "rotate", id: single.id, centre, base: single.rotation, start: angle, now: angle });
        return;
      }
      if (control?.kind === "resize") {
        const box = { x: single.x, y: single.y, width: single.width, height: single.height };
        setDrag({ kind: "resize", id: single.id, handle: control.handle, start: p, box, now: box });
        return;
      }
    }
    if (single?.edge && single.points.length >= 2) {
      const ends = [single.points[0], single.points[single.points.length - 1]];
      const end = ends.findIndex((q) => Math.hypot(q.x - p.x, q.y - p.y) <= reach);
      if (end >= 0) {
        setDrag({ kind: "end", id: single.id, source: end === 0, now: p });
        return;
      }
      const bend = single.bends.findIndex((q) => Math.hypot(q.x - p.x, q.y - p.y) <= reach);
      if (bend >= 0) {
        setDrag({ kind: "bend", id: single.id, index: bend, points: single.bends.map((q) => ({ ...q })) });
        return;
      }
    }
    const hit = hitItem(items, p, 6 / camera.zoom);
    if (!hit) {
      if (!e.shiftKey) setSelection([]);
      setDrag({ kind: "marquee", start: p, now: p });
      return;
    }
    if (e.shiftKey) {
      setSelection((s) => (s.includes(hit.id) ? s.filter((id) => id !== hit.id) : [...s, hit.id]));
      return;
    }
    if (!selection.includes(hit.id)) setSelection([hit.id]);
    if (!hit.edge || selection.includes(hit.id)) setDrag({ kind: "move", start: p, dx: 0, dy: 0, guides: [] });
  };

  const onPointerMove = (e: ReactPointerEvent<SVGSVGElement>) => {
    const p = toWorld(e.clientX, e.clientY);
    if (!drag) {
      const hit = hitItem(items, p, 6 / camera.zoom);
      const near = hover ? byId.get(hover) : undefined;
      // Near a shape's connection points, it stays the one hovered.
      const keepNear = near && !near.edge && connectionPoints(near).some((q) => Math.hypot(q.x - p.x, q.y - p.y) <= reach * 1.5);
      if (!keepNear) setHover(hit && !hit.edge ? hit.id : null);
      return;
    }
    switch (drag.kind) {
      case "pan":
        setCamera({ ...drag.origin, x: drag.origin.x + e.clientX - drag.start.x, y: drag.origin.y + e.clientY - drag.start.y });
        return;
      case "move": {
        const boxes = items.filter((i) => moving.has(i.id) && !i.edge);
        const area = boundsOf(boxes);
        let [dx, dy] = [p.x - drag.start.x, p.y - drag.start.y];
        let guides: Guide[] = [];
        if (area) {
          const others = items.filter((i) => !i.edge && !moving.has(i.id));
          const snapped = aligned({ ...area, x: area.x + dx, y: area.y + dy }, others, 6 / camera.zoom);
          guides = snapped.guides;
          dx = snapped.dx ? dx + snapped.dx : grid ? snap(area.x + dx, grid) - area.x : dx;
          dy = snapped.dy ? dy + snapped.dy : grid ? snap(area.y + dy, grid) - area.y : dy;
        }
        setDrag({ ...drag, dx, dy, guides });
        return;
      }
      case "resize": {
        const raw = resized(drag.box, drag.handle, p.x - drag.start.x, p.y - drag.start.y, e.shiftKey);
        const now = grid
          ? { x: snap(raw.x, grid), y: snap(raw.y, grid), width: Math.max(grid, snap(raw.width, grid)), height: Math.max(grid, snap(raw.height, grid)) }
          : raw;
        setDrag({ ...drag, now });
        return;
      }
      case "bend": {
        const points = drag.points.map((q, i) => (i === drag.index ? { x: snap(p.x, grid), y: snap(p.y, grid) } : q));
        setDrag({ ...drag, points });
        return;
      }
      case "rotate":
        setDrag({ ...drag, now: pointerAngle(drag.centre, p) });
        return;
      default:
        setDrag({ ...drag, now: p });
    }
  };

  const onPointerUp = (e: ReactPointerEvent<SVGSVGElement>) => {
    const done = drag;
    setDrag(null);
    if (!done) return;
    const p = toWorld(e.clientX, e.clientY);
    const target = (exclude: string) => {
      const hit = hitItem(items, p, 6 / camera.zoom);
      return hit && !hit.edge && hit.id !== exclude ? hit.id : "";
    };
    switch (done.kind) {
      case "move":
        if (done.dx || done.dy) void apply({ kind: "move", ids: selection, dx: done.dx, dy: done.dy });
        break;
      case "resize": {
        const { x, y, width, height } = done.now;
        const same = x === done.box.x && y === done.box.y && width === done.box.width && height === done.box.height;
        if (!same) void apply({ kind: "resize", id: done.id, x, y, width, height });
        break;
      }
      case "rotate": {
        const angle = turned(done.base, done.start, done.now, e.shiftKey);
        if (angle !== done.base) void apply({ kind: "setStyle", ids: [done.id], key: "rotation", value: angle ? String(angle) : "" });
        break;
      }
      case "marquee": {
        const area = boxBetween(done.start, done.now);
        if (area.width < 2 && area.height < 2) break;
        const inside = items.filter((i) => (i.edge ? i.points.every((q) => within({ ...q, width: 0, height: 0 }, area)) : within(i, area)));
        setSelection((s) => [...new Set([...(e.shiftKey ? s : []), ...inside.map((i) => i.id)])]);
        break;
      }
      case "connect":
        if (Math.hypot(p.x - done.from.x, p.y - done.from.y) > reach) {
          void apply({ kind: "connect", source: done.source, target: target(done.source), from: done.from, to: p });
        }
        break;
      case "end": {
        const edge = byId.get(done.id);
        const other = done.source ? edge?.target : edge?.source;
        void apply({ kind: "reconnect", id: done.id, source: done.source, cell: target(other ?? ""), at: p });
        break;
      }
      case "bend":
        void apply({ kind: "setBends", id: done.id, points: done.points });
        break;
      case "pan":
        break;
    }
  };

  const onDoubleClick = (e: ReactMouseEvent<SVGSVGElement>) => {
    const hit = hitItem(items, toWorld(e.clientX, e.clientY), 6 / camera.zoom);
    if (hit) {
      setSelection([hit.id]);
      setEditing({ id: hit.id, text: hit.text });
    }
  };

  const commitText = () => {
    if (!editing) return;
    const was = byId.get(editing.id)?.text ?? "";
    const { id, text } = editing;
    setEditing(null);
    if (text !== was) void apply({ kind: "setText", id, text });
  };

  const onKeyDown = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    if (editing || e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement || e.target instanceof HTMLSelectElement) return;
    const command = e.metaKey || e.ctrlKey;
    const key = e.key.toLowerCase();
    if (command && key === "z") {
      e.preventDefault();
      if (e.shiftKey) redo();
      else undo();
    } else if (command && key === "y") {
      e.preventDefault();
      redo();
    } else if (command && key === "a") {
      e.preventDefault();
      setSelection(items.filter((i) => !i.parent).map((i) => i.id));
    } else if (command && key === "d" && selection.length) {
      e.preventDefault();
      void apply({ kind: "duplicate", ids: selection, dx: 20, dy: 20 });
    } else if (command && key === "g" && !e.shiftKey && selectedShapes.length > 1) {
      e.preventDefault();
      void apply({ kind: "group", ids: selectedShapes.map((i) => i.id) });
    } else if (command && key === "g" && e.shiftKey && single) {
      e.preventDefault();
      void apply({ kind: "ungroup", id: single.id });
    } else if ((e.key === "Delete" || e.key === "Backspace") && selection.length) {
      e.preventDefault();
      void apply({ kind: "delete", ids: selection });
    } else if (e.key === "Escape") {
      setSelection([]);
    } else if (e.key === " ") {
      e.preventDefault();
      space.current = true;
    } else if (e.key.startsWith("Arrow") && selection.length) {
      e.preventDefault();
      const step = e.shiftKey ? 10 : 1;
      const dx = e.key === "ArrowLeft" ? -step : e.key === "ArrowRight" ? step : 0;
      const dy = e.key === "ArrowUp" ? -step : e.key === "ArrowDown" ? step : 0;
      void apply({ kind: "move", ids: selection, dx, dy });
    } else if (key === "f2" && single) {
      setEditing({ id: single.id, text: single.text });
    }
  };

  const exportAs = async (format: string) => {
    if (latest.current === null) return;
    setNotice(null);
    setError(null);
    try {
      const png =
        format === "png" || format === "pdf"
          ? await pictureOf(await backend.diagramPageSvg(latest.current, page), format === "pdf" ? "image/jpeg" : "image/png")
          : null;
      const written = await backend.diagramExport(name, latest.current, page, format, png);
      if (written) setNotice(t("Saved {path}.", { path: written }));
    } catch (e) {
      setError(message(e));
    }
  };

  const zoomTo = (zoom: number) => {
    const box = svg.current?.getBoundingClientRect();
    const [px, py] = [(box?.width ?? 0) / 2, (box?.height ?? 0) / 2];
    setCamera((c) => ({ zoom, x: px - ((px - c.x) / c.zoom) * zoom, y: py - ((py - c.y) / c.zoom) * zoom }));
  };

  const fit = () => {
    const box = svg.current?.getBoundingClientRect();
    if (!view || !box || !view.items.length) return;
    const zoom = Math.min(2, Math.max(0.1, Math.min(box.width / (view.width + 80), box.height / (view.height + 80))));
    setCamera({ zoom, x: box.width / 2 - (view.x + view.width / 2) * zoom, y: box.height / 2 - (view.y + view.height / 2) * zoom });
  };

  // What the selection's style says, for the controls.
  const style = single ? styleOf(single.style) : null;
  const ids = selection;
  const setStyle = (key: string, value: string) => void apply({ kind: "setStyle", ids, key, value });
  const fontStyle = Number(style?.get("fontStyle") ?? 0) || 0;
  const toggleFont = (bit: number) => setStyle("fontStyle", String(fontStyle ^ bit));
  const edges = selected.filter((i) => i.edge);
  const line = style?.get("curved") === "1" ? "curved" : style?.get("edgeStyle") ? "orthogonal" : "straight";
  const setLine = (kind: string) =>
    void apply(
      { kind: "setStyle", ids, key: "edgeStyle", value: kind === "orthogonal" ? "orthogonalEdgeStyle" : "" },
      { kind: "setStyle", ids, key: "curved", value: kind === "curved" ? "1" : "" },
    );

  const pages = view?.pages ?? [];
  const selectionBox = boundsOf(selectedShapes);
  // Where a rubber band starts: the shape's point, or the edge's fixed end.
  const rubberFrom = (() => {
    if (drag?.kind === "connect") return drag.from;
    if (drag?.kind !== "end") return null;
    const points = byId.get(drag.id)?.points ?? [];
    return points[drag.source ? points.length - 1 : 0] ?? drag.now;
  })();

  return (
    <div
      ref={root}
      className="diagram-view"
      tabIndex={0}
      onKeyDown={onKeyDown}
      onKeyUp={(e) => {
        if (e.key === " ") space.current = false;
      }}
    >
      <div className="diagram-toolbar" role="toolbar" aria-label={t("Diagram")}>
        <MenuButton label={t("More")} icon={<span aria-hidden="true">⋯</span>} iconOnly items={[
          { label: t("Duplicate"), disabled: !ids.length, onSelect: () => void apply({ kind: "duplicate", ids, dx: 20, dy: 20 }) },
          { label: t("To Front"), disabled: !ids.length, onSelect: () => void apply({ kind: "toFront", ids }) },
          { label: t("To Back"), disabled: !ids.length, onSelect: () => void apply({ kind: "toBack", ids }) },
          { label: t("Group"), disabled: selectedShapes.length < 2, onSelect: () => void apply({ kind: "group", ids: selectedShapes.map((item) => item.id) }) },
          { label: t("Ungroup"), disabled: !single?.container, onSelect: () => single && void apply({ kind: "ungroup", id: single.id }) },
        ]} />
        <select
          value=""
          aria-label={t("Align")}
          disabled={selectedShapes.length < 2}
          onChange={(e) => {
            const how = e.target.value;
            e.target.value = "";
            if (how === "horizontal" || how === "vertical") void apply({ kind: "distribute", ids, horizontal: how === "horizontal" });
            else if (how) void apply({ kind: "align", ids, how });
          }}
        >
          <option value="" disabled>
            {t("Align")}
          </option>
          <option value="left">{t("Left")}</option>
          <option value="center">{t("Center")}</option>
          <option value="right">{t("Right")}</option>
          <option value="top">{t("Top")}</option>
          <option value="middle">{t("Middle")}</option>
          <option value="bottom">{t("Bottom")}</option>
          <option value="horizontal">{t("Distribute Horizontally")}</option>
          <option value="vertical">{t("Distribute Vertically")}</option>
        </select>
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
        <span className="drawing-separator" />
        <label className="drawing-check" title={t("Snap to Grid")}>
          <input type="checkbox" checked={snapping} onChange={(e) => setSnapping(e.target.checked)} />
          {t("Grid")}
        </label>
        <MenuButton label={t("Export…")} items={[
          { label: t("Picture (SVG)"), onSelect: () => void exportAs("svg") },
          { label: t("Picture (PNG)"), onSelect: () => void exportAs("png") },
          { label: "PDF", onSelect: () => void exportAs("pdf") },
          { label: "draw.io", onSelect: () => void exportAs("drawio") },
        ]} />
        <span className="drawing-spacer" />
        <button className="icon-button" title={t("Undo")} aria-label={t("Undo")} disabled={!past.current.length} onClick={undo}>
          <Icon name="undo" size={16} />
        </button>
        <button className="icon-button" title={t("Redo")} aria-label={t("Redo")} disabled={!future.current.length} onClick={redo}>
          <Icon name="redo" size={16} />
        </button>
        <button className="icon-button" title={t("Delete")} aria-label={t("Delete")} disabled={!ids.length} onClick={() => void apply({ kind: "delete", ids })}>
          <Icon name="trash" size={16} />
        </button>
        <span className="mindmap-zoom">
          <button aria-label={t("Zoom Out")} onClick={() => zoomTo(Math.max(0.1, camera.zoom / 1.25))}>
            −
          </button>
          <button className="link" onClick={fit}>
            {Math.round(camera.zoom * 100)}%
          </button>
          <button aria-label={t("Zoom In")} onClick={() => zoomTo(Math.min(4, camera.zoom * 1.25))}>
            +
          </button>
        </span>
      </div>
      {error && <p className="hint error">{error}</p>}
      {notice && <p className="notice" role="status">{notice}</p>}
      <div className="diagram-body">
        <aside className="diagram-palette" aria-label={t("Shapes")}>
          {GROUPS.map((group) => (
            <details key={group} open={!["Entity Relation", "BPMN"].includes(group)}>
              <summary>{t(group)}</summary>
              <div className="diagram-shapes">
                {shapes
                  .filter((s) => s.group === group)
                  .map((shape) => (
                    <button
                      key={shape.id}
                      className="diagram-shape"
                      title={t(shape.name)}
                      aria-label={t(shape.name)}
                      draggable
                      onDragStart={(e) => e.dataTransfer.setData(DRAG_TYPE, shape.id)}
                      onClick={() => addShape(shape, freeMiddle(shape.width, shape.height))}
                    >
                      <ShapeIcon shape={shape} />
                    </button>
                  ))}
              </div>
            </details>
          ))}
        </aside>
        <div className="diagram-stage">
          {/* Over the canvas, so choosing a shape moves nothing under the pointer. */}
          {ids.length > 0 && (
            <div className="diagram-format" role="toolbar" aria-label={t("Format")}>
              {selectedShapes.length > 0 && <MenuButton label={t("Fill")} icon={<span className="menu-swatch" style={{ background: style?.get("fillColor") ?? "#ffffff" }} />} iconOnly items={FILLS.map((color) => ({
                label: color || t("None"), icon: <span className="menu-swatch" style={{ background: color || "transparent" }} />, checked: (style?.get("fillColor") ?? "#ffffff") === (color || "none"), onSelect: () => setStyle("fillColor", color || "none"),
              }))} />}
              <MenuButton label={t("Line Color")} icon={<Icon name="line" size={16} />} iconOnly items={STROKES.map((color) => ({
                label: color || t("None"), icon: <span className="menu-swatch" style={{ background: color || "transparent" }} />, checked: style?.get("strokeColor") === (color || "none"), onSelect: () => setStyle("strokeColor", color || "none"),
              }))} />
              <select value={style?.get("strokeWidth") ?? "1"} aria-label={t("Line Width")} onChange={(e) => setStyle("strokeWidth", e.target.value)}>
                {["1", "2", "3", "4", "6"].map((w) => (
                  <option key={w} value={w}>
                    {w} pt
                  </option>
                ))}
              </select>
              <label className="drawing-check">
                <input type="checkbox" checked={style?.get("dashed") === "1"} onChange={(e) => setStyle("dashed", e.target.checked ? "1" : "")} />
                {t("Dashed")}
              </label>
              <span className="drawing-separator" />
              <select value={style?.get("fontSize") ?? "12"} aria-label={t("Font Size")} onChange={(e) => setStyle("fontSize", e.target.value)}>
                {["9", "10", "11", "12", "14", "16", "18", "20", "24", "28", "36"].map((s) => (
                  <option key={s} value={s}>
                    {s}
                  </option>
                ))}
              </select>
              <button className={fontStyle & 1 ? "selected" : undefined} aria-label={t("Bold")} onClick={() => toggleFont(1)}>
                <b>B</b>
              </button>
              <button className={fontStyle & 2 ? "selected" : undefined} aria-label={t("Italic")} onClick={() => toggleFont(2)}>
                <i>I</i>
              </button>
              <select value={style?.get("align") ?? "center"} aria-label={t("Text Alignment")} onChange={(e) => setStyle("align", e.target.value)}>
                <option value="left">{t("Left")}</option>
                <option value="center">{t("Center")}</option>
                <option value="right">{t("Right")}</option>
              </select>
              {edges.length > 0 && (
                <>
                  <span className="drawing-separator" />
                  <MenuButton label={t("Connector")} icon={<Icon name="diagram" size={16} />} iconOnly items={([
                    ["straight", "Straight"], ["orthogonal", "Orthogonal"], ["curved", "Curved"],
                  ] as const).map(([value, label]) => ({ label: t(label), checked: line === value, onSelect: () => setLine(value) }))} />
                  <MenuButton label={t("Start Arrow")} icon={<Icon name="back" size={16} />} iconOnly items={ARROWS.map((arrow) => ({ label: t(ARROW_NAMES[arrow]), checked: (style?.get("startArrow") ?? "none") === arrow, onSelect: () => setStyle("startArrow", arrow) }))} />
                  <MenuButton label={t("End Arrow")} icon={<Icon name="forward" size={16} />} iconOnly items={ARROWS.map((arrow) => ({ label: t(ARROW_NAMES[arrow]), checked: (style?.get("endArrow") ?? "classic") === arrow, onSelect: () => setStyle("endArrow", arrow) }))} />
                </>
              )}
            </div>
          )}
          <svg
            ref={svg}
            className={["diagram-canvas", drag ? "dragging" : "", drag?.kind === "pan" ? "panning" : ""].join(" ")}
            style={{ cursor: drag?.kind === "move" ? "move" : drag?.kind === "connect" ? "crosshair"
              : drag?.kind === "resize" ? (drag.handle.length === 1 ? ("ns".includes(drag.handle) ? "ns-resize" : "ew-resize")
                : drag.handle === "nw" || drag.handle === "se" ? "nwse-resize" : "nesw-resize") : undefined }}
            onPointerDown={onPointerDown}
            onPointerMove={onPointerMove}
            onPointerUp={onPointerUp}
            onPointerCancel={() => setDrag(null)}
            onPointerLeave={() => !drag && setHover(null)}
            onDoubleClick={onDoubleClick}
            onDragOver={(e) => {
              if (e.dataTransfer.types.includes(DRAG_TYPE)) e.preventDefault();
            }}
            onDrop={(e) => {
              const shape = shapes.find((s) => s.id === e.dataTransfer.getData(DRAG_TYPE));
              if (!shape) return;
              e.preventDefault();
              addShape(shape, toWorld(e.clientX, e.clientY));
            }}
          >
            <defs>
              <pattern id="diagram-grid" width={grid || 10} height={grid || 10} patternUnits="userSpaceOnUse">
                <path d={`M${grid || 10} 0H0V${grid || 10}`} fill="none" className="diagram-grid-line" />
              </pattern>
            </defs>
            <g transform={`translate(${camera.x} ${camera.y}) scale(${camera.zoom})`}>
              {grid > 0 && <rect x={-20000} y={-20000} width={40000} height={40000} fill="url(#diagram-grid)" />}
              {view?.background && <rect x={view.x - 40} y={view.y - 40} width={view.width + 80} height={view.height + 80} fill={view.background} />}
              {items.map((item) => (
                <Drawn
                  key={item.id}
                  item={item}
                  offset={drag?.kind === "move" && moving.has(item.id) ? { x: drag.dx, y: drag.dy } : null}
                />
              ))}
              {selected.map((item) =>
                item.edge ? (
                  <g key={`s${item.id}`} className="diagram-selected">
                    <path d={item.path} fill="none" strokeWidth={item.strokeWidth + 6 / camera.zoom} className="diagram-halo" />
                    {[item.points[0], item.points[item.points.length - 1]].filter(Boolean).map((q, i) => (
                      <circle key={i} cx={q.x} cy={q.y} r={5 / camera.zoom} className="diagram-end" />
                    ))}
                    {(drag?.kind === "bend" && drag.id === item.id ? drag.points : item.bends).map((q, i) => (
                      <rect key={`b${i}`} x={q.x - 4 / camera.zoom} y={q.y - 4 / camera.zoom} width={8 / camera.zoom} height={8 / camera.zoom} className="diagram-bend" />
                    ))}
                  </g>
                ) : null,
              )}
              {selectionBox && drag?.kind !== "resize" && (
                <rect
                  x={selectionBox.x + (drag?.kind === "move" ? drag.dx : 0)}
                  y={selectionBox.y + (drag?.kind === "move" ? drag.dy : 0)}
                  width={selectionBox.width}
                  height={selectionBox.height}
                  className="diagram-selection"
                  strokeWidth={1 / camera.zoom}
                />
              )}
              {single && !single.edge && !drag && (
                <g className="diagram-rotate" aria-label={t("Rotate")}>
                  <line
                    x1={single.x + single.width / 2}
                    y1={single.y}
                    x2={rotateKnob(single, camera.zoom).x}
                    y2={rotateKnob(single, camera.zoom).y}
                    strokeWidth={1 / camera.zoom}
                  />
                  <circle
                    cx={rotateKnob(single, camera.zoom).x}
                    cy={rotateKnob(single, camera.zoom).y}
                    r={5 / camera.zoom}
                    strokeWidth={1 / camera.zoom}
                  >
                    <title>{t("Rotate")}</title>
                  </circle>
                </g>
              )}
              {drag?.kind === "rotate" && single && (
                <rect
                  x={single.x}
                  y={single.y}
                  width={single.width}
                  height={single.height}
                  transform={`rotate(${turned(drag.base, drag.start, drag.now, false)} ${drag.centre.x} ${drag.centre.y})`}
                  className="diagram-selection"
                  strokeWidth={1 / camera.zoom}
                />
              )}
              {single && !single.edge && !drag &&
                handles(single).map((h) => (
                  <rect
                    key={h.handle}
                    x={h.x - 4 / camera.zoom}
                    y={h.y - 4 / camera.zoom}
                    width={8 / camera.zoom}
                    height={8 / camera.zoom}
                    className={`diagram-handle ${h.handle}`}
                    strokeWidth={1 / camera.zoom}
                  />
                ))}
              {/* Keep the pressed port mounted while its pointer is captured. */}
              {portOwner && (!drag || drag.kind === "connect") &&
                connectionPoints(portOwner).map((q, i) => (
                  <circle key={`c${i}`} cx={q.x} cy={q.y} r={4.5 / camera.zoom} className="diagram-connect" data-source={portOwner.id} data-index={i} strokeWidth={1 / camera.zoom} />
                ))}
              {drag?.kind === "resize" && (
                <rect {...drag.now} className="diagram-selection" strokeWidth={1 / camera.zoom} />
              )}
              {drag?.kind === "marquee" && <rect {...boxBetween(drag.start, drag.now)} className="diagram-marquee" strokeWidth={1 / camera.zoom} />}
              {rubberFrom && (drag?.kind === "connect" || drag?.kind === "end") && (
                <line
                  x1={rubberFrom.x}
                  y1={rubberFrom.y}
                  x2={drag.now.x}
                  y2={drag.now.y}
                  className="diagram-rubber"
                  strokeWidth={1.5 / camera.zoom}
                />
              )}
              {drag?.kind === "move" &&
                drag.guides.map((g, i) =>
                  g.vertical ? (
                    <line key={i} x1={g.at} x2={g.at} y1={-20000} y2={20000} className="diagram-guide" strokeWidth={1 / camera.zoom} />
                  ) : (
                    <line key={i} y1={g.at} y2={g.at} x1={-20000} x2={20000} className="diagram-guide" strokeWidth={1 / camera.zoom} />
                  ),
                )}
            </g>
          </svg>
          {editing && byId.get(editing.id) && (
            <textarea
              className="diagram-text"
              autoFocus
              value={editing.text}
              style={(() => {
                const item = byId.get(editing.id)!;
                const box = item.text || !item.edge ? { x: item.textX, y: item.textY, width: item.textWidth, height: item.textHeight } : item;
                const width = Math.max(box.width, 80);
                return {
                  left: camera.x + box.x * camera.zoom,
                  top: camera.y + box.y * camera.zoom,
                  width: width * camera.zoom,
                  minHeight: Math.max(box.height, item.fontSize * 1.6) * camera.zoom,
                  fontSize: item.fontSize * camera.zoom,
                };
              })()}
              onChange={(e) => setEditing({ ...editing, text: e.target.value })}
              onBlur={commitText}
              onKeyDown={(e) => {
                e.stopPropagation();
                if (e.key === "Enter" && !e.shiftKey) {
                  e.preventDefault();
                  commitText();
                } else if (e.key === "Escape") {
                  setEditing(null);
                }
              }}
            />
          )}
          {pages.length > 0 && (
            <div className="diagram-pages" role="tablist" aria-label={t("Pages")}>
              {pages.map((title, i) => (
                <button
                  key={`${i}-${title}`}
                  role="tab"
                  aria-selected={i === page}
                  className={i === page ? "selected" : undefined}
                  onClick={() => {
                    setSelection([]);
                    setPage(i);
                  }}
                  onDoubleClick={() => {
                    const next = window.prompt(t("Rename Page"), title);
                    if (next && next.trim() && next !== title) void apply({ kind: "renamePage", name: next });
                  }}
                >
                  {title}
                </button>
              ))}
              <button aria-label={t("Add Page")} title={t("Add Page")} onClick={() => void apply({ kind: "addPage", name: "" })}>
                +
              </button>
              {pages.length > 1 && (
                <button aria-label={t("Remove Page")} title={t("Remove Page")} onClick={() => void apply({ kind: "removePage" })}>
                  −
                </button>
              )}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
