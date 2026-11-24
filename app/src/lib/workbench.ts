// The open tabs of the workbench, as pure functions so they are easy to test.

export type Tab =
  | { kind: "library" }
  | { kind: "item"; id: string; title: string }
  /** A PDF attachment open for reading; `id` is the attachment's. */
  | { kind: "pdf"; id: string; title: string; page?: number; itemId?: string }
  | { kind: "search" }
  /** The in-app browser's controls and what it read. */
  | { kind: "browser" }
  /** The graph of the whole vault. */
  | { kind: "graph" }
  /** A canvas in the vault, by path. */
  | { kind: "canvas"; path: string; title: string }
  /** A drawing (an Excalidraw scene) in the vault, by path. */
  | { kind: "drawing"; path: string; title: string }
  /** A draw.io diagram in the vault, by path. */
  | { kind: "diagram"; path: string; title: string }
  | { kind: "find" }
  | { kind: "notes" }
  | { kind: "settings" };

export type TabStyle = "horizontal" | "vertical-left" | "vertical-right";
export type Edge = "left" | "right";

export interface TabsState {
  tabs: Tab[];
  selected: string;
}

export const LIBRARY: Tab = { kind: "library" };

export const initialTabs: TabsState = { tabs: [LIBRARY], selected: "library" };

export function tabId(tab: Tab): string {
  if (tab.kind === "item" || tab.kind === "pdf") return `${tab.kind}:${tab.id}`;
  return tab.kind === "canvas" || tab.kind === "drawing" || tab.kind === "diagram" ? `${tab.kind}:${tab.path}` : tab.kind;
}

export function isClosable(tab: Tab): boolean {
  return tab.kind !== "library";
}

export function selectedTab(state: TabsState): Tab {
  return state.tabs.find((t) => tabId(t) === state.selected) ?? LIBRARY;
}

/** Brings a tab to the front, opening it if needed (an open one takes the
 * new details, such as the page to show). */
export function openTab(state: TabsState, tab: Tab): TabsState {
  const id = tabId(tab);
  const exists = state.tabs.some((t) => tabId(t) === id);
  return { tabs: exists ? state.tabs.map((t) => (tabId(t) === id ? tab : t)) : [...state.tabs, tab], selected: id };
}

export function selectTab(state: TabsState, id: string): TabsState {
  return state.tabs.some((t) => tabId(t) === id) ? { ...state, selected: id } : state;
}

/** Closes a tab; the one after it (or before, at the end) comes to the front. */
export function closeTab(state: TabsState, id: string): TabsState {
  const index = state.tabs.findIndex((t) => tabId(t) === id);
  if (index < 0 || !isClosable(state.tabs[index])) return state;
  const tabs = state.tabs.filter((_, i) => i !== index);
  const selected = state.selected === id ? tabId(tabs[Math.min(index, tabs.length - 1)]) : state.selected;
  return { tabs, selected };
}

/** Moves a tab to another position (drag and drop in the tab strip). */
export function moveTab(state: TabsState, id: string, toIndex: number): TabsState {
  const from = state.tabs.findIndex((t) => tabId(t) === id);
  if (from < 0) return state;
  const tabs = [...state.tabs];
  const [tab] = tabs.splice(from, 1);
  tabs.splice(Math.max(0, Math.min(toIndex, tabs.length)), 0, tab);
  return { ...state, tabs };
}

/** The workbench's tab groups: one, or a second beside it. Each tab is in
 * one group only; `focus` is where new tabs open. */
export interface Panes {
  main: TabsState;
  side: TabsState | null;
  focus: "main" | "side";
}

export const initialPanes: Panes = { main: initialTabs, side: null, focus: "main" };

function holds(state: TabsState | null, id: string): boolean {
  return state !== null && state.tabs.some((t) => tabId(t) === id);
}

/** The tab in front of the group in focus. */
export function focusedTab(panes: Panes): Tab {
  return selectedTab(panes.focus === "side" && panes.side ? panes.side : panes.main);
}

/** Opens a tab in the group in focus, or brings it to the front where it
 * is open already. */
export function openInPanes(panes: Panes, tab: Tab): Panes {
  const id = tabId(tab);
  if (panes.side && holds(panes.side, id)) return { ...panes, side: openTab(panes.side, tab), focus: "side" };
  if (!panes.side || panes.focus === "main" || holds(panes.main, id)) {
    return { ...panes, main: openTab(panes.main, tab), focus: "main" };
  }
  return { ...panes, side: openTab(panes.side, tab) };
}

export function selectInPanes(panes: Panes, id: string): Panes {
  if (panes.side && holds(panes.side, id)) return { ...panes, side: selectTab(panes.side, id), focus: "side" };
  return { ...panes, main: selectTab(panes.main, id), focus: "main" };
}

/** The side group without a tab; none when it was the last one. */
function closeInSide(side: TabsState, id: string): TabsState | null {
  const index = side.tabs.findIndex((t) => tabId(t) === id);
  if (index < 0) return side;
  const tabs = side.tabs.filter((_, i) => i !== index);
  if (tabs.length === 0) return null;
  const selected = side.selected === id ? tabId(tabs[Math.min(index, tabs.length - 1)]) : side.selected;
  return { tabs, selected };
}

export function closeInPanes(panes: Panes, id: string): Panes {
  if (panes.side && holds(panes.side, id)) {
    const side = closeInSide(panes.side, id);
    return { ...panes, side, focus: side ? panes.focus : "main" };
  }
  return { ...panes, main: closeTab(panes.main, id) };
}

/** Moves a tab into the other group at `index` (the end by default),
 * making the side group if needed; the library tab stays where it is. */
export function moveAcross(panes: Panes, id: string, index = Number.MAX_SAFE_INTEGER): Panes {
  const fromSide = holds(panes.side, id);
  const from = fromSide ? panes.side : panes.main;
  const tab = from?.tabs.find((t) => tabId(t) === id);
  if (!tab || !isClosable(tab)) return panes;
  if (fromSide && panes.side) {
    const side = closeInSide(panes.side, id);
    return { main: moveTab(openTab(panes.main, tab), id, index), side, focus: "main" };
  }
  const side = moveTab(openTab(panes.side ?? { tabs: [], selected: id }, tab), id, index);
  return { main: closeTab(panes.main, id), side, focus: "side" };
}

/** Drops a dragged tab at `index` in a group: a move within it, or across. */
export function dropInPanes(panes: Panes, id: string, pane: "main" | "side", index: number): Panes {
  const inSide = holds(panes.side, id);
  if ((pane === "side") === inSide) {
    return pane === "side" && panes.side
      ? { ...panes, side: moveTab(panes.side, id, index), focus: "side" }
      : { ...panes, main: moveTab(panes.main, id, index), focus: "main" };
  }
  return moveAcross(panes, id, index);
}

/** Puts every tab of the side group back in the main one. */
export function joinPanes(panes: Panes): Panes {
  if (!panes.side) return panes;
  const main = panes.side.tabs.reduce(openTab, panes.main);
  return { main: selectTab(main, panes.side.selected), side: null, focus: "main" };
}

/** Remembers a tab that came to the front, for going back on a phone:
 * newest last, each tab once. */
export function visit(history: string[], id: string): string[] {
  return [...history.filter((h) => h !== id), id].slice(-50);
}

/** Going back on a phone, where there is no tab strip: the tab in front is
 * closed and the one seen before it that is still open comes back; the
 * library when there is none. Nothing to do on the library itself. */
export function goBack(panes: Panes, history: string[]): Panes | null {
  const front = focusedTab(panes);
  const id = tabId(front);
  if (id === "library") return null;
  const next = closeInPanes(panes, id);
  const previous = [...history]
    .reverse()
    .find((h) => h !== id && (h === "library" || holds(next.main, h) || holds(next.side, h)));
  return selectInPanes(next, previous ?? "library");
}
