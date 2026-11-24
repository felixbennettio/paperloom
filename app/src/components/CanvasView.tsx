import {
  useCallback,
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
  type MouseEvent as ReactMouseEvent,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { useI18n } from "../i18n";
import { MenuButton } from "./MenuButton";
import { backend, type CanvasArrow, type CanvasBoard, type CanvasCard, type NoteInfo } from "../lib/api";
import {
  CARD_COLORS,
  anchor,
  arrowHead,
  bounds,
  cardAt,
  cardColor,
  center,
  curve,
  curvePath,
  facing,
  isSide,
  midpoint,
  moving,
  newId,
  without,
  type Point,
  type Side,
} from "../lib/canvas";
import { Icon } from "./Icon";

interface Props {
  path: string;
  onOpenNote: (path: string) => void;
}

/** What a pointer held down is doing. */
type Drag =
  | { kind: "pan"; start: Point; origin: Point }
  | { kind: "move"; ids: string[]; start: Point; origins: Map<string, Point> }
  | { kind: "resize"; id: string; start: Point; size: Point }
  | { kind: "connect"; from: string; side: Side; to: Point };

interface View {
  x: number;
  y: number;
  zoom: number;
}

const SIDES: Side[] = ["top", "right", "bottom", "left"];

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

function blankCard(kind: string, at: Point, width: number, height: number): CanvasCard {
  return {
    id: newId(),
    kind,
    x: Math.round(at.x - width / 2),
    y: Math.round(at.y - height / 2),
    width,
    height,
    color: null,
    text: null,
    file: null,
    subpath: null,
    url: null,
    label: null,
  };
}

/** The first lines of a note, without its front matter. */
function preview(text: string): string {
  const body = text.startsWith("---") ? text.replace(/^---\n[\s\S]*?\n---\n?/, "") : text;
  return body.slice(0, 600);
}

/** A board of cards (text, notes, web pages, groups) joined by arrows,
 * kept as a JSON Canvas file in the vault. Drag the background to pan,
 * Ctrl/⌘ and the wheel to zoom, double-click to add a card, drag from a
 * card's side to another card to join them. */
export function CanvasView({ path, onOpenNote }: Props) {
  const { t } = useI18n();
  const [board, setBoard] = useState<CanvasBoard | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [view, setView] = useState<View>({ x: 0, y: 0, zoom: 1 });
  const [selected, setSelected] = useState<string | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const [drag, setDrag] = useState<Drag | null>(null);
  const [previews, setPreviews] = useState<Record<string, string>>({});
  const [adding, setAdding] = useState<"note" | "link" | null>(null);
  const [entry, setEntry] = useState("");
  const [notes, setNotes] = useState<NoteInfo[]>([]);
  const surface = useRef<HTMLDivElement>(null);
  const latest = useRef<CanvasBoard | null>(null);
  const timer = useRef<number | null>(null);
  /** A board just loaded is shown whole once the surface is there. */
  const fitting = useRef(false);

  const save = useCallback(() => {
    timer.current = null;
    const current = latest.current;
    if (current) backend.vaultWriteCanvas(path, current).catch((e: unknown) => setError(message(e)));
  }, [path]);

  /** Shows a change now and saves it shortly after. */
  const change = useCallback(
    (next: CanvasBoard) => {
      latest.current = next;
      setBoard(next);
      if (timer.current !== null) window.clearTimeout(timer.current);
      timer.current = window.setTimeout(save, 400);
    },
    [save],
  );

  useEffect(() => {
    let current = true;
    backend.vaultReadCanvas(path).then(
      (loaded) => {
        if (!current) return;
        latest.current = loaded;
        fitting.current = true;
        setBoard(loaded);
      },
      (e: unknown) => current && setError(message(e)),
    );
    return () => {
      current = false;
      // Whatever waits to be saved is saved now.
      if (timer.current !== null) {
        window.clearTimeout(timer.current);
        save();
      }
    };
  }, [path, save]);

  useEffect(() => {
    if (!board || !fitting.current) return;
    fitting.current = false;
    const area = bounds(board);
    const box = surface.current?.getBoundingClientRect();
    if (!area || !box) return;
    const fitted = Math.min(box.width / (area.width + 80), box.height / (area.height + 80));
    const zoom = Math.min(1, box.width <= 560 ? fitted : Math.max(0.2, fitted));
    setView({
      zoom,
      x: box.width / 2 - (area.x + area.width / 2) * zoom,
      y: box.height / 2 - (area.y + area.height / 2) * zoom,
    });
  }, [board]);

  const fit = () => {
    const area = latest.current && bounds(latest.current);
    const box = surface.current?.getBoundingClientRect();
    if (!area || !box) return;
    const zoom = Math.min(1, box.width / (area.width + 80), box.height / (area.height + 80));
    setView({ zoom, x: box.width / 2 - (area.x + area.width / 2) * zoom, y: box.height / 2 - (area.y + area.height / 2) * zoom });
  };

  useEffect(() => {
    const element = surface.current;
    if (!element) return;
    let width = element.clientWidth;
    const observer = new ResizeObserver(() => {
      const next = element.clientWidth;
      if (next !== width && next <= 560) fit();
      width = next;
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [board !== null]);

  const zoomBy = (factor: number) => {
    const box = surface.current?.getBoundingClientRect();
    if (!box) return;
    setView((v) => {
      const zoom = Math.min(3, Math.max(0.2, v.zoom * factor));
      return { zoom, x: box.width / 2 - (box.width / 2 - v.x) * zoom / v.zoom, y: box.height / 2 - (box.height / 2 - v.y) * zoom / v.zoom };
    });
  };

  // Note cards show the start of their note.
  const files = board?.cards.flatMap((c) => (c.kind === "file" && c.file?.toLowerCase().endsWith(".md") ? [c.file] : [])) ?? [];
  const fileKey = [...new Set(files)].sort().join("\n");
  useEffect(() => {
    let current = true;
    for (const file of fileKey ? fileKey.split("\n") : []) {
      backend.vaultRead(file).then(
        (text) => current && setPreviews((all) => ({ ...all, [file]: preview(text) })),
        () => current && setPreviews((all) => ({ ...all, [file]: "" })),
      );
    }
    return () => {
      current = false;
    };
  }, [fileKey]);

  // The wheel pans, and zooms with Ctrl or ⌘ (pinch on a trackpad).
  useEffect(() => {
    const element = surface.current;
    if (!element) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const box = element.getBoundingClientRect();
      setView((v) => {
        if (!e.ctrlKey && !e.metaKey) return { ...v, x: v.x - e.deltaX, y: v.y - e.deltaY };
        const zoom = Math.min(3, Math.max(0.2, v.zoom * Math.exp(-e.deltaY * 0.0015)));
        const px = e.clientX - box.left;
        const py = e.clientY - box.top;
        return { zoom, x: px - ((px - v.x) / v.zoom) * zoom, y: py - ((py - v.y) / v.zoom) * zoom };
      });
    };
    element.addEventListener("wheel", onWheel, { passive: false });
    return () => element.removeEventListener("wheel", onWheel);
  }, [board !== null]);

  const toWorld = (clientX: number, clientY: number): Point => {
    const box = surface.current?.getBoundingClientRect();
    return { x: (clientX - (box?.left ?? 0) - view.x) / view.zoom, y: (clientY - (box?.top ?? 0) - view.y) / view.zoom };
  };

  const middle = (): Point => {
    const box = surface.current?.getBoundingClientRect();
    return box ? toWorld(box.left + box.width / 2, box.top + box.height / 2) : { x: 0, y: 0 };
  };

  if (!board) {
    return <div className="canvas-view">{error ? <p className="hint error">{error}</p> : null}</div>;
  }

  const update = (id: string, patch: Partial<CanvasCard>) =>
    change({ ...board, cards: board.cards.map((c) => (c.id === id ? { ...c, ...patch } : c)) });

  const add = (card: CanvasCard, edit = false) => {
    change({ ...board, cards: [...board.cards, card] });
    setSelected(card.id);
    if (edit) setEditing(card.id);
  };

  const remove = () => {
    if (!selected) return;
    change(without(board, [selected]));
    setSelected(null);
  };

  const begin = (e: ReactPointerEvent, next: Drag) => {
    e.preventDefault();
    surface.current?.setPointerCapture(e.pointerId);
    setDrag(next);
  };

  const onPointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    setSelected(null);
    setEditing(null);
    begin(e, { kind: "pan", start: { x: e.clientX, y: e.clientY }, origin: { x: view.x, y: view.y } });
  };

  const onCardDown = (e: ReactPointerEvent, card: CanvasCard) => {
    e.stopPropagation();
    if (e.button !== 0 || editing === card.id) return;
    setSelected(card.id);
    if (editing) setEditing(null);
    const ids = moving(board, card.id);
    const origins = new Map(board.cards.filter((c) => ids.includes(c.id)).map((c) => [c.id, { x: c.x, y: c.y }]));
    begin(e, { kind: "move", ids, start: toWorld(e.clientX, e.clientY), origins });
  };

  const onPointerMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (!drag) return;
    if (drag.kind === "pan") {
      setView({ ...view, x: drag.origin.x + e.clientX - drag.start.x, y: drag.origin.y + e.clientY - drag.start.y });
      return;
    }
    const at = toWorld(e.clientX, e.clientY);
    if (drag.kind === "connect") {
      setDrag({ ...drag, to: at });
      return;
    }
    const dx = Math.round(at.x - drag.start.x);
    const dy = Math.round(at.y - drag.start.y);
    const cards = board.cards.map((c) => {
      if (drag.kind === "move") {
        const origin = drag.origins.get(c.id);
        return origin ? { ...c, x: origin.x + dx, y: origin.y + dy } : c;
      }
      return c.id === drag.id
        ? { ...c, width: Math.max(80, drag.size.x + dx), height: Math.max(40, drag.size.y + dy) }
        : c;
    });
    const next = { ...board, cards };
    latest.current = next;
    setBoard(next);
  };

  const onPointerUp = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (!drag) return;
    surface.current?.releasePointerCapture(e.pointerId);
    if (drag.kind === "connect") {
      const target = cardAt(board, drag.to, drag.from);
      const source = board.cards.find((c) => c.id === drag.from);
      if (target && source) {
        const arrow: CanvasArrow = {
          id: newId(),
          fromNode: drag.from,
          fromSide: drag.side,
          fromEnd: null,
          toNode: target.id,
          toSide: facing(target, center(source)),
          toEnd: null,
          color: null,
          label: null,
        };
        change({ ...board, arrows: [...board.arrows, arrow] });
        setSelected(arrow.id);
      }
    } else if (drag.kind !== "pan") {
      change(board);
    }
    setDrag(null);
  };

  const onDoubleClick = (e: ReactMouseEvent<HTMLDivElement>) => {
    if (e.target !== e.currentTarget) return;
    add({ ...blankCard("text", toWorld(e.clientX, e.clientY), 260, 140), text: "" }, true);
  };

  const onKeyDown = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    if (editing || adding) return;
    if (e.key === "Delete" || e.key === "Backspace") {
      e.preventDefault();
      remove();
    }
  };

  const openCard = (card: CanvasCard) => {
    if (card.kind === "text" || card.kind === "group") setEditing(card.id);
    else if (card.kind === "file" && card.file?.toLowerCase().endsWith(".md")) onOpenNote(card.file);
    else if (card.kind === "link" && card.url) void backend.openUrl(card.url);
  };

  const finishAdding = () => {
    const value = entry.trim();
    if (adding === "note") {
      const note = notes.find((n) => n.path === value || n.title === value);
      if (note) add({ ...blankCard("file", middle(), 400, 300), file: note.path });
    } else if (adding === "link" && /^https?:\/\/\S+$/.test(value)) {
      add({ ...blankCard("link", middle(), 400, 120), url: value });
    }
    setAdding(null);
    setEntry("");
  };

  const startAdding = (kind: "note" | "link") => {
    setAdding(kind);
    setEntry("");
    if (kind === "note") backend.vaultNotes().then(setNotes, () => setNotes([]));
  };

  const arrowParts = (arrow: CanvasArrow) => {
    const from = board.cards.find((c) => c.id === arrow.fromNode);
    const to = board.cards.find((c) => c.id === arrow.toNode);
    if (!from || !to) return null;
    const fromSide = isSide(arrow.fromSide) ? arrow.fromSide : facing(from, center(to));
    const toSide = isSide(arrow.toSide) ? arrow.toSide : facing(to, center(from));
    return curve(anchor(from, fromSide), fromSide, anchor(to, toSide), toSide);
  };

  const chosen = board.cards.find((c) => c.id === selected) ?? null;
  const chosenArrow = board.arrows.find((a) => a.id === selected) ?? null;
  const dragCursor = drag?.kind === "pan" || drag?.kind === "move" ? "grabbing"
    : drag?.kind === "resize" ? "nwse-resize" : drag?.kind === "connect" ? "crosshair" : undefined;
  const connecting = drag?.kind === "connect" ? drag : null;
  const groupsFirst = [...board.cards].sort((a, b) => Number(b.kind === "group") - Number(a.kind === "group"));

  const recolor = (color: string | null) => {
    if (chosen) update(chosen.id, { color });
    else if (chosenArrow)
      change({ ...board, arrows: board.arrows.map((a) => (a.id === chosenArrow.id ? { ...a, color } : a)) });
  };

  return (
    <div className="canvas-view">
      <div className="canvas-toolbar" role="toolbar" aria-label={t("Canvas")}>
        <MenuButton label={t("Add")} icon={<Icon name="plus" size={15} />} items={[
          { label: t("Add Card"), icon: <Icon name="doc" size={15} />, onSelect: () => add({ ...blankCard("text", middle(), 260, 140), text: "" }, true) },
          { label: t("Add Note…"), icon: <Icon name="note" size={15} />, onSelect: () => startAdding("note") },
          { label: t("Add Web Page…"), icon: <Icon name="link" size={15} />, onSelect: () => startAdding("link") },
          { label: t("Add Group"), icon: <Icon name="folder" size={15} />, onSelect: () => add({ ...blankCard("group", middle(), 600, 400), label: t("Group") }) },
        ]} />
        {adding && (
          <input
            autoFocus
            className="canvas-entry"
            value={entry}
            list={adding === "note" ? "canvas-notes" : undefined}
            placeholder={adding === "note" ? t("Note title") : "https://"}
            onChange={(e) => setEntry(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") finishAdding();
              if (e.key === "Escape") setAdding(null);
            }}
            onBlur={() => setAdding(null)}
          />
        )}
        <datalist id="canvas-notes">
          {notes.map((n) => (
            <option key={n.path} value={n.path}>
              {n.title}
            </option>
          ))}
        </datalist>
        {(chosen || chosenArrow) && (
          <span className="canvas-colors">
            <MenuButton label={t("Color")} icon={<Icon name="shapes" size={15} />} iconOnly items={[
              { label: t("No Colour"), onSelect: () => recolor(null) },
              ...Object.entries(CARD_COLORS).map(([key, css]) => ({ label: css, icon: <span className="menu-swatch" style={{ background: css }} />, onSelect: () => recolor(key) })),
            ]} />
            <button title={t("Delete")} aria-label={t("Delete")} onClick={remove}>
              <Icon name="trash" size={15} />
            </button>
          </span>
        )}
        <span className="canvas-zoom">
          <button aria-label={t("Zoom Out")} onClick={() => zoomBy(1 / 1.25)}>
            −
          </button>
          <span className="zoom-label">{Math.round(view.zoom * 100)}%</span>
          <button className="narrow-fit" onClick={fit}>{t("Fit")}</button>
          <button aria-label={t("Zoom In")} onClick={() => zoomBy(1.25)}>
            +
          </button>
        </span>
      </div>
      {error && <p className="hint error">{error}</p>}
      <div
        ref={surface}
        className={drag?.kind === "pan" ? "canvas-surface panning" : "canvas-surface"}
        style={{ cursor: dragCursor }}
        tabIndex={0}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={() => {
          if (drag?.kind === "move" || drag?.kind === "resize") change(latest.current ?? board);
          setDrag(null);
        }}
        onDoubleClick={onDoubleClick}
        onKeyDown={onKeyDown}
      >
        <div className="canvas-world" style={{ transform: `translate(${view.x}px, ${view.y}px) scale(${view.zoom})`, "--canvas-zoom": view.zoom } as CSSProperties}>
          <svg className="canvas-arrows" width="1" height="1">
            {board.arrows.map((arrow) => {
              const parts = arrowParts(arrow);
              if (!parts) return null;
              const color = cardColor(arrow.color) ?? "var(--muted)";
              const label = arrow.label ? midpoint(parts) : null;
              return (
                <g key={arrow.id} className={arrow.id === selected ? "selected" : ""}>
                  <path
                    className="arrow-hit"
                    d={curvePath(parts)}
                    onPointerDown={(e) => {
                      e.stopPropagation();
                      setSelected(arrow.id);
                    }}
                  />
                  <path className="arrow-line" d={curvePath(parts)} style={{ stroke: color }} />
                  {arrow.toEnd !== "none" && <path d={arrowHead(parts[2], parts[3])} style={{ fill: color }} />}
                  {arrow.fromEnd === "arrow" && <path d={arrowHead(parts[1], parts[0])} style={{ fill: color }} />}
                  {label && (
                    <text x={label.x} y={label.y} textAnchor="middle" className="arrow-label">
                      {arrow.label}
                    </text>
                  )}
                </g>
              );
            })}
            {connecting &&
              (() => {
                const source = board.cards.find((c) => c.id === connecting.from);
                if (!source) return null;
                const start = anchor(source, connecting.side);
                return <path className="arrow-line pending" d={`M ${start.x} ${start.y} L ${connecting.to.x} ${connecting.to.y}`} />;
              })()}
          </svg>
          {groupsFirst.map((card) => {
            const color = cardColor(card.color);
            const classes = ["canvas-card", card.kind, card.id === selected ? "selected" : "", editing === card.id ? "editing" : ""].join(" ");
            return (
              <div
                key={card.id}
                className={classes}
                style={{
                  left: card.x,
                  top: card.y,
                  width: card.width,
                  height: card.height,
                  cursor: dragCursor,
                  borderColor: color ?? undefined,
                  background: color ? `color-mix(in srgb, ${color} ${card.kind === "group" ? 6 : 12}%, var(--surface-strong))` : undefined,
                }}
                onPointerDown={(e) => onCardDown(e, card)}
                onDoubleClick={(e) => {
                  e.stopPropagation();
                  openCard(card);
                }}
              >
                {card.kind === "group" &&
                  (editing === card.id ? (
                    <input
                      autoFocus
                      className="group-label"
                      value={card.label ?? ""}
                      onChange={(e) => update(card.id, { label: e.target.value })}
                      onBlur={() => setEditing(null)}
                      onKeyDown={(e) => e.key === "Enter" && setEditing(null)}
                    />
                  ) : (
                    <span className="group-label">{card.label}</span>
                  ))}
                {card.kind === "text" &&
                  (editing === card.id ? (
                    <textarea
                      autoFocus
                      value={card.text ?? ""}
                      onChange={(e) => update(card.id, { text: e.target.value })}
                      onBlur={() => setEditing(null)}
                      onKeyDown={(e) => e.key === "Escape" && setEditing(null)}
                    />
                  ) : (
                    <div className="card-text">{card.text || <span className="muted">{t("Double-click to write")}</span>}</div>
                  ))}
                {card.kind === "file" && card.file && (
                  <>
                    <button
                      className="link card-title"
                      onPointerDown={(e) => e.stopPropagation()}
                      onClick={() => card.file && onOpenNote(card.file)}
                    >
                      {card.file.replace(/^.*\//, "").replace(/\.md$/i, "")}
                    </button>
                    <div className="card-text">{previews[card.file] ?? ""}</div>
                  </>
                )}
                {card.kind === "link" && card.url && (
                  <>
                    <button
                      className="link card-title"
                      onPointerDown={(e) => e.stopPropagation()}
                      onClick={() => card.url && void backend.openUrl(card.url)}
                    >
                      {card.url.replace(/^https?:\/\//, "")}
                    </button>
                    <div className="card-text muted">{card.url}</div>
                  </>
                )}
                {card.id === selected &&
                  SIDES.map((side) => {
                    const at = anchor(card, side);
                    return (
                      <span
                        key={side}
                        className="connect-handle"
                        style={{ left: at.x - card.x, top: at.y - card.y }}
                        title={t("Drag to another card to join them")}
                        onPointerDown={(e) => {
                          e.stopPropagation();
                          begin(e, { kind: "connect", from: card.id, side, to: at });
                        }}
                      />
                    );
                  })}
                {card.id === selected && (
                  <span
                    className="resize-handle"
                    onPointerDown={(e) => {
                      e.stopPropagation();
                      begin(e, {
                        kind: "resize",
                        id: card.id,
                        start: toWorld(e.clientX, e.clientY),
                        size: { x: card.width, y: card.height },
                      });
                    }}
                  />
                )}
              </div>
            );
          })}
        </div>
        {board.cards.length === 0 && <p className="canvas-empty muted">{t("Double-click anywhere to add a card.")}</p>}
      </div>
    </div>
  );
}
