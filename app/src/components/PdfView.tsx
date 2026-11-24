import { TextLayer } from "pdfjs-dist";
import "pdfjs-dist/web/pdf_viewer.css";
import DOMPurify from "dompurify";
import { createPortal } from "react-dom";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type FormEvent, type MouseEvent, type ReactNode } from "react";
import { useI18n } from "../i18n";
import { MenuButton, type MenuAction } from "./MenuButton";
import { usePlugins } from "./PluginLayer";
import { pluginHost, type ReaderEvent, type ReaderMenu } from "../lib/plugins";
import {
  backend,
  type AiAnswer,
  type AiTask,
  type AnnotationInfo,
  type AnnotationKind,
  type AnnotationPoint,
  type AnnotationRect,
  type PageEdit,
  type PdfFormField,
} from "../lib/api";
import {
  arrowHead,
  intersects,
  loadSignature,
  MARK_COLORS,
  mergeLines,
  openPdf,
  pageTexts,
  scannedPageLines,
  placeSignature,
  rectFromPoints,
  saveSignature,
  STAMP_WORDS,
  stampFontSize,
  stampRect,
  type PageViewport,
  type PDFDocumentProxy,
  type Signature,
} from "../lib/pdf";
import { pageRecognizer } from "../lib/recognize";
import { browserStorage } from "../lib/preferences";
import { aiSettings } from "../lib/ai";
import { AiAnswerView } from "./AiAnswer";
import { Icon } from "./Icon";
import { SignatureDialog } from "./SignatureDialog";

interface Size {
  width: number;
  height: number;
}

/** What the pointer does on the pages: select text, or draw a kind of mark. */
type Tool = "select" | "ink" | "text" | "square" | "circle" | "arrow" | "stamp" | "sign";
type ShapeKind = "square" | "circle" | "arrow";

/** A text box being written or changed. */
interface TextDraft {
  pageIndex: number;
  /** Its top-left corner, in PDF points. */
  x: number;
  y: number;
  /** The mark it changes; null for a new box. */
  markId: string | null;
  value: string;
  color: string;
  fontSize: number;
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Reads a PDF attachment and marks it up. Pages render when they come near
 * the screen; marks are stored in the library in PDF coordinates, so the
 * Apple apps show the same ones.
 */
interface OutlineEntry {
  title: string;
  level: number;
  /** From 0; null when the entry points nowhere in the document. */
  page: number | null;
}

/** The PDF's table of contents, flattened, with the page each entry opens. */
async function readOutline(doc: PDFDocumentProxy): Promise<OutlineEntry[]> {
  const entries: OutlineEntry[] = [];
  type Node = { title: string; dest: unknown; items: Node[] };
  const walk = async (nodes: Node[], level: number) => {
    for (const node of nodes) {
      let page: number | null = null;
      try {
        const dest = typeof node.dest === "string" ? await doc.getDestination(node.dest) : node.dest;
        if (Array.isArray(dest) && dest[0]) {
          page = typeof dest[0] === "number" ? dest[0] : await doc.getPageIndex(dest[0]);
        }
      } catch {
        page = null;
      }
      entries.push({ title: node.title, level, page });
      await walk(node.items ?? [], level + 1);
    }
  };
  await walk(((await doc.getOutline()) ?? []) as unknown as Node[], 0);
  return entries;
}

interface ViewProps {
  attachmentId: string;
  /** A page to show (from 0), e.g. from a search result or a note. */
  page?: number;
  /** Opens a note (the literature note made from this PDF). */
  onOpenNote?: (path: string) => void;
}

interface ReaderPopupData {
  x: number;
  y: number;
  /** Over selected text: the reader's own actions come first. */
  selection?: boolean;
  html?: ReaderFragment[];
  items?: MenuAction[];
}

function readerMenuActions(menus: ReaderMenu[]): MenuAction[] {
  return menus.flatMap(({ plugin, menu, items }) => items.map((item) => ({
    label: item.label,
    disabled: item.disabled,
    onSelect: () => pluginHost.readerCommand(plugin, menu, item.index),
  })));
}

type ReaderFragment = { plugin: string; render: number; html: string };

function ReaderFragmentView({ entry }: { entry: ReaderFragment }) {
  const forward = (event: React.SyntheticEvent) => {
    const target = event.target as HTMLInputElement;
    const node = target.closest("[data-reader-node]")?.getAttribute("data-reader-node");
    if (!node) return;
    const key = event as React.KeyboardEvent;
    pluginHost.readerEvent(entry.plugin, entry.render, node, { event: event.type, value: target.value,
      checked: target.checked, key: key.key, ctrlKey: key.ctrlKey, metaKey: key.metaKey, shiftKey: key.shiftKey });
  };
  return <div className="plugin-block" onClick={forward} onDoubleClick={forward} onInput={forward} onChange={forward} onKeyDown={forward}
    dangerouslySetInnerHTML={{ __html: DOMPurify.sanitize(entry.html) }} />;
}

/** Reader fragments follow the same sanitization as item pane sections. */
function ReaderHtml({ event, attachmentKey, itemKey, params = {}, className = "" }: {
  event: ReaderEvent; attachmentKey: string; itemKey: string | null; params?: object; className?: string;
}) {
  const { readerEvents } = usePlugins();
  const [html, setHtml] = useState<ReaderFragment[]>([]);
  const serialized = JSON.stringify(params);
  useEffect(() => {
    setHtml([]);
    return pluginHost.watchReader(event, attachmentKey, itemKey, JSON.parse(serialized), setHtml);
  }, [event, attachmentKey, itemKey, serialized, readerEvents]);
  if (!html.some((entry) => entry.html)) return null;
  return <div className={className}>{html.map((entry) => (
    <ReaderFragmentView key={entry.plugin} entry={entry} />
  ))}</div>;
}

function AnnotationPluginMenu({ mark, itemKey }: { mark: AnnotationInfo; itemKey: string | null }) {
  const { t } = useI18n();
  const { readerEvents } = usePlugins();
  const [menus, setMenus] = useState<ReaderMenu[]>([]);
  const request = useRef(0);
  useEffect(() => {
    request.current++;
    setMenus([]);
    return () => { request.current++; };
  }, [mark, itemKey, readerEvents]);
  if (!Object.values(readerEvents).some((types) => types.includes("createAnnotationContextMenu"))) return null;
  return <MenuButton label={t("More")} icon={<span aria-hidden="true">⋯</span>} iconOnly
    items={readerMenuActions(menus)} onOpen={() => {
      const id = ++request.current;
      setMenus([]);
      void pluginHost.readerMenu("createAnnotationContextMenu", mark.attachmentId, itemKey, { ids: [mark.id], currentID: mark.id }).then((result) => {
        if (id === request.current) setMenus(result);
      });
    }} />;
}

/** The popup stays within the window and outside the scrolling pages. */
function ReaderPopup({ popup, onClose, children }: { popup: ReaderPopupData; onClose: () => void; children?: ReactNode }) {
  const { t } = useI18n();
  const element = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ left: popup.x, top: popup.y });
  useLayoutEffect(() => {
    const box = element.current?.getBoundingClientRect();
    if (!box) return;
    setPosition({
      left: Math.max(8, Math.min(popup.x, window.innerWidth - box.width - 8)),
      top: Math.max(8, Math.min(popup.y, window.innerHeight - box.height - 8)),
    });
    if (popup.items) element.current?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
  }, [popup]);
  return createPortal(
    <div ref={element} className={`pdf-reader-popup ${popup.items ? "action-menu" : "pdf-reader-selection"}`}
      role={popup.items ? "menu" : "dialog"} aria-label={t("More")} style={position}
      onMouseDown={(event) => event.preventDefault()}
      onKeyDown={(event) => {
        if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key) && popup.items) {
          event.preventDefault();
          const buttons = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>("button:not(:disabled)"));
          const current = buttons.indexOf(document.activeElement as HTMLButtonElement);
          const index = event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1
            : (current + (event.key === "ArrowDown" ? 1 : -1) + buttons.length) % buttons.length;
          buttons[index]?.focus();
        } else if (event.key === "Escape" || event.key === "Tab") onClose();
      }}>
      {children}
      {popup.html?.map((entry) => <ReaderFragmentView key={entry.plugin} entry={entry} />)}
      {popup.items?.map((item, index) => <button key={index} type="button" role="menuitem" disabled={item.disabled}
        onClick={() => { onClose(); item.onSelect(); }}><span>{item.label}</span></button>)}
    </div>, document.body,
  );
}

/** Every successful annotation mutation refreshes the plugin projection in commit order. */
const annotationBackend = {
  addAnnotation: (...args: Parameters<typeof backend.addAnnotation>) =>
    pluginHost.annotationMutation(() => backend.addAnnotation(...args)),
  updateAnnotation: (mark: AnnotationInfo, patch: { color?: string; comment?: string | null }) =>
    pluginHost.annotationMutation(async () => {
      const current = (await backend.annotations(mark.attachmentId)).find((item) => item.id === mark.id);
      if (!current) throw new Error("Annotation no longer exists");
      return backend.updateAnnotation(mark.id, patch.color ?? current.color,
        patch.comment === undefined ? current.comment : patch.comment);
    }),
  removeAnnotation: (...args: Parameters<typeof backend.removeAnnotation>) =>
    pluginHost.annotationMutation(() => backend.removeAnnotation(...args)),
};

export function PdfView({ attachmentId, page, onOpenNote }: ViewProps) {
  const { t, language } = useI18n();
  const { readerEvents } = usePlugins();
  const [readerPopup, setReaderPopup] = useState<ReaderPopupData | null>(null);
  const popupRequest = useRef(0);
  const popupSelection = useRef<{ range: Range; text: string } | null>(null);
  const releasePopup = useRef<(() => void) | null>(null);
  const closeReaderPopup = useCallback(() => {
    releasePopup.current?.();
    releasePopup.current = null;
    popupRequest.current++;
    popupSelection.current = null;
    setReaderPopup(null);
  }, []);
  const scroller = useRef<HTMLDivElement>(null);
  const viewports = useRef(new Map<number, PageViewport>());
  const texts = useRef(new Map<number, string>());
  const [doc, setDoc] = useState<PDFDocumentProxy | null>(null);
  const [sizes, setSizes] = useState<Size[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [marks, setMarks] = useState<AnnotationInfo[]>([]);
  const editingComments = useRef(new Map<string, string>());
  const commentInputs = useRef(new Map<string, HTMLTextAreaElement>());
  const refreshMarks = useRef<() => void>(() => {});
  const [color, setColor] = useState(MARK_COLORS[0].hex);
  const [zoom, setZoom] = useState(1);
  const [width, setWidth] = useState(0);
  const [current, setCurrent] = useState(1);
  const [query, setQuery] = useState("");
  const [found, setFound] = useState<number | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [showMarks, setShowMarks] = useState(false);
  const [showAi, setShowAi] = useState(false);
  const [question, setQuestion] = useState("");
  const [answer, setAnswer] = useState<AiAnswer | null>(null);
  const [asking, setAsking] = useState(false);
  const [aiError, setAiError] = useState<string | null>(null);
  const [twoUp, setTwoUp] = useState(false);
  const [night, setNight] = useState(false);
  const [showOutline, setShowOutline] = useState(false);
  const [outline, setOutline] = useState<OutlineEntry[]>([]);
  const [itemId, setItemId] = useState<string | null>(null);
  /** Bumped when the file changes (its pages were edited), to read it again. */
  const [version, setVersion] = useState(0);
  /** Selecting text, or drawing marks with the pen or a shape. */
  const [tool, setTool] = useState<Tool>("select");
  const [textDraft, setTextDraft] = useState<TextDraft | null>(null);
  const [stampWord, setStampWord] = useState(STAMP_WORDS[0]);
  const [signature, setSignature] = useState<Signature | null>(() => loadSignature(browserStorage()));
  const [askingSignature, setAskingSignature] = useState(false);
  /** The PDF's form, and what has been filled in since it was saved. */
  const [fields, setFields] = useState<PdfFormField[]>([]);
  const [filled, setFilled] = useState<Record<string, string>>({});

  useEffect(() => {
    let cancelled = false;
    let request = 0;
    const reload = () => {
      const current = ++request;
      void backend.annotations(attachmentId).then((saved) => {
        if (cancelled || current !== request) return;
        // Inputs are uncontrolled so an active draft survives a committed refresh.
        for (const mark of saved) {
          const input = commentInputs.current.get(mark.id);
          if (input && !editingComments.current.has(mark.id)) input.value = mark.comment ?? "";
        }
        setMarks(saved);
      }).catch((error) => { if (!cancelled) setNotice(message(error)); });
    };
    refreshMarks.current = reload;
    const unsubscribe = pluginHost.subscribeAnnotations(attachmentId, reload);
    reload();
    return () => { cancelled = true; unsubscribe(); };
  }, [attachmentId]);

  useEffect(() => {
    closeReaderPopup();
    return closeReaderPopup;
  }, [attachmentId, itemId, readerEvents, closeReaderPopup]);

  useEffect(() => {
    const outside = (event: Event) => {
      if (!(event.target instanceof Element) || !event.target.closest(".pdf-reader-popup")) closeReaderPopup();
    };
    const key = (event: KeyboardEvent) => { if (event.key === "Escape") closeReaderPopup(); };
    const selectionChanged = () => {
      const saved = popupSelection.current;
      if (!saved) return;
      const selection = window.getSelection();
      const range = selection?.rangeCount ? selection.getRangeAt(0) : null;
      if (!range || selection?.isCollapsed || selection?.toString().trim() !== saved.text
        || range.startContainer !== saved.range.startContainer || range.startOffset !== saved.range.startOffset
        || range.endContainer !== saved.range.endContainer || range.endOffset !== saved.range.endOffset) closeReaderPopup();
    };
    document.addEventListener("pointerdown", outside);
    document.addEventListener("selectionchange", selectionChanged);
    document.addEventListener("scroll", outside, true);
    window.addEventListener("resize", closeReaderPopup);
    window.addEventListener("keydown", key);
    return () => {
      document.removeEventListener("pointerdown", outside);
      document.removeEventListener("selectionchange", selectionChanged);
      document.removeEventListener("scroll", outside, true);
      window.removeEventListener("resize", closeReaderPopup);
      window.removeEventListener("keydown", key);
    };
  }, [closeReaderPopup]);

  useEffect(() => {
    let cancelled = false;
    let opened: PDFDocumentProxy | null = null;
    (async () => {
      try {
        const pdf = await openPdf(await backend.readAttachment(attachmentId));
        opened = pdf;
        const pageSizes: Size[] = [];
        for (let n = 1; n <= pdf.numPages; n++) {
          const viewport = (await pdf.getPage(n)).getViewport({ scale: 1 });
          pageSizes.push({ width: viewport.width, height: viewport.height });
        }
        const form = await backend.pdfFormFields(attachmentId).catch(() => []);
        if (cancelled) return;
        setFields(form);
        setFilled({});
        setSizes(pageSizes);
        setDoc(pdf);
        void pluginHost.readerOpened(attachmentId, () => !cancelled);
        refreshMarks.current();
        setItemId((await backend.attachmentInfo(attachmentId).catch(() => null))?.itemId ?? null);
        setOutline(await readOutline(pdf).catch(() => []));
        // Keep the page text for full-text search, once.
        if (!(await backend.attachmentHasText(attachmentId).catch(() => true))) {
          void pageRecognizer()
            .then((recognize) => pageTexts(pdf, recognize))
            .then((texts) => backend.setAttachmentText(attachmentId, texts))
            .catch(() => undefined);
        }
      } catch (e) {
        if (!cancelled) setError(message(e));
      }
    })();
    return () => {
      cancelled = true;
      void opened?.destroy();
    };
  }, [attachmentId, version]);

  /** Turns, moves, adds or removes the current page, or saves a copy with
   * the marks written in. */
  const editPages = async (action: string) => {
    const index = current - 1;
    const count = doc?.numPages ?? 0;
    let edit: PageEdit | null = null;
    if (action === "left" || action === "right") edit = { kind: "rotate", pages: [index], degrees: action === "left" ? -90 : 90 };
    else if (action === "up" && index > 0) edit = { kind: "move", from: index, to: index - 1 };
    else if (action === "down" && index < count - 1) edit = { kind: "move", from: index, to: index + 1 };
    else if (action === "blank") edit = { kind: "insertBlank", at: index + 1 };
    else if (action === "delete" && window.confirm(t("Delete page {page}? Its marks go with it.", { page: current }))) {
      edit = { kind: "delete", pages: [index] };
    }
    try {
      if (action === "export") {
        const written = await backend.exportAnnotatedPdf(attachmentId);
        if (written) setNotice(t("Saved {path}.", { path: written }));
        return;
      }
      if (!edit) return;
      await backend.editPdfPages(attachmentId, edit);
      setVersion((v) => v + 1);
    } catch (e) {
      setNotice(message(e));
    }
  };

  /** Reads the scanned pages' pictures and writes their text into the PDF,
   * invisible, so it can be selected and searched. */
  const recognizeText = async () => {
    if (!doc) return;
    if (!(await pageRecognizer())) {
      setNotice(t("Text recognition is not available on this computer."));
      return;
    }
    setNotice(t("Recognizing text…"));
    try {
      const { pages, scanned } = await scannedPageLines(doc, (png) => backend.recognizePageLines(png));
      if (pages.length === 0) {
        setNotice(
          scanned === 0
            ? t("No scanned pages: every page has text already.")
            : t("No words were found on the scanned pages."),
        );
        return;
      }
      await backend.addTextLayer(attachmentId, pages);
      setNotice(t("Pages with recognized text: {count}. The text can be selected and searched now.", { count: pages.length }));
      setVersion((v) => v + 1);
    } catch (e) {
      setNotice(message(e));
    }
  };

  /** Writes what was filled in into the file. */
  const saveForm = async () => {
    try {
      await backend.fillPdfForm(
        attachmentId,
        Object.entries(filled).map(([name, value]) => ({ name, value })),
      );
      setNotice(t("The form is saved in the PDF."));
      setVersion((v) => v + 1);
    } catch (e) {
      setNotice(message(e));
    }
  };

  useEffect(() => {
    const element = scroller.current;
    if (!element) return;
    const observer = new ResizeObserver(() => setWidth(element.clientWidth));
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  // Shows the page asked for (from a search result), once the pages are laid out.
  useEffect(() => {
    if (!doc || page === undefined) return;
    const timer = window.setTimeout(
      () => scroller.current?.querySelector(`[data-page="${page}"]`)?.scrollIntoView({ block: "start" }),
      100,
    );
    return () => window.clearTimeout(timer);
  }, [doc, page]);

  // Fit the widest page to the width, then zoom.
  const scale = useMemo(() => {
    const widest = Math.max(1, ...sizes.map((s) => s.width));
    // Two pages side by side share the width, with a gap between them.
    const room = twoUp ? (width - 72) / 2 : width - 48;
    return width > 0 ? Math.max(0.2, (room / widest) * zoom) : zoom;
  }, [sizes, width, zoom, twoUp]);

  const onScroll = () => {
    const element = scroller.current;
    if (!element) return;
    const line = element.getBoundingClientRect().top + element.clientHeight / 3;
    const pages = element.querySelectorAll<HTMLElement>("[data-page]");
    for (const page of pages) {
      if (page.getBoundingClientRect().bottom > line) {
        setCurrent(Number(page.dataset.page) + 1);
        break;
      }
    }
  };

  /** The selection as one box per line on each page, in PDF coordinates. */
  const selectionByPage = useCallback((): { pageIndex: number; rects: AnnotationRect[]; text: string }[] => {
    const selection = window.getSelection();
    const element = scroller.current;
    if (!selection || selection.isCollapsed || !element) return [];
    const boxes = Array.from(selection.getRangeAt(0).getClientRects());
    const result: { pageIndex: number; rects: AnnotationRect[]; text: string }[] = [];
    for (const page of element.querySelectorAll<HTMLElement>("[data-page]")) {
      const index = Number(page.dataset.page);
      const viewport = viewports.current.get(index);
      if (!viewport) continue;
      const frame = page.getBoundingClientRect();
      const inside = boxes.filter((b) => {
        const x = (b.left + b.right) / 2;
        const y = (b.top + b.bottom) / 2;
        return x >= frame.left && x <= frame.right && y >= frame.top && y <= frame.bottom;
      });
      const rects = mergeLines(
        inside.map((b) => ({ left: b.left - frame.left, top: b.top - frame.top, right: b.right - frame.left, bottom: b.bottom - frame.top })),
      ).map((b) => rectFromPoints(viewport.convertToPdfPoint(b.left, b.top), viewport.convertToPdfPoint(b.right, b.bottom)));
      if (rects.length) result.push({ pageIndex: index, rects, text: selection.toString().trim() });
    }
    return result;
  }, []);

  /** Over selected text, the reader offers marks, copying and AI help, as
   * Zotero's reader does; plugins that listen add what they draw below. */
  const selectionPopup = useCallback(() => {
    closeReaderPopup();
    if (tool !== "select") return;
    const selection = window.getSelection();
    if (!selection?.rangeCount || selection.isCollapsed || !selection.toString().trim()) return;
    const range = selection.getRangeAt(0);
    if (!scroller.current?.contains(range.startContainer) || !scroller.current.contains(range.endContainer)) return;
    const pages = selectionByPage();
    if (!pages.length) return;
    const box = range.getBoundingClientRect();
    popupSelection.current = { range: range.cloneRange(), text: selection.toString().trim() };
    const id = popupRequest.current;
    const at = { x: box.left, y: box.bottom + 6, selection: true };
    setReaderPopup(at);
    if (!Object.values(readerEvents).some((types) => types.includes("renderTextSelectionPopup"))) return;
    releasePopup.current = pluginHost.watchReader("renderTextSelectionPopup", attachmentId, itemId, {
      annotation: { text: selection.toString().trim(), type: "highlight", pageIndex: pages[0].pageIndex, rects: pages[0].rects },
    }, (html) => {
      if (id === popupRequest.current) setReaderPopup({ ...at, html });
    });
  }, [attachmentId, itemId, tool, readerEvents, selectionByPage, closeReaderPopup]);

  useEffect(() => {
    if (!navigator.maxTouchPoints) return;
    let timer: ReturnType<typeof setTimeout>;
    const changed = () => {
      clearTimeout(timer);
      timer = setTimeout(selectionPopup, 180);
    };
    document.addEventListener("selectionchange", changed);
    return () => { clearTimeout(timer); document.removeEventListener("selectionchange", changed); };
  }, [selectionPopup]);

  const viewContextMenu = async (event: MouseEvent<HTMLDivElement>) => {
    if (!Object.values(readerEvents).some((types) => types.includes("createViewContextMenu"))) return;
    event.preventDefault();
    closeReaderPopup();
    const id = popupRequest.current;
    const { clientX: x, clientY: y } = event;
    const menus = await pluginHost.readerMenu("createViewContextMenu", attachmentId, itemId, {});
    if (id === popupRequest.current) {
      const items = readerMenuActions(menus);
      if (items.length) setReaderPopup({ x, y, items });
    }
  };

  const mark = async (kind: AnnotationKind, markColor = color) => {
    const pages = selectionByPage();
    if (!pages.length) {
      setNotice(t("Select some text first."));
      return;
    }
    try {
      const added: AnnotationInfo[] = [];
      for (const page of pages) {
        added.push(
          await annotationBackend.addAnnotation({ attachmentId, kind, color: markColor, pageIndex: page.pageIndex, rects: page.rects, text: page.text || null, comment: null }),
        );
      }
      setMarks((m) => [...m, ...added]);
      window.getSelection()?.removeAllRanges();
    } catch (e) {
      setNotice(message(e));
    }
  };

  /** Keeps a stroke drawn on a page as an ink mark. */
  const addInk = async (pageIndex: number, stroke: AnnotationPoint[]) => {
    try {
      const saved = await annotationBackend.addAnnotation({
        attachmentId,
        kind: "ink",
        color,
        pageIndex,
        rects: [],
        text: null,
        comment: null,
        strokes: [stroke],
      });
      setMarks((m) => [...m, saved]);
    } catch (e) {
      setNotice(message(e));
    }
  };

  const erase = async () => {
    const pages = selectionByPage();
    const doomed = marks.filter((m) =>
      pages.some((p) => p.pageIndex === m.pageIndex && p.rects.some((r) => m.rects.some((mr) => intersects(r, mr)))),
    );
    try {
      for (const m of doomed) await annotationBackend.removeAnnotation(m.id);
      setMarks((all) => all.filter((m) => !doomed.includes(m)));
      window.getSelection()?.removeAllRanges();
    } catch (e) {
      setNotice(message(e));
    }
  };

  const pageText = async (index: number): Promise<string> => {
    const cached = texts.current.get(index);
    if (cached !== undefined || !doc) return cached ?? "";
    const content = await (await doc.getPage(index + 1)).getTextContent();
    const text = content.items.map((item) => ("str" in item ? item.str : "")).join("").toLowerCase();
    texts.current.set(index, text);
    return text;
  };

  /** Goes to the next page that has the search text, from the start again after the last. */
  const find = async (event: FormEvent) => {
    event.preventDefault();
    const words = query.trim().toLowerCase().replace(/\s+/g, " ");
    if (!words || !doc) return;
    const start = found === null ? current - 1 : found + 1;
    for (let step = 0; step < doc.numPages; step++) {
      const index = (start + step) % doc.numPages;
      if ((await pageText(index)).replace(/\s+/g, " ").includes(words)) {
        setFound(index);
        scroller.current?.querySelector(`[data-page="${index}"]`)?.scrollIntoView({ block: "start", behavior: "smooth" });
        return;
      }
    }
    setFound(null);
    setNotice(t("No matches"));
  };

  /** Runs an AI request into the Ask AI panel. */
  const runAi = async (request: () => Promise<AiAnswer>) => {
    setShowAi(true);
    setAsking(true);
    setAiError(null);
    try {
      setAnswer(await request());
    } catch (e) {
      setAiError(message(e));
    } finally {
      setAsking(false);
    }
  };

  const ask = () => runAi(() => backend.aiAsk(aiSettings(), attachmentId, question, language));

  /** Translates or explains the selected text. */
  const helpWith = (task: AiTask) => {
    const text = window.getSelection()?.toString().trim() ?? "";
    if (!text) {
      setNotice(t("Select some text first."));
      return;
    }
    return runAi(() => backend.aiTransform(aiSettings(), task, text, language));
  };

  const goTo = (index: number) =>
    scroller.current?.querySelector(`[data-page="${index}"]`)?.scrollIntoView({ block: "start", behavior: "smooth" });

  const copyEmbed = async (mark: AnnotationInfo) => {
    try {
      await navigator.clipboard.writeText(await backend.annotationEmbed(mark.id));
      setNotice(t("Copied. Paste it into a note to embed this highlight."));
    } catch (e) {
      setNotice(message(e));
    }
  };

  const saveComment = async (mark: AnnotationInfo, comment: string) => {
    if ((mark.comment ?? "") === comment.trim()) return;
    try {
      const saved = await annotationBackend.updateAnnotation(mark, { comment: comment.trim() || null });
      setMarks((all) => all.map((m) => (m.id === saved.id ? saved : m)));
    } catch (e) {
      setNotice(message(e));
    }
  };

  const saveColor = async (mark: AnnotationInfo, color: string) => {
    if (mark.color === color) return;
    try {
      const saved = await annotationBackend.updateAnnotation(mark, { color });
      setMarks((all) => all.map((m) => (m.id === saved.id ? saved : m)));
    } catch (e) {
      setNotice(message(e));
    }
  };

  const removeMark = async (mark: AnnotationInfo) => {
    try {
      await annotationBackend.removeAnnotation(mark.id);
      setMarks((all) => all.filter((m) => m.id !== mark.id));
    } catch (e) {
      setNotice(message(e));
    }
  };

  /** Keeps a rectangle, ellipse or arrow drawn from one point to another. */
  const addShape = async (pageIndex: number, kind: ShapeKind, from: AnnotationPoint, to: AnnotationPoint) => {
    if (Math.hypot(to.x - from.x, to.y - from.y) < 4) return;
    try {
      const saved = await annotationBackend.addAnnotation({
        attachmentId,
        kind,
        color,
        pageIndex,
        rects: kind === "arrow" ? [] : [rectFromPoints([from.x, from.y], [to.x, to.y])],
        text: null,
        comment: null,
        strokes: kind === "arrow" ? [[from, to]] : [],
      });
      setMarks((m) => [...m, saved]);
    } catch (e) {
      setNotice(message(e));
    }
  };

  /** Keeps a mark someone just placed. */
  const keep = async (mark: Parameters<typeof backend.addAnnotation>[0]) => {
    try {
      const saved = await annotationBackend.addAnnotation(mark);
      setMarks((m) => [...m, saved]);
    } catch (e) {
      setNotice(message(e));
    }
  };

  /** A click with the text box, stamp or signature tool. */
  const place = (pageIndex: number, at: AnnotationPoint) => {
    const base = { attachmentId, color, pageIndex, text: null };
    if (tool === "text") {
      setTextDraft({ pageIndex, x: at.x, y: at.y, markId: null, value: "", color, fontSize: 14 });
    } else if (tool === "stamp") {
      void keep({ ...base, kind: "stamp", rects: [stampRect(stampWord, at.x, at.y)], comment: stampWord });
    } else if (tool === "sign") {
      if (signature) void keep({ ...base, kind: "ink", rects: [], comment: null, strokes: placeSignature(signature, at.x, at.y, 160) });
      else setAskingSignature(true);
    }
  };

  /** Keeps the text box being written, or its changed text; an emptied box goes. */
  const commitText = async (value: string, rect: AnnotationRect | null) => {
    const draft = textDraft;
    setTextDraft(null);
    if (!draft) return;
    const text = value.trim();
    const mark = draft.markId ? marks.find((m) => m.id === draft.markId) : undefined;
    try {
      if (mark) {
        if (!text) {
          await removeMark(mark);
        } else if (text !== (mark.comment ?? "")) {
          const saved = await annotationBackend.updateAnnotation(mark, { comment: text });
          setMarks((all) => all.map((m) => (m.id === saved.id ? saved : m)));
        }
      } else if (text && rect) {
        const saved = await annotationBackend.addAnnotation({
          attachmentId,
          kind: "text",
          color: draft.color,
          pageIndex: draft.pageIndex,
          rects: [rect],
          text: null,
          comment: text,
          fontSize: draft.fontSize,
        });
        setMarks((m) => [...m, saved]);
      }
    } catch (e) {
      setNotice(message(e));
    }
  };

  const literatureNote = async () => {
    if (!itemId) return;
    try {
      const note = await backend.vaultCreateLiteratureNote(itemId);
      onOpenNote?.(note.path);
    } catch (e) {
      setNotice(message(e));
    }
  };

  // Page order, then top to bottom (PDF y grows upwards).
  const ordered = [...marks].sort((a, b) => a.pageIndex - b.pageIndex || (b.rects[0]?.y ?? 0) - (a.rects[0]?.y ?? 0));

  if (error) {
    return (
      <div className="empty">
        <Icon name="doc" size={40} />
        <h3>{t("Cannot Open PDF")}</h3>
        <p>{error}</p>
      </div>
    );
  }

  return (
    <div className="pdf-view">
      <div className="pdf-toolbar" role="toolbar" aria-label={t("Formatting")}>
        <div className="tool-group" role="group" aria-label={t("Annotations")}>
          <button type="button" className="icon-button" title={t("Highlight")} aria-label={t("Highlight")}
            onMouseDown={(e) => e.preventDefault()} onClick={() => void mark("highlight")}>
            <Icon name="highlighter" size={16} />
          </button>
          <button type="button" className="icon-button" title={t("Underline")} aria-label={t("Underline")}
            onMouseDown={(e) => e.preventDefault()} onClick={() => void mark("underline")}>
            <Icon name="underline" size={16} />
          </button>
          {([["ink", "Draw", "pen"], ["text", "Text Box", "text"]] as const).map(([id, label, icon]) => (
            <button key={id} type="button" className={tool === id ? "icon-button selected" : "icon-button"} title={t(label)} aria-label={t(label)}
              aria-pressed={tool === id} onClick={() => setTool((current) => current === id ? "select" : id)}>
              <Icon name={icon} size={16} />
            </button>
          ))}
          <MenuButton label={t("More Tools")} icon={<Icon name="shapes" size={15} />} iconOnly items={[
            { label: t("Strikethrough"), onSelect: () => void mark("strikeout") },
            ...([ ["square", "Rectangle"], ["circle", "Ellipse"], ["arrow", "Arrow"], ["stamp", "Stamp"], ["sign", "Sign"] ] as const).map(([id, label]) => ({
              label: t(label), checked: tool === id,
              onSelect: () => {
                setTool((current) => current === id ? "select" : id);
                if (id === "sign" && !signature) setAskingSignature(true);
              },
            })),
            ...(tool === "sign" && signature ? [{ label: t("Change Signature…"), onSelect: () => setAskingSignature(true) }] : []),
            { label: t("Erase Marks"), onSelect: () => void erase() },
          ]} />
        </div>
        <MenuButton label={t("Color")} icon={<span className="menu-swatch" style={{ background: color }} />} iconOnly items={MARK_COLORS.map((value) => ({
          label: t(value.name), icon: <span className="menu-swatch" style={{ background: value.hex }} />, checked: value.hex === color, onSelect: () => setColor(value.hex),
        }))} />
        {tool === "stamp" && <MenuButton label={t("Stamp")} items={STAMP_WORDS.map((word) => ({ label: t(word), checked: word === stampWord, onSelect: () => setStampWord(word) }))} />}
        <form className="search-field" onSubmit={find}>
          <Icon name="search" size={14} />
          <input
            type="search"
            value={query}
            placeholder={t("Find in PDF")}
            onChange={(e) => {
              setQuery(e.target.value);
              setFound(null);
            }}
          />
        </form>
        <MenuButton label={t("Edit Pages")} icon={<Icon name="doc" size={15} />} disabled={!doc} items={([
          ["left", "Turn Page Left"], ["right", "Turn Page Right"], ["up", "Move Page Up"], ["down", "Move Page Down"],
          ["blank", "Add Blank Page After"], ["delete", "Delete Page…"], ["export", "Save a Copy with Annotations…"],
        ] as const).map(([action, label]) => ({ label: t(label), onSelect: () => void editPages(action) })).concat([
          { label: t("Recognize Text"), onSelect: () => void recognizeText() },
        ])} />
        <MenuButton label={t("More")} icon={<span aria-hidden="true">⋯</span>} iconOnly items={[
          { label: t("Two Pages"), checked: twoUp, onSelect: () => setTwoUp((value) => !value) },
          { label: t("Night"), checked: night, onSelect: () => setNight((value) => !value) },
          ...(outline.length > 0 ? [{ label: t("Contents"), checked: showOutline, onSelect: () => setShowOutline((value) => !value) }] : []),
          { label: `${t("Annotations")} (${marks.length})`, checked: showMarks, onSelect: () => setShowMarks((value) => !value) },
          { label: t("Translate"), onSelect: () => void helpWith("translate") },
          { label: t("Explain"), onSelect: () => void helpWith("explain") },
          { label: t("Ask AI"), checked: showAi, onSelect: () => setShowAi((value) => !value) },
          ...(itemId && onOpenNote ? [{ label: t("Literature Note"), onSelect: () => void literatureNote() }] : []),
          ...(Object.keys(filled).length > 0 ? [{ label: t("Discard Changes"), onSelect: () => setFilled({}) }] : []),
        ]} />
        <span className="mindmap-zoom">
          <button aria-label={t("Zoom Out")} title={t("Zoom Out")} onClick={() => setZoom((value) => Math.max(0.4, value / 1.2))}>−</button>
          <button className="link" aria-label={t("Zoom to Fit")} title={t("Zoom to Fit")} onClick={() => setZoom(1)}>{Math.round(zoom * 100)}%</button>
          <button aria-label={t("Zoom In")} title={t("Zoom In")} onClick={() => setZoom((value) => Math.min(5, value * 1.2))}>+</button>
        </span>
        {doc && <span className="muted page-count">{current} / {doc.numPages}</span>}
        {fields.length > 0 && <button className="primary" disabled={Object.keys(filled).length === 0} onClick={() => void saveForm()}>{t("Save Form")}</button>}
        {doc && <ReaderHtml event="renderToolbar" attachmentKey={attachmentId} itemKey={itemId} className="pdf-reader-toolbar" />}
      </div>
      {notice && (
        <p className="notice" role="status">
          {notice}
          <button className="icon-button" aria-label={t("OK")} onClick={() => setNotice(null)}>
            <Icon name="close" size={12} />
          </button>
        </p>
      )}
      {askingSignature && (
        <SignatureDialog
          onSave={(written) => {
            saveSignature(browserStorage(), written);
            setSignature(written);
            setAskingSignature(false);
            setTool("sign");
          }}
          onClose={() => setAskingSignature(false)}
        />
      )}
      <div className="pdf-body">
      {showOutline && (
        <aside className="pdf-annotations pdf-outline">
          <ul>
            {outline.map((entry, n) => (
              <li key={n} style={{ paddingInlineStart: entry.level * 12 }}>
                <button className="link" disabled={entry.page === null} onClick={() => entry.page !== null && goTo(entry.page)}>
                  {entry.title}
                </button>
              </li>
            ))}
          </ul>
        </aside>
      )}
      <div className={`pdf-scroll${twoUp ? " two-up" : ""}${night ? " night" : ""}`} ref={scroller} onScroll={onScroll}
        onMouseUp={(event) => { if (event.button === 0) void selectionPopup(); }} onContextMenu={(event) => void viewContextMenu(event)}>
        {doc &&
          sizes.map((size, index) => (
            <PdfPage
              key={index}
              doc={doc}
              index={index}
              size={size}
              scale={scale}
              marks={marks.filter((m) => m.pageIndex === index)}
              tool={tool}
              color={color}
              draft={textDraft?.pageIndex === index ? textDraft : null}
              onStroke={(stroke) => void addInk(index, stroke)}
              onShape={(kind, from, to) => void addShape(index, kind, from, to)}
              onPlace={(at) => place(index, at)}
              onEditText={(mark) => {
                const r = mark.rects[0];
                if (!r) return;
                setTextDraft({
                  pageIndex: index,
                  x: r.x,
                  y: r.y + r.height,
                  markId: mark.id,
                  value: mark.comment ?? "",
                  color: mark.color,
                  fontSize: mark.fontSize ?? 14,
                });
              }}
              onCommitText={(value, rect) => void commitText(value, rect)}
              onViewport={(viewport) => viewports.current.set(index, viewport)}
              fields={fields.filter((f) => f.widgets.some((w) => w.pageIndex === index))}
              filled={filled}
              onFill={(name, value) => setFilled((all) => ({ ...all, [name]: value }))}
            />
          ))}
      </div>
      {showAi && (
        <aside className="pdf-annotations pdf-ai">
          <form
            onSubmit={(e) => {
              e.preventDefault();
              void ask();
            }}
          >
            <textarea
              rows={3}
              value={question}
              placeholder={t("A question about this paper")}
              onChange={(e) => setQuestion(e.target.value)}
            />
            <button className="primary" disabled={asking || !question.trim()}>
              {asking ? t("Working…") : t("Ask")}
            </button>
          </form>
          {aiError && <p className="notice">{aiError}</p>}
          {answer && <AiAnswerView answer={answer} />}
        </aside>
      )}
      {showMarks && (
        <aside className="pdf-annotations">
          {ordered.length === 0 && <p className="muted">{t("Select text and highlight it to see it here.")}</p>}
          <ul>
            {ordered.map((mark) => (
              <li key={mark.id} style={{ borderInlineStartColor: mark.color }}>
                <div className="row">
                  <button className="link" onClick={() => goTo(mark.pageIndex)}>
                    {t("Page {page}", { page: mark.pageIndex + 1 })}
                  </button>
                  <span className="spacer" />
                  <MenuButton label={t("Color")} icon={<span className="menu-swatch" style={{ background: mark.color }} />} iconOnly
                    items={MARK_COLORS.map((value) => ({ label: t(value.name),
                      icon: <span className="menu-swatch" style={{ background: value.hex }} />, checked: value.hex === mark.color,
                      onSelect: () => void saveColor(mark, value.hex),
                    }))} />
                  <AnnotationPluginMenu mark={mark} itemKey={itemId} />
                </div>
                <ReaderHtml event="renderSidebarAnnotationHeader" attachmentKey={attachmentId} itemKey={itemId} params={{ annotation: { ...mark, type: mark.kind } }} />
                {mark.text && <p className="mark-text">{mark.text}</p>}
                {!!mark.tags?.length && <p>{mark.tags.join(" · ")}</p>}
                <textarea
                  ref={(input) => { if (input) commentInputs.current.set(mark.id, input); else commentInputs.current.delete(mark.id); }}
                  rows={2}
                  defaultValue={mark.comment ?? ""}
                  placeholder={t("Add a comment")}
                  onFocus={(e) => editingComments.current.set(mark.id, e.target.value)}
                  onBlur={(e) => {
                    const value = e.target.value;
                    const original = editingComments.current.get(mark.id);
                    editingComments.current.delete(mark.id);
                    if (value === original) refreshMarks.current();
                    else void saveComment(mark, value).finally(() => refreshMarks.current());
                  }}
                />
                <div className="row">
                  <button onClick={() => void copyEmbed(mark)}>{t("Copy for Notes")}</button>
                  <span className="spacer" />
                  <button className="icon-button danger" title={t("Remove")} aria-label={t("Remove")} onClick={() => void removeMark(mark)}>
                    <Icon name="trash" size={14} />
                  </button>
                </div>
              </li>
            ))}
          </ul>
        </aside>
      )}
      </div>
      {readerPopup && (
        <ReaderPopup popup={readerPopup} onClose={closeReaderPopup}>
          {readerPopup.selection && (
            <div className="selection-actions" role="toolbar" aria-label={t("Annotations")}>
              {MARK_COLORS.map((value) => (
                <button key={value.hex} type="button" className="swatch" title={t(value.name)} aria-label={`${t("Highlight")}: ${t(value.name)}`}
                  style={{ background: value.hex }} onClick={() => { setColor(value.hex); void mark("highlight", value.hex); }} />
              ))}
              <button type="button" className="icon-button" title={t("Underline")} aria-label={t("Underline")} onClick={() => void mark("underline")}>
                <Icon name="underline" size={15} />
              </button>
              <button type="button" onClick={() => {
                const text = popupSelection.current?.text ?? "";
                closeReaderPopup();
                if (text) void navigator.clipboard.writeText(text).catch((e: unknown) => setNotice(message(e)));
              }}>{t("Copy")}</button>
              <button type="button" onClick={() => { void helpWith("translate"); closeReaderPopup(); }}>{t("Translate")}</button>
              <button type="button" onClick={() => { void helpWith("explain"); closeReaderPopup(); }}>{t("Explain")}</button>
            </div>
          )}
        </ReaderPopup>
      )}
    </div>
  );
}

interface PageProps {
  doc: PDFDocumentProxy;
  index: number;
  size: Size;
  scale: number;
  marks: AnnotationInfo[];
  tool: Tool;
  /** The colour new marks take. */
  color: string;
  /** The text box being written on this page, if any. */
  draft: TextDraft | null;
  onStroke: (stroke: AnnotationPoint[]) => void;
  onShape: (kind: ShapeKind, from: AnnotationPoint, to: AnnotationPoint) => void;
  /** A click with the text box, stamp or signature tool, in PDF points. */
  onPlace: (at: AnnotationPoint) => void;
  onEditText: (mark: AnnotationInfo) => void;
  /** The text written and the box it fills (PDF points). */
  onCommitText: (value: string, rect: AnnotationRect | null) => void;
  onViewport: (viewport: PageViewport) => void;
  /** The form fields with a box on this page, and what has been filled in. */
  fields: PdfFormField[];
  filled: Record<string, string>;
  onFill: (name: string, value: string) => void;
}

/** A form field's box over the page, to fill in as Acrobat does. */
function FieldBox({ field, widget, value, box, scale, onFill }: {
  field: PdfFormField;
  widget: PdfFormField["widgets"][number];
  value: string;
  box: CSSProperties;
  scale: number;
  onFill: (value: string) => void;
}) {
  const size = (field.fontSize || (field.multiline ? 10 : Math.min(12, Math.max(4, widget.height * 0.7)))) * scale;
  const common = {
    className: `pdf-field ${field.kind}`,
    style: { ...box, fontSize: size },
    disabled: field.readOnly,
    title: field.name,
  };
  switch (field.kind) {
    case "text":
      return field.multiline ? (
        <textarea {...common} value={value} maxLength={field.maxLength || undefined} onChange={(e) => onFill(e.target.value)} />
      ) : (
        <input {...common} value={value} maxLength={field.maxLength || undefined} onChange={(e) => onFill(e.target.value)} />
      );
    case "checkbox":
      return (
        <input
          {...common}
          type="checkbox"
          checked={value === widget.onValue}
          onChange={(e) => onFill(e.target.checked ? widget.onValue : "Off")}
        />
      );
    case "radio":
      return (
        <input {...common} type="radio" name={`pdf-field-${field.name}`} checked={value === widget.onValue} onChange={() => onFill(widget.onValue)} />
      );
    case "choice": {
      const options = field.options.includes(value) || !value ? field.options : [value, ...field.options];
      return (
        <select {...common} value={value} onChange={(e) => onFill(e.target.value)}>
          {!field.options.includes("") && <option value="" />}
          {options.map((option) => (
            <option key={option} value={option}>
              {field.optionLabels[field.options.indexOf(option)] || option}
            </option>
          ))}
        </select>
      );
    }
    default:
      return null;
  }
}

/** A rectangle, ellipse or arrow from one point to another (page pixels). */
function ShapeMark({ kind, from, to, color, width, title }: {
  kind: ShapeKind;
  from: number[];
  to: number[];
  color: string;
  width: number;
  title?: string | null;
}) {
  const style = { stroke: color, strokeWidth: width };
  const label = title ? <title>{title}</title> : null;
  if (kind === "arrow") {
    const [a, b] = arrowHead(from, to, 5 * width);
    return (
      <g style={style}>
        {label}
        <line x1={from[0]} y1={from[1]} x2={to[0]} y2={to[1]} />
        <polyline points={`${a.join(",")} ${to.join(",")} ${b.join(",")}`} />
      </g>
    );
  }
  const [x, y] = [Math.min(from[0], to[0]), Math.min(from[1], to[1])];
  const [w, h] = [Math.abs(to[0] - from[0]), Math.abs(to[1] - from[1])];
  return kind === "square" ? (
    <rect x={x} y={y} width={w} height={h} style={style}>
      {label}
    </rect>
  ) : (
    <ellipse cx={x + w / 2} cy={y + h / 2} rx={w / 2} ry={h / 2} style={style}>
      {label}
    </ellipse>
  );
}

/** Writes a text box in place; leaving it keeps what was written. */
function TextBoxEditor({ draft, viewport, scale, onCommit }: {
  draft: TextDraft;
  viewport: PageViewport;
  scale: number;
  onCommit: (value: string, rect: AnnotationRect | null) => void;
}) {
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const element = box.current;
    if (!element) return;
    element.innerText = draft.value;
    element.focus();
    const range = document.createRange();
    range.selectNodeContents(element);
    range.collapse(false);
    window.getSelection()?.removeAllRanges();
    window.getSelection()?.addRange(range);
    // Filled once, when the box opens.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  const [left, top] = viewport.convertToViewportPoint(draft.x, draft.y);
  const commit = () => {
    const element = box.current;
    const page = element?.parentElement?.getBoundingClientRect();
    if (!element || !page) return;
    const frame = element.getBoundingClientRect();
    const a = viewport.convertToPdfPoint(frame.left - page.left, frame.top - page.top);
    const b = viewport.convertToPdfPoint(frame.right - page.left, frame.bottom - page.top);
    onCommit(element.innerText, rectFromPoints(a, b));
  };
  return (
    <div
      ref={box}
      className="pdf-text-box editing"
      contentEditable
      suppressContentEditableWarning
      role="textbox"
      aria-multiline="true"
      style={{ left, top, color: draft.color, fontSize: draft.fontSize * scale }}
      onBlur={commit}
      onKeyDown={(e) => e.key === "Escape" && e.currentTarget.blur()}
    />
  );
}

/** One page: canvas, selectable text layer, and the marks over it. */
function PdfPage(props: PageProps) {
  const { doc, index, size, scale, marks, tool, color, draft, onStroke, onShape, onPlace, onEditText, onCommitText, onViewport } =
    props;
  const { fields, filled, onFill } = props;
  const frame = useRef<HTMLDivElement>(null);
  /** The stroke or shape being drawn, in page pixels. */
  const [stroke, setStroke] = useState<[number, number][] | null>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const textLayer = useRef<HTMLDivElement>(null);
  const [near, setNear] = useState(false);
  const [viewport, setViewport] = useState<PageViewport | null>(null);

  useEffect(() => {
    const element = frame.current;
    if (!element) return;
    const observer = new IntersectionObserver((entries) => {
      if (entries.some((e) => e.isIntersecting)) setNear(true);
    }, { rootMargin: "800px 0px" });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    if (!near) return;
    let cancelled = false;
    let renderTask: { cancel(): void } | null = null;
    let text: TextLayer | null = null;
    (async () => {
      const page = await doc.getPage(index + 1);
      if (cancelled) return;
      const view = page.getViewport({ scale });
      setViewport(view);
      onViewport(view);
      const element = canvas.current;
      const context = element?.getContext("2d");
      if (!element || !context) return;
      const ratio = window.devicePixelRatio || 1;
      element.width = Math.floor(view.width * ratio);
      element.height = Math.floor(view.height * ratio);
      const task = page.render({ canvasContext: context, viewport: view, transform: ratio === 1 ? undefined : [ratio, 0, 0, ratio, 0, 0] });
      renderTask = task;
      await task.promise.catch(() => undefined);
      const container = textLayer.current;
      if (cancelled || !container) return;
      container.replaceChildren();
      text = new TextLayer({ textContentSource: page.streamTextContent(), container, viewport: view });
      await text.render().catch(() => undefined);
    })();
    return () => {
      cancelled = true;
      renderTask?.cancel();
      text?.cancel();
    };
    // onViewport only records the viewport; a new function each render is fine.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [near, doc, index, scale]);

  const style = { width: size.width * scale, height: size.height * scale, "--scale-factor": scale } as CSSProperties;
  const corners = (r: AnnotationRect): [number[], number[]] | null => {
    if (!viewport) return null;
    const [x1, y1, x2, y2] = viewport.convertToViewportRectangle([r.x, r.y, r.x + r.width, r.y + r.height]);
    return [
      [x1, y1],
      [x2, y2],
    ];
  };
  const shaping = tool === "square" || tool === "circle" || tool === "arrow" ? tool : null;
  return (
    <div className="pdf-page" data-page={index} ref={frame} style={style}>
      <canvas ref={canvas} style={{ width: "100%", height: "100%" }} />
      <div className="pdf-marks">
        {viewport && (
          <svg className="pdf-ink" width="100%" height="100%">
            {marks
              .filter((m) => m.kind === "ink")
              .flatMap((m) =>
                (m.strokes ?? []).map((points, i) => (
                  <polyline
                    key={`${m.id}:${i}`}
                    points={points.map((p) => viewport.convertToViewportPoint(p.x, p.y).join(",")).join(" ")}
                    style={{ stroke: m.color, strokeWidth: 2 * scale }}
                  >
                    {m.comment && <title>{m.comment}</title>}
                  </polyline>
                )),
              )}
            {marks.map((m) => {
              if (m.kind === "square" || m.kind === "circle") {
                const ends = m.rects[0] ? corners(m.rects[0]) : null;
                return ends && <ShapeMark key={m.id} kind={m.kind} from={ends[0]} to={ends[1]} color={m.color} width={2 * scale} title={m.comment} />;
              }
              const line = m.kind === "arrow" ? m.strokes?.[0] : undefined;
              if (!line || line.length < 2) return null;
              const tail = viewport.convertToViewportPoint(line[0].x, line[0].y);
              const head = viewport.convertToViewportPoint(line[line.length - 1].x, line[line.length - 1].y);
              return <ShapeMark key={m.id} kind="arrow" from={tail} to={head} color={m.color} width={2 * scale} title={m.comment} />;
            })}
            {stroke && tool === "ink" && (
              <polyline points={stroke.map((p) => p.join(",")).join(" ")} style={{ stroke: color, strokeWidth: 2 * scale }} />
            )}
            {stroke && shaping && stroke.length > 1 && (
              <ShapeMark kind={shaping} from={stroke[0]} to={stroke[stroke.length - 1]} color={color} width={2 * scale} />
            )}
          </svg>
        )}
        {viewport &&
          marks
            .filter((m) => m.kind === "highlight" || m.kind === "underline" || m.kind === "strikeout" || m.kind === "note")
            .flatMap((m) =>
            m.rects.map((r, i) => {
              const [x1, y1, x2, y2] = viewport.convertToViewportRectangle([r.x, r.y, r.x + r.width, r.y + r.height]);
              const box = { left: Math.min(x1, x2), top: Math.min(y1, y2), width: Math.abs(x2 - x1), height: Math.abs(y2 - y1) };
              return <div key={`${m.id}:${i}`} className={`pdf-mark ${m.kind}`} style={{ ...box, "--mark": m.color } as CSSProperties} title={m.comment ?? undefined} />;
            }),
          )}
        {viewport &&
          marks
            .filter((m) => m.kind === "stamp" && m.rects[0])
            .map((m) => {
              const r = m.rects[0];
              const [x1, y1, x2, y2] = viewport.convertToViewportRectangle([r.x, r.y, r.x + r.width, r.y + r.height]);
              const word = m.comment ?? "";
              return (
                <div
                  key={m.id}
                  className="pdf-stamp"
                  style={{
                    left: Math.min(x1, x2),
                    top: Math.min(y1, y2),
                    width: Math.abs(x2 - x1),
                    height: Math.abs(y2 - y1),
                    color: m.color,
                    fontSize: stampFontSize(word, r.width, r.height) * scale,
                  }}
                >
                  {word}
                </div>
              );
            })}
        {viewport &&
          marks
            .filter((m) => m.kind === "text" && m.id !== draft?.markId && m.rects[0])
            .map((m) => {
              const r = m.rects[0];
              const [left, top] = viewport.convertToViewportPoint(r.x, r.y + r.height);
              return (
                <div
                  key={m.id}
                  className="pdf-text-box"
                  style={{ left, top, color: m.color, fontSize: (m.fontSize ?? 14) * scale }}
                  onDoubleClick={() => onEditText(m)}
                >
                  {m.comment}
                </div>
              );
            })}
      </div>
      <div className="textLayer" ref={textLayer} />
      {viewport && fields.length > 0 && (
        <div className="pdf-form">
          {fields.flatMap((field) =>
            field.widgets
              .map((widget, n) => ({ widget, n }))
              .filter(({ widget }) => widget.pageIndex === index)
              .map(({ widget, n }) => {
                const [x1, y1, x2, y2] = viewport.convertToViewportRectangle([
                  widget.x,
                  widget.y,
                  widget.x + widget.width,
                  widget.y + widget.height,
                ]);
                const box = { left: Math.min(x1, x2), top: Math.min(y1, y2), width: Math.abs(x2 - x1), height: Math.abs(y2 - y1) };
                return (
                  <FieldBox
                    key={`${field.name}:${n}`}
                    field={field}
                    widget={widget}
                    value={filled[field.name] ?? field.value}
                    box={box}
                    scale={scale}
                    onFill={(value) => onFill(field.name, value)}
                  />
                );
              }),
          )}
        </div>
      )}
      {tool !== "select" && viewport && (
        <div
          className="pdf-draw"
          onPointerDown={(e) => {
            const box = e.currentTarget.getBoundingClientRect();
            e.currentTarget.setPointerCapture(e.pointerId);
            setStroke([[e.clientX - box.left, e.clientY - box.top]]);
          }}
          onPointerMove={(e) => {
            if (!stroke) return;
            const box = e.currentTarget.getBoundingClientRect();
            const point: [number, number] = [e.clientX - box.left, e.clientY - box.top];
            setStroke((s) => (s ? (tool === "ink" ? [...s, point] : [s[0], point]) : s));
          }}
          onPointerUp={() => {
            if (stroke) {
              const points = stroke.map(([x, y]) => {
                const [px, py] = viewport.convertToPdfPoint(x, y);
                return { x: px, y: py };
              });
              if (tool === "ink") onStroke(points);
              else if (tool === "text" || tool === "stamp" || tool === "sign") onPlace(points[0]);
              else onShape(tool, points[0], points[points.length - 1]);
            }
            setStroke(null);
          }}
        />
      )}
      {draft && viewport && <TextBoxEditor draft={draft} viewport={viewport} scale={scale} onCommit={onCommitText} />}
    </div>
  );
}
