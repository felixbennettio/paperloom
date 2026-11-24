import {
  useEffect,
  useId,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
  type PointerEvent as ReactPointerEvent,
} from "react";
import { useI18n } from "../i18n";
import {
  backend,
  type MindExtra,
  type MindMapEdit,
  type MindMapLayout,
  type MindMapTheme,
  type MindMarker,
  type MindTopic,
} from "../lib/api";
import { fitView, onStep, presentationSteps, revealTopic, stepBounds, THEME_NAMES, withMapTheme } from "../lib/mindmap";
import { pictureOf } from "../lib/picture";
import { MenuButton } from "./MenuButton";
import { Icon } from "./Icon";

interface Props {
  /** The note's text (an outline with a `mindmap` property). */
  text: string;
  title: string;
  onChange: (text: string) => void;
  /** Follows a topic's link: a web page, or a note by its name. */
  onOpenLink?: (link: string, web: boolean) => void;
}

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** The centre's branch a topic is on: `0.3.1` → 3. */
function branchOf(id: string): number {
  return Number(id.split(".")[1] ?? 0);
}

/** The colour of the branch a topic is on, in a map's theme. */
function branchColor(theme: MindMapTheme | undefined, id: string): string {
  const branches = theme?.branches ?? [];
  return branches.length ? branches[branchOf(id) % branches.length] : "var(--accent)";
}

/** A theme's colours for the map's styles to take; none for the classic
 * one, which follows the app's own light or dark look. */
function themeVariables(theme: MindMapTheme | undefined): CSSProperties | undefined {
  if (!theme || theme.id === "classic") return undefined;
  return {
    "--map-bg": theme.background,
    "--map-centre": theme.centre,
    "--map-centre-text": theme.centreText,
    "--map-topic": theme.topic,
    "--map-text": theme.text,
    "--map-line": theme.line,
  } as CSSProperties;
}

/** A topic's parent and its place under it: `0.3.1` → `0.3`, 1. */
function placeOf(id: string): [string, number] {
  const at = id.lastIndexOf(".");
  return at < 0 ? ["", 0] : [id.slice(0, at), Number(id.slice(at + 1))];
}

/** Whether two topics are under the same parent (the centre is under none). */
function siblings(a: string, b: string): boolean {
  const [p] = placeOf(a);
  return p !== "" && p === placeOf(b)[0];
}

/** Whether a topic is in the run of siblings from `first` to `last`. */
function inRun(id: string, first: string, last: string): boolean {
  const [parent, index] = placeOf(id);
  const [from, a] = placeOf(first);
  const [to, b] = placeOf(last);
  return parent !== "" && parent === from && from === to && index >= Math.min(a, b) && index <= Math.max(a, b);
}

/** A mind map: the note's outline as topics around a centre. Tab adds a
 * topic under the selected one, Enter one beside it, Delete removes it,
 * F2 or Space renames it, the arrow keys move between topics, and a topic
 * dragged onto another moves under it. Shift-click takes in a run of
 * siblings for a summary or boundary; a relation joins the selected topic
 * to the next one clicked. */
export function MindMapView({ text, title, onChange, onOpenLink }: Props) {
  const { t } = useI18n();
  const [layout, setLayout] = useState<MindMapLayout | null>(null);
  const [themes, setThemes] = useState<MindMapTheme[]>([]);
  const [markers, setMarkers] = useState<MindMarker[]>([]);
  /** The step of a presentation shown, if one is under way. */
  const [presenting, setPresenting] = useState<number | null>(null);
  const [focus, setFocus] = useState("0");
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [folded, setFolded] = useState<string[]>([]);
  const [view, setView] = useState({ x: 0, y: 0, zoom: 1 });
  const [pan, setPan] = useState<{ x: number; y: number; ox: number; oy: number } | null>(null);
  const [dragging, setDragging] = useState<{
    id: string; over: string | null; x: number; y: number; dx: number; dy: number; moved: boolean;
  } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  /** The last of a run of siblings that starts at the selected topic. */
  const [runEnd, setRunEnd] = useState<string | null>(null);
  /** A relation waits for its other topic. */
  const [relating, setRelating] = useState<string | null>(null);
  /** The relation, summary or boundary picked, by its index. */
  const [extra, setExtra] = useState<number | null>(null);
  const [extraDraft, setExtraDraft] = useState("");
  /** The selected topic's link is being set: what is typed so far. */
  const [linkDraft, setLinkDraft] = useState<string | null>(null);
  /** The selected topic's labels are being set, separated by commas. */
  const [labelDraft, setLabelDraft] = useState<string | null>(null);
  /** Escape was pressed in the extra's text: it is not kept. */
  const extraCancelled = useRef(false);
  const arrow = `mindmap-arrow-${useId().replace(/[^\w-]/g, "")}`;
  const surface = useRef<HTMLDivElement>(null);
  const centred = useRef(false);
  /** Escape was pressed: the edit ends without being kept. */
  const cancelled = useRef(false);

  useEffect(() => {
    let current = true;
    backend.mindmapLayout(text, title, "", folded).then(
      (laid) => {
        if (!current) return;
        setLayout(laid);
        setError(null);
        // The first time, the map in the middle of the view.
        const box = surface.current?.getBoundingClientRect();
        if (!centred.current && box) {
          centred.current = true;
          const zoom = box.width <= 560
            ? Math.min(1, (box.width - 32) / laid.width, (box.height - 32) / laid.height)
            : Math.min(1, Math.max(0.3, Math.min(box.width / laid.width, box.height / laid.height)));
          setView({ zoom, x: (box.width - laid.width * zoom) / 2, y: (box.height - laid.height * zoom) / 2 });
        }
      },
      (e: unknown) => current && setError(message(e)),
    );
    return () => {
      current = false;
    };
  }, [text, title, folded]);

  // A link or labels being typed are for the topic they were started on.
  useEffect(() => {
    setLinkDraft(null);
    setLabelDraft(null);
  }, [focus]);

  // The wheel pans, and zooms with Ctrl or ⌘.
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
  }, []);

  const apply = async (edit: MindMapEdit, thenEdit = false) => {
    try {
      const change = await backend.mindmapEdit(text, title, edit);
      onChange(change.text);
      if (change.focus) setFocus(change.focus);
      setRunEnd(null);
      setExtra(null);
      if (thenEdit) {
        cancelled.current = false;
        setEditing(change.focus);
        setDraft("");
      }
      surface.current?.focus();
    } catch (e) {
      setError(message(e));
    }
  };

  const topics = layout?.topics ?? [];
  const byId = new Map(topics.map((topic) => [topic.id, topic]));
  const selected = byId.get(focus) ?? topics[0];
  const org = layout?.style === "org";
  const extras = layout?.extras ?? [];
  const picked = extras.find((e) => e.index === extra);
  const run = selected && runEnd && siblings(runEnd, selected.id) ? runEnd : null;
  const runLast = run ?? selected?.id ?? "0";

  const fit = () => {
    const box = surface.current?.getBoundingClientRect();
    if (!layout || !box) return;
    const zoom = Math.min(1, (box.width - 32) / layout.width, (box.height - 32) / layout.height);
    setView({ zoom, x: (box.width - layout.width * zoom) / 2, y: (box.height - layout.height * zoom) / 2 });
  };

  // A pane narrowed to a phone should still show the map it was displaying.
  useEffect(() => {
    const element = surface.current;
    if (!element || !layout) return;
    let width = element.clientWidth;
    const observer = new ResizeObserver(() => {
      const next = element.clientWidth;
      if (next !== width && next <= 560) fit();
      width = next;
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [layout]);

  const zoomBy = (factor: number) => {
    const box = surface.current?.getBoundingClientRect();
    if (!box) return;
    setView((v) => {
      const zoom = Math.min(3, Math.max(0.2, v.zoom * factor));
      return { zoom, x: box.width / 2 - (box.width / 2 - v.x) * zoom / v.zoom, y: box.height / 2 - (box.height / 2 - v.y) * zoom / v.zoom };
    });
  };

  // Selection and edits keep a topic in view without changing the zoom.
  useEffect(() => {
    const topic = layout?.topics.find((topic) => topic.id === (editing ?? focus));
    const node = surface.current?.querySelector<HTMLElement>(".mindmap-topic.selected");
    const box = surface.current?.getBoundingClientRect();
    if (presenting !== null || !topic || !node || !box) return;
    const bounds = { ...topic, width: node.offsetWidth, height: node.offsetHeight };
    setView((v) => revealTopic(bounds, box, v));
  }, [focus, editing, layout, presenting]);

  const pickExtra = (e: MindExtra) => {
    extraCancelled.current = false;
    setExtra(e.index);
    setExtraDraft(e.text);
    setRelating(null);
  };
  const keepExtraText = () => {
    if (extraCancelled.current) {
      extraCancelled.current = false;
      return;
    }
    if (picked && extraDraft.trim() !== picked.text) {
      void apply({ kind: "retextExtra", index: picked.index, text: extraDraft });
    }
  };

  const commit = () => {
    if (cancelled.current) {
      cancelled.current = false;
      return;
    }
    if (!editing) return;
    const was = byId.get(editing)?.text ?? "";
    const value = draft.trim() || was || t("Topic");
    setEditing(null);
    if (value !== was) void apply({ kind: "rename", id: editing, text: value });
    surface.current?.focus();
  };

  const startEditing = (topic: MindTopic) => {
    cancelled.current = false;
    setFocus(topic.id);
    setEditing(topic.id);
    setDraft(topic.text);
  };

  const toggleFold = (id: string) =>
    setFolded((all) => (all.includes(id) ? all.filter((f) => f !== id) : [...all, id]));

  /** The topic next to the selected one in a direction. */
  const neighbour = (key: string): MindTopic | undefined => {
    if (!selected) return undefined;
    const children = topics.filter((c) => c.parent === selected.id);
    const siblings = topics.filter((c) => c.parent === selected.parent && c.side === selected.side);
    const index = siblings.findIndex((s) => s.id === selected.id);
    const parent = selected.parent ? byId.get(selected.parent) : undefined;
    const inward = org ? "ArrowUp" : selected.side < 0 ? "ArrowRight" : "ArrowLeft";
    const outward = org ? "ArrowDown" : selected.side < 0 ? "ArrowLeft" : "ArrowRight";
    if (key === inward) return parent;
    if (key === outward) {
      // From the centre, the branches on the side pressed.
      const side = key === "ArrowLeft" ? -1 : 1;
      return selected.id === "0" ? children.find((c) => org || c.side === side) : children[0];
    }
    if (selected.id === "0") return undefined;
    const back = org ? "ArrowLeft" : "ArrowUp";
    return key === back ? siblings[index - 1] : siblings[index + 1];
  };

  const onKeyDown = (e: ReactKeyboardEvent<HTMLDivElement>) => {
    if (presenting !== null) {
      e.preventDefault();
      if (["ArrowRight", "ArrowDown", " ", "PageDown", "Enter"].includes(e.key)) step(1);
      else if (["ArrowLeft", "ArrowUp", "PageUp", "Backspace"].includes(e.key)) step(-1);
      else if (e.key === "Escape") stopPresenting();
      return;
    }
    if (editing || !selected) return;
    const topic = t("Topic");
    if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
      e.preventDefault();
      setLabelDraft(null);
      setLinkDraft(selected.link);
    } else if (e.key === "Escape") {
      setRelating(null);
      setRunEnd(null);
      setExtra(null);
    } else if ((e.key === "Delete" || e.key === "Backspace") && picked) {
      e.preventDefault();
      void apply({ kind: "removeExtra", index: picked.index });
    } else if (e.key === "Tab") {
      e.preventDefault();
      if (selected.folded) toggleFold(selected.id);
      void apply({ kind: "addChild", id: selected.id, text: topic }, true);
    } else if (e.key === "Enter") {
      e.preventDefault();
      void apply(
        selected.id === "0" ? { kind: "addChild", id: "0", text: topic } : { kind: "addSibling", id: selected.id, text: topic },
        true,
      );
    } else if ((e.key === "Delete" || e.key === "Backspace") && selected.id !== "0") {
      e.preventDefault();
      void apply({ kind: "delete", id: selected.id });
    } else if (e.key === "F2" || e.key === " ") {
      e.preventDefault();
      startEditing(selected);
    } else if (e.key === "." && selected.children > 0) {
      toggleFold(selected.id);
    } else if (e.key.startsWith("Arrow")) {
      e.preventDefault();
      const next = neighbour(e.key);
      if (next) setFocus(next.id);
    }
  };

  const onPointerDown = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (presenting !== null) {
      step(1);
      return;
    }
    if (e.button !== 0 || e.target !== e.currentTarget) return;
    if (editing) commit();
    setExtra(null);
    setRunEnd(null);
    setRelating(null);
    e.currentTarget.setPointerCapture(e.pointerId);
    setPan({ x: e.clientX, y: e.clientY, ox: view.x, oy: view.y });
  };

  const onPointerMove = (e: ReactPointerEvent<HTMLDivElement>) => {
    if (pan) setView((v) => ({ ...v, x: pan.ox + e.clientX - pan.x, y: pan.oy + e.clientY - pan.y }));
    if (!dragging) return;
    const dx = e.clientX - dragging.x;
    const dy = e.clientY - dragging.y;
    if (!dragging.moved && Math.hypot(dx, dy) <= 4) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    setDragging({ ...dragging, moved: true, dx: dx / view.zoom, dy: dy / view.zoom, over: dropTarget(e.clientX, e.clientY, dragging.id) });
  };

  const dropTarget = (x: number, y: number, id: string): string | null => {
    // Capture keeps receiving the gesture outside a topic. Test the stationary
    // topics explicitly, excluding the dragged topic and its descendants.
    const nodes = surface.current?.querySelectorAll<HTMLElement>("[data-topic-id]") ?? [];
    for (const node of nodes) {
      const candidate = node.dataset.topicId!;
      if (candidate === id || candidate.startsWith(`${id}.`)) continue;
      const box = node.getBoundingClientRect();
      if (x >= box.left && x <= box.right && y >= box.top && y <= box.bottom) return candidate;
    }
    return null;
  };

  const drop = (e: ReactPointerEvent<HTMLDivElement>) => {
    const over = dragging?.moved ? dropTarget(e.clientX, e.clientY, dragging.id) : null;
    if (over && dragging) {
      const target = byId.get(over);
      if (target) void apply({ kind: "move", id: dragging.id, parent: target.id, index: target.children });
    }
    setDragging(null);
  };

  const steps = presentationSteps(topics);

  /** Moves the presentation on (or back); past the last step it ends. */
  const step = (by: number) => {
    if (presenting === null) return;
    const next = presenting + by;
    if (next >= steps.length) stopPresenting();
    else setPresenting(Math.max(0, next));
  };

  const stopPresenting = () => {
    setPresenting(null);
    if (document.fullscreenElement) void document.exitFullscreen().catch(() => undefined);
  };

  // Each step fits its branch in the view.
  useEffect(() => {
    const box = surface.current?.getBoundingClientRect();
    if (presenting === null || !layout || !box) return;
    const shown = steps[presenting] ?? "0";
    setView(fitView(stepBounds(layout.topics, shown, layout.width, layout.height), box));
    // `steps` follows `layout`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [presenting, layout]);

  // Leaving full screen ends the presentation.
  useEffect(() => {
    if (presenting === null) return;
    const left = () => {
      if (!document.fullscreenElement) setPresenting(null);
    };
    document.addEventListener("fullscreenchange", left);
    return () => document.removeEventListener("fullscreenchange", left);
  }, [presenting]);

  const exportAs = async (format: string) => {
    setNotice(null);
    setError(null);
    try {
      const png =
        format === "png" || format === "pdf"
          ? await pictureOf(await backend.mindmapSvg(text, title, "", folded), format === "pdf" ? "image/jpeg" : "image/png")
          : null;
      const written = await backend.mindmapExport(text, title, format, png);
      if (written) setNotice(t("Saved {path}.", { path: written }));
    } catch (e) {
      setError(message(e));
    }
  };

  const setStyle = (style: string) => {
    if (/^mindmap\s*:/im.test(text)) onChange(text.replace(/^(mindmap\s*:).*$/im, `$1 ${style}`));
  };

  useEffect(() => {
    backend.mindmapThemes().then(setThemes, () => setThemes([]));
    backend.mindmapMarkers().then(setMarkers, () => setMarkers([]));
  }, []);

  return (
    <div className="mindmap-view">
      <div className="mindmap-toolbar" role="toolbar" aria-label={t("Mind Map")}>
        <MenuButton label={t("Add")} icon={<Icon name="plus" size={15} />} items={[
          { label: t("Add Subtopic"), disabled: !selected, onSelect: () => selected && void apply({ kind: "addChild", id: selected.id, text: t("Topic") }, true) },
          { label: t("Add Topic"), disabled: !selected || selected.id === "0", onSelect: () => selected && void apply({ kind: "addSibling", id: selected.id, text: t("Topic") }, true) },
        ]} />
        <MenuButton label={t("More")} icon={<span aria-hidden="true">⋯</span>} iconOnly items={[
          { label: t("Rename"), disabled: !selected, onSelect: () => selected && startEditing(selected) },
          { label: t("Delete"), disabled: !selected || selected.id === "0", onSelect: () => selected && void apply({ kind: "delete", id: selected.id }) },
          { label: selected?.folded ? t("Unfold") : t("Fold"), disabled: !selected || selected.children === 0, onSelect: () => selected && toggleFold(selected.id) },
          { label: t("Relation"), disabled: !selected, checked: relating !== null, onSelect: () => setRelating(relating ? null : (selected?.id ?? null)) },
          { label: t("Summary"), disabled: !selected || selected.id === "0", onSelect: () => selected && void apply({ kind: "summarize", first: selected.id, last: runLast, text: t("Summary") }) },
          { label: t("Boundary"), disabled: !selected || selected.id === "0", onSelect: () => selected && void apply({ kind: "enclose", first: selected.id, last: runLast, text: "" }) },
          { label: t("Link"), disabled: !selected, onSelect: () => { setLabelDraft(null); setLinkDraft(linkDraft !== null ? null : (selected?.link ?? "")); } },
          { label: t("Labels"), disabled: !selected, onSelect: () => { setLinkDraft(null); setLabelDraft(labelDraft !== null ? null : (selected?.labels ?? []).join(", ")); } },
          { label: t("Present"), disabled: !layout, onSelect: () => {
            setEditing(null);
            setPresenting(0);
            surface.current?.focus();
            void surface.current?.requestFullscreen?.().catch(() => undefined);
          } },
          ...markers.map((marker) => ({ label: `${t("Marker")} ${marker.emoji}`, disabled: !selected, onSelect: () => selected && void apply({ kind: "toggleMarker", id: selected.id, marker: marker.id }) })),
          ...themes.map((theme) => ({ label: `${t("Theme")}: ${t(THEME_NAMES[theme.id] ?? theme.id)}`, checked: (layout?.theme.id ?? "classic") === theme.id, onSelect: () => onChange(withMapTheme(text, theme.id)) })),
        ]} />
        <MenuButton label={t("Layout")} icon={<Icon name="mindmap" size={15} />} items={([
          ["logic", "Both Sides"], ["right", "To the Right"], ["org", "Top Down"], ["tree", "Tree"], ["timeline", "Timeline"], ["fishbone", "Fishbone"],
        ] as const).map(([style, label]) => ({ label: t(label), checked: (layout?.style ?? "logic") === style, onSelect: () => setStyle(style) }))} />
        <MenuButton label={t("Export…")} icon={<Icon name="doc" size={15} />} items={[
          { label: "XMind", onSelect: () => void exportAs("xmind") },
          { label: "OPML", onSelect: () => void exportAs("opml") },
          { label: "FreeMind", onSelect: () => void exportAs("mm") },
          { label: t("Picture (SVG)"), onSelect: () => void exportAs("svg") },
          { label: t("Picture (PNG)"), onSelect: () => void exportAs("png") },
          { label: "PDF", onSelect: () => void exportAs("pdf") },
        ]} />
        <span className="mindmap-zoom">
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
      {picked && (
        <div className="mindmap-extra-bar">
          <span>{picked.kind === "relation" ? t("Relation") : picked.kind === "summary" ? t("Summary") : t("Boundary")}</span>
          <input
            value={extraDraft}
            placeholder={t("Text")}
            aria-label={t("Text")}
            onChange={(e) => setExtraDraft(e.target.value)}
            onBlur={keepExtraText}
            onKeyDown={(e) => {
              if (e.key === "Enter") e.currentTarget.blur();
              if (e.key === "Escape") {
                extraCancelled.current = true;
                setExtra(null);
              }
            }}
          />
          <button onClick={() => void apply({ kind: "removeExtra", index: picked.index })}>{t("Remove")}</button>
        </div>
      )}
      {linkDraft !== null && selected && (
        <form
          className="mindmap-extra-bar"
          onSubmit={(e) => {
            e.preventDefault();
            setLinkDraft(null);
            if (linkDraft.trim() !== selected.link) void apply({ kind: "setLink", id: selected.id, link: linkDraft.trim() });
          }}
        >
          <span>{t("Link")}</span>
          <input
            autoFocus
            value={linkDraft}
            placeholder={t("A web address or a note's name")}
            aria-label={t("Link")}
            onChange={(e) => setLinkDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") {
                setLinkDraft(null);
                surface.current?.focus();
              }
            }}
          />
          <button type="submit" className="primary">
            {t("OK")}
          </button>
          {selected.link && (
            <button
              type="button"
              onClick={() => {
                setLinkDraft(null);
                void apply({ kind: "setLink", id: selected.id, link: "" });
              }}
            >
              {t("Remove Link")}
            </button>
          )}
        </form>
      )}
      {labelDraft !== null && selected && (
        <form
          className="mindmap-extra-bar"
          onSubmit={(e) => {
            e.preventDefault();
            setLabelDraft(null);
            const labels = labelDraft
              .split(/[,，;；]/)
              .map((l) => l.trim())
              .filter(Boolean);
            if (labels.join("\n") !== selected.labels.join("\n")) void apply({ kind: "setLabels", id: selected.id, labels });
          }}
        >
          <span>{t("Labels")}</span>
          <input
            autoFocus
            value={labelDraft}
            placeholder={t("Separated by commas")}
            aria-label={t("Labels")}
            onChange={(e) => setLabelDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") {
                setLabelDraft(null);
                surface.current?.focus();
              }
            }}
          />
          <button type="submit" className="primary">
            {t("OK")}
          </button>
        </form>
      )}
      {relating && <p className="hint">{t("Click the topic to relate it to. Esc cancels.")}</p>}
      {error && <p className="hint error">{error}</p>}
      {notice && <p className="notice" role="status">{notice}</p>}
      <div
        ref={surface}
        className={["mindmap-surface", pan || dragging?.moved ? "panning" : "", presenting !== null ? "presenting" : ""].join(" ")}
        style={themeVariables(layout?.theme)}
        tabIndex={0}
        onKeyDown={onKeyDown}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={(e) => {
          setPan(null);
          drop(e);
        }}
        onPointerCancel={() => { setPan(null); setDragging(null); }}
        onLostPointerCapture={() => { setPan(null); setDragging(null); }}
      >
        {presenting !== null && (
          <div className="mindmap-present-bar" onPointerDown={(e) => e.stopPropagation()}>
            <button aria-label={t("Previous")} title={t("Previous")} disabled={presenting === 0} onClick={() => step(-1)}>
              ‹
            </button>
            <span>
              {presenting + 1} / {steps.length}
            </span>
            <button aria-label={t("Next")} title={t("Next")} onClick={() => step(1)}>
              ›
            </button>
            <button onClick={stopPresenting}>{t("End Presentation")}</button>
          </div>
        )}
        {layout && (
          <div
            className="mindmap-world"
            style={{ width: layout.width, height: layout.height, transform: `translate(${view.x}px, ${view.y}px) scale(${view.zoom})`, "--mindmap-zoom": view.zoom } as CSSProperties}
          >
            <svg className="mindmap-edges" width={layout.width} height={layout.height}>
              <defs>
                <marker id={arrow} viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto-start-reverse">
                  <path d="M0 0L10 5L0 10Z" className="mindmap-arrow" />
                </marker>
              </defs>
              {extras
                .filter((e) => e.kind === "boundary")
                .map((e) => (
                  <g
                    key={`b${e.index}`}
                    className={e.index === extra ? "mindmap-boundary picked" : "mindmap-boundary"}
                    onPointerDown={(event) => {
                      event.stopPropagation();
                      pickExtra(e);
                    }}
                  >
                    <path d={e.path} />
                    <path d={e.path} className="hit" />
                  </g>
                ))}
              {topics
                .filter((topic) => topic.edge)
                .map((topic) => (
                  <path
                    key={topic.id}
                    d={topic.edge}
                    style={{
                      stroke: branchColor(layout.theme, topic.id),
                      strokeWidth: topic.depth === 1 ? 3 : 2,
                      opacity: presenting !== null && !onStep(topic.id, steps[presenting] ?? "0") ? 0.12 : undefined,
                    }}
                  />
                ))}
              {extras
                .filter((e) => e.kind !== "boundary")
                .map((e) => (
                  <g
                    key={`e${e.index}`}
                    className={`mindmap-${e.kind}${e.index === extra ? " picked" : ""}`}
                    onPointerDown={(event) => {
                      event.stopPropagation();
                      pickExtra(e);
                    }}
                  >
                    <path d={e.path} className="hit" />
                    <path d={e.path} markerEnd={e.kind === "relation" ? `url(#${arrow})` : undefined} />
                  </g>
                ))}
            </svg>
            {extras
              .filter((e) => e.width > 0)
              .map((e) => (
                <div
                  key={`l${e.index}`}
                  className={`mindmap-extra-label ${e.kind}${e.index === extra ? " picked" : ""}`}
                  style={{ left: e.x, top: e.y, width: e.width, minHeight: e.height }}
                  onPointerDown={(event) => {
                    event.stopPropagation();
                    pickExtra(e);
                  }}
                >
                  {e.text}
                </div>
              ))}
            {topics.map((topic) => {
              const color = branchColor(layout.theme, topic.id);
              const filled = topic.depth === 1 && layout.theme.filled;
              const classes = [
                "mindmap-topic",
                `depth-${Math.min(topic.depth, 2)}`,
                topic.id === focus ? "selected" : "",
                dragging?.id === topic.id && dragging.moved ? "dragging" : "",
                editing === topic.id ? "editing" : "",
                run && selected && topic.id !== focus && inRun(topic.id, selected.id, run) ? "in-run" : "",
                dragging?.over === topic.id && dragging.id !== topic.id ? "drop-target" : "",
                presenting !== null && !onStep(topic.id, steps[presenting] ?? "0") ? "dimmed" : "",
                topic.labels.length > 0 ? "labelled" : "",
              ].join(" ");
              return (
                <div
                  key={topic.id}
                  className={classes}
                  data-topic-id={topic.id}
                  style={{
                    left: topic.x,
                    top: topic.y,
                    width: topic.width,
                    minHeight: topic.height,
                    fontSize: topic.fontSize,
                    transform: dragging?.id === topic.id && dragging.moved ? `translate(${dragging.dx}px, ${dragging.dy}px)` : undefined,
                    borderColor: topic.depth === 0 ? undefined : color,
                    background: filled ? color : undefined,
                    color: filled ? layout.theme.centreText : undefined,
                  }}
                  title={topic.note || undefined}
                  onPointerDown={(e) => {
                    e.stopPropagation();
                    if (e.button !== 0) return;
                    if (editing && editing !== topic.id) commit();
                    setExtra(null);
                    if (relating) {
                      if (topic.id !== relating) void apply({ kind: "relate", from: relating, to: topic.id, text: "" });
                      setRelating(null);
                      return;
                    }
                    if (e.shiftKey && selected && siblings(topic.id, selected.id)) {
                      setRunEnd(topic.id);
                      return;
                    }
                    setRunEnd(null);
                    setFocus(topic.id);
                    if (topic.id !== "0" && editing !== topic.id) setDragging({ id: topic.id, over: null, x: e.clientX, y: e.clientY, dx: 0, dy: 0, moved: false });
                  }}
                  onDoubleClick={() => startEditing(topic)}
                >
                  {editing === topic.id ? (
                    <textarea
                      autoFocus
                      value={draft}
                      onChange={(e) => setDraft(e.target.value)}
                      onBlur={commit}
                      onKeyDown={(e) => {
                        e.stopPropagation();
                        if (e.key === "Enter" && !e.shiftKey) {
                          e.preventDefault();
                          commit();
                        } else if (e.key === "Escape") {
                          cancelled.current = true;
                          setEditing(null);
                          surface.current?.focus();
                        }
                      }}
                    />
                  ) : (
                    <span>{topic.text}</span>
                  )}
                  {topic.link && editing !== topic.id && (
                    <button
                      className="mindmap-link"
                      title={topic.link}
                      aria-label={t("Open Link")}
                      onPointerDown={(e) => e.stopPropagation()}
                      onClick={() => onOpenLink?.(topic.link, topic.webLink)}
                    >
                      ↗
                    </button>
                  )}
                  {topic.labels.length > 0 && (
                    <span className="mindmap-labels">
                      {topic.labels.map((label) => (
                        <span key={label} className="mindmap-label">
                          {label}
                        </span>
                      ))}
                    </span>
                  )}
                  {topic.note && <span className="mindmap-note" aria-label={t("Has a note")} />}
                  {topic.children > 0 && topic.id !== "0" && (
                    <button
                      className={topic.side < 0 ? "mindmap-fold left" : org ? "mindmap-fold below" : "mindmap-fold"}
                      onPointerDown={(e) => e.stopPropagation()}
                      onClick={() => toggleFold(topic.id)}
                      aria-label={topic.folded ? t("Unfold") : t("Fold")}
                    >
                      {topic.folded ? topic.children : "−"}
                    </button>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
