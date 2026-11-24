import { useEffect, useMemo, useRef, useState, type PointerEvent } from "react";
import { useI18n } from "../i18n";
import { backend, type GraphNode, type NoteGraph } from "../lib/api";
import { Icon } from "./Icon";

interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

interface Props {
  /** A note to draw the graph around; the whole vault without one. */
  center?: string | null;
  depth?: number;
  /** Small, in a side panel: no header. */
  compact?: boolean;
  onOpenNote: (path: string) => void;
  onOpenItem: (itemId: string, title: string) => void;
}

/** Notes, the papers they cite and links still to write, laid out by the
 * core; drag to move around, scroll to zoom, click to open. */
export function GraphView({ center = null, depth = 1, compact = false, onOpenNote, onOpenItem }: Props) {
  const { t } = useI18n();
  const [graph, setGraph] = useState<NoteGraph | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [box, setBox] = useState<Box | null>(null);
  const [hover, setHover] = useState<number | null>(null);
  const [revision, setRevision] = useState(0);
  const [panning, setPanning] = useState(false);
  const svg = useRef<SVGSVGElement>(null);
  const drag = useRef<{ x: number; y: number; box: Box; moved: boolean } | null>(null);
  const dragged = useRef(false);

  useEffect(() => {
    let current = true;
    backend.vaultGraph(center, depth).then(
      (loaded) => {
        if (!current) return;
        setGraph(loaded);
        setBox(fit(loaded.nodes));
        setError(null);
      },
      (e) => current && setError(e instanceof Error ? e.message : String(e)),
    );
    return () => {
      current = false;
    };
  }, [center, depth, revision]);

  // Zooms around the pointer; a listener of its own so it may stop the page
  // from scrolling.
  useEffect(() => {
    const element = svg.current;
    if (!element) return;
    const wheel = (event: WheelEvent) => {
      event.preventDefault();
      const rect = element.getBoundingClientRect();
      const factor = Math.exp(event.deltaY * 0.0015);
      setBox((b) => {
        if (!b) return b;
        const [px, py] = toGraph(b, rect, event.clientX, event.clientY);
        return { x: px - (px - b.x) * factor, y: py - (py - b.y) * factor, w: b.w * factor, h: b.h * factor };
      });
    };
    element.addEventListener("wheel", wheel, { passive: false });
    return () => element.removeEventListener("wheel", wheel);
  }, [graph]);

  const neighbours = useMemo(() => {
    const near = new Map<number, Set<number>>();
    for (const { from, to } of graph?.edges ?? []) {
      if (!near.has(from)) near.set(from, new Set());
      if (!near.has(to)) near.set(to, new Set());
      near.get(from)!.add(to);
      near.get(to)!.add(from);
    }
    return near;
  }, [graph]);

  const labels = useMemo(() => labelBoxes(graph?.nodes ?? []), [graph]);

  const down = (event: PointerEvent<SVGSVGElement>) => {
    if (!box || event.button !== 0) return;
    dragged.current = false;
    drag.current = { x: event.clientX, y: event.clientY, box, moved: false };
  };
  const move = (event: PointerEvent<SVGSVGElement>) => {
    const start = drag.current;
    const element = svg.current;
    if (!start || !element) return;
    const rect = element.getBoundingClientRect();
    const dx = event.clientX - start.x;
    const dy = event.clientY - start.y;
    if (!start.moved) {
      if (Math.abs(dx) + Math.abs(dy) <= 3) return;
      start.moved = true;
      dragged.current = true;
      // Capture only a drag: a click must still reach the node beneath it.
      element.setPointerCapture(event.pointerId);
      setPanning(true);
    }
    const scale = scaleOf(start.box, rect);
    setBox({ ...start.box, x: start.box.x - dx * scale, y: start.box.y - dy * scale });
  };
  const up = () => {
    drag.current = null;
    setPanning(false);
  };

  const open = (node: GraphNode) => {
    if (dragged.current) return;
    if (node.kind === "note") onOpenNote(node.id);
    else if (node.kind === "item" && node.itemId) onOpenItem(node.itemId, node.title);
  };

  const zoomedIn = box !== null && graph !== null && box.w < 900;
  const lit = (i: number) => hover === null || hover === i || (neighbours.get(hover)?.has(i) ?? false);
  const zoomBy = (factor: number) => setBox((b) => b && ({
    x: b.x + b.w * (1 - factor) / 2,
    y: b.y + b.h * (1 - factor) / 2,
    w: b.w * factor,
    h: b.h * factor,
  }));

  return (
    <div className={compact ? "graph-view compact" : "graph-view"}>
      {!compact && (
        <header className="view-header">
          <h2>{t("Graph")}</h2>
          <span className="spacer" />
          <span className="graph-controls">
            <button aria-label={t("Zoom Out")} onClick={() => zoomBy(1.25)}>−</button>
            <button disabled={!graph} onClick={() => graph && setBox(fit(graph.nodes))}>{t("Fit")}</button>
            <button aria-label={t("Zoom In")} onClick={() => zoomBy(1 / 1.25)}>+</button>
          </span>
          <button className="icon-button" title={t("Reload")} aria-label={t("Reload")} onClick={() => setRevision((r) => r + 1)}>
            <Icon name="restore" size={15} />
          </button>
        </header>
      )}
      {error && <p className="notice">{error}</p>}
      {graph && graph.nodes.length === 0 ? (
        <div className="empty">
          <Icon name="graph" size={40} />
          <p>{t("Nothing to draw yet.")}</p>
        </div>
      ) : (
        <svg
          ref={svg}
          className={panning ? "graph panning" : "graph"}
          viewBox={box ? `${box.x} ${box.y} ${box.w} ${box.h}` : "0 0 100 100"}
          onPointerDown={down}
          onPointerMove={move}
          onPointerUp={up}
          onPointerCancel={up}
          onLostPointerCapture={up}
        >
          {graph &&
            graph.edges.map(({ from, to }, i) => {
            const a = graph.nodes[from];
            const b = graph.nodes[to];
            const shown = hover === null || hover === from || hover === to;
            return <line key={i} className={shown ? "edge" : "edge faded"} x1={a.x} y1={a.y} x2={b.x} y2={b.y} />;
          })}
          {graph?.nodes.map((node, i) => {
            const radius = 4 + Math.sqrt(node.degree) * 2;
            const isCenter = node.id === center;
            return (
              <g
                key={node.id}
                className={`node ${node.kind}${isCenter ? " center" : ""}${lit(i) ? "" : " faded"}`}
                onPointerEnter={() => setHover(i)}
                onPointerLeave={() => setHover(null)}
                onClick={() => open(node)}
              >
                <circle className="node-hit" cx={node.x} cy={node.y} r={radius} />
                <circle cx={node.x} cy={node.y} r={radius} />
                {(zoomedIn || compact || hover === i || isCenter || node.degree >= 3) && (
                  <text x={node.x} y={labels[i].y + 10}>
                    {node.title}
                  </text>
                )}
                <title>{node.title}</title>
              </g>
            );
          })}
        </svg>
      )}
    </div>
  );
}

/** Graph units per screen pixel: the view keeps its proportions
 * (`xMidYMid meet`), so the larger ratio wins. */
function scaleOf(box: Box, rect: DOMRect): number {
  return Math.max(box.w / Math.max(rect.width, 1), box.h / Math.max(rect.height, 1));
}

/** A point on screen in graph units. */
function toGraph(box: Box, rect: DOMRect, clientX: number, clientY: number): [number, number] {
  const scale = scaleOf(box, rect);
  return [
    box.x + box.w / 2 + (clientX - rect.left - rect.width / 2) * scale,
    box.y + box.h / 2 + (clientY - rect.top - rect.height / 2) * scale,
  ];
}

/** A view of every node with a margin. */
function fit(nodes: GraphNode[]): Box {
  if (nodes.length === 0) return { x: -100, y: -100, w: 200, h: 200 };
  let [left, top, right, bottom] = [Infinity, Infinity, -Infinity, -Infinity];
  for (const node of nodes) {
    left = Math.min(left, node.x);
    top = Math.min(top, node.y);
    right = Math.max(right, node.x);
    bottom = Math.max(bottom, node.y);
  }
  for (const label of labelBoxes(nodes)) {
    left = Math.min(left, label.x);
    top = Math.min(top, label.y);
    right = Math.max(right, label.x + label.w);
    bottom = Math.max(bottom, label.y + label.h);
  }
  const margin = 40;
  return {
    x: left - margin,
    y: top - margin,
    w: Math.max(right - left + 2 * margin, 120),
    h: Math.max(bottom - top + 2 * margin, 120),
  };
}

/** Keep nearby titles on separate lines, and include their extents in the
 * initial fit. Measuring in graph units keeps this stable while zooming. */
function labelBoxes(nodes: GraphNode[]): Box[] {
  const context = document.createElement("canvas").getContext("2d");
  if (context) context.font = `10px ${getComputedStyle(document.body).fontFamily}`;
  const occupied: Box[] = nodes.map((node) => {
    const radius = 4 + Math.sqrt(node.degree) * 2 + 2;
    return { x: node.x - radius, y: node.y - radius, w: radius * 2, h: radius * 2 };
  });
  return nodes.map((node) => {
    const width = (context?.measureText(node.title).width ?? node.title.length * 7) + 4;
    const label = { x: node.x - width / 2, y: node.y + 4 + Math.sqrt(node.degree) * 2 + 3, w: width, h: 12 };
    let collision: Box | undefined;
    while ((collision = occupied.find((other) => label.x < other.x + other.w && label.x + label.w > other.x && label.y < other.y + other.h && label.y + label.h > other.y))) {
      label.y = collision.y + collision.h + 2;
    }
    occupied.push(label);
    return label;
  });
}
