import { useCallback, useEffect, useRef, useState, type CSSProperties } from "react";
import { exit, onBackButtonPress } from "@tauri-apps/api/app";
import { translatorHost, TRANSLATORS_CHANGED } from "./lib/translators";
import { TranslatorPicker } from "./components/TranslatorPicker";
import { BrowserView, NO_BROWSER, type BrowserState } from "./components/BrowserView";
import { AddItemDialog, ItemView, LibraryView } from "./components/LibraryView";
import { DuplicatesView } from "./components/DuplicatesView";
import { FeedView } from "./components/FeedView";
import { SavedSearchDialog } from "./components/SavedSearchDialog";
import { Icon } from "./components/Icon";
import { VaultView } from "./components/VaultView";
import { FindView } from "./components/FindView";
import { CanvasView } from "./components/CanvasView";
import { DiagramView } from "./components/DiagramView";
import { PluginLayer, usePlugins } from "./components/PluginLayer";
import { pluginHost } from "./lib/plugins";
import { DrawingView } from "./components/DrawingView";
import { GraphView } from "./components/GraphView";
import { PdfView } from "./components/PdfView";
import { SearchView } from "./components/SearchView";
import { ShareDialog } from "./components/ShareDialog";
import { SettingsView } from "./components/SettingsView";
import { Splitter } from "./components/Splitter";
import { Sidebar } from "./components/Sidebar";
import { MenuButton } from "./components/MenuButton";
import { TabStrip, tabTitle } from "./components/TabStrip";
import { WriterQuestionDialog } from "./components/WriterQuestionDialog";
import { I18nProvider, resolveLanguage, useI18n } from "./i18n";
import {
  backend,
  isTauri,
  readTextFile,
  type BrowserEvent,
  type CollectionInfo,
  type FeedInfo,
  type ItemSummary,
  type LibraryInfo,
  type NewItem,
  type SavedSearch,
  type SearchField,
  type Shared,
  type WriterQuestion,
  type TagInfo,
} from "./lib/api";
import { ALL, filterFor, type Scope } from "./lib/scope";
import { newSearch } from "./lib/searches";
import { firstPages, indexPdf } from "./lib/pdf";
import { pageRecognizer } from "./lib/recognize";
import {
  NOTES_WIDTH,
  SIDEBAR_WIDTH,
  SPLIT_RATIO,
  browserStorage,
  clamp,
  loadPreferences,
  savePreferences,
  type Preferences,
  type Range,
} from "./lib/preferences";
import {
  closeInPanes,
  dropInPanes,
  focusedTab,
  goBack,
  initialPanes,
  isClosable,
  joinPanes,
  moveAcross,
  openInPanes,
  selectInPanes,
  selectedTab,
  tabId,
  visit,
  type Tab,
} from "./lib/workbench";

/** The sizes the dividers drag. */
type Resizable = "sidebarWidth" | "notesWidth" | "splitRatio";

export default function App() {
  const [preferences, setPreferences] = useState(() => loadPreferences(browserStorage()));
  useEffect(() => savePreferences(preferences, browserStorage()), [preferences]);
  // Automatic backups: a minute after starting, then every hour, each time
  // only if one is due.
  useEffect(() => {
    if (preferences.autoBackup === "off") return;
    const hours = preferences.autoBackup === "daily" ? 24 : 24 * 7;
    const run = () =>
      void backend
        .backUpIfDue(preferences.backupFolder || null, hours, 10)
        .catch((e: unknown) => console.warn("Automatic backup failed", e));
    const first = window.setTimeout(run, 60_000);
    const every = window.setInterval(run, 3_600_000);
    return () => {
      window.clearTimeout(first);
      window.clearInterval(every);
    };
  }, [preferences.autoBackup, preferences.backupFolder]);
  const language =
    preferences.language === "system"
      ? resolveLanguage(typeof navigator === "undefined" ? [] : navigator.languages ?? [navigator.language])
      : preferences.language;
  return (
    <I18nProvider language={language}>
      <Workbench preferences={preferences} onPreferences={setPreferences} />
    </I18nProvider>
  );
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function Workbench({ preferences, onPreferences }: { preferences: Preferences; onPreferences: (p: Preferences) => void }) {
  const { t } = useI18n();
  const plugins = usePlugins();
  const [panes, setPanes] = useState(initialPanes);
  const [items, setItems] = useState<ItemSummary[]>([]);
  const [collections, setCollections] = useState<CollectionInfo[]>([]);
  const [tags, setTags] = useState<TagInfo[]>([]);
  const [searches, setSearches] = useState<SavedSearch[]>([]);
  const [feeds, setFeeds] = useState<FeedInfo[]>([]);
  const [searchFields, setSearchFields] = useState<SearchField[]>([]);
  const [editingSearch, setEditingSearch] = useState<SavedSearch | null>(null);
  const [scope, setScope] = useState<Scope>(ALL);
  const [noteRequest, setNoteRequest] = useState<{ path: string; n: number } | null>(null);
  const collectionId = scope.kind === "collection" ? scope.id : null;
  const [info, setInfo] = useState<LibraryInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [browser, setBrowser] = useState<BrowserState>(NO_BROWSER);
  /** What other apps shared (Android), one dialog at a time. */
  const [shares, setShares] = useState<{ id: number; shared: Shared }[]>([]);
  /** Writing plugins asking to use the library, one question at a time. */
  const [writerQuestions, setWriterQuestions] = useState<WriterQuestion[]>([]);
  const shareCount = useRef(0);

  const reloadItems = useCallback(() => {
    const listed =
      scope.kind === "search"
        ? backend.savedSearches().then((all) => {
            const search = all.find((s) => s.id === scope.id);
            return search ? backend.runSearch(search) : [];
          })
        : backend.listItems(filterFor(scope));
    listed.then(setItems, (e) => setError(message(e)));
    backend.tags().then(setTags, (e) => setError(message(e)));
    void pluginHost.itemsChanged();
  }, [scope]);
  // A Zotero plugin that changes items shows its change.
  useEffect(() => {
    pluginHost.onItemsChanged = reloadItems;
  }, [reloadItems]);
  const reloadCollections = useCallback(() => {
    backend.listCollections().then(setCollections, (e) => setError(message(e)));
  }, []);
  const reloadSearches = useCallback(() => {
    backend.savedSearches().then(setSearches, (e) => setError(message(e)));
  }, []);
  const reloadFeeds = useCallback(() => {
    backend.feeds().then(setFeeds, (e) => setError(message(e)));
  }, []);

  useEffect(reloadItems, [reloadItems]);
  useEffect(() => {
    reloadCollections();
    reloadSearches();
    reloadFeeds();
    // The feeds are read again when the app opens; one that fails says why
    // in its own place.
    backend.refreshFeeds().then(setFeeds, () => undefined);
    backend.searchFields().then(setSearchFields, (e) => setError(message(e)));
    backend.libraryInfo().then(setInfo, (e) => setError(message(e)));
  }, [reloadCollections, reloadSearches, reloadFeeds]);

  const open = (tab: Tab) => setPanes((p) => openInPanes(p, tab));

  // What was shared before the app opened, then whatever arrives later.
  useEffect(() => {
    const receive = () =>
      backend.takeShared().then((shared) => {
        const count = shared.texts.length + shared.pdfs.length + shared.references.length + shared.unsupported.length;
        if (count > 0) setShares((all) => [...all, { id: shareCount.current++, shared }]);
      }, (e) => setError(message(e)));
    void receive();
    const unlisten = backend.onShared(() => void receive());
    return () => void unlisten.then((stop) => stop());
  }, []);
  /** The tab in front of the pane in focus. */
  const current = focusedTab(panes);
  // Phones have no tab strip: Android's back gesture, and a back button on
  // pages the bottom bar does not reach, return to the page seen before.
  const history = useRef<string[]>([]);
  const latest = useRef(panes);
  latest.current = panes;
  const currentId = tabId(current);
  useEffect(() => {
    history.current = visit(history.current, currentId);
  }, [currentId]);
  const back = useCallback(() => {
    const next = goBack(latest.current, history.current);
    if (next) setPanes(next);
    return next !== null;
  }, []);
  const previousId = [...history.current].reverse().find((h) => h !== currentId);
  const previous =
    [...panes.main.tabs, ...(panes.side?.tabs ?? [])].find((tab) => tabId(tab) === previousId && previousId !== "library") ?? null;
  useEffect(() => {
    if (!isTauri() || !/Android/i.test(navigator.userAgent)) return;
    let listener: { unregister: () => Promise<void> } | null = null;
    let gone = false;
    void onBackButtonPress(() => {
      // An open menu or dialog closes first.
      const overlays = document.querySelectorAll<HTMLElement>(".action-menu, .dialog-backdrop");
      const top = overlays[overlays.length - 1];
      if (top?.classList.contains("dialog-backdrop")) top.click();
      else if (top) top.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
      else if (!back()) void exit(0);
    }).then((handle) => {
      if (gone) void handle.unregister();
      else listener = handle;
    });
    return () => {
      gone = true;
      void listener?.unregister();
    };
  }, [back]);
  useEffect(() => {
    if (current.kind === "item") pluginHost.selectionChanged([current.id]);
  }, [current]);
  useEffect(() => {
    const readers = [...panes.main.tabs, ...(panes.side?.tabs ?? [])].flatMap((tab) =>
      tab.kind === "pdf" ? [{ attachmentKey: tab.id, parentItemKey: tab.itemId }] : []);
    pluginHost.tabsChanged(readers, current.kind === "pdf" ? current.id : null);
  }, [panes, current]);
  useEffect(() => {
    pluginHost.selectTab = async (viewID) => {
      const reader = pluginHost.readerView(viewID);
      setPanes((p) => selectInPanes(p, reader ? `pdf:${reader.attachmentKey}` : "library"));
    };
  }, []);
  useEffect(() => {
    pluginHost.selectItems = async (keys) => {
      if (!keys.length) return;
      const item = await backend.itemDetail(keys[0]);
      if (!item) throw new Error("The selected item no longer exists.");
      setPanes((p) => openInPanes(p, { kind: "item", id: item.id, title: item.title }));
    };
  }, []);
  const collection = collections.find((c) => c.id === collectionId);

  const addItem = async (item: NewItem): Promise<string | null> => {
    try {
      await backend.addItem(item, collectionId);
      reloadItems();
      return null;
    } catch (e) {
      return message(e);
    }
  };

  const addByIdentifier = async (input: string): Promise<string | null> => {
    try {
      await backend.addByIdentifier(input, collectionId);
      reloadItems();
      return null;
    } catch (e) {
      return message(e);
    }
  };

  const importFiles = async (files: File[]): Promise<string> => {
    let added = 0;
    let duplicates = 0;
    let skipped = 0;
    const problems: string[] = [];
    for (const file of files) {
      try {
        const report = await backend.importReferences(await readTextFile(file), collectionId);
        added += report.added;
        duplicates += report.duplicates;
        skipped += report.skipped;
      } catch (e) {
        problems.push(`${file.name}: ${message(e)}`);
      }
    }
    reloadItems();
    return [t("Added {added}, {duplicates} already in the library, {skipped} skipped.", { added, duplicates, skipped }), ...problems].join("\n");
  };

  /** Moves a Zotero library in and says how it went. */
  const importEndNote = async (): Promise<string | null> => {
    try {
      const report = await backend.importEndNote();
      if (!report) return null;
      reloadItems();
      if (report.notes > 0) window.dispatchEvent(new Event("paperloom:synced"));
      const lines = [
        t("Items moved in from EndNote: {added}; already here: {duplicates}; files: {files}; notes: {notes}.", {
          added: report.added,
          duplicates: report.duplicates,
          files: report.files,
          notes: report.notes,
        }),
      ];
      if (report.missingFiles) lines.push(t("Attached files not on this computer: {count}.", { count: report.missingFiles }));
      return [...lines, ...report.problems].join("\n");
    } catch (e) {
      return message(e);
    }
  };

  const importZotero = async (): Promise<string | null> => {
    try {
      const report = await backend.importZotero();
      if (!report) return null;
      reloadItems();
      reloadCollections();
      if (report.notes > 0) window.dispatchEvent(new Event("paperloom:synced"));
      const lines = [
        t("Items moved in from Zotero: {added}; already here: {duplicates}; files: {files}; annotations: {annotations}; notes: {notes}.", {
          added: report.added,
          duplicates: report.duplicates,
          files: report.files,
          annotations: report.annotations,
          notes: report.notes,
        }),
      ];
      if (report.missingFiles) lines.push(t("Attached files not on this computer: {count}.", { count: report.missingFiles }));
      if (report.fromBackup) lines.push(t("Zotero was open, so its last backup was read."));
      return [...lines, ...report.problems].join("\n");
    } catch (e) {
      return message(e);
    }
  };

  /** Asks for PDFs and adds them. */
  const importPdfs = async (): Promise<string | null> => {
    const paths = await backend.pickPdfFiles().catch((e) => {
      setError(message(e));
      return [] as string[];
    });
    return paths.length ? addPdfs(paths) : null;
  };

  /** Adds PDFs (picked, or downloaded in the browser), each recognised from
   * its first pages; says how it went. */
  const addPdfs = async (paths: string[]): Promise<string> => {
    let added = 0;
    let joined = 0;
    let unrecognised = 0;
    const problems: string[] = [];
    // Scanned papers are read from their pages' pictures, where the computer can.
    const recognize = await pageRecognizer();
    for (const path of paths) {
      const name = path.split(/[\\/]/).pop() ?? path;
      try {
        const bytes = await backend.readPickedFile(path);
        const pages = await firstPages(bytes, 2, recognize).catch((e: unknown) => {
          recognize?.problem?.(`the first pages could not be read: ${message(e)}`);
          return { text: "", title: null };
        });
        const result = await backend.importPdf(path, pages.text, pages.title, collectionId);
        void indexPdf(result.attachment.id, bytes, backend.setAttachmentText, recognize).catch(() => undefined);
        if (result.existing) joined++;
        else added++;
        if (result.matchedBy === "none") unrecognised++;
        if (result.note) problems.push(`${name}: ${result.note}`);
      } catch (e) {
        problems.push(`${name}: ${message(e)}`);
      }
    }
    reloadItems();
    const lines = [t("PDFs added: {added}; added to papers already in the library: {joined}.", { added, joined })];
    if (unrecognised) lines.push(t("Not recognised, so named after their files: {count}. Fill in their details.", { count: unrecognised }));
    return [...lines, ...problems].join("\n");
  };

  /** What the in-app browser says; the latest handler, so it sees the
   * current collection. */
  const browserUrl = useRef("");
  const detectionGeneration = useRef(0);
  const onBrowser = useRef<(event: BrowserEvent) => void>(() => undefined);
  useEffect(() => {
    onBrowser.current = (event) => {
      if (event.kind === "navigating") {
        browserUrl.current = event.url;
        detectionGeneration.current++;
        translatorHost.reset();
        setBrowser((s) => ({ ...s, url: event.url, title: "", translator: null, reading: false, hits: null, notice: null }));
      } else if (event.kind === "showing") {
        const { url, title } = event;
        if (browserUrl.current !== url) {
          detectionGeneration.current++;
          translatorHost.reset();
        }
        browserUrl.current = url;
        setBrowser((s) => ({ ...s, url, title, translator: s.url === url ? s.translator : null }));
      } else if (event.kind === "html") {
        if (event.url !== browserUrl.current) return;
        const generation = ++detectionGeneration.current;
        setBrowser((s) => ({ ...s, translator: null }));
        void translatorHost.detect(event.url, event.html).then((translator) => {
          if (generation === detectionGeneration.current && event.url === browserUrl.current) {
            setBrowser((s) => ({ ...s, translator }));
          }
        }, (error: unknown) => {
          if (generation === detectionGeneration.current) setBrowser((s) => ({ ...s, notice: error instanceof Error ? error.message : String(error) }));
        });
      } else if (event.kind === "reading") {
        setBrowser((s) => ({ ...s, reading: true, notice: null }));
      } else if (event.kind === "read") {
        const hits = event.page.hits;
        setBrowser((s) => ({ ...s, reading: false, hits }));
        setPanes((p) => openInPanes(p, { kind: "browser" }));
      } else if (event.kind === "failed") {
        const notice = event.message;
        setBrowser((s) => ({ ...s, reading: false, notice }));
      } else if (event.kind === "downloaded") {
        const adding = t("Adding {name}…", { name: event.name });
        setBrowser((s) => ({ ...s, notice: adding }));
        void addPdfs([event.path]).then((notice) => setBrowser((s) => ({ ...s, notice })));
      } else {
        const notice = t("Saved {path}.", { path: event.path });
        setBrowser((s) => ({ ...s, notice }));
      }
    };
  });
  useEffect(() => {
    let unsubscribe: (() => void) | undefined;
    let active = true;
    backend.onBrowserEvent((event) => onBrowser.current(event)).then((off) => (active ? (unsubscribe = off) : off()));
    return () => {
      active = false;
      unsubscribe?.();
    };
  }, []);

  useEffect(() => {
    const changed = () => {
      detectionGeneration.current++;
      translatorHost.reset();
      setBrowser((s) => ({ ...s, translator: null }));
      if (browserUrl.current) void backend.browserSnapshot().catch(() => undefined);
    };
    window.addEventListener(TRANSLATORS_CHANGED, changed);
    return () => { window.removeEventListener(TRANSLATORS_CHANGED, changed); translatorHost.dispose(); };
  }, []);

  // Works the browser extension added show at once.
  const reloadLatest = useRef(reloadItems);
  reloadLatest.current = reloadItems;
  useEffect(() => {
    let unsubscribe: (() => void) | undefined;
    let active = true;
    backend.onConnectorAdded(() => reloadLatest.current()).then((off) => (active ? (unsubscribe = off) : off()));
    return () => {
      active = false;
      unsubscribe?.();
    };
  }, []);
  useEffect(() => {
    let unsubscribe: (() => void) | undefined;
    let active = true;
    backend
      .onWriterQuestion((question) => setWriterQuestions((all) => [...all, question]))
      .then((off) => (active ? (unsubscribe = off) : off()));
    return () => {
      active = false;
      unsubscribe?.();
    };
  }, []);

  /** Shows a note in the Notes tab. */
  const openNote = (path: string) => {
    setNoteRequest((r) => ({ path, n: (r?.n ?? 0) + 1 }));
    open({ kind: "notes" });
  };

  /** Runs a change, then shows the list again. */
  const change = (action: Promise<unknown>) => action.then(reloadItems, (e) => setError(message(e)));

  const scopeTitle = (() => {
    switch (scope.kind) {
      case "all":
        return t("All Items");
      case "starred":
        return t("Starred");
      case "unread":
        return t("Unread");
      case "trash":
        return t("Trash");
      case "duplicates":
        return t("Duplicate Items");
      case "search":
        return searches.find((s) => s.id === scope.id)?.name ?? t("Saved Searches");
      case "feed":
        return feeds.find((f) => f.id === scope.id)?.title ?? t("Feeds");
      case "collection":
        return collection?.name ?? t("All Items");
      case "tag":
        return scope.name;
    }
  })();

  const trash = (item: ItemSummary) => {
    backend.trashItem(item.id).then(() => {
      setPanes((p) => closeInPanes(p, `item:${item.id}`));
      reloadItems();
    }, (e) => setError(message(e)));
  };

  const render = (current: Tab) => {
    switch (current.kind) {
      case "library":
        if (scope.kind === "feed") {
          const feed = feeds.find((f) => f.id === scope.id);
          return feed ? (
            <FeedView
              key={feed.id}
              feed={feed}
              collectionId={null}
              onOpenItem={(id, title) => open({ kind: "item", id, title })}
              onChanged={() => {
                reloadFeeds();
                reloadItems();
              }}
            />
          ) : null;
        }
        if (scope.kind === "duplicates") {
          return <DuplicatesView onOpen={(item) => open({ kind: "item", id: item.id, title: item.title })} onMerged={reloadItems} />;
        }
        return (
          <LibraryView
            onChanged={reloadItems}
            title={scopeTitle}
            scope={scope}
            items={items}
            tags={tags}
            onOpen={(item) => open({ kind: "item", id: item.id, title: item.title })}
            onTrash={trash}
            onStar={(item) => change(backend.setStarred(item.id, !item.starred))}
            onRestore={(item) => change(backend.restoreItem(item.id))}
            onDeleteForever={(item) => {
              setPanes((p) => closeInPanes(p, `item:${item.id}`));
              void change(backend.deleteItem(item.id));
            }}
            onEmptyTrash={() => change(backend.emptyTrash())}
            onAdd={() => setAdding(true)}
            onImport={importFiles}
            onImportPdfs={importPdfs}
            onImportZotero={importZotero}
            onImportEndNote={importEndNote}
          />
        );
      case "item":
        return (
          <ItemView
            key={current.id}
            itemId={current.id}
            tags={tags}
            onRead={(file, title) => open({ kind: "pdf", id: file.id, title, itemId: current.id })}
            onChanged={reloadItems}
            onOpenNote={openNote}
            onOpenItem={(item) => open({ kind: "item", id: item.id, title: item.title })}
          />
        );
      case "pdf":
        return <PdfView key={current.id} attachmentId={current.id} page={current.page} onOpenNote={openNote} />;
      case "find":
        return (
          <FindView
            onOpenItem={(id, title) => open({ kind: "item", id, title })}
            onOpenPdf={(id, title, page) => open({ kind: "pdf", id, title, page: page ?? undefined })}
            onOpenNote={openNote}
          />
        );
      case "search":
        return (
          <SearchView
            collectionId={collectionId}
            collectionName={collection?.name ?? null}
            onAdded={reloadItems}
            onOpenItem={(id, title) => open({ kind: "item", id, title })}
          />
        );
      case "browser":
        return (
          <BrowserView
            state={browser}
            onState={setBrowser}
            collectionId={collectionId}
            collectionName={collection?.name ?? null}
            onAdded={reloadItems}
            onOpenItem={(id, title) => open({ kind: "item", id, title })}
          />
        );
      case "graph":
        return <GraphView onOpenNote={openNote} onOpenItem={(id, title) => open({ kind: "item", id, title })} />;
      case "canvas":
        return <CanvasView key={current.path} path={current.path} onOpenNote={openNote} />;
      case "drawing":
        return <DrawingView key={current.path} path={current.path} />;
      case "diagram":
        return <DiagramView key={current.path} path={current.path} />;
      case "notes":
        return (
          <VaultView
            request={noteRequest}
            onOpenGraph={() => open({ kind: "graph" })}
            onOpenCanvas={(path) => open({ kind: "canvas", path, title: path.replace(/^.*\//, "").replace(/\.canvas$/i, "") })}
            onOpenDrawing={(path) => open({ kind: "drawing", path, title: path.replace(/^.*\//, "").replace(/\.excalidraw$/i, "") })}
            onOpenDiagram={(path) => open({ kind: "diagram", path, title: path.replace(/^.*\//, "").replace(/\.drawio(\.svg)?$/i, "") })}
            onOpenPdf={(id, title, page, itemId) => open({ kind: "pdf", id, title, page, itemId })}
            onOpenItem={(id, title) => open({ kind: "item", id, title })}
          />
        );
      case "settings":
        return (
          <SettingsView
            preferences={preferences}
            onChange={onPreferences}
            info={info}
            onSynced={() => {
              reloadItems();
              reloadCollections();
            }}
          />
        );
    }
  };

  const paneState = (pane: "main" | "side") => (pane === "side" && panes.side ? panes.side : panes.main);
  const strip = (pane: "main" | "side", vertical: boolean) => {
    const front = selectedTab(paneState(pane));
    const split =
      pane === "side"
        ? { label: t("Close Split View"), act: () => setPanes((p) => joinPanes(p)) }
        : !panes.side && isClosable(front)
          ? { label: t("Open in Split View"), act: () => setPanes((p) => moveAcross(p, tabId(front))) }
          : null;
    return (
      <TabStrip
        state={paneState(pane)}
        vertical={vertical}
        onSelect={(id) => setPanes((p) => selectInPanes(p, id))}
        onClose={(id) => setPanes((p) => closeInPanes(p, id))}
        onMove={(id, index) => setPanes((p) => dropInPanes(p, id, pane, index))}
        split={split}
      />
    );
  };
  const libraryNavigation = current.kind === "library" || current.kind === "item" || current.kind === "pdf";
  // Dragged sizes: shown while a divider moves, saved when it is let go.
  const [dragged, setDragged] = useState<Partial<Record<Resizable, number>>>({});
  const drag = useRef<{ from: number; value: number } | null>(null);
  const splitPanes = useRef<HTMLElement>(null);
  const sidebarWidth = dragged.sidebarWidth ?? preferences.sidebarWidth;
  const notesWidth = dragged.notesWidth ?? preferences.notesWidth;
  const splitRatio = dragged.splitRatio ?? preferences.splitRatio;
  const notesShown = preferences.showNotes && current.kind !== "notes";
  /** Room the editor keeps however the dividers are dragged. */
  const editorRoom = (without: Resizable) =>
    window.innerWidth -
    52 -
    (without !== "sidebarWidth" && libraryNavigation ? sidebarWidth : 0) -
    (without !== "notesWidth" && notesShown && window.innerWidth > 1000 ? notesWidth : 0) -
    360;
  const resize = (key: Resizable, range: Range, to: (from: number, dx: number) => number, max = () => range.max) => ({
    // What screen readers announce: pixels, or the split's percentage.
    value: Math.round((dragged[key] ?? preferences[key]) * (key === "splitRatio" ? 100 : 1)),
    min: Math.round(range.min * (key === "splitRatio" ? 100 : 1)),
    max: Math.round(range.max * (key === "splitRatio" ? 100 : 1)),
    onStart: () => {
      drag.current = { from: preferences[key], value: preferences[key] };
    },
    onMove: (dx: number) => {
      const d = drag.current;
      if (!d) return;
      d.value = clamp(to(d.from, dx), { min: range.min, max: Math.max(range.min, Math.min(range.max, max())) });
      setDragged({ [key]: d.value } as Partial<Record<Resizable, number>>);
    },
    onEnd: () => {
      const d = drag.current;
      drag.current = null;
      setDragged({});
      if (d && d.value !== preferences[key]) onPreferences({ ...preferences, [key]: d.value });
    },
    onReset: () => onPreferences({ ...preferences, [key]: range.initial }),
  });
  const notes = notesShown && (
    <aside className={`notes-dock ${preferences.notesEdge}`} style={{ width: notesWidth }} aria-label={t("Notes panel")}>
      <Splitter
        className={`notes-splitter ${preferences.notesEdge}`}
        label={t("Resize the notes panel")}
        {...resize(
          "notesWidth",
          NOTES_WIDTH,
          (from, dx) => (preferences.notesEdge === "right" ? from - dx : from + dx),
          () => editorRoom("notesWidth"),
        )}
      />
      <VaultView compact />
    </aside>
  );
  const nav = [
    { tab: { kind: "library" } as Tab, icon: "library" as const, label: t("Library") },
    { tab: { kind: "find" } as Tab, icon: "search" as const, label: t("Search Everything") },
    { tab: { kind: "search" } as Tab, icon: "globe" as const, label: t("Online Search") },
    { tab: { kind: "browser" } as Tab, icon: "compass" as const, label: t("Browser") },
    { tab: { kind: "notes" } as Tab, icon: "note" as const, label: t("Notes") },
    { tab: { kind: "settings" } as Tab, icon: "settings" as const, label: t("Settings") },
  ];


  return (
    <div
      className={libraryNavigation ? "app" : "app without-sidebar"}
      style={{ "--sidebar-width": `${sidebarWidth}px` } as CSSProperties}
    >
      <nav className="activity-bar" aria-label="Paperloom">
        {nav.map(({ tab, icon, label }) => (
          <button key={tab.kind} className={current.kind === tab.kind ? "selected" : ""} title={label} aria-label={label} onClick={() => open(tab)}>
            <Icon name={icon} size={22} />
          </button>
        ))}
        {plugins.ribbon.map((ribbon) => (
          <button
            key={`${ribbon.plugin}:${ribbon.index}`}
            className="plugin-ribbon"
            title={ribbon.title}
            aria-label={ribbon.title}
            onClick={() => pluginHost.clickRibbon(ribbon)}
          >
            {(ribbon.title || "?").charAt(0).toUpperCase()}
          </button>
        ))}
        <span className="spacer" />
        <button
          title={t("Notes panel")}
          aria-label={t("Notes panel")}
          aria-pressed={preferences.showNotes}
          onClick={() => onPreferences({ ...preferences, showNotes: !preferences.showNotes })}
        >
          <Icon name={preferences.notesEdge === "left" ? "panelLeft" : "panelRight"} size={20} />
        </button>
      </nav>
      {libraryNavigation && (
        <aside className="sidebar" aria-label={t("Library")}>
          <Sidebar
            collections={collections}
            searches={searches}
            feeds={feeds}
            tags={tags}
            scope={scope}
            onScope={(next) => {
              setScope(next);
              open({ kind: "library" });
            }}
            onCreate={(name, parentId) =>
              backend.createCollection(name, parentId).then(reloadCollections, (e) => setError(message(e)))
            }
            onTagColor={(name, color) => change(backend.setTagColor(name, color))}
            onRenameTag={(name, newName) => {
              if (scope.kind === "tag" && scope.name === name) setScope({ kind: "tag", name: newName.trim() });
              void change(backend.renameTag(name, newName));
            }}
            onDeleteTag={(name) => {
              if (scope.kind === "tag" && scope.name === name) setScope(ALL);
              void change(backend.deleteTag(name));
            }}
            onAddFeed={async (url) => {
              try {
                const feed = await backend.addFeed(url);
                reloadFeeds();
                setScope({ kind: "feed", id: feed.id });
                open({ kind: "library" });
                return null;
              } catch (e) {
                return message(e);
              }
            }}
            onRenameFeed={(id, title) => backend.renameFeed(id, title).then(reloadFeeds, (e) => setError(message(e)))}
            onRemoveFeed={(id) => {
              if (scope.kind === "feed" && scope.id === id) setScope(ALL);
              backend.removeFeed(id).then(reloadFeeds, (e) => setError(message(e)));
            }}
            onEditSearch={(search) =>
              setEditingSearch(
                search ??
                  newSearch(searchFields, { tags: tags.map((tag) => tag.name), collections: collections.map((c) => c.id) }),
              )
            }
          />
        </aside>
      )}
      {libraryNavigation && (
        <Splitter
          className="sidebar-splitter"
          label={t("Resize the sidebar")}
          {...resize("sidebarWidth", SIDEBAR_WIDTH, (from, dx) => from + dx, () => editorRoom("sidebarWidth"))}
        />
      )}
      <main className="workspace">
        {/* The page's title for screen readers; the sidebar shows the name. */}
        <h1 className="visually-hidden">{t("Paperloom")}</h1>
        {!panes.side && preferences.tabStyle === "vertical-left" && strip("main", true)}
        {preferences.notesEdge === "left" && notes}
        {panes.side ? (
          <section
            className="editor split"
            ref={splitPanes}
            style={{ gridTemplateColumns: `minmax(0, ${splitRatio}fr) minmax(0, ${1 - splitRatio}fr)` }}
          >
            {(["main", "side"] as const).map((pane) => (
              <div
                key={pane}
                className={panes.focus === pane ? "pane focused" : "pane"}
                onPointerDownCapture={() => panes.focus !== pane && setPanes((p) => ({ ...p, focus: pane }))}
              >
                {strip(pane, false)}
                <div className="editor-body">{render(selectedTab(paneState(pane)))}</div>
              </div>
            ))}
            {/* After the panes, so `.pane:first-child` still finds the first one. */}
            <Splitter
              className="split-splitter"
              style={{ left: `${splitRatio * 100}%` }}
              label={t("Resize the split view")}
              {...resize("splitRatio", SPLIT_RATIO, (from, dx) => from + dx / (splitPanes.current?.getBoundingClientRect().width || 1))}
            />
          </section>
        ) : (
          <section className="editor">
            {preferences.tabStyle === "horizontal" && strip("main", false)}
            {!["library", "find", "notes", "search", "browser", "settings"].includes(current.kind) && (
              <button className="phone-back" onClick={back}>
                <Icon name="back" size={18} />
                <span>{previous ? tabTitle(previous, t) : scopeTitle}</span>
              </button>
            )}
            <div className="editor-body">{render(current)}</div>
          </section>
        )}
        {preferences.notesEdge === "right" && notes}
        {!panes.side && preferences.tabStyle === "vertical-right" && strip("main", true)}
      </main>
      <footer className="status-bar">
        <span>{t("Items: {count}", { count: items.length })}</span>
        <span>{scopeTitle}</span>
        <span className="spacer" />
        {Object.entries(plugins.status)
          .filter(([, text]) => text)
          .map(([id, text]) => (
            <span key={id}>{text}</span>
          ))}
        {info && (
          <span>
            {t("Core")} {info.coreVersion}
          </span>
        )}
      </footer>
      <nav className="bottom-nav" aria-label="Paperloom">
        {nav.filter(({ tab }) => tab.kind === "library" || tab.kind === "find" || tab.kind === "notes").map(({ tab, icon, label }) => (
          <button key={tab.kind} className={current.kind === tab.kind ? "selected" : ""} aria-label={label} title={label} onClick={() => open(tab)}>
            <Icon name={icon} size={20} />
            <span>{tab.kind === "find" ? t("Search") : label}</span>
          </button>
        ))}
        <MenuButton
          label={t("More")}
          icon={<span className="nav-more-icon" aria-hidden="true">⋯</span>}
          className={["search", "browser", "settings"].includes(current.kind) ? "selected" : ""}
          items={nav.filter(({ tab }) => tab.kind === "search" || tab.kind === "browser" || tab.kind === "settings").map(({ tab, icon, label }) => ({
            label,
            icon: <Icon name={icon} size={18} />,
            checked: current.kind === tab.kind,
            onSelect: () => open(tab),
          }))}
        />
      </nav>
      {shares.length > 0 && (
        <ShareDialog
          key={shares[0].id}
          shared={shares[0].shared}
          collectionId={collectionId}
          collectionName={collection?.name ?? null}
          onAddPdfs={addPdfs}
          onAdded={reloadItems}
          onOpenItem={(id, title) => open({ kind: "item", id, title })}
          onClose={() => setShares((all) => all.slice(1))}
        />
      )}
      {writerQuestions.length > 0 && (
        <WriterQuestionDialog
          key={writerQuestions[0].id}
          question={writerQuestions[0]}
          onAnswer={(allow) => {
            void backend.answerWriter(writerQuestions[0].id, allow);
            setWriterQuestions((all) => all.slice(1));
          }}
        />
      )}
      {adding && <AddItemDialog onAdd={addItem} onAddIdentifier={addByIdentifier} onClose={() => setAdding(false)} />}
      {editingSearch && (
        <SavedSearchDialog
          search={editingSearch}
          fields={searchFields}
          collections={collections}
          tags={tags}
          onSaved={(saved) => {
            setEditingSearch(null);
            reloadSearches();
            if (scope.kind === "search" && scope.id === saved.id) reloadItems();
            else setScope({ kind: "search", id: saved.id });
            open({ kind: "library" });
          }}
          onDelete={(search) =>
            backend.deleteSavedSearch(search.id).then(() => {
              setEditingSearch(null);
              if (scope.kind === "search" && scope.id === search.id) setScope(ALL);
              reloadSearches();
            }, (e) => setError(message(e)))
          }
          onClose={() => setEditingSearch(null)}
        />
      )}
      <TranslatorPicker />
      <PluginLayer onOpenLink={(link) => openNote(/\.md$/i.test(link) ? link : `${link.split("#")[0]}.md`)} />
      {error && (
        <div className="toast" role="alert">
          <strong>{t("Something went wrong")}</strong>
          <span>{error}</span>
          <button onClick={() => setError(null)}>{t("OK")}</button>
        </div>
      )}
    </div>
  );
}
