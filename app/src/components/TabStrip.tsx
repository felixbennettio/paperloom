import { useCallback, useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n";
import { isClosable, tabId, type Tab, type TabsState } from "../lib/workbench";
import { Icon, type IconName } from "./Icon";

const ICONS: Record<Tab["kind"], IconName> = {
  library: "library",
  // A record card for the item, a page for its PDF: they share a title.
  item: "template",
  pdf: "doc",
  search: "globe",
  browser: "compass",
  graph: "graph",
  canvas: "canvas",
  drawing: "pen",
  diagram: "diagram",
  find: "search",
  notes: "note",
  settings: "settings",
};

export function tabTitle(tab: Tab, t: (key: string) => string): string {
  switch (tab.kind) {
    case "library":
      return t("Library");
    case "item":
    case "pdf":
    case "canvas":
    case "drawing":
    case "diagram":
      return tab.title;
    case "search":
      return t("Online Search");
    case "browser":
      return t("Browser");
    case "graph":
      return t("Graph");
    case "find":
      return t("Search Everything");
    case "notes":
      return t("Notes");
    case "settings":
      return t("Settings");
  }
}

interface Props {
  state: TabsState;
  vertical: boolean;
  onSelect: (id: string) => void;
  onClose: (id: string) => void;
  onMove: (id: string, toIndex: number) => void;
  /** Opens the front tab beside this group, or puts the side group back. */
  split?: { label: string; act: () => void } | null;
}

/** Document tabs, in a row or (Edge style) in a column. Tabs can be dragged. */
export function TabStrip({ state, vertical, onSelect, onClose, onMove, split }: Props) {
  const { t } = useI18n();
  const scroller = useRef<HTMLDivElement>(null);
  // Which ends have tabs scrolled out of sight, to fade those ends.
  const [more, setMore] = useState({ before: false, after: false });
  const measure = useCallback(() => {
    const s = scroller.current;
    if (!s) return;
    const [at, size, view] = vertical ? [s.scrollTop, s.scrollHeight, s.clientHeight] : [s.scrollLeft, s.scrollWidth, s.clientWidth];
    const next = { before: at > 1, after: at + view < size - 1 };
    setMore((m) => (m.before === next.before && m.after === next.after ? m : next));
  }, [vertical]);
  useEffect(() => {
    const s = scroller.current;
    if (!s || typeof ResizeObserver === "undefined") return;
    const watch = new ResizeObserver(measure);
    watch.observe(s);
    return () => watch.disconnect();
  }, [measure]);
  // The selected tab is always in sight, also when it was just opened past
  // the end of a full strip.
  useEffect(() => {
    const s = scroller.current;
    const tab = s?.querySelector<HTMLElement>(".tab.selected");
    if (!s || !tab) return;
    const r = tab.getBoundingClientRect();
    const v = s.getBoundingClientRect();
    if (vertical) {
      if (r.top < v.top) s.scrollTop -= v.top - r.top + 8;
      else if (r.bottom > v.bottom) s.scrollTop += r.bottom - v.bottom + 8;
    } else if (r.left < v.left) s.scrollLeft -= v.left - r.left + 8;
    else if (r.right > v.right) s.scrollLeft += r.right - v.right + 8;
    measure();
  }, [state.selected, state.tabs.length, vertical, measure]);
  return (
    <div
      className={vertical ? "tab-strip vertical" : "tab-strip"}
      onDragOver={(e) => e.preventDefault()}
      onDrop={(e) => {
        // The tab was dropped past the last tab or into an empty group.
        const dragged = e.dataTransfer.getData("text/x-paperloom-tab");
        if (dragged && (e.target === e.currentTarget || (e.target as HTMLElement).classList.contains("tab-scroll"))) onMove(dragged, state.tabs.length);
      }}
    >
      <div
        ref={scroller}
        className={["tab-scroll", more.before && "more-before", more.after && "more-after"].filter(Boolean).join(" ")}
        role="tablist"
        aria-orientation={vertical ? "vertical" : "horizontal"}
        onScroll={measure}
        onWheel={(e) => {
          // A mouse wheel scrolls a row of tabs sideways.
          if (!vertical && Math.abs(e.deltaY) > Math.abs(e.deltaX)) e.currentTarget.scrollLeft += e.deltaY;
        }}
      >
        {state.tabs.map((tab, index) => {
          const id = tabId(tab);
          const selected = id === state.selected;
          const title = tabTitle(tab, t);
          return (
            <div
              key={id}
              role="tab"
              aria-selected={selected}
              tabIndex={selected ? 0 : -1}
              className={selected ? "tab selected" : "tab"}
              title={title}
              draggable
              onClick={() => onSelect(id)}
              onKeyDown={(e) => {
                const next = e.key === (vertical ? "ArrowDown" : "ArrowRight") ? 1 : e.key === (vertical ? "ArrowUp" : "ArrowLeft") ? -1 : 0;
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  onSelect(id);
                } else if ((e.key === "Delete" || e.key === "Backspace") && isClosable(tab)) {
                  e.preventDefault();
                  onClose(id);
                } else if (next) {
                  // Arrow keys move between tabs, as in any tab list.
                  e.preventDefault();
                  const to = (index + next + state.tabs.length) % state.tabs.length;
                  onSelect(tabId(state.tabs[to]));
                  (e.currentTarget.parentElement?.children[to] as HTMLElement | undefined)?.focus();
                }
              }}
              onAuxClick={(e) => e.button === 1 && onClose(id)}
              onDragStart={(e) => e.dataTransfer.setData("text/x-paperloom-tab", id)}
              onDragOver={(e) => e.preventDefault()}
              onDrop={(e) => {
                e.stopPropagation();
                const dragged = e.dataTransfer.getData("text/x-paperloom-tab");
                if (dragged) onMove(dragged, index);
              }}
            >
              <Icon name={ICONS[tab.kind]} size={15} />
              <span className="tab-title">{title}</span>
              {isClosable(tab) && (
                // For the pointer only: the keyboard closes a tab with
                // Delete, and a control inside a tab confuses screen readers.
                <span
                  className="tab-close"
                  title={t("Close tab")}
                  aria-hidden="true"
                  onClick={(e) => {
                    e.stopPropagation();
                    onClose(id);
                  }}
                >
                  <Icon name="close" size={12} />
                </span>
              )}
            </div>
          );
        })}
      </div>
      {split && (
        <button className="icon-button tab-split" title={split.label} aria-label={split.label} onClick={split.act}>
          <Icon name="split" size={15} />
        </button>
      )}
    </div>
  );
}
