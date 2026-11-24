import DOMPurify from "dompurify";
import { useCallback, useEffect, useRef, useState, type FormEvent, type MouseEvent, type ReactElement } from "react";
import { flushSync } from "react-dom";
import { useI18n } from "../i18n";
import { audioType, base64Of, recordingName, startRecording, type Recording } from "../lib/recorder";
import { aiSettings } from "../lib/ai";
import { pluginHost } from "../lib/plugins";
import { backend, localNow, type LinkSuggestion, type NoteInfo, type NoteMention, type NoteOutline } from "../lib/api";
import { GraphView } from "./GraphView";
import { MindMapView } from "./MindMapView";
import { Icon } from "./Icon";
import { MenuButton, type MenuAction } from "./MenuButton";
import { NoteEditor, type NoteEditorHandle } from "./NoteEditor";
import { NoteHistory } from "./NoteHistory";
import { NoteProperties } from "./NoteProperties";
import { usePlugins } from "./PluginLayer";

type Mode = "live" | "source" | "reading" | "map";

/** Whether a note's properties make it a mind map. */
const isMap = (outline: NoteOutline | null) => outline?.properties.some((p) => p.key.toLowerCase() === "mindmap") ?? false;
type Panel = "outline" | "links" | "search" | "history" | "graph";

const SAVE_DELAY = 600;

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** The core's reading-view HTML without scripts; keeps our links and colors. */
function clean(html: string): string {
  return DOMPurify.sanitize(html, {
    ALLOWED_URI_REGEXP: /^(?:(?:https?|mailto|paperloom|paperloom-vault):|[^a-z]|[a-z+.-]+(?:[^a-z+.\-:]|$))/i,
  });
}

const folderOf = (path: string) => (path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "");

interface Props {
  /** The side dock: no file tree or side panel. */
  compact?: boolean;
  /** A note to open (from search); `n` changes for every request. */
  request?: { path: string; n: number } | null;
  /** Opens a PDF at a page (an embedded highlight was clicked). */
  onOpenPdf?: (attachmentId: string, title: string, page: number, itemId: string) => void;
  /** Opens an item (a `[[@paper]]` link was clicked). */
  onOpenItem?: (itemId: string, title: string) => void;
  /** Shows the whole vault's graph. */
  onOpenGraph?: () => void;
  /** Opens a canvas. */
  onOpenCanvas?: (path: string) => void;
  /** Opens a drawing. */
  onOpenDrawing?: (path: string) => void;
  /** Opens a draw.io diagram. */
  onOpenDiagram?: (path: string) => void;
}

/**
 * The Markdown vault: a file tree, a note in live preview, source or
 * reading mode, its outline, backlinks and unlinked mentions, search, and
 * the quick switcher (Ctrl/⌘ O).
 */
export function VaultView({ compact = false, request = null, onOpenPdf, onOpenItem, onOpenGraph, onOpenCanvas, onOpenDrawing, onOpenDiagram }: Props) {
  const { t, language } = useI18n();
  // Plugins that start after a note is shown draw in it then.
  const { drawing } = usePlugins();
  const [drafting, setDrafting] = useState(false);
  const [notes, setNotes] = useState<NoteInfo[]>([]);
  const [folders, setFolders] = useState<string[]>([]);
  const [canvases, setCanvases] = useState<string[]>([]);
  const [drawings, setDrawings] = useState<string[]>([]);
  const [diagrams, setDiagrams] = useState<string[]>([]);
  const [path, setPath] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [recording, setRecording] = useState<Recording | null>(null);
  const [mode, setMode] = useState<Mode>("live");
  const [outline, setOutline] = useState<NoteOutline | null>(null);
  const [html, setHtml] = useState("");
  const [panel, setPanel] = useState<Panel>("outline");
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<NoteMention[]>([]);
  const [suggestions, setSuggestions] = useState<LinkSuggestion[]>([]);
  const [switching, setSwitching] = useState(false);
  /** Choosing a template for a new note, or to put into this one. */
  const [picking, setPicking] = useState<"new" | "insert" | null>(null);
  const [title, setTitle] = useState("");
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  /** Bumped on every open, so the editor reloads a note changed elsewhere. */
  const [version, setVersion] = useState(0);
  const editor = useRef<NoteEditorHandle>(null);
  const editable = mode === "live" || mode === "source";
  // Plugins' commands read and change the open note through the editor.
  useEffect(() => {
    if (!path || !editable) {
      pluginHost.setActiveNote(null);
      return;
    }
    pluginHost.setActiveNote({
      path,
      state: () => editor.current?.state() ?? null,
      apply: (proposal) => editor.current?.apply(proposal) ?? false,
      setValue: (value) => editor.current?.setValue(value),
    });
    return () => pluginHost.setActiveNote(null);
  }, [path, version, editable]);
  const reader = useRef<HTMLDivElement>(null);
  const pending = useRef<{ path: string; text: string; timer: number } | null>(null);

  const reloadList = useCallback(async () => {
    try {
      const [n, f, c, d, g] = await Promise.all([
        backend.vaultNotes(),
        backend.vaultFolders(),
        backend.vaultCanvases().catch(() => [] as string[]),
        backend.vaultDrawings().catch(() => [] as string[]),
        backend.vaultDiagrams().catch(() => [] as string[]),
      ]);
      setNotes(n);
      setFolders(f);
      setCanvases(c);
      setDrawings(d);
      setDiagrams(g);
      void pluginHost.filesChanged();
      return n;
    } catch (e) {
      setError(message(e));
      return [];
    }
  }, []);

  const save = useCallback(async () => {
    const write = pending.current;
    if (!write) return;
    pending.current = null;
    window.clearTimeout(write.timer);
    try {
      await backend.vaultWrite(write.path, write.text);
      void pluginHost.itemsChanged();
      setOutline(await backend.vaultOutline(write.path));
    } catch (e) {
      setError(message(e));
    }
  }, []);

  const open = useCallback(
    async (next: string, line?: number) => {
      await save();
      try {
        const content = await backend.vaultRead(next);
        setPath(next);
        setText(content);
        setVersion((v) => v + 1);
        setTitle(next.replace(/^.*\//, "").replace(/\.md$/i, ""));
        const opened = await backend.vaultOutline(next);
        setOutline(opened);
        // A map opens as a map; other notes leave the map view.
        setMode((m) => (isMap(opened) ? "map" : m === "map" ? "live" : m));
        if (line !== undefined) window.setTimeout(() => editor.current?.goToLine(line), 50);
      } catch (e) {
        setError(message(e));
      }
    },
    [save],
  );

  useEffect(() => {
    void reloadList().then((list) => {
      if (request) void open(request.path);
      else if (list.length) void open(list[0].path);
    });
    // Only on first show; later requests are handled below.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [reloadList, open]);

  useEffect(() => {
    if (request) void open(request.path);
  }, [request, open]);

  // Saves typing shortly after it stops, and before leaving the note.
  const change = (value: string) => {
    if (!path) return;
    setText(value);
    if (pending.current) window.clearTimeout(pending.current.timer);
    pending.current = { path, text: value, timer: window.setTimeout(() => void save(), SAVE_DELAY) };
  };
  useEffect(() => () => void save(), [save]);

  // Picks up edits made in other apps when the window comes back.
  useEffect(() => {
    const refresh = async () => {
      if (pending.current) return;
      if (await backend.vaultRefresh().catch(() => false)) {
        await reloadList();
        if (path) void open(path);
      }
    };
    // Notes sync wrote: reload whether or not files look changed.
    const synced = async () => {
      if (pending.current) return;
      await reloadList();
      if (path) void open(path);
    };
    window.addEventListener("focus", refresh);
    window.addEventListener("paperloom:synced", synced);
    return () => {
      window.removeEventListener("focus", refresh);
      window.removeEventListener("paperloom:synced", synced);
    };
  }, [path, reloadList, open]);

  useEffect(() => {
    if (compact) return;
    const keys = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "o") {
        event.preventDefault();
        setSwitching(true);
      }
    };
    window.addEventListener("keydown", keys);
    return () => window.removeEventListener("keydown", keys);
  }, [compact]);

  // The reading view, with embedded images loaded from the vault.
  useEffect(() => {
    if (mode !== "reading" || !path) return;
    let current = true;
    const urls: string[] = [];
    backend.vaultRender(path, text).then(async (plain) => {
      // Plugins that change notes as they are shown (Markdown post processors).
      const rendered = pluginHost.hasPostProcessors() ? await pluginHost.postProcess(plain, path) : plain;
      if (!current) return;
      // In the page before it is searched below: a render left to React's
      // own time could come after (it did in WebKitGTK), and then plugins'
      // code blocks, recordings and pictures were missed.
      flushSync(() => setHtml(clean(rendered)));
      // Code blocks a plugin draws (a Dataview query, say), as it drew them.
      for (const code of reader.current?.querySelectorAll<HTMLElement>("pre > code[class*='language-']") ?? []) {
        const language = [...code.classList].find((c) => c.startsWith("language-"))?.slice("language-".length) ?? "";
        if (!pluginHost.drawsCode(language)) continue;
        const drawn = await pluginHost.renderCodeBlock(language, code.textContent ?? "", path);
        if (!current || drawn === null) continue;
        const holder = document.createElement("div");
        holder.className = "plugin-block";
        holder.innerHTML = clean(drawn);
        code.parentElement?.replaceWith(holder);
      }
      // A recording plays where it is embedded.
      for (const holder of reader.current?.querySelectorAll<HTMLElement>(".embed-audio[data-audio]") ?? []) {
        const src = holder.dataset.audio;
        if (!src) continue;
        try {
          const url = URL.createObjectURL(new Blob([await backend.vaultFile(src)], { type: audioType(src) }));
          urls.push(url);
          const player = document.createElement("audio");
          player.controls = true;
          player.src = url;
          player.title = src;
          holder.replaceChildren(player);
        } catch {
          holder.append(` (${t("missing")})`);
        }
      }
      for (const img of reader.current?.querySelectorAll<HTMLImageElement>("img[data-src]") ?? []) {
        const src = img.dataset.src;
        if (!src) continue;
        try {
          // Pictures in other formats are recognised from their bytes; an SVG
          // (a diagram, say) shows only when it says it is one.
          const type = /\.svg$/i.test(src) ? "image/svg+xml" : "";
          const url = URL.createObjectURL(new Blob([await backend.vaultFile(src)], { type }));
          urls.push(url);
          img.src = url;
        } catch {
          img.alt = `${img.alt} (${t("missing")})`;
        }
      }
    }, (e) => setError(message(e)));
    return () => {
      current = false;
      urls.forEach((url) => URL.revokeObjectURL(url));
    };
  }, [mode, path, text, t, drawing]);

  const openHighlight = async (annotationId: string) => {
    const place = await backend.annotationLocation(annotationId).catch(() => null);
    if (place) onOpenPdf?.(place.attachmentId, place.itemTitle, place.pageIndex, place.itemId);
    else setError(t("This highlight is no longer in the library."));
  };

  const openTarget = async (target: string) => {
    if (!path) return;
    const [name, sub] = target.split("#");
    if (name.startsWith("@")) {
      if (sub?.startsWith("hl-")) {
        await openHighlight(sub.slice(3));
      } else {
        const id = await backend.resolveItemReference(name).catch(() => null);
        if (id) onOpenItem?.(id, name.slice(1));
        else setError(t("No item in the library matches {reference}.", { reference: name }));
      }
      return;
    }
    if (!name) return;
    const found = await backend.vaultResolve(name, path);
    if (found) {
      await open(found);
      if (sub) {
        const heading = (await backend.vaultOutline(found)).headings.find((h) => h.text === sub);
        if (heading) window.setTimeout(() => editor.current?.goToLine(heading.line), 80);
      }
    } else {
      const created = await backend.vaultCreateNote("", name);
      await reloadList();
      await open(created.path);
    }
  };

  const readerClick = (event: MouseEvent<HTMLDivElement>) => {
    const link = (event.target as HTMLElement).closest("a");
    if (!link) return;
    const href = link.getAttribute("href") ?? "";
    event.preventDefault();
    if (href.startsWith("paperloom:note/")) {
      const [target, anchor] = href.slice("paperloom:note/".length).split("#");
      void open(decodeURIComponent(target)).then(() => {
        if (anchor) window.setTimeout(() => reader.current?.querySelector(`#${CSS.escape(anchor)}`)?.scrollIntoView(), 120);
      });
    } else if (href.startsWith("paperloom:drawing/")) {
      onOpenDrawing?.(decodeURIComponent(href.slice("paperloom:drawing/".length)));
    } else if (href.startsWith("paperloom:diagram/")) {
      onOpenDiagram?.(decodeURIComponent(href.slice("paperloom:diagram/".length)));
    } else if (href.startsWith("paperloom:new/")) {
      void openTarget(decodeURIComponent(href.slice("paperloom:new/".length)));
    } else if (href.startsWith("paperloom:annotation/")) {
      void openHighlight(decodeURIComponent(href.slice("paperloom:annotation/".length)));
    } else if (href.startsWith("paperloom:item/")) {
      onOpenItem?.(decodeURIComponent(href.slice("paperloom:item/".length)), link.textContent ?? "");
    } else if (href.startsWith("paperloom:tag/")) {
      const tag = decodeURIComponent(href.slice("paperloom:tag/".length));
      setPanel("search");
      setQuery(`tag:${tag}`);
      void runSearch(`tag:${tag}`);
    } else if (/^https?:/.test(href)) {
      void backend.openUrl(href);
    }
  };

  const runSearch = async (words: string) => {
    try {
      setResults(await backend.vaultSearch(words, 100));
    } catch (e) {
      setError(message(e));
    }
  };

  const newNote = async (folder: string) => {
    await save();
    try {
      const created = await backend.vaultCreateNote(folder, t("Untitled"));
      await reloadList();
      await open(created.path);
    } catch (e) {
      setError(message(e));
    }
  };

  const newMap = async (folder: string) => {
    await save();
    try {
      const created = await backend.vaultCreateNote(folder, t("Mind Map"));
      await backend.vaultWrite(created.path, await backend.newMindmapText(created.title));
      await reloadList();
      await open(created.path);
    } catch (e) {
      setError(message(e));
    }
  };

  const importMap = async (folder: string) => {
    await save();
    try {
      const note = await backend.vaultImportMindmap(folder);
      if (!note) return;
      await reloadList();
      await open(note.path);
    } catch (e) {
      setError(message(e));
    }
  };

  /** A new drawing (or draw.io diagram) beside the note, embedded where the cursor is, and opened. */
  /** Starts recording the microphone, or stops and embeds the recording
   * (kept beside the note) where the cursor is. */
  const toggleRecording = async () => {
    if (!path) return;
    if (!recording) {
      try {
        setRecording(await startRecording());
      } catch (e) {
        setError(t("The microphone is not available: {reason}", { reason: message(e) }));
      }
      return;
    }
    const session = recording;
    setRecording(null);
    try {
      const { data, extension } = await session.stop();
      const made = await backend.vaultAddFile(folderOf(path), `${recordingName(new Date())}.${extension}`, await base64Of(data));
      editor.current?.insert(`![[${made}]]`);
      await reloadList();
    } catch (e) {
      setError(message(e));
    }
  };

  const insertDrawing = async (kind: "drawing" | "diagram") => {
    if (!path) return;
    try {
      const made =
        kind === "drawing"
          ? await backend.vaultCreateDrawing(folderOf(path), t("Drawing"), "blank")
          : await backend.vaultCreateDiagram(folderOf(path), t("Diagram"));
      editor.current?.insert(`![[${made}]]`);
      await reloadList();
      if (kind === "drawing") onOpenDrawing?.(made);
      else onOpenDiagram?.(made);
    } catch (e) {
      setError(message(e));
    }
  };

  /** A mind map of the open note, drafted by the AI provider, as a new note beside it. */
  const draftMap = async () => {
    if (!path) return;
    await save();
    setDrafting(true);
    try {
      const drafted = await backend.aiMindmap(aiSettings(), title, text, language);
      const created = await backend.vaultCreateNote(folderOf(path), `${title} – ${t("Mind Map")}`);
      await backend.vaultWrite(created.path, drafted);
      await reloadList();
      await open(created.path);
    } catch (e) {
      setError(message(e));
    } finally {
      setDrafting(false);
    }
  };

  const exportAs = async (format: "docx" | "latex") => {
    if (!path) return;
    await save();
    try {
      const written = await (format === "docx" ? backend.vaultExportDocx(path) : backend.vaultExportLatex(path));
      if (written) setError(t("Saved {path}.", { path: written }));
    } catch (e) {
      setError(message(e));
    }
  };

  const today = async () => {
    await save();
    try {
      const note = await backend.vaultDailyNote(localNow());
      await reloadList();
      await open(note.path);
    } catch (e) {
      setError(message(e));
    }
  };

  const applyTemplate = async (template: NoteInfo) => {
    const purpose = picking;
    setPicking(null);
    try {
      if (purpose === "new") {
        await save();
        const created = await backend.vaultCreateFromTemplate(path ? folderOf(path) : "", t("Untitled"), template.path, localNow());
        await reloadList();
        await open(created.path);
      } else if (purpose === "insert" && path) {
        const filled = await backend.vaultTemplateText(template.path, title, localNow());
        if (mode === "reading") {
          setMode("live");
          window.setTimeout(() => editor.current?.insert(filled), 80);
        } else {
          editor.current?.insert(filled);
        }
      }
    } catch (e) {
      setError(message(e));
    }
  };

  const newFolder = async () => {
    const name = window.prompt(t("Folder name"))?.trim();
    if (!name) return;
    await backend.vaultCreateFolder(name).catch((e) => setError(message(e)));
    await reloadList();
  };

  const rename = async () => {
    if (!path) return;
    const name = title.trim();
    const current = path.replace(/^.*\//, "").replace(/\.md$/i, "");
    if (!name || name === current) {
      setTitle(current);
      return;
    }
    await save();
    const folder = folderOf(path);
    const next = `${folder ? `${folder}/` : ""}${name}.md`;
    try {
      const changed = await backend.vaultRename(path, next);
      await reloadList();
      await open(next);
      if (changed) setError(t("Notes with updated links: {count}.", { count: changed }));
    } catch (e) {
      setError(message(e));
      setTitle(current);
    }
  };

  const remove = async () => {
    if (!path || !window.confirm(t("Move “{name}” to the vault’s trash?", { name: title }))) return;
    pending.current = null;
    try {
      await backend.vaultDelete(path);
      const list = await reloadList();
      setPath(null);
      setText("");
      if (list.length) await open(list[0].path);
    } catch (e) {
      setError(message(e));
    }
  };

  const chooseFolder = async () => {
    await save();
    try {
      if (await backend.vaultChooseFolder()) {
        setPath(null);
        const list = await reloadList();
        if (list.length) await open(list[0].path);
      }
    } catch (e) {
      setError(message(e));
    }
  };

  const tree = (parent: string, depth: number): ReactElement[] => {
    const childFolders = folders.filter((f) => folderOf(f) === parent);
    const childNotes = notes.filter((n) => n.folder === parent);
    const childCanvases = canvases.filter((c) => folderOf(c) === parent);
    const childDrawings = drawings.filter((d) => folderOf(d) === parent);
    const childDiagrams = diagrams.filter((d) => folderOf(d) === parent);
    return [
      ...childFolders.flatMap((folder) => [
        <li key={`folder:${folder}`}>
          <button
            className="nav-item"
            style={{ paddingInlineStart: 8 + depth * 14 }}
            onClick={() =>
              setCollapsed((c) => {
                const next = new Set(c);
                if (!next.delete(folder)) next.add(folder);
                return next;
              })
            }
            onDoubleClick={() => void newNote(folder)}
            title={t("Double-click for a new note here")}
          >
            <Icon name="folder" size={15} />
            <span>{folder.replace(/^.*\//, "")}</span>
          </button>
        </li>,
        ...(collapsed.has(folder) ? [] : tree(folder, depth + 1)),
      ]),
      ...childNotes.map((note) => (
        <li key={note.path}>
          <button
            className={note.path === path ? "nav-item selected" : "nav-item"}
            style={{ paddingInlineStart: 8 + depth * 14 }}
            onClick={() => void open(note.path)}
          >
            <Icon name="note" size={15} />
            <span>{note.title}</span>
          </button>
        </li>
      )),
      ...childCanvases.map((canvas) => (
        <li key={`canvas:${canvas}`}>
          <button className="nav-item" style={{ paddingInlineStart: 8 + depth * 14 }} onClick={() => onOpenCanvas?.(canvas)}>
            <Icon name="canvas" size={15} />
            <span>{canvas.replace(/^.*\//, "").replace(/\.canvas$/i, "")}</span>
          </button>
        </li>
      )),
      ...childDrawings.map((drawing) => (
        <li key={`drawing:${drawing}`}>
          <button className="nav-item" style={{ paddingInlineStart: 8 + depth * 14 }} onClick={() => onOpenDrawing?.(drawing)}>
            <Icon name="pen" size={15} />
            <span>{drawing.replace(/^.*\//, "").replace(/\.excalidraw$/i, "")}</span>
          </button>
        </li>
      )),
      ...childDiagrams.map((diagram) => (
        <li key={`diagram:${diagram}`}>
          <button className="nav-item" style={{ paddingInlineStart: 8 + depth * 14 }} onClick={() => onOpenDiagram?.(diagram)}>
            <Icon name="diagram" size={15} />
            <span>{diagram.replace(/^.*\//, "").replace(/\.drawio(\.svg)?$/i, "")}</span>
          </button>
        </li>
      )),
    ];
  };

  const newDiagram = async () => {
    try {
      const made = await backend.vaultCreateDiagram(path ? folderOf(path) : "", t("Diagram"));
      await reloadList();
      onOpenDiagram?.(made);
    } catch (e) {
      setError(message(e));
    }
  };

  const newDrawing = async () => {
    try {
      const made = await backend.vaultCreateDrawing(path ? folderOf(path) : "", t("Drawing"), "blank");
      await reloadList();
      onOpenDrawing?.(made);
    } catch (e) {
      setError(message(e));
    }
  };

  const newCanvas = async () => {
    try {
      const made = await backend.vaultCreateCanvas(path ? folderOf(path) : "", t("Untitled"));
      await reloadList();
      onOpenCanvas?.(made);
    } catch (e) {
      setError(message(e));
    }
  };

  useEffect(() => {
    if (panel !== "links" || !path) return;
    let current = true;
    backend.vaultLinkSuggestions(path, 8).then(
      (found) => current && setSuggestions(found),
      () => current && setSuggestions([]),
    );
    return () => {
      current = false;
    };
  }, [panel, path, version]);

  /** Puts a suggested link at the cursor. */
  const linkTo = (suggestion: LinkSuggestion) => {
    if (mode === "reading") {
      setMode("live");
      window.setTimeout(() => editor.current?.insert(suggestion.insert), 80);
    } else {
      editor.current?.insert(suggestion.insert);
    }
    setSuggestions((all) => all.filter((s) => s !== suggestion));
  };

  const mentionList = (items: NoteMention[], empty: string) =>
    items.length === 0 ? (
      <p className="muted">{empty}</p>
    ) : (
      <ul className="mentions">
        {items.map((m) => (
          <li key={`${m.path}:${m.line}`}>
            <button className="link" onClick={() => void open(m.path, m.line)}>
              {m.title}
            </button>
            <p>{m.context}</p>
          </li>
        ))}
      </ul>
    );

  const modes = ([
    ["live", "Live Preview", "edit"],
    ["source", "Source", "doc"],
    ["reading", "Reading View", "note"],
    ["map", "Mind Map", "mindmap"],
  ] as const).filter(([id]) => id !== "map" || isMap(outline));
  const noteActions: MenuAction[] = [
    ...(path ? [{ label: t("Insert Template"), icon: <Icon name="template" size={15} />, onSelect: () => setPicking("insert") }] : []),
    ...(path && !compact && (mode === "live" || mode === "source") ? [
      { label: t("Insert Drawing"), icon: <Icon name="pen" size={15} />, onSelect: () => void insertDrawing("drawing") },
      { label: t("Insert Diagram"), icon: <Icon name="diagram" size={15} />, onSelect: () => void insertDrawing("diagram") },
      { label: recording ? t("Stop Recording") : t("Record Audio"), icon: <Icon name="mic" size={15} />, checked: recording !== null, onSelect: () => void toggleRecording() },
    ] : []),
    ...(path && !compact && !isMap(outline) ? [
      { label: t("Mind Map with AI"), icon: <Icon name="mindmap" size={15} />, disabled: drafting, onSelect: () => void draftMap() },
    ] : []),
    ...(path && !compact ? [
      { label: t("Export as Word…"), icon: <Icon name="doc" size={15} />, onSelect: () => void exportAs("docx") },
      { label: t("Export as LaTeX…"), icon: <Icon name="latex" size={15} />, onSelect: () => void exportAs("latex") },
      { label: t("Move to Trash"), icon: <Icon name="trash" size={15} />, onSelect: () => void remove() },
    ] : []),
  ];
  // A narrow window hides the file tree and its New menu, so this menu
  // carries everything that menu creates.
  const narrowActions: MenuAction[] = compact ? [] : [
    { label: t("New Note"), icon: <Icon name="note" size={15} />, onSelect: () => void newNote(path ? folderOf(path) : "") },
    { label: t("Today's Note"), icon: <Icon name="calendar" size={15} />, onSelect: () => void today() },
    { label: t("New from Template"), icon: <Icon name="template" size={15} />, onSelect: () => setPicking("new") },
    { label: t("New Folder"), icon: <Icon name="folder" size={15} />, onSelect: () => void newFolder() },
    { label: t("New Mind Map"), icon: <Icon name="mindmap" size={15} />, onSelect: () => void newMap(path ? folderOf(path) : "") },
    { label: t("Import Mind Map…"), icon: <Icon name="import" size={15} />, onSelect: () => void importMap(path ? folderOf(path) : "") },
    ...(onOpenDiagram ? [{ label: t("New Diagram"), icon: <Icon name="diagram" size={15} />, onSelect: () => void newDiagram() }] : []),
    ...(onOpenDrawing ? [{ label: t("New Drawing"), icon: <Icon name="pen" size={15} />, onSelect: () => void newDrawing() }] : []),
    ...(onOpenCanvas ? [{ label: t("New Canvas"), icon: <Icon name="canvas" size={15} />, onSelect: () => void newCanvas() }] : []),
    ...(onOpenGraph ? [{ label: t("Graph"), icon: <Icon name="graph" size={15} />, onSelect: onOpenGraph }] : []),
    ...noteActions,
  ];

  return (
    <div className={compact ? "vault-view compact" : "vault-view"}>
      {!compact && (
        <nav className="vault-tree" aria-label={t("Notes")}>
          <header>
            <h2>{t("Notes")}</h2>
            <MenuButton
              label={t("New")}
              icon={<Icon name="plus" size={15} />}
              items={[
                { label: t("New Note"), onSelect: () => void newNote(path ? folderOf(path) : "") },
                { label: t("New Folder"), onSelect: () => void newFolder() },
                { label: t("New from Template"), onSelect: () => setPicking("new") },
                { label: t("Today's Note"), onSelect: () => void today() },
                { label: t("New Mind Map"), onSelect: () => void newMap(path ? folderOf(path) : "") },
                { label: t("Import Mind Map…"), onSelect: () => void importMap(path ? folderOf(path) : "") },
                ...(onOpenDiagram ? [{ label: t("New Diagram"), onSelect: () => void newDiagram() }] : []),
                ...(onOpenDrawing ? [{ label: t("New Drawing"), onSelect: () => void newDrawing() }] : []),
                ...(onOpenCanvas ? [{ label: t("New Canvas"), onSelect: () => void newCanvas() }] : []),
              ]}
            />
            {onOpenGraph && (
              <button className="icon-button" title={t("Graph")} aria-label={t("Graph")} onClick={onOpenGraph}>
                <Icon name="graph" size={15} />
              </button>
            )}
          </header>
          <ul>{tree("", 0)}</ul>
          <button className="link vault-folder" onClick={() => void chooseFolder()}>
            {t("Open Another Vault…")}
          </button>
        </nav>
      )}
      <section className="vault-note">
        <header className="note-header">
          <button className="icon-button" title={t("Quick Switcher")} aria-label={t("Quick Switcher")} onClick={() => setSwitching(true)}>
            <Icon name="search" size={15} />
          </button>
          <input
            className="note-title"
            value={title}
            disabled={!path}
            aria-label={t("Title")}
            onChange={(e) => setTitle(e.target.value)}
            onBlur={() => void rename()}
            onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
          />
          <div className="segmented note-modes" role="radiogroup" aria-label={t("Mode")}>
            {modes.map(([id, label, icon]) => (
              <button key={id} role="radio" aria-checked={mode === id} title={t(label)} aria-label={t(label)} className={mode === id ? "selected" : ""} onClick={() => setMode(id)}>
                <Icon name={icon} size={15} />
                <span>{t(label)}</span>
              </button>
            ))}
          </div>
          <MenuButton label={t("Mode")} icon={<Icon name={modes.find(([id]) => id === mode)?.[2] ?? "edit"} size={15} />} iconOnly className="note-mode-menu" items={modes.map(([id, label]) => ({ label: t(label), checked: mode === id, onSelect: () => setMode(id) }))} />
          <MenuButton label={t("More")} icon={<span aria-hidden="true">⋯</span>} iconOnly className="note-more-wide" items={noteActions} disabled={!path} />
          {!compact && <MenuButton label={t("More")} icon={<span aria-hidden="true">⋯</span>} iconOnly className="note-more-narrow" items={narrowActions} />}
        </header>
        {error && (
          <p className="notice" role="status">
            {error}
            <button className="icon-button" aria-label={t("OK")} onClick={() => setError(null)}>
              <Icon name="close" size={12} />
            </button>
          </p>
        )}
        {!path ? (
          <div className="empty">
            <Icon name="note" size={40} />
            <h3>{t("No Notes Yet")}</h3>
            <button className="primary" onClick={() => void newNote("")}>
              {t("New Note")}
            </button>
          </div>
        ) : mode === "map" && isMap(outline) ? (
          <MindMapView
            key={path}
            text={text}
            title={title}
            onChange={change}
            onOpenLink={(link, web) => void (web ? backend.openUrl(link).catch((e: unknown) => setError(message(e))) : openTarget(link))}
          />
        ) : mode === "reading" ? (
          <article className="markdown reader" ref={reader} onClick={readerClick} dangerouslySetInnerHTML={{ __html: html }} />
        ) : (
          <NoteEditor
            key={`${path}:${version}`}
            ref={editor}
            initialText={text}
            live={mode === "live"}
            onChange={change}
            onEditorUpdate={(origin, userEvent, changes) => pluginHost.editorUpdated(origin, userEvent, changes)}
            onOpenLink={(target) => void openTarget(target)}
            complete={(q) => backend.vaultLinkCompletions(path, q, 20)}
            cite={(q) => backend.citationCompletions(q, 20)}
          />
        )}
      </section>
      {!compact && path && (
        <div className="vault-panel">
          <div className="segmented vault-panel-tabs" role="tablist" aria-label={t("Notes")}>
            {(
              [
                ["outline", "Outline", "doc"],
                ["links", "Backlinks", "link"],
                ["search", "Search", "search"],
                ["history", "History", "restore"],
                ["graph", "Graph", "graph"],
              ] as const
            ).map(([id, label, icon]) => (
              <button key={id} role="tab" aria-selected={panel === id} title={t(label)} aria-label={t(label)} className={panel === id ? "selected" : ""} onClick={() => setPanel(id)}>
                <Icon name={icon} size={16} />
              </button>
            ))}
          </div>
          {panel === "outline" && outline && (
            <div className="panel-body">
              <NoteProperties
                path={path}
                properties={outline.properties}
                beforeSave={save}
                onSaved={() => void open(path)}
                onError={setError}
              />
              {outline.headings.length === 0 && <p className="muted">{t("No headings")}</p>}
              <ul className="outline">
                {outline.headings.map((h) => (
                  <li key={`${h.line}`} style={{ paddingInlineStart: (h.level - 1) * 12 }}>
                    <button className="link" onClick={() => (mode === "reading" ? reader.current?.querySelector(`#${CSS.escape(h.anchor)}`)?.scrollIntoView() : editor.current?.goToLine(h.line))}>
                      {h.text}
                    </button>
                  </li>
                ))}
              </ul>
              {outline.tags.length > 0 && (
                <p className="note-tags">
                  {outline.tags.map((tag) => (
                    <span key={tag} className="tag-chip">
                      #{tag}
                    </span>
                  ))}
                </p>
              )}
            </div>
          )}
          {panel === "links" && outline && (
            <div className="panel-body">
              <h3>{t("Linked mentions")}</h3>
              {mentionList(outline.backlinks, t("No notes link here yet."))}
              <h3>{t("Unlinked mentions")}</h3>
              {mentionList(outline.unlinkedMentions, t("No other notes mention this title."))}
              <h3>{t("Suggested links")}</h3>
              {suggestions.length === 0 ? (
                <p className="muted">{t("Nothing related found yet.")}</p>
              ) : (
                <ul className="mentions">
                  {suggestions.map((s) => (
                    <li key={s.path ?? s.itemId ?? s.insert}>
                      <button
                        className="link"
                        onClick={() => (s.path ? void open(s.path) : s.itemId ? onOpenItem?.(s.itemId, s.title) : undefined)}
                      >
                        {s.title}
                      </button>
                      <button className="suggest-link" title={s.insert} onClick={() => linkTo(s)}>
                        {t("Link")}
                      </button>
                      <p dangerouslySetInnerHTML={{ __html: DOMPurify.sanitize(s.snippetHtml, { ALLOWED_TAGS: ["b"] }) }} />
                    </li>
                  ))}
                </ul>
              )}
            </div>
          )}
          {panel === "history" && (
            <div className="panel-body">
              <NoteHistory path={path} beforeRestore={save} onRestored={() => void open(path)} onError={setError} />
            </div>
          )}
          {panel === "graph" && (
            <div className="panel-body graph-panel">
              <GraphView
                compact
                center={path}
                depth={1}
                onOpenNote={(next) => void open(next)}
                onOpenItem={(id, name) => onOpenItem?.(id, name)}
              />
            </div>
          )}
          {panel === "search" && (
            <div className="panel-body">
              <form
                className="search-field"
                onSubmit={(e: FormEvent) => {
                  e.preventDefault();
                  void runSearch(query);
                }}
              >
                <Icon name="search" size={14} />
                <input type="search" value={query} placeholder={t("Words, tag:x, path:x")} onChange={(e) => setQuery(e.target.value)} />
              </form>
              {mentionList(results, query ? t("No matches") : "")}
            </div>
          )}
        </div>
      )}
      {switching && <QuickSwitcher canvases={onOpenCanvas ? canvases : []} onOpenCanvas={onOpenCanvas} onClose={() => setSwitching(false)} onOpen={(p) => void open(p)} onCreate={(name) => void openTarget(name)} />}
      {picking && <TemplatePicker onClose={() => setPicking(null)} onPick={(template) => void applyTemplate(template)} />}
    </div>
  );
}

/** The notes in the templates folder, to start a note from or put in one. */
function TemplatePicker({ onClose, onPick }: { onClose: () => void; onPick: (template: NoteInfo) => void }) {
  const { t } = useI18n();
  const [templates, setTemplates] = useState<NoteInfo[] | null>(null);
  const [folder, setFolder] = useState("Templates");
  useEffect(() => {
    backend.vaultTemplates().then(setTemplates, () => setTemplates([]));
    backend.vaultSettings().then((s) => setFolder(s.templatesFolder), () => undefined);
  }, []);
  return (
    <div className="dialog-backdrop" role="presentation" onClick={onClose}>
      <div className="dialog switcher" role="dialog" aria-modal="true" aria-label={t("Templates")} onClick={(e) => e.stopPropagation()}>
        <h3>{t("Templates")}</h3>
        {templates !== null && templates.length === 0 && (
          <p className="muted">{t("No templates yet. Notes in the “{folder}” folder can be used as templates.", { folder })}</p>
        )}
        <ul>
          {(templates ?? []).map((template) => (
            <li key={template.path}>
              <button onClick={() => onPick(template)}>
                <strong>{template.title}</strong>
              </button>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}

/** Finds a note by name as you type; Enter opens the first, or creates it. */
function QuickSwitcher({ canvases, onOpenCanvas, onClose, onOpen, onCreate }: { canvases: string[]; onOpenCanvas?: (path: string) => void; onClose: () => void; onOpen: (path: string) => void; onCreate: (name: string) => void }) {
  const { t } = useI18n();
  const [query, setQuery] = useState("");
  const [found, setFound] = useState<NoteInfo[]>([]);
  const [index, setIndex] = useState(0);
  const boards = canvases.filter((path) => path.toLowerCase().includes(query.trim().toLowerCase())).slice(0, 20);
  const count = found.length + boards.length;
  useEffect(() => {
    let current = true;
    backend.vaultSwitcher(query, 20).then((list) => {
      if (current) {
        setFound(list);
        setIndex(0);
      }
    });
    return () => {
      current = false;
    };
  }, [query]);
  const choose = (i: number) => {
    const note = found[i];
    if (note) onOpen(note.path);
    else if (boards[i - found.length]) onOpenCanvas?.(boards[i - found.length]);
    else if (query.trim()) onCreate(query.trim());
    onClose();
  };
  return (
    <div className="dialog-backdrop" role="presentation" onClick={onClose}>
      <div className="dialog switcher" role="dialog" aria-modal="true" aria-label={t("Quick Switcher")} onClick={(e) => e.stopPropagation()}>
        <input
          autoFocus
          value={query}
          placeholder={t("Find or create a note…")}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape") onClose();
            else if (e.key === "ArrowDown") setIndex((i) => Math.min(i + 1, Math.max(0, count - 1)));
            else if (e.key === "ArrowUp") setIndex((i) => Math.max(i - 1, 0));
            else if (e.key === "Enter") choose(index);
          }}
        />
        <ul>
          {found.map((note, i) => (
            <li key={note.path}>
              <button className={i === index ? "selected" : ""} onMouseEnter={() => setIndex(i)} onClick={() => choose(i)}>
                <strong>{note.title}</strong>
                {note.folder && <span className="muted"> {note.folder}</span>}
              </button>
            </li>
          ))}
          {boards.map((path, i) => (
            <li key={`canvas:${path}`}>
              <button className={i + found.length === index ? "selected" : ""} onMouseEnter={() => setIndex(i + found.length)} onClick={() => choose(i + found.length)}>
                <Icon name="canvas" size={15} />
                <strong>{path}</strong>
              </button>
            </li>
          ))}
          {count === 0 && query.trim() && (
            <li>
              <button className="selected" onClick={() => choose(-1)}>
                {t("Create “{name}”", { name: query.trim() })}
              </button>
            </li>
          )}
        </ul>
      </div>
    </div>
  );
}
