// Obsidian plugins in the web view. Each turned-on plugin runs in a frame
// of its own, served from the app's `plugin:` scheme and sandboxed (an
// opaque origin: no app pages, no app commands). The frame's runtime asks
// for whatever the plugin does to the vault, the open note or the web, and
// this host does it, or refuses.

import { publishPluginDecorations, validatePluginDecorations } from "./pluginEditor";
import { backend, type ObsidianPlugin, type ZoteroPlugin } from "./api";

export interface PluginCommand {
  id: string;
  name: string;
  plugin: string;
  /** It works on the open note. */
  editor: boolean;
}

export interface PluginRibbon {
  plugin: string;
  index: number;
  icon: string;
  title: string;
}

/** A section a Zotero plugin adds to the item pane. */
export interface PluginSection {
  plugin: string;
  paneID: string;
  label: string;
}

/** A column a Zotero plugin adds to the item list. */
export interface PluginColumn {
  plugin: string;
  key: string;
  label: string;
}

export const READER_EVENTS = [
  "renderTextSelectionPopup", "renderSidebarAnnotationHeader", "renderToolbar",
  "createAnnotationContextMenu", "createViewContextMenu",
] as const;
export type ReaderEvent = typeof READER_EVENTS[number];

export interface ReaderMenuItem {
  index: number;
  label: string;
  disabled: boolean;
}

export interface ReaderMenu {
  plugin: string;
  menu: number;
  items: ReaderMenuItem[];
}

export type ItemMenuChild = ReaderMenuItem | { separator: true };
export type ItemMenuEntry = ItemMenuChild | { label: string; disabled: boolean; children: ItemMenuChild[] };

export interface PluginItemMenu {
  plugin: string;
  menu: number;
  items: ItemMenuEntry[];
}

function itemMenuEntries(value: unknown): ItemMenuEntry[] {
  let count = 0;
  const indexes = new Set<number>();
  const read = (value: unknown, nested = false): ItemMenuEntry[] | null => {
    if (!Array.isArray(value) || value.length > 50) return null;
    const entries: ItemMenuEntry[] = [];
    for (const item of value) {
      if (++count > 50 || !item || typeof item !== "object" || Array.isArray(item)) return null;
      if (item.separator === true) {
        if ("label" in item || "index" in item || "children" in item) return null;
        entries.push({ separator: true });
        continue;
      }
      if (typeof item.label !== "string" || !item.label.trim() || typeof item.disabled !== "boolean") return null;
      if ("children" in item) {
        if (nested || "index" in item) return null;
        const children = read(item.children, true) as ItemMenuChild[] | null;
        if (!children) return null;
        if (children.length) entries.push({ label: item.label.slice(0, 500), disabled: item.disabled, children });
      } else {
        if (!Number.isSafeInteger(item.index) || item.index < 0 || indexes.has(item.index)) return null;
        indexes.add(item.index);
        entries.push({ index: item.index, label: item.label.slice(0, 500), disabled: item.disabled });
      }
    }
    return entries;
  };
  return read(value) ?? [];
}

export interface PluginNotice {
  id: number;
  plugin: string;
  text: string;
}

export interface RunningPlugin {
  id: string;
  name: string;
  hasSettings: boolean;
  /** What it uses that Paperloom does not offer yet. */
  partial: string[];
  error: string | null;
}

export interface PluginState {
  running: RunningPlugin[];
  commands: PluginCommand[];
  ribbon: PluginRibbon[];
  notices: PluginNotice[];
  status: Record<string, string>;
  /** A plugin's settings or modal on show. */
  showing: { plugin: string; kind: string } | null;
  sections: PluginSection[];
  columns: PluginColumn[];
  readerEvents: Record<string, ReaderEvent[]>;
  /** Bumped when what plugins draw in notes changes (a code block language,
   * a post processor, a plugin stopping), so a note on show is drawn again. */
  drawing: number;
}

export interface EditorSnapshot {
  protocol: 1;
  session: string;
  generation: number;
  revision: number;
  contextRevision: number;
  value: string;
  ranges: { anchor: number; head: number }[];
  mainIndex: number;
  mode: "source" | "live";
  focused: boolean;
  composing: boolean;
}

export interface EditorProposal {
  operation: string;
  session: string;
  generation: number;
  baseRevision: number;
  baseContextRevision: number;
  /** Ordered edits, each using offsets in the result of the preceding edit. */
  edits: { from: number; to: number; insert: string }[];
  ranges: { anchor: number; head: number }[];
  mainIndex: number;
  focus?: boolean;
  scroll?: boolean;
}

export interface SuggestKey { key: string; modifiers: string[] }
export interface PluginSuggestion {
  plugin: string;
  request: number;
  session: string;
  generation: number;
  revision: number;
  contextRevision: number;
  priority: number;
  start: number;
  end: number;
  items: { id: string; label: string; secondary?: string }[];
  selected: number;
  keys: SuggestKey[];
}
export interface EditorMenu extends PluginItemMenu {
  session: string; generation: number; revision: number; contextRevision: number;
}
export interface EditorKeys { key: string; shiftKey?: boolean; ctrlKey?: boolean; altKey?: boolean; metaKey?: boolean }
const sameEditorContext = (a: EditorSnapshot | null, b: Pick<EditorSnapshot, "session" | "generation" | "revision" | "contextRevision">) =>
  !!a && a.session === b.session && a.generation === b.generation && a.revision === b.revision && a.contextRevision === b.contextRevision;

/** The open note, as plugins may read and change it. */
export interface ActiveNote {
  path: string;
  state(): EditorSnapshot | null;
  apply(proposal: EditorProposal): boolean;
  /** Vault file writes also update the visible document. */
  setValue(text: string): void;
}

/** A plugin to run: an Obsidian one, by its id, or a Zotero one, by
 * `zotero:` and its id. */
interface Startable {
  key: string;
  id: string;
  name: string;
  version: string;
  kind: "obsidian" | "zotero";
  obsidian?: ObsidianPlugin;
}

interface Running {
  plugin: Startable;
  frame: HTMLIFrameElement;
  info: RunningPlugin;
  started?: boolean;
  ready?: boolean;
  /** Startup did not finish in time: nothing more is queued for it. */
  failed?: boolean;
  startTimer?: ReturnType<typeof setTimeout>;
  pending?: Context[];
  initialItems?: Projection[];
  initialSeq?: number;
  initialTabs?: object;
}

type Projection = { key: string; deleted?: boolean; attachments?: { key: string }[] } & Record<string, unknown>;
type ReaderView = { viewID: string; attachmentKey: string; parentItemKey: string };
type Context = { type: string } & Record<string, unknown>;
/** How long a Zotero plugin may take to start, and how much waits for it. */
const START_TIMEOUT = 30_000;
const PENDING_EVENT_KEYS = 2_000;
const PENDING_MESSAGES = 200;
type ItemEvent = {
  event: "add" | "modify" | "trash" | "delete";
  objectType: "item";
  itemKeys: string[];
  changed?: Record<string, string[]>;
};

function normalized(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(normalized).join(",")}]`;
  if (value && typeof value === "object") return `{${Object.entries(value).sort(([a], [b]) => a.localeCompare(b))
    .map(([key, entry]) => `${JSON.stringify(key)}:${normalized(entry)}`).join(",")}}`;
  return JSON.stringify(value) ?? "null";
}

function itemDiff(before: Projection[] | null, items: Projection[]): ItemEvent[] {
  if (!before) return [];
  const previous = new Map(before.map((item) => [item.key, item]));
  const current = new Map(items.map((item) => [item.key, item]));
  const groups = new Map<ItemEvent["event"], ItemEvent>();
  for (const event of ["add", "modify", "trash", "delete"] as const) groups.set(event, { event, objectType: "item", itemKeys: [] });
  for (const item of items) {
    const old = previous.get(item.key);
    const changed = old ? [...new Set([...Object.keys(old), ...Object.keys(item)])]
      .filter((field) => field !== "key" && (field === "deleted" ? !!old.deleted !== !!item.deleted : normalized(old[field]) !== normalized(item[field]))) : [];
    const event = !old ? "add" : !old.deleted && item.deleted ? "trash" : changed.length ? "modify" : null;
    if (!event) continue;
    const group = groups.get(event)!;
    group.itemKeys.push(item.key);
    if (event === "modify") (group.changed ??= {})[item.key] = changed;
  }
  for (const item of before) if (!current.has(item.key)) groups.get("delete")!.itemKeys.push(item.key);
  return [...groups.values()].flatMap((group) => {
    const batches: ItemEvent[] = [];
    for (let i = 0; i < group.itemKeys.length; i += 500) {
      const itemKeys = group.itemKeys.slice(i, i + 500);
      batches.push({ ...group, itemKeys, ...(group.changed ? { changed: Object.fromEntries(itemKeys.map((key) => [key, group.changed![key]])) } : {}) });
    }
    return batches;
  });
}

type Call = { type: "call"; id: number; method: string; args: unknown[] };

/** Where the frames come from: the app's own scheme, which the Windows and
 * Android web views reach as an http host. */
export function pluginFrameUrl(userAgent = navigator.userAgent, page = "host.html"): string {
  return /Windows|Android/i.test(userAgent) ? `http://plugin.localhost/${page}` : `plugin://localhost/${page}`;
}

/** The host name a plugin's request goes to, or null if it is no web address. */
export function siteOf(url: string): string | null {
  try {
    const parsed = new URL(url);
    return parsed.protocol === "https:" || parsed.protocol === "http:" ? parsed.host : null;
  } catch {
    return null;
  }
}

const SITES_KEY = "paperloom.pluginSites";

function storage(): Storage | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null;
  }
}

/** The sites each plugin may reach, as the person allowed them. */
function loadSites(): Record<string, string[]> {
  try {
    return JSON.parse(storage()?.getItem(SITES_KEY) ?? "{}") as Record<string, string[]>;
  } catch {
    return {};
  }
}

export class PluginHost {
  private state: PluginState = { running: [], commands: [], ribbon: [], notices: [], status: {}, showing: null, sections: [], columns: [], readerEvents: {}, drawing: 0 };
  private listeners = new Set<() => void>();
  private annotationListeners = new Map<string, Set<() => void>>();
  private plugins = new Map<string, Running>();
  private active: ActiveNote | null = null;
  private suggestion: PluginSuggestion | null = null;
  private suggestListeners = new Set<() => void>();
  private suggestPlugins = new Set<string>();
  private editorMenuPlugins = new Set<string>();
  private editorRequest = 0;
  private suggestQuery: { request: number; editor: EditorSnapshot; pending: Set<string>; results: Map<string, PluginSuggestion>; timer: ReturnType<typeof setTimeout> } | null = null;
  private editorMenus = new Map<number, { plugin: string; editor: EditorSnapshot; resolve: (menu: EditorMenu | null) => void; timer: ReturnType<typeof setTimeout> }>();
  private shownEditorMenus: EditorMenu[] = [];
  private editorMenuEpoch = 0;
  private lastEditor: (EditorSnapshot & { path: string }) | null = null;
  private commandChecks = new Map<string, { plugin: string; command: string; resolve: (enabled: boolean) => void; timer: ReturnType<typeof setTimeout> }>();
  private commandCheckId = 0;
  private layer: HTMLElement | null = null;
  private noticeId = 1;
  /** The code block languages plugins draw, and which plugin draws each. */
  private codeLanguages = new Map<string, string>();
  /** Plugins that change notes as they are shown (Markdown post
   * processors), in the order they asked to. */
  private postProcessors: string[] = [];
  private renders = new Map<number, (html: string | null) => void>();
  private columnRequests = new Map<number, { plugin: string; resolve: (values: Record<string, string>) => void }>();
  private readerRenders = new Map<number, { plugin: string; resolve: (html: string) => void }>();
  private readerViews = new Map<number, { plugin: string; update: (html: string) => void }>();
  private readerMenus = new Map<number, { plugin: string; resolve: (items: ReaderMenuItem[]) => void }>();
  private itemMenus = new Map<number, { plugin: string; resolve: (items: ItemMenuEntry[]) => void }>();
  private renderId = 1;
  private baseline: Projection[] | null = null;
  private seq = 0;
  private itemTimer: ReturnType<typeof setTimeout> | null = null;
  private fetchingItems = false;
  private itemsDirty = false;
  private annotationTail: Promise<unknown> = Promise.resolve();
  private itemWaiters: (() => void)[] = [];
  private origins = new Map<string, Set<string>>();
  private itemWrites: Promise<unknown> = Promise.resolve();
  private committingItem: Promise<void> | null = null;
  private itemRevision = 0;
  private writeHistory = new Map<string, { at: number; plugins: string[] }>();
  private selection: string[] = [];
  private selectionTimer: ReturnType<typeof setTimeout> | null = null;
  private readers: ReaderView[] = [];
  private selectedViewID = "zotero-pane";
  private tabsTimer: ReturnType<typeof setTimeout> | null = null;
  private readerRequests: { attachmentKey: string; parentItemKey?: string }[] = [];
  private selectedAttachmentKey: string | null = null;
  private openedViews = new Set<string>();
  private successfulOpens = new WeakSet<object>();
  private lastTabs = normalized({ readers: [], selectedViewID: "zotero-pane" });
  /** Opens a note by link (`workspace.openLinkText`). */
  openNote: (link: string) => void = () => {};
  /** A plugin changed the library's items. */
  onItemsChanged: () => void = () => {};
  /** Applies a plugin's selection to library navigation before acknowledging. */
  selectItems: (itemKeys: string[]) => Promise<void> = async () => { throw new Error("Library selection is unavailable."); };
  selectTab: (viewID: string) => Promise<void> = async () => { throw new Error("Tab selection is unavailable."); };
  /** Asks whether a plugin may reach a site. */
  allowSite: (plugin: string, site: string) => boolean = () => false;
  /** Plugins that need Node or Electron stay off here. */
  mobile = /Android|iPhone|iPad/i.test(typeof navigator === "undefined" ? "" : navigator.userAgent);

  constructor() {
    if (typeof window !== "undefined") window.addEventListener("message", (event) => void this.receive(event));
  }

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  /** Called only after a complete projection commits a change for this attachment. */
  subscribeAnnotations(attachmentKey: string, listener: () => void): () => void {
    const listeners = this.annotationListeners.get(attachmentKey) ?? new Set<() => void>();
    listeners.add(listener);
    this.annotationListeners.set(attachmentKey, listeners);
    return () => {
      listeners.delete(listener);
      if (!listeners.size) this.annotationListeners.delete(attachmentKey);
    };
  }

  snapshot = () => this.state;

  private update(patch: Partial<PluginState>) {
    this.state = { ...this.state, ...patch };
    for (const listener of this.listeners) listener();
  }

  private publish() {
    this.update({ running: [...this.plugins.values()].map((p) => p.info) });
  }

  /** Where the frames go (kept there; moving a frame would reload it). */
  setLayer(layer: HTMLElement | null) {
    this.layer = layer;
  }

  /** Starts the turned-on plugins and stops the others. */
  async reload() {
    const [obsidian, zotero] = await Promise.all([
      backend.obsidianPlugins().catch(() => [] as ObsidianPlugin[]),
      backend.zoteroPlugins().catch(() => [] as ZoteroPlugin[]),
    ]);
    const wanted: Startable[] = [
      ...obsidian
        .filter((p) => p.enabled && !(p.isDesktopOnly && this.mobile))
        .map((p): Startable => ({ key: p.id, id: p.id, name: p.name, version: p.version, kind: "obsidian", obsidian: p })),
      ...zotero
        .filter((p) => p.enabled && !this.mobile)
        .map((p): Startable => ({ key: `zotero:${p.id}`, id: p.id, name: p.name, version: p.version, kind: "zotero" })),
    ];
    const keys = new Set(wanted.map((p) => p.key));
    for (const key of [...this.plugins.keys()]) if (!keys.has(key)) this.stop(key);
    for (const plugin of wanted) if (!this.plugins.has(plugin.key)) this.start(plugin);
  }

  private start(plugin: Startable) {
    if (!this.layer) return;
    const frame = document.createElement("iframe");
    frame.setAttribute("sandbox", "allow-scripts");
    frame.className = "plugin-frame";
    frame.title = plugin.name;
    frame.src = pluginFrameUrl(navigator.userAgent, plugin.kind === "zotero" ? "zotero.html" : "host.html");
    this.layer.appendChild(frame);
    this.plugins.set(plugin.key, {
      plugin,
      frame,
      info: { id: plugin.key, name: plugin.name, hasSettings: false, partial: [], error: null },
      initialItems: this.baseline ?? undefined,
      initialSeq: this.seq,
      initialTabs: this.tabContext(),
    });
    this.publish();
  }

  /** A Zotero plugin's item pane section for an item (HTML, to clean
   * before showing), or null. */
  async renderSection(section: PluginSection, itemKey: string): Promise<string | null> {
    // A plugin registers its sections while it starts, before it has the
    // library's items; drawn then, a section would come back empty. Wait
    // for it to be ready or to fail (startup gives up after 30 seconds).
    for (let waited = 0; waited < 310; waited++) {
      const starting = this.plugins.get(section.plugin);
      if (!starting || starting.failed || starting.ready) break;
      await new Promise((resolve) => window.setTimeout(resolve, 100));
    }
    const running = this.plugins.get(section.plugin);
    if (!running?.ready) return null;
    const id = this.renderId++;
    return new Promise((resolve) => {
      this.renders.set(id, resolve);
      this.post(running, { type: "render-section", id, paneID: section.paneID, itemKey });
      window.setTimeout(() => {
        if (this.renders.delete(id)) resolve(null);
      }, 5000);
    });
  }

  /** A Zotero plugin's column values for a batch of items, as plain text. */
  columnValues(column: PluginColumn, itemKeys: string[]): Promise<Record<string, string>> {
    const running = this.plugins.get(column.plugin);
    if (!running) return Promise.resolve({});
    const id = this.renderId++;
    return new Promise((resolve) => {
      this.columnRequests.set(id, { plugin: column.plugin, resolve });
      this.post(running, { type: "column-values", id, key: column.key, itemKeys });
      window.setTimeout(() => {
        if (this.columnRequests.delete(id)) resolve({});
      }, 5000);
    });
  }

  /** Asks the listening Zotero plugins for HTML to sanitize before showing. */
  readerRender(event: ReaderEvent, attachmentKey: string, itemKey: string | null, params: object): Promise<{ plugin: string; html: string }[]> {
    return Promise.all(this.readerPlugins(event).map(async (running) => {
      const plugin = running.plugin.key;
      const id = this.renderId++;
      const html = await new Promise<string>((resolve) => {
        this.readerRenders.set(id, { plugin, resolve });
        this.post(running, { type: "reader-render", id, event, attachmentKey, itemKey, params });
        window.setTimeout(() => {
          if (this.readerRenders.delete(id)) resolve("");
        }, 5000);
      });
      return { plugin, html };
    }));
  }

  /** Retains sandbox DOM until the reader fragment is unmounted. */
  watchReader(event: ReaderEvent, attachmentKey: string, itemKey: string | null, params: object,
    update: (entries: { plugin: string; render: number; html: string }[]) => void): () => void {
    const entries = this.readerPlugins(event).map((running) => ({ plugin: running.plugin.key, render: this.renderId++, html: "" }));
    for (const entry of entries) {
      this.readerViews.set(entry.render, { plugin: entry.plugin, update: (html) => {
        entry.html = html;
        update(entries.map((value) => ({ ...value })));
      } });
      this.post(this.plugins.get(entry.plugin)!, { type: "reader-render", id: entry.render, live: true, event, attachmentKey, itemKey, params });
    }
    return () => {
      for (const entry of entries) {
        this.readerViews.delete(entry.render);
        const running = this.plugins.get(entry.plugin);
        if (running) this.post(running, { type: "reader-release", id: entry.render });
      }
    };
  }

  readerEvent(plugin: string, render: number, node: string, event: object) {
    const running = this.plugins.get(plugin);
    if (running && this.readerViews.get(render)?.plugin === plugin) this.post(running, { type: "reader-event", ...event, id: render, node });
  }

  /** Asks the listening Zotero plugins for menu labels and command indexes. */
  readerMenu(event: ReaderEvent, attachmentKey: string, itemKey: string | null, params: object): Promise<ReaderMenu[]> {
    return Promise.all(this.readerPlugins(event).map(async (running) => {
      const plugin = running.plugin.key;
      const menu = this.renderId++;
      const items = await new Promise<ReaderMenuItem[]>((resolve) => {
        this.readerMenus.set(menu, { plugin, resolve });
        this.post(running, { type: "reader-menu", id: menu, event, attachmentKey, itemKey, params });
        window.setTimeout(() => {
          if (this.readerMenus.delete(menu)) resolve([]);
        }, 5000);
      });
      return { plugin, menu, items };
    }));
  }

  readerCommand(plugin: string, menu: number, index: number) {
    const running = this.plugins.get(plugin);
    if (running?.plugin.kind === "zotero") this.post(running, { type: "reader-command", menu, index });
  }

  /** Asks every running Zotero plugin for its item menu. */
  itemMenu(itemKeys: string[]): Promise<PluginItemMenu[]> {
    return Promise.all([...this.plugins.values()].filter((running) => running.plugin.kind === "zotero").map(async (running) => {
      const plugin = running.plugin.key;
      const menu = this.renderId++;
      const items = await new Promise<ItemMenuEntry[]>((resolve) => {
        this.itemMenus.set(menu, { plugin, resolve });
        this.post(running, { type: "item-menu", id: menu, itemKeys });
        window.setTimeout(() => {
          if (this.itemMenus.delete(menu)) resolve([]);
        }, 2000);
      });
      return { plugin, menu, items };
    }));
  }

  itemCommand(plugin: string, menu: number, index: number, path: string[], itemKeys: string[]) {
    const running = this.plugins.get(plugin);
    if (running?.plugin.kind === "zotero") this.post(running, { type: "item-command", menu, index, path, itemKeys });
  }

  private readerPlugins(event: ReaderEvent): Running[] {
    return [...this.plugins.values()].filter((running) =>
      running.plugin.kind === "zotero" && this.state.readerEvents[running.plugin.key]?.includes(event),
    );
  }

  /** Keep each native annotation commit observable, even within a 50 ms burst.
   * Establish the baseline before the first mutation and refresh before the next. */
  annotationMutation<T>(mutate: () => Promise<T>): Promise<T> {
    const result = this.annotationTail.then(async () => {
      await this.itemsChanged();
      const value = await mutate();
      await this.itemsChanged();
      return value;
    });
    this.annotationTail = result.catch(() => {});
    return result;
  }

  /** Every item invalidation goes through the complete, last-good projection. */
  itemsChanged(): Promise<void> {
    this.itemsDirty = true;
    const settled = new Promise<void>((resolve) => this.itemWaiters.push(resolve));
    if (!this.fetchingItems && this.itemTimer === null) {
      this.itemTimer = setTimeout(() => { this.itemTimer = null; void this.refreshItems(); }, 50);
    }
    return settled;
  }

  private async refreshItems() {
    this.fetchingItems = true;
    while (this.itemsDirty) {
      this.itemsDirty = false;
      try {
        await this.committingItem?.catch(() => {});
        const revision = this.itemRevision;
        const items: unknown = JSON.parse(await backend.zoteroItems());
        if (revision !== this.itemRevision || this.committingItem) {
          this.itemsDirty = true;
          continue;
        }
        if (!Array.isArray(items) || items.some((item) => !item || typeof item.key !== "string")
          || new Set(items.map((item) => item.key)).size !== items.length) throw new Error("Invalid Zotero item projection");
        const records = new Map((items as Projection[]).map((item) => [item.key, item]));
        for (const item of items as Projection[]) if (item.itemType === "annotation") {
          if (records.get(String(item.parentItem))?.itemType !== "attachment"
            || !["highlight", "underline", "note", "ink", "text"].includes(String(item.annotationType))) {
            throw new Error("Invalid Zotero annotation projection");
          }
        }
        const projection: Projection[] = (items as Projection[]).map((item) => ({ ...item, deleted: !!item.deleted,
          ...(Array.isArray(item.tags) ? { tags: [...item.tags].sort((a, b) => normalized(a).localeCompare(normalized(b))) } : {}),
        }));
        const events = itemDiff(this.baseline, projection);
        const changedKeys = new Set(events.flatMap((event) => event.itemKeys));
        const attachments = new Set([...(this.baseline ?? []), ...projection]
          .filter((item) => item.itemType === "annotation" && changedKeys.has(item.key))
          .map((item) => String(item.parentItem)));
        const origins = this.origins;
        this.origins = new Map();
        this.baseline = projection;
        this.resolveReaders();
        const seq = ++this.seq;
        for (const running of this.plugins.values()) {
          if (running.plugin.kind !== "zotero") continue;
          if (!running.initialItems) {
            running.initialItems = projection;
            running.initialSeq = seq;
            running.initialTabs = this.tabContext();
            if (!running.ready) continue;
          }
          const delivered = events.flatMap((event) => {
            const itemKeys = event.itemKeys.filter((key) => event.event !== "modify" || !origins.get(key)?.has(running.plugin.key));
            return itemKeys.length ? [{ ...event, itemKeys,
              ...(event.changed ? { changed: Object.fromEntries(itemKeys.map((key) => [key, event.changed![key]])) } : {}),
            }] : [];
          });
          this.context(running, { type: "items", items: projection, seq, events: delivered });
        }
        this.sendTabs();
        for (const key of attachments) for (const listener of this.annotationListeners.get(key) ?? []) listener();
      } catch (error) {
        // A failed read must never turn into a mass deletion or lose write origins.
        console.error("Refreshing Zotero plugin items:", error);
      }
    }
    this.fetchingItems = false;
    const waiters = this.itemWaiters.splice(0);
    for (const resolve of waiters) resolve();
  }

  selectionChanged(itemKeys: string[]) {
    const keys = [...new Set(itemKeys)];
    if (normalized(keys) === normalized(this.selection)) return;
    this.selection = keys;
    if (this.selectionTimer !== null) return;
    this.selectionTimer = setTimeout(() => this.sendSelection(), 16);
  }

  private sendSelection() {
    if (this.selectionTimer !== null) clearTimeout(this.selectionTimer);
    this.selectionTimer = null;
    const message = { type: "selection", seq: ++this.seq, itemKeys: this.selection };
    for (const running of this.plugins.values()) if (running.plugin.kind === "zotero") this.context(running, message);
  }

  /** All PDF tabs, including tabs hidden behind another tab or in the other pane. */
  tabsChanged(readers: { attachmentKey: string; parentItemKey?: string }[], selectedAttachmentKey: string | null) {
    this.readerRequests = readers.map(({ attachmentKey, parentItemKey }) => ({ attachmentKey, parentItemKey }));
    this.selectedAttachmentKey = selectedAttachmentKey;
    this.resolveReaders();
    if (this.tabsTimer === null) this.tabsTimer = setTimeout(() => this.sendTabs(), 16);
  }

  private resolveReaders() {
    const old = this.readers;
    this.readers = this.readerRequests.flatMap(({ attachmentKey, parentItemKey }) => {
      const parent = this.baseline?.find((item) => item.attachments?.some((attachment) => attachment.key === attachmentKey));
      if (!parent || (parentItemKey && parent.key !== parentItemKey)) return [];
      const previous = old.find((reader) => reader.attachmentKey === attachmentKey);
      return [{ viewID: previous?.viewID ?? crypto.randomUUID(), attachmentKey, parentItemKey: parent.key }];
    });
    for (const viewID of this.openedViews) if (!this.readers.some((reader) => reader.viewID === viewID)) this.openedViews.delete(viewID);
    this.selectedViewID = this.readers.find((reader) => reader.attachmentKey === this.selectedAttachmentKey)?.viewID ?? "zotero-pane";
  }

  private tabContext() {
    return { seq: this.seq, readers: this.readers, selectedViewID: this.selectedViewID };
  }

  private sendTabs() {
    if (this.tabsTimer !== null) clearTimeout(this.tabsTimer);
    this.tabsTimer = null;
    const state = normalized({ readers: this.readers, selectedViewID: this.selectedViewID });
    if (state === this.lastTabs) return;
    this.lastTabs = state;
    const message = { type: "tabs", ...this.tabContext(), seq: ++this.seq };
    for (const running of this.plugins.values()) if (running.plugin.kind === "zotero") this.context(running, message);
  }

  readerView(viewID: string) { return this.readers.find((reader) => reader.viewID === viewID); }

  /** Called only after PdfView has loaded a valid document. Tab remounts and page edits share the open instance. */
  async readerOpened(attachmentKey: string, isCurrent: () => boolean = () => true) {
    if (!this.baseline?.some((item) => item.attachments?.some((attachment) => attachment.key === attachmentKey))) await this.itemsChanged();
    const reader = this.readers.find((view) => view.attachmentKey === attachmentKey);
    if (!isCurrent() || !reader || this.openedViews.has(reader.viewID)) return;
    this.openedViews.add(reader.viewID);
    this.sendTabs();
    this.sendFileOpen(attachmentKey);
  }

  /** An external open gets a fresh instance on every successful invocation. Failed opens never notify. */
  async openAttachment(attachmentKey: string) {
    await backend.openAttachment(attachmentKey);
    await this.fileOpened(attachmentKey, {});
  }

  async fileOpened(attachmentKey: string, instance: object) {
    if (this.successfulOpens.has(instance)) return;
    this.successfulOpens.add(instance);
    if (!this.baseline?.some((item) => item.attachments?.some((attachment) => attachment.key === attachmentKey))) await this.itemsChanged();
    if (this.baseline?.some((item) => item.attachments?.some((attachment) => attachment.key === attachmentKey))) this.sendFileOpen(attachmentKey);
  }

  private sendFileOpen(attachmentKey: string) {
    const message = { type: "notifier", seq: ++this.seq, events: [{ event: "open", objectType: "file", itemKeys: [attachmentKey] }] };
    for (const running of this.plugins.values()) if (running.plugin.kind === "zotero") this.context(running, message);
  }

  /** Library context for a plugin. Until it is ready, only the latest item
   * snapshot, selection and tabs wait for it, with a bounded event backlog;
   * selection goes straight to a started runtime, so a selectItems reply
   * never arrives before the selection it made. */
  private context(running: Running, message: Context) {
    if (running.ready || (message.type === "selection" && running.started)) return this.post(running, message);
    if (running.failed) return;
    const pending = (running.pending ??= []);
    const earlier = pending.findIndex((queued) => queued.type === message.type);
    if (message.type === "items" && earlier >= 0) {
      const events = [...((pending[earlier].events as ItemEvent[]) ?? []), ...((message.events as ItemEvent[]) ?? [])];
      const keys = events.reduce((count, event) => count + event.itemKeys.length, 0);
      message = { ...message, events: keys > PENDING_EVENT_KEYS ? [] : events };
      pending.splice(earlier, 1);
    } else if ((message.type === "selection" || message.type === "tabs") && earlier >= 0) {
      pending.splice(earlier, 1);
    } else if (pending.length >= PENDING_MESSAGES) {
      return;
    }
    pending.push(message);
  }

  /** Whether a plugin draws code blocks in this language. */
  drawsCode(language: string): boolean {
    return this.codeLanguages.has(language);
  }

  /** A code block as the plugin for its language draws it (HTML, to clean
   * before showing), or null. */
  renderCodeBlock(language: string, source: string, path: string): Promise<string | null> {
    const running = this.plugins.get(this.codeLanguages.get(language) ?? "");
    if (!running) return Promise.resolve(null);
    const id = this.renderId++;
    return new Promise((resolve) => {
      this.renders.set(id, resolve);
      this.post(running, { type: "render-code", id, language, source, path });
      window.setTimeout(() => {
        if (this.renders.delete(id)) resolve(null);
      }, 5000);
    });
  }

  /** Whether a plugin changes notes as they are shown. */
  hasPostProcessors(): boolean {
    return this.postProcessors.length > 0;
  }

  /** A rendered note as the plugins' post processors change it, one plugin
   * after another (HTML, to clean before showing). */
  async postProcess(html: string, path: string): Promise<string> {
    for (const plugin of this.postProcessors) {
      const running = this.plugins.get(plugin);
      if (!running) continue;
      const id = this.renderId++;
      const changed = await new Promise<string | null>((resolve) => {
        this.renders.set(id, resolve);
        this.post(running, { type: "post-process", id, html, path });
        window.setTimeout(() => {
          if (this.renders.delete(id)) resolve(null);
        }, 5000);
      });
      if (changed !== null) html = changed;
    }
    return html;
  }

  stop(id: string) {
    const running = this.plugins.get(id);
    if (!running) return;
    for (const [language, plugin] of [...this.codeLanguages]) if (plugin === id) this.codeLanguages.delete(language);
    this.postProcessors = this.postProcessors.filter((plugin) => plugin !== id);
    clearTimeout(running.startTimer);
    running.pending = [];
    this.post(running, { type: "stop" });
    running.frame.remove();
    this.plugins.delete(id);
    publishPluginDecorations(id, null);
    this.suggestPlugins.delete(id);
    this.editorMenuPlugins.delete(id);
    if (this.suggestion?.plugin === id || this.suggestQuery?.pending.has(id)) this.closeSuggestions();
    for (const [request, pending] of this.editorMenus) if (pending.plugin === id) {
      clearTimeout(pending.timer); this.editorMenus.delete(request); pending.resolve(null);
    }
    this.closeEditorMenus();
    for (const [request, pending] of this.commandChecks) {
      if (pending.plugin !== id) continue;
      clearTimeout(pending.timer);
      this.commandChecks.delete(request);
      pending.resolve(false);
    }
    for (const [request, pending] of this.columnRequests) {
      if (pending.plugin !== id) continue;
      this.columnRequests.delete(request);
      pending.resolve({});
    }
    for (const [request, view] of this.readerViews) {
      if (view.plugin === id) { this.readerViews.delete(request); view.update(""); }
    }
    for (const [request, pending] of this.readerRenders) {
      if (pending.plugin !== id) continue;
      this.readerRenders.delete(request);
      pending.resolve("");
    }
    for (const [request, pending] of this.readerMenus) {
      if (pending.plugin !== id) continue;
      this.readerMenus.delete(request);
      pending.resolve([]);
    }
    for (const [request, pending] of this.itemMenus) {
      if (pending.plugin !== id) continue;
      this.itemMenus.delete(request);
      pending.resolve([]);
    }
    const readerEvents = { ...this.state.readerEvents };
    delete readerEvents[id];
    const status = { ...this.state.status };
    delete status[id];
    this.update({
      commands: this.state.commands.filter((c) => c.plugin !== id),
      ribbon: this.state.ribbon.filter((r) => r.plugin !== id),
      sections: this.state.sections.filter((s) => s.plugin !== id),
      columns: this.state.columns.filter((c) => c.plugin !== id),
      readerEvents,
      status,
      drawing: this.state.drawing + 1,
      showing: this.state.showing?.plugin === id ? null : this.state.showing,
    });
    this.publish();
  }

  private post(running: Running, message: object) {
    running.frame.contentWindow?.postMessage(message, "*");
  }

  /** The note open in the editor, or none. */
  setActiveNote(note: ActiveNote | null) {
    this.closeSuggestions();
    this.closeEditorMenus();
    const previous = this.lastEditor;
    if (previous) this.broadcastEditor({ type: "editor/close", session: previous.session, generation: previous.generation });
    this.active = note;
    this.lastEditor = this.editorState();
    this.broadcastEditor({ type: "active", editor: this.lastEditor });
    if (this.lastEditor) this.broadcastEditor({ type: "editor/open", editor: this.lastEditor });
  }

  private editorState() {
    const state = this.active?.state();
    return this.active && state ? { path: this.active.path, ...state } : null;
  }

  private broadcastEditor(message: object) {
    for (const running of this.plugins.values()) if (running.plugin.kind === "obsidian") this.post(running, message);
  }

  /** CM owns revisions; publish immediately, including selection-only changes. */
  editorUpdated(origin = "user", userEvent?: string, changes: EditorProposal["edits"] = []) {
    const editor = this.editorState();
    const previous = this.lastEditor;
    if (!editor || !previous || editor.session !== previous.session || editor.generation !== previous.generation) {
      this.setActiveNote(this.active);
      return;
    }
    this.closeEditorMenus();
    this.lastEditor = editor;
    this.broadcastEditor({ type: "editor/update", editor, baseRevision: previous.revision, baseContextRevision: previous.contextRevision,
      changes, origin, userEvent });
    if (editor.revision !== previous.revision) this.broadcastEditor({ type: "event", name: "editor-change", editor });
    // As in Obsidian, typing opens suggestions; moving the cursor only
    // updates or closes a list already open.
    if (editor.revision !== previous.revision || this.suggestion || this.suggestQuery) this.querySuggestions();
  }

  subscribeSuggestions = (listener: () => void) => { this.suggestListeners.add(listener); return () => { this.suggestListeners.delete(listener); }; };
  suggestionSnapshot = () => this.suggestion;
  private showSuggestion(value: PluginSuggestion | null) {
    this.suggestion = value;
    for (const listener of this.suggestListeners) listener();
  }
  closeSuggestions() {
    const query = this.suggestQuery;
    if (query) {
      clearTimeout(query.timer);
      for (const plugin of this.suggestPlugins) {
        const running = this.plugins.get(plugin);
        if (running) this.post(running, { type: "suggest/close", request: query.request });
      }
    }
    this.suggestQuery = null;
    this.showSuggestion(null);
  }
  private querySuggestions() {
    const old = this.suggestQuery;
    if (old) clearTimeout(old.timer);
    this.suggestQuery = null;
    this.showSuggestion(null);
    const editor = this.editorState();
    if (!editor || !editor.focused || editor.composing || editor.ranges.length !== 1 || editor.ranges[0].anchor !== editor.ranges[0].head) {
      // An ineligible context must also invalidate pending runtime work.
      if (old) for (const plugin of this.suggestPlugins) {
        const running = this.plugins.get(plugin);
        if (running) this.post(running, { type: "suggest/close", request: old.request });
      }
      return;
    }
    const plugins = [...this.plugins].filter(([id, running]) => this.suggestPlugins.has(id) && running.ready && !running.failed);
    if (!plugins.length) return;
    const request = ++this.editorRequest;
    const query = { request, editor, pending: new Set(plugins.map(([id]) => id)), results: new Map<string, PluginSuggestion>(),
      timer: setTimeout(() => this.finishSuggestions(request), 1000) };
    this.suggestQuery = query;
    for (const [, running] of plugins) this.post(running, { type: "suggest/query", request, editor });
  }
  private finishSuggestions(request: number) {
    const query = this.suggestQuery;
    if (!query || query.request !== request || !sameEditorContext(this.editorState(), query.editor)) return;
    clearTimeout(query.timer);
    query.pending.clear();
    // Map order is plugin load order, independent of asynchronous reply order.
    const results = [...this.plugins.keys()].flatMap(plugin => query.results.get(plugin) ?? [])
      .sort((a, b) => b.priority - a.priority);
    const winner = results[0] ?? null;
    this.showSuggestion(winner);
    for (const plugin of this.suggestPlugins) if (plugin !== winner?.plugin) {
      const running = this.plugins.get(plugin);
      if (running) this.post(running, { type: "suggest/close", request });
    }
  }
  chooseSuggestion(suggestion: PluginSuggestion, id: string, keys: EditorKeys) {
    if (this.suggestion !== suggestion || !sameEditorContext(this.editorState(), suggestion) || !suggestion.items.some(item => item.id === id)) return;
    const running = this.plugins.get(suggestion.plugin);
    if (running) this.post(running, { type: "suggest/choose", request: suggestion.request, session: suggestion.session,
      generation: suggestion.generation, revision: suggestion.revision, contextRevision: suggestion.contextRevision, id, ...keys });
  }
  hasEditorMenu() { return this.editorMenuPlugins.size > 0; }
  async editorMenu(): Promise<EditorMenu[]> {
    this.closeSuggestions();
    this.closeEditorMenus();
    const epoch = this.editorMenuEpoch;
    const editor = this.editorState();
    if (!editor || editor.composing) return [];
    const menus = await Promise.all([...this.plugins].filter(([, r]) => r.plugin.kind === "obsidian" && r.ready && !r.failed).map(async ([plugin, running]) => {
      const request = ++this.editorRequest;
      return new Promise<EditorMenu | null>(resolve => {
        const timer = setTimeout(() => { this.editorMenus.delete(request); this.post(running, { type: "menu/close", request }); resolve(null); }, 750);
        this.editorMenus.set(request, { plugin, editor, resolve, timer });
        this.post(running, { type: "menu/request", request, editor });
      });
    }));
    if (epoch !== this.editorMenuEpoch) return [];
    this.shownEditorMenus = sameEditorContext(this.editorState(), editor) ? menus.filter((menu): menu is EditorMenu => !!menu) : [];
    return this.shownEditorMenus;
  }
  closeEditorMenus() {
    this.editorMenuEpoch++;
    for (const [request, pending] of this.editorMenus) {
      clearTimeout(pending.timer); pending.resolve(null);
      const running = this.plugins.get(pending.plugin);
      if (running) this.post(running, { type: "menu/close", request });
    }
    this.editorMenus.clear();
    for (const menu of this.shownEditorMenus) {
      const running = this.plugins.get(menu.plugin);
      if (running) this.post(running, { type: "menu/close", request: menu.menu });
    }
    this.shownEditorMenus = [];
  }
  chooseEditorMenu(menu: EditorMenu, index: number, keys: Omit<EditorKeys, "key"> = {}) {
    const enabled = menu.items.some(item => "index" in item ? item.index === index && !item.disabled
      : "children" in item && !item.disabled && item.children.some(child => "index" in child && child.index === index && !child.disabled));
    if (!this.shownEditorMenus.includes(menu) || !enabled || !sameEditorContext(this.editorState(), menu)) return;
    const running = this.plugins.get(menu.plugin);
    if (running) this.post(running, { type: "menu/choose", request: menu.menu, session: menu.session, generation: menu.generation,
      revision: menu.revision, contextRevision: menu.contextRevision, index, ...keys });
    this.closeEditorMenus();
  }

  /** Tells the plugins the vault's files changed (`changed`, if one note did). */
  async filesChanged(changed?: string) {
    if (!this.plugins.size) return;
    // Notes with a lasting identity are Zotero note items too.
    if ([...this.plugins.values()].some((running) => running.plugin.kind === "zotero")) void this.itemsChanged();
    const [files, metadata] = await Promise.all([this.files(), this.metadata()]);
    for (const running of this.plugins.values()) this.post(running, { type: "files", files, metadata, changed });
  }

  private async metadata(): Promise<Record<string, unknown>> {
    try {
      return JSON.parse(await backend.obsidianMetadata()) as Record<string, unknown>;
    } catch {
      return {};
    }
  }

  private async files() {
    try {
      return (await backend.vaultNotes()).map((n) => ({ path: n.path, stat: { ctime: n.modified, mtime: n.modified, size: n.size } }));
    } catch {
      return [];
    }
  }

  run(command: PluginCommand) {
    const running = this.plugins.get(command.plugin);
    if (running) this.post(running, { type: "command", id: command.id, editor: this.editorState() });
  }

  check(command: PluginCommand): Promise<boolean> {
    const running = this.plugins.get(command.plugin);
    if (!running || running.plugin.kind !== "obsidian") return Promise.resolve(false);
    const request = `check-${++this.commandCheckId}`;
    return new Promise((resolve) => {
      const timer = setTimeout(() => { this.commandChecks.delete(request); resolve(false); }, 5000);
      this.commandChecks.set(request, { plugin: command.plugin, command: command.id, resolve, timer });
      this.post(running, { type: "command/check", id: command.id, request, editor: this.editorState() });
    });
  }

  clickRibbon(ribbon: PluginRibbon) {
    const running = this.plugins.get(ribbon.plugin);
    if (running) this.post(running, { type: "ribbon", index: ribbon.index });
  }

  showSettings(id: string) {
    const running = this.plugins.get(id);
    if (running) this.post(running, { type: "settings" });
  }

  /** Closes the settings or modal on show. */
  closeShown() {
    const showing = this.state.showing;
    const running = showing ? this.plugins.get(showing.plugin) : undefined;
    if (running) this.post(running, { type: "settings-close" });
    this.show(null);
  }

  private show(showing: PluginState["showing"]) {
    for (const [id, running] of this.plugins) running.frame.classList.toggle("shown", showing?.plugin === id);
    this.update({ showing });
  }

  dismissNotice(id: number) {
    this.update({ notices: this.state.notices.filter((n) => n.id !== id) });
  }

  private notice(plugin: string, text: string, timeout: number) {
    const id = this.noticeId++;
    this.update({ notices: [...this.state.notices, { id, plugin, text }].slice(-5) });
    if (timeout > 0) window.setTimeout(() => this.dismissNotice(id), timeout);
  }

  private async receive(event: MessageEvent) {
    const running = [...this.plugins.values()].find((p) => p.frame.contentWindow === event.source);
    if (!running || typeof event.data !== "object" || event.data === null) return;
    const message = event.data as { type?: string } & Record<string, unknown>;
    const id = running.plugin.key;
    switch (message.type) {
      case "loaded": {
        if (running.plugin.kind === "zotero") {
          if (running.started) break;
          try {
            const [code] = await Promise.all([backend.zoteroPluginCode(running.plugin.id), this.itemsChanged()]);
            if (!this.baseline) throw new Error("Could not read the library for Zotero plugins.");
            if (this.plugins.get(running.plugin.key) !== running) break;
            const manifest = { id: running.plugin.id, name: running.plugin.name, version: running.plugin.version };
            running.started = true;
            this.post(running, { type: "start", manifest, code, items: running.initialItems, seq: running.initialSeq,
              selection: { seq: this.seq, itemKeys: this.selection }, tabs: running.initialTabs });
            running.startTimer = setTimeout(() => {
              if (running.ready || this.plugins.get(running.plugin.key) !== running) return;
              running.failed = true;
              running.pending = [];
              this.fail(running, "It did not finish starting within 30 seconds.");
            }, START_TIMEOUT);
          } catch (error) {
            this.fail(running, error instanceof Error ? error.message : String(error));
          }
          break;
        }
        try {
          const code = await backend.obsidianPluginCode(id);
          const manifest = { ...running.plugin.obsidian, dir: `.obsidian/plugins/${id}` };
          const userAgent = navigator.userAgent;
          const platform = {
            isMobile: this.mobile,
            isDesktop: !this.mobile,
            isMobileApp: this.mobile,
            isAndroidApp: /Android/i.test(userAgent),
            isMacOS: /Mac OS X/i.test(userAgent),
            isWin: /Windows/i.test(userAgent),
            isLinux: /Linux/i.test(userAgent) && !/Android/i.test(userAgent),
          };
          const [files, metadata] = await Promise.all([this.files(), this.metadata()]);
          this.post(running, { type: "start", manifest, code, files, metadata, platform });
        } catch (error) {
          this.fail(running, error instanceof Error ? error.message : String(error));
        }
        break;
      }
      case "ready":
        clearTimeout(running.startTimer);
        running.ready = true;
        running.failed = false;
        for (const pending of running.pending ?? []) this.post(running, pending);
        running.pending = [];
        if (running.plugin.kind === "obsidian") { this.post(running, { type: "editor/snapshot", editor: this.editorState() }); this.querySuggestions(); }
        break;
      case "menu/register":
        if (running.plugin.kind === "obsidian") this.editorMenuPlugins.add(id);
        break;
      case "decoration/result": {
        if (running.plugin.kind !== "obsidian") break;
        const result = validatePluginDecorations(id, message, this.editorState());
        if (result) publishPluginDecorations(id, result);
        break;
      }
      case "suggest/register":
        if (running.plugin.kind === "obsidian") this.suggestPlugins.add(id);
        break;
      case "suggest/trigger":
        if (this.suggestPlugins.has(id)) this.querySuggestions();
        break;
      case "suggest/close":
        if (this.suggestion?.plugin === id && message.request === this.suggestion.request) this.closeSuggestions();
        break;
      case "suggest/result": {
        const query = this.suggestQuery;
        if (!query || message.request !== query.request || !sameEditorContext(query.editor, message as unknown as EditorSnapshot) ||
          !sameEditorContext(this.editorState(), query.editor) || !this.suggestPlugins.has(id) ||
          (!query.pending.has(id) && this.suggestion?.plugin !== id)) break;
        query.pending.delete(id);
        const items = message.items;
        if (Array.isArray(items) && items.length > 0 && items.length <= 100 &&
          items.every(item => item && typeof item.id === "string" && item.id.length <= 200 && typeof item.label === "string" && item.label.length <= 500 &&
            (item.secondary === undefined || typeof item.secondary === "string" && item.secondary.length <= 500)) &&
          new Set(items.map(item => item.id)).size === items.length &&
          typeof message.priority === "number" && Number.isFinite(message.priority) &&
          Number.isSafeInteger(message.start) && Number.isSafeInteger(message.end) && (message.start as number) >= 0 &&
          (message.end as number) >= (message.start as number) && (message.end as number) <= query.editor.value.length) {
          const keys = Array.isArray(message.keys) ? message.keys.filter(k => k && typeof k.key === "string" && k.key.length <= 40 &&
            Array.isArray(k.modifiers) && k.modifiers.length <= 4 && k.modifiers.every((m: unknown) => typeof m === "string" && ["Mod", "Ctrl", "Control", "Meta", "Command", "Alt", "Shift"].includes(m))).slice(0, 100) : [];
          query.results.set(id, { plugin: id, request: query.request, session: query.editor.session, generation: query.editor.generation,
            revision: query.editor.revision, contextRevision: query.editor.contextRevision, priority: message.priority,
            start: message.start as number, end: message.end as number,
            items: items.map(item => ({ id: item.id, label: item.label, secondary: item.secondary })),
            selected: Number.isSafeInteger(message.selected) && (message.selected as number) >= 0 && (message.selected as number) < items.length ? message.selected as number : 0,
            keys });
        } else query.results.delete(id);
        if (!query.pending.size) this.finishSuggestions(query.request);
        break;
      }
      case "menu/result": {
        const pending = this.editorMenus.get(message.request as number);
        if (!pending || pending.plugin !== id || !sameEditorContext(pending.editor, message as unknown as EditorSnapshot)) break;
        clearTimeout(pending.timer); this.editorMenus.delete(message.request as number);
        const items = itemMenuEntries(message.items);
        pending.resolve(sameEditorContext(this.editorState(), pending.editor) && items.length ? { plugin: id, menu: message.request as number,
          session: pending.editor.session, generation: pending.editor.generation, revision: pending.editor.revision, contextRevision: pending.editor.contextRevision, items } : null);
        break;
      }
      case "editor/propose": {
        if (running.plugin.kind !== "obsidian") break;
        const proposal = message as unknown as EditorProposal;
        let accepted = false;
        try { accepted = this.active?.apply(proposal) ?? false; } catch { /* Invalid proposals are rejected atomically. */ }
        this.post(running, { type: accepted ? "editor/ack" : "editor/reject", operation: proposal.operation,
          reason: accepted ? undefined : "stale-or-invalid", editor: this.editorState() });
        break;
      }
      case "command/check": {
        const pending = this.commandChecks.get(String(message.request));
        if (!pending || pending.plugin !== id || pending.command !== message.id) break;
        clearTimeout(pending.timer);
        this.commandChecks.delete(String(message.request));
        pending.resolve(message.enabled === true);
        break;
      }
      case "call":
        await this.answer(running, message as unknown as Call);
        break;
      case "commands": {
        const listed = (message.commands as { id: string; name: string; editor: boolean }[]) ?? [];
        this.update({
          commands: [
            ...this.state.commands.filter((c) => c.plugin !== id),
            ...listed.map((c) => ({ ...c, name: `${running.plugin.name}: ${c.name}`, plugin: id })),
          ],
        });
        break;
      }
      case "ribbon":
        this.update({
          ribbon: [...this.state.ribbon, { plugin: id, index: Number(message.index), icon: String(message.icon ?? ""), title: String(message.title ?? "") }],
        });
        break;
      case "status":
        this.update({ status: { ...this.state.status, [id]: String(message.text ?? "") } });
        break;
      case "notice":
        this.notice(id, String(message.text ?? ""), Number(message.timeout ?? 4000));
        break;
      case "section":
        this.update({
          sections: [
            ...this.state.sections.filter((s) => !(s.plugin === id && s.paneID === message.paneID)),
            { plugin: id, paneID: String(message.paneID ?? ""), label: String(message.label ?? "") },
          ],
        });
        break;
      case "column":
        if (typeof message.key !== "string" || !message.key || typeof message.label !== "string" || !message.label) break;
        this.update({
          columns: [
            ...this.state.columns.filter((c) => !(c.plugin === id && c.key === message.key)),
            { plugin: id, key: message.key, label: message.label },
          ],
        });
        break;
      case "column-removed":
        this.update({ columns: this.state.columns.filter((c) => !(c.plugin === id && c.key === message.key)) });
        break;
      case "column-values": {
        const request = Number(message.id);
        const pending = this.columnRequests.get(request);
        if (!pending || pending.plugin !== id) break;
        this.columnRequests.delete(request);
        const values = message.values;
        pending.resolve(
          values && typeof values === "object" && !Array.isArray(values)
            ? Object.fromEntries(Object.entries(values).filter((entry): entry is [string, string] => typeof entry[1] === "string"))
            : {},
        );
        break;
      }
      case "reader-events": {
        if (running.plugin.kind !== "zotero" || !Array.isArray(message.types)) break;
        const types = READER_EVENTS.filter((type) => (message.types as unknown[]).includes(type));
        const readerEvents = { ...this.state.readerEvents };
        if (types.length) readerEvents[id] = types;
        else delete readerEvents[id];
        this.update({ readerEvents });
        break;
      }
      case "reader-rendered": {
        const request = Number(message.id);
        const view = this.readerViews.get(request);
        if (view?.plugin === id) {
          view.update(typeof message.html === "string" ? message.html : "");
          break;
        }
        const pending = this.readerRenders.get(request);
        if (!pending || pending.plugin !== id) break;
        this.readerRenders.delete(request);
        pending.resolve(typeof message.html === "string" ? message.html : "");
        break;
      }
      case "reader-menu": {
        const request = Number(message.id);
        const pending = this.readerMenus.get(request);
        if (!pending || pending.plugin !== id) break;
        this.readerMenus.delete(request);
        const items: ReaderMenuItem[] = [];
        for (const item of Array.isArray(message.items) ? message.items : []) {
          if (!item || !Number.isInteger(item.index) || item.index < 0 || typeof item.label !== "string") continue;
          items.push({ index: item.index, label: item.label, disabled: Boolean(item.disabled) });
        }
        pending.resolve(items);
        break;
      }
      case "item-menu": {
        if (typeof message.id !== "number" || !Number.isSafeInteger(message.id)) break;
        const pending = this.itemMenus.get(message.id);
        if (!pending || pending.plugin !== id) break;
        this.itemMenus.delete(message.id);
        pending.resolve(itemMenuEntries(message.items));
        break;
      }
      case "codeblock":
        this.codeLanguages.set(String(message.language ?? ""), id);
        this.update({ drawing: this.state.drawing + 1 });
        break;
      case "postprocessor":
        if (!this.postProcessors.includes(id)) this.postProcessors.push(id);
        this.update({ drawing: this.state.drawing + 1 });
        break;
      case "rendered": {
        const resolve = this.renders.get(Number(message.id));
        this.renders.delete(Number(message.id));
        resolve?.(typeof message.html === "string" ? message.html : null);
        break;
      }
      case "settings-available":
        running.info = { ...running.info, hasSettings: true };
        this.publish();
        break;
      case "show":
        this.show({ plugin: id, kind: String(message.kind ?? "") });
        break;
      case "hide":
        if (this.state.showing?.plugin === id) this.show(null);
        break;
      case "partial":
        running.info = { ...running.info, partial: [...running.info.partial, String(message.what ?? "")] };
        this.publish();
        break;
      case "error":
        this.fail(running, String(message.message ?? ""));
        break;
    }
  }

  private fail(running: Running, message: string) {
    running.info = { ...running.info, error: message.split("\n")[0] };
    this.publish();
    this.notice(running.plugin.key, `${running.plugin.name}: ${running.info.error}`, 8000);
  }

  private async answer(running: Running, call: Call) {
    const reply = (ok: boolean, value?: unknown, error?: string) => this.post(running, { type: "reply", id: call.id, ok, value, error });
    try {
      reply(true, await this.perform(running, call.method, call.args));
    } catch (error) {
      reply(false, undefined, error instanceof Error ? error.message : String(error));
    }
  }

  private async perform(running: Running, method: string, args: unknown[]): Promise<unknown> {
    const text = (i: number) => String(args[i] ?? "");
    const active = this.active;
    switch (method) {
      case "vault.read":
        if (active && text(0) === active.path) {
          const state = active.state();
          if (state) return state.value;
        }
        return backend.vaultRead(text(0));
      case "vault.write":
      case "vault.create": {
        if (!text(0).toLowerCase().endsWith(".md")) throw new Error("Plugins can only write notes here.");
        if (active && text(0) === active.path && active.state()) active.setValue(text(1));
        else await backend.vaultWrite(text(0), text(1));
        await this.filesChanged(text(0));
        return null;
      }
      case "vault.createFolder":
        await backend.vaultCreateFolder(text(0));
        return null;
      case "vault.delete":
        await backend.vaultDelete(text(0));
        await this.filesChanged();
        return null;
      case "vault.rename":
        await backend.vaultRename(text(0), text(1));
        await this.filesChanged();
        return null;
      case "saveData":
        await backend.saveObsidianPluginData(running.plugin.id, text(0));
        return null;
      case "prefs.save":
        if (running.plugin.kind !== "zotero") throw new Error("Only Zotero plugins keep preferences here.");
        await backend.saveZoteroPluginPrefs(running.plugin.id, text(0));
        return null;
      case "items.update": {
        if (running.plugin.kind !== "zotero") throw new Error("Only Zotero plugins change items here.");
        const annotation = this.baseline?.some((item) => item.key === text(0) && item.itemType === "annotation");
        if (annotation && args[2] != null && (!Array.isArray(args[2]) || args[2].some((tag) => typeof tag !== "string"))) throw new Error("Invalid field tags.");
        const tags = Array.isArray(args[2]) ? (args[2] as unknown[]).map(String) : null;
        const write = this.itemWrites.then(async () => {
          const key = text(0);
          // Only the last few seconds matter; forget older writes.
          const now = Date.now();
          for (const [written, entry] of this.writeHistory) if (now - entry.at >= 5000) this.writeHistory.delete(written);
          const history = this.writeHistory.get(key);
          const recent = history && Date.now() - history.at < 5000 ? history.plugins : [];
          if (recent.length >= 20 && recent.at(-1) !== running.plugin.key) throw new Error("Repeated alternating plugin item writes stopped; possible plugin write loop.");
          const before = this.baseline?.find((item) => item.key === key);
          this.itemRevision++;
          this.committingItem = backend.zoteroUpdateItem(key, text(1), tags);
          try { await this.committingItem; }
          finally { this.committingItem = null; this.itemRevision++; }
          const origins = this.origins.get(key) ?? new Set<string>();
          origins.add(running.plugin.key);
          this.origins.set(key, origins);
          await this.itemsChanged();
          if (normalized(before) !== normalized(this.baseline?.find((item) => item.key === key))) {
            this.writeHistory.set(key, { at: Date.now(), plugins: recent.at(-1) === running.plugin.key ? recent : [...recent, running.plugin.key] });
          }
          this.onItemsChanged();
          return null;
        });
        this.itemWrites = write.catch(() => {});
        return write;
      }

      case "notes.create":
      case "notes.update": {
        if (running.plugin.kind !== "zotero") throw new Error("Only Zotero plugins write notes here.");
        const write = this.itemWrites.then(async () => {
          let key = text(0);
          if (method === "notes.create") {
            key = await backend.zoteroCreateNote(args[0] == null ? null : text(0), text(1));
          } else {
            await backend.zoteroUpdateNote(key, text(1));
            // The writing plugin does not hear its own change back.
            const origins = this.origins.get(key) ?? new Set<string>();
            origins.add(running.plugin.key);
            this.origins.set(key, origins);
          }
          await this.itemsChanged();
          void this.filesChanged();
          return method === "notes.create" ? key : null;
        });
        this.itemWrites = write.catch(() => {});
        return write;
      }
      case "pane.selectItems": {
        if (running.plugin.kind !== "zotero") throw new Error("Only Zotero plugins select library items here.");
        if (!Array.isArray(args[0]) || args[0].some((key) => typeof key !== "string")) throw new Error("Invalid item selection.");
        const keys = [...new Set(args[0] as string[])];
        if (keys.some((key) => !this.baseline?.some((item) => item.key === key))) throw new Error("Cannot select an unknown item.");
        await this.selectItems(keys);
        this.selectionChanged(keys);
        this.sendSelection();
        return true;
      }
      case "tabs.select": {
        if (running.plugin.kind !== "zotero") throw new Error("Only Zotero plugins select reader tabs here.");
        const viewID = text(0);
        if (viewID !== "zotero-pane" && !this.readerView(viewID)) throw new Error("Cannot select an unknown tab.");
        await this.selectTab(viewID);
        this.selectedAttachmentKey = this.readerView(viewID)?.attachmentKey ?? null;
        this.resolveReaders();
        this.sendTabs();
        return true;
      }
      case "editor.setValue":
      case "editor.replaceSelection":
        throw new Error("Editor writes require a revisioned editor/propose message.");
      case "workspace.open":
        this.openNote(text(0));
        return null;
      case "markdown.render":
        return backend.vaultRender(text(1) || active?.path || "", text(0));
      case "requestUrl": {
        const request = (args[0] ?? {}) as { url?: string; method?: string; headers?: Record<string, string>; body?: string | null };
        const site = siteOf(request.url ?? "");
        if (!site) throw new Error("Plugins may only reach web addresses.");
        const sites = loadSites();
        const allowed = sites[running.plugin.key] ?? [];
        if (!allowed.includes(site)) {
          if (!this.allowSite(running.plugin.name, site)) throw new Error(`Not allowed to reach ${site}.`);
          sites[running.plugin.key] = [...allowed, site];
          storage()?.setItem(SITES_KEY, JSON.stringify(sites));
        }
        const headers = Object.entries(request.headers ?? {}).map(([name, value]) => ({ name, value: String(value) }));
        return backend.pluginFetch(request.url ?? "", request.method ?? "GET", headers, request.body ?? null);
      }
      default:
        throw new Error(`${method} is not available to plugins here.`);
    }
  }
}

export const pluginHost = new PluginHost();
