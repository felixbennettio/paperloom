import { Fragment, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useI18n } from "../i18n";
import {
  backend,
  ITEM_TYPES,
  type AttachmentInfo,
  type ReadStatus,
  type TagInfo,
  type IdentifierKind,
  type ItemDetail,
  type AiAnswer,
  type ItemSummary,
  type NewItem,
  type NoteMention,
  type ReferenceFormat,
} from "../lib/api";
import { sortItems, toggleSort, type Scope, type Sort, type SortKey } from "../lib/scope";
import { aiSettings } from "../lib/ai";
import { pluginHost, type ItemMenuChild, type PluginItemMenu, type PluginSection } from "../lib/plugins";
import DOMPurify from "dompurify";
import { usePlugins } from "./PluginLayer";
import { AiAnswerView } from "./AiAnswer";
import { CitationBox } from "./CitationBox";
import { EditItemDialog } from "./EditItemDialog";
import { Icon } from "./Icon";
import { MenuButton, type MenuAction } from "./MenuButton";
import { WordCiteDialog } from "./WordCiteDialog";

export const ITEM_TYPE_NAMES: Record<string, string> = {
  "journal-article": "Journal Article",
  "conference-paper": "Conference Paper",
  book: "Book",
  "book-chapter": "Book Chapter",
  thesis: "Thesis",
  report: "Report",
  preprint: "Preprint",
  patent: "Patent",
  standard: "Standard",
  dataset: "Dataset",
  webpage: "Web Page",
  other: "Other",
};

interface ListProps {
  title: string;
  scope: Scope;
  items: ItemSummary[];
  tags: TagInfo[];
  onOpen: (item: ItemSummary) => void;
  onTrash: (item: ItemSummary) => void;
  onStar: (item: ItemSummary) => void;
  onRestore: (item: ItemSummary) => void;
  onDeleteForever: (item: ItemSummary) => void;
  onEmptyTrash: () => void;
  onAdd: () => void;
  /** Reads the chosen files; returns a report to show. */
  onImport: (files: File[]) => Promise<string>;
  /** Asks for PDFs and adds them; a report, or null when cancelled. */
  onImportPdfs: () => Promise<string | null>;
  /** Moves a Zotero library in; says how it went, or null if cancelled. */
  onImportZotero: () => Promise<string | null>;
  onImportEndNote: () => Promise<string | null>;
  /** After the items changed (citation counts were looked up). */
  onChanged: () => void;
}

/** A tag name with its color dot. */
export function TagChip({ name, color }: { name: string; color: string | null }) {
  return (
    <span className="tag-chip">
      <span className="tag-dot" style={{ background: color ?? "transparent" }} />
      {name}
    </span>
  );
}

interface ItemPopupData {
  x: number;
  y: number;
  item: ItemSummary;
  menus: PluginItemMenu[];
}

function ItemPopup({ popup, actions, onClose }: { popup: ItemPopupData; actions: MenuAction[]; onClose: () => void }) {
  const element = useRef<HTMLDivElement>(null);
  const previousFocus = useRef(document.activeElement as HTMLElement | null);
  const [position, setPosition] = useState({ left: popup.x, top: popup.y });
  const close = (restoreFocus = false) => {
    onClose();
    if (restoreFocus) previousFocus.current?.focus();
  };
  useLayoutEffect(() => {
    const box = element.current?.getBoundingClientRect();
    if (!box) return;
    setPosition({
      left: Math.max(8, Math.min(popup.x, window.innerWidth - box.width - 8)),
      top: Math.max(8, Math.min(popup.y, window.innerHeight - box.height - 8)),
    });
  }, [popup]);
  useLayoutEffect(() => {
    element.current?.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus();
  }, []);
  useEffect(() => {
    const outside = (event: PointerEvent) => {
      if (!element.current?.contains(event.target as Node)) onClose();
    };
    const moved = (event: Event) => {
      if (!(event.target instanceof Node) || !element.current?.contains(event.target)) onClose();
    };
    document.addEventListener("pointerdown", outside);
    window.addEventListener("resize", moved);
    document.addEventListener("scroll", moved, true);
    return () => {
      document.removeEventListener("pointerdown", outside);
      window.removeEventListener("resize", moved);
      document.removeEventListener("scroll", moved, true);
    };
  }, [onClose]);
  const separator = <div role="separator" className="menu-separator" />;
  const command = (entry: ItemMenuChild, plugin: string, menu: number, disabled = false, submenu?: string) => (
    "separator" in entry ? separator : <button type="button" role="menuitem" title={entry.label}
      aria-label={submenu ? `${submenu}: ${entry.label}` : entry.label} disabled={disabled || entry.disabled}
      className={submenu ? "menu-nested" : undefined} onMouseDown={(event) => event.preventDefault()}
      onClick={() => {
        close(true);
        pluginHost.itemCommand(plugin, menu, entry.index, submenu ? [submenu, entry.label] : [entry.label], [popup.item.id]);
      }}><span>{entry.label}</span></button>
  );
  return createPortal(
    <div ref={element} className="action-menu" role="menu" aria-label={popup.item.title} style={position}
      onBlur={(event) => {
        if (!event.currentTarget.contains(event.relatedTarget)) onClose();
      }}
      onKeyDown={(event) => {
        if (event.key === "Escape") {
          event.preventDefault();
          event.stopPropagation();
          close(true);
        } else if (event.key === "Tab") {
          close(true);
        } else if (["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) {
          event.preventDefault();
          const buttons = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>("button:not(:disabled)"));
          const current = buttons.indexOf(document.activeElement as HTMLButtonElement);
          const index = event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1
            : (current + (event.key === "ArrowDown" ? 1 : -1) + buttons.length) % buttons.length;
          buttons[index]?.focus();
        }
      }}>
      {actions.map((action, index) => <button key={index} type="button" role="menuitem" title={action.label}
        onMouseDown={(event) => event.preventDefault()} onClick={() => { close(true); action.onSelect(); }}>
        <span>{action.label}</span>
      </button>)}
      {popup.menus.some((menu) => menu.items.length > 0) && separator}
      {popup.menus.map(({ plugin, menu, items }) => <Fragment key={`${plugin}:${menu}`}>
        {items.map((entry, index) => <Fragment key={index}>
          {"children" in entry ? <div role="group" aria-label={entry.label} style={{ display: "flex", flexDirection: "column", gap: 2 }}>
            <div role="presentation" className="menu-heading">{entry.label}</div>
            {entry.children.map((child, index) => <Fragment key={index}>{command(child, plugin, menu, entry.disabled, entry.label)}</Fragment>)}
          </div> : command(entry, plugin, menu)}
        </Fragment>)}
      </Fragment>)}
    </div>, document.body,
  );
}

/** The items of the selected view as a table. */
export function LibraryView(props: ListProps) {
  const { title, scope, items, tags, onOpen, onTrash, onStar, onRestore, onDeleteForever, onEmptyTrash, onAdd, onImport, onImportPdfs, onImportZotero, onImportEndNote, onChanged } = props;
  const { t } = useI18n();
  const [query, setQuery] = useState("");
  const [formats, setFormats] = useState<ReferenceFormat[]>([]);
  const [notice, setNotice] = useState<string | null>(null);
  const [readingPdfs, setReadingPdfs] = useState(false);
  const [counting, setCounting] = useState(false);
  const [citingWord, setCitingWord] = useState(false);
  const [sort, setSort] = useState<Sort>({ key: "dateAdded", descending: true });
  const [itemPopup, setItemPopup] = useState<ItemPopupData | null>(null);
  const popupRequest = useRef(0);
  const closeItemPopup = useCallback(() => {
    popupRequest.current++;
    setItemPopup(null);
  }, []);
  const { columns } = usePlugins();
  const [columnValues, setColumnValues] = useState<Record<string, Record<string, string>>>({});
  const sentItems = useRef<ItemSummary[] | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const inTrash = scope.kind === "trash";
  const colors = useMemo(() => new Map(tags.map((tag) => [tag.name, tag.color])), [tags]);
  useEffect(() => {
    closeItemPopup();
    return () => { popupRequest.current++; };
  }, [scope, query, sort, closeItemPopup]);
  useEffect(() => {
    backend.referenceFormats().then(setFormats, () => setFormats([]));
  }, []);

  const exportAs = async (format: string) => {
    try {
      const path = await backend.exportReferencesFile(items.map((i) => i.id), format);
      if (path) setNotice(t("Saved to {path}", { path }));
    } catch (e) {
      setNotice(e instanceof Error ? e.message : String(e));
    }
  };
  const shown = useMemo(() => {
    const q = query.trim().toLowerCase();
    const matching = q
      ? items.filter((i) => [i.title, i.authors, i.doi ?? "", ...i.tags].some((field) => field.toLowerCase().includes(q)))
      : items;
    return sortItems(matching, sort);
  }, [items, query, sort]);

  useEffect(() => {
    let cancelled = false;
    setColumnValues({});
    if (shown.length && columns.length) {
      const itemKeys = shown.map((item) => item.id);
      const load = async () => {
        // The plugins see the library as it is now; a new search or sort
        // doesn't change it.
        if (sentItems.current !== items) {
          sentItems.current = items;
          await pluginHost.itemsChanged();
        }
        if (cancelled) return;
        const values = await Promise.all(columns.map(async (column) => [
          `${column.plugin}:${column.key}`,
          await pluginHost.columnValues(column, itemKeys),
        ] as const));
        if (!cancelled) setColumnValues(Object.fromEntries(values));
      };
      void load();
    }
    return () => {
      cancelled = true;
    };
  }, [shown, columns]);
  useEffect(() => {
    if (itemPopup && !shown.some((item) => item.id === itemPopup.item.id)) closeItemPopup();
  }, [shown, itemPopup, closeItemPopup]);
  const popupItem = items.find((item) => item.id === itemPopup?.item.id);
  const itemActions: MenuAction[] = popupItem ? [
    { label: t("Open"), onSelect: () => onOpen(popupItem) },
    ...(inTrash ? [
      { label: t("Restore"), onSelect: () => onRestore(popupItem) },
      { label: t("Delete Forever"), onSelect: () => onDeleteForever(popupItem) },
    ] : [
      { label: popupItem.starred ? t("Remove Star") : t("Add Star"), onSelect: () => onStar(popupItem) },
      { label: t("Move to Trash"), onSelect: () => onTrash(popupItem) },
    ]),
  ] : [];

  const header = (key: SortKey, label: string, className?: string) => (
    <th className={className} aria-sort={sort.key === key ? (sort.descending ? "descending" : "ascending") : undefined}>
      <button className="sort" onClick={() => setSort((s) => toggleSort(s, key))}>
        {label}
        {sort.key === key && <span aria-hidden="true">{sort.descending ? " ↓" : " ↑"}</span>}
      </button>
    </th>
  );

  const importFrom = async (action: () => Promise<string | null>) => {
    setReadingPdfs(true);
    try {
      const report = await action();
      if (report) setNotice(report);
    } finally {
      setReadingPdfs(false);
    }
  };
  const importActions: MenuAction[] = [
    { label: readingPdfs ? t("Reading PDFs…") : t("Add PDF…"), disabled: readingPdfs, onSelect: () => void importFrom(onImportPdfs) },
    { label: t("Import…"), onSelect: () => fileInput.current?.click() },
    { label: t("From Zotero…"), disabled: readingPdfs, onSelect: () => void importFrom(onImportZotero) },
    { label: t("From EndNote…"), disabled: readingPdfs, onSelect: () => void importFrom(onImportEndNote) },
  ];
  const exportActions: MenuAction[] = [
    ...formats.map((format) => ({ label: format.name, disabled: items.length === 0, onSelect: () => void exportAs(format.id) })),
    {
      label: counting ? t("Working…") : t("Count Citations"),
      disabled: counting || items.length === 0,
      onSelect: () => {
        setCounting(true);
        void backend.countCitations(items.map((item) => item.id)).then((count) => {
          setNotice(t("Citations counted for items: {count}.", { count }));
          onChanged();
        }, (error) => setNotice(error instanceof Error ? error.message : String(error))).finally(() => setCounting(false));
      },
    },
    { label: t("Cite in Word Document…"), onSelect: () => setCitingWord(true) },
  ];

  return (
    <div className="library-view">
      <header className="view-header">
        <h2 title={title}>{title}</h2>
        <label className="search-field">
          <Icon name="search" size={15} />
          <input type="search" value={query} placeholder={t("Titles, authors, DOIs")} onChange={(e) => setQuery(e.target.value)} />
        </label>
        {inTrash ? (
          <button
            className="danger"
            disabled={items.length === 0}
            onClick={() => {
              if (window.confirm(t("Items in the trash, with their files and annotations, are deleted for good."))) onEmptyTrash();
            }}
          >
            {t("Empty Trash")}
          </button>
        ) : (
          <>
            <input
              ref={fileInput}
              type="file"
              hidden
              multiple
              accept=".ris,.enw,.bib,.nbib,.json,.txt,.csv,.xml"
              onChange={async (e) => {
                const files = Array.from(e.target.files ?? []);
                e.target.value = "";
                if (files.length) setNotice(await onImport(files));
              }}
            />
            <div className="library-actions">
              <MenuButton label={t("Import")} items={importActions} />
              <MenuButton label={t("Export…")} items={exportActions} />
            </div>
            <MenuButton label={t("More")} icon={<span aria-hidden="true">⋯</span>} iconOnly className="library-more" items={[...importActions, ...exportActions]} />
            <button className="primary library-add" aria-label={t("Add Item")} title={t("Add Item")} onClick={onAdd}>
              <Icon name="plus" size={15} />
              <span>{t("Add Item")}</span>
            </button>
          </>
        )}
      </header>
      {citingWord && <WordCiteDialog onClose={() => setCitingWord(false)} />}
      {notice && (
        <p className="notice" role="status">
          {notice}
          <button className="icon-button" aria-label={t("OK")} onClick={() => setNotice(null)}>
            <Icon name="close" size={12} />
          </button>
        </p>
      )}
      {items.length === 0 ? (
        <div className="empty">
          <Icon name={inTrash ? "trash" : "library"} size={40} />
          {inTrash ? (
            <h3>{t("Trash Is Empty")}</h3>
          ) : (
            <>
              <h3>{t("No Items Yet")}</h3>
              <p>{t("Add a paper with the + button: paste its DOI or fill in the details.")}</p>
            </>
          )}
        </div>
      ) : shown.length === 0 ? (
        <div className="empty">
          <Icon name="search" size={40} />
          <p>{t("No results for “{query}”.", { query })}</p>
        </div>
      ) : (
        <div className="table-scroll">
          <table className="items">
            <thead>
              <tr>
                <th className="star">
                  <span className="visually-hidden">{t("Starred")}</span>
                </th>
                {header("title", t("Title"))}
                {header("authors", t("Authors"), "col-authors")}
                {header("year", t("Year"), "narrow col-year")}
                <th className="col-publication">{t("Publication")}</th>
                {header("citations", t("Cited by"), "narrow optional")}
                {header("dateAdded", t("Date Added"), "col-date optional")}
                {columns.map((column) => <th key={`${column.plugin}:${column.key}`} className="plugin-column">{column.label}</th>)}
                <th className="actions">
                  <span className="visually-hidden">{t("Actions")}</span>
                </th>
              </tr>
            </thead>
            <tbody>
              {shown.map((item) => (
                <tr key={item.id} onClick={(event) => {
                  pluginHost.selectionChanged([item.id]);
                  // Phones have no double tap: a tap anywhere on the row opens it.
                  if (window.matchMedia?.("(max-width: 720px)").matches && !(event.target as Element).closest("button, a, input")) onOpen(item);
                }} onDoubleClick={() => onOpen(item)} onContextMenu={(event) => {
                  event.preventDefault();
                  pluginHost.selectionChanged([item.id]);
                  const request = ++popupRequest.current;
                  setItemPopup({ x: event.clientX, y: event.clientY, item, menus: [] });
                  void pluginHost.itemMenu([item.id]).then((menus) => {
                    if (request === popupRequest.current) {
                      setItemPopup((popup) => popup?.item.id === item.id ? { ...popup, menus } : popup);
                    }
                  });
                }}>
                  <td className="star">
                    {!inTrash && (
                      <button
                        className={item.starred ? "icon-button starred" : "icon-button"}
                        aria-pressed={item.starred}
                        title={item.starred ? t("Remove Star") : t("Add Star")}
                        aria-label={item.starred ? t("Remove Star") : t("Add Star")}
                        onClick={() => onStar(item)}
                      >
                        <Icon name="star" size={15} />
                      </button>
                    )}
                  </td>
                  <td>
                    <button className="link" onClick={() => onOpen(item)}>
                      <span className="title-text">{item.title}</span>
                    </button>
                    {(item.hasPdf || item.doi || item.tags.length > 0) && (
                      <div className="item-meta">
                        {item.hasPdf && (
                          <span className="pdf-badge" title="PDF">
                            PDF
                          </span>
                        )}
                        {item.doi && <span className="doi">{item.doi}</span>}
                        {item.tags.map((tag) => (
                          <TagChip key={tag} name={tag} color={colors.get(tag) ?? null} />
                        ))}
                      </div>
                    )}
                  </td>
                  <td className="col-authors">{item.authors}</td>
                  <td className="narrow col-year">{item.year ?? ""}</td>
                  <td className="col-publication">{item.containerTitle ?? ""}</td>
                  <td className="narrow optional">{item.citedBy ?? ""}</td>
                  <td className="col-date muted optional">{item.dateAdded ? new Date(item.dateAdded).toLocaleDateString() : ""}</td>
                  {columns.map((column) => (
                    <td key={`${column.plugin}:${column.key}`} className="plugin-column">{columnValues[`${column.plugin}:${column.key}`]?.[item.id] ?? ""}</td>
                  ))}
                  <td className="actions">
                    {inTrash ? (
                      <>
                        <button className="icon-button" title={t("Restore")} aria-label={t("Restore")} onClick={() => onRestore(item)}>
                          <Icon name="restore" size={15} />
                        </button>
                        <button
                          className="icon-button danger"
                          title={t("Delete Forever")}
                          aria-label={t("Delete Forever")}
                          onClick={() => onDeleteForever(item)}
                        >
                          <Icon name="close" size={15} />
                        </button>
                      </>
                    ) : (
                      <button className="icon-button" title={t("Move to Trash")} aria-label={t("Move to Trash")} onClick={() => onTrash(item)}>
                        <Icon name="trash" size={15} />
                      </button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {itemPopup && <ItemPopup key={popupRequest.current} popup={itemPopup} actions={itemActions} onClose={closeItemPopup} />}
    </div>
  );
}

const KIND_NAMES: Record<IdentifierKind, string> = { doi: "DOI", arxiv: "arXiv", pmid: "PubMed", isbn: "ISBN" };

function fileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(0)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** An item's attached files: read a PDF, open others, attach more, remove. */
function FilesSection({ itemId, onRead }: { itemId: string; onRead: (file: AttachmentInfo) => void }) {
  const { t } = useI18n();
  const [files, setFiles] = useState<AttachmentInfo[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [finding, setFinding] = useState(false);
  const reload = () => {
    backend.attachments(itemId).then(setFiles, (e) => setError(String(e)));
  };
  useEffect(reload, [itemId]);
  const act = (action: Promise<unknown>) => action.then(reload, (e) => setError(e instanceof Error ? e.message : String(e)));
  const findPdf = () => {
    setFinding(true);
    setError(null);
    backend
      .findAvailablePdf(itemId)
      .then(
        (found) => (found ? reload() : setError(t("No open-access PDF was found."))),
        (e) => setError(e instanceof Error ? e.message : String(e)),
      )
      .finally(() => setFinding(false));
  };

  return (
    <section className="files">
      <header>
        <h2>{t("Files")}</h2>
        {!files.some((f) => f.isPdf) && (
          <button onClick={findPdf} disabled={finding}>
            {finding ? t("Finding a PDF…") : t("Find Available PDF")}
          </button>
        )}
        <button onClick={() => act(backend.attachFiles(itemId))}>{t("Attach File…")}</button>
      </header>
      {error && <p className="notice">{error}</p>}
      {files.length === 0 && <p className="muted">{t("Attach the paper's PDF to read and mark it up here.")}</p>}
      {files.map((file) => (
        <div className="file-row" key={file.id}>
          <Icon name="doc" size={20} />
          <span className="file-name">{file.title}</span>
          <span className="muted">{fileSize(file.size)}</span>
          {file.isPdf ? (
            <button className="primary" onClick={() => onRead(file)}>
              {t("Read")}
            </button>
          ) : (
            <button onClick={() => act(pluginHost.openAttachment(file.id))}>{t("Open")}</button>
          )}
          <button className="icon-button" title={t("Remove")} aria-label={t("Remove")} onClick={() => act(backend.removeAttachment(file.id))}>
            <Icon name="trash" size={15} />
          </button>
        </div>
      ))}
    </section>
  );
}


const READ_STATUSES: { id: ReadStatus; label: string }[] = [
  { id: "unread", label: "Unread" },
  { id: "reading", label: "In Progress" },
  { id: "read", label: "Finished" },
];

/** The notes that link to an item or quote its highlights. */
function NotesSection({ itemId, onOpenNote }: { itemId: string; onOpenNote: (path: string) => void }) {
  const { t } = useI18n();
  const [found, setFound] = useState<NoteMention[]>([]);
  useEffect(() => {
    let current = true;
    backend.vaultItemMentions(itemId).then(
      (mentions) => current && setFound(mentions),
      () => current && setFound([]),
    );
    return () => {
      current = false;
    };
  }, [itemId]);

  return (
    <section className="files">
      <header>
        <h2>{t("Notes")}</h2>
      </header>
      {found.length === 0 ? (
        <p className="muted">{t("No notes mention this paper yet.")}</p>
      ) : (
        <ul className="mentions">
          {found.map((m) => (
            <li key={`${m.path}:${m.line}`}>
              <button className="link" onClick={() => onOpenNote(m.path)}>
                {m.title}
              </button>
              <p>{m.context}</p>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

/** The items related to this one, as Zotero relates them: open one, add
 * one found by title, author or DOI, or take one off. */
function RelatedSection({ itemId, onOpen }: { itemId: string; onOpen: (item: ItemSummary) => void }) {
  const message = (e: unknown) => (e instanceof Error ? e.message : String(e));
  const { t } = useI18n();
  const [related, setRelated] = useState<ItemSummary[]>([]);
  const [adding, setAdding] = useState(false);
  const [query, setQuery] = useState("");
  const [library, setLibrary] = useState<ItemSummary[]>([]);
  const [error, setError] = useState<string | null>(null);
  const reload = useCallback(() => {
    backend.relatedItems(itemId).then(setRelated, (e) => setError(message(e)));
  }, [itemId]);
  useEffect(reload, [reload]);
  useEffect(() => {
    if (adding) backend.listItems({}).then(setLibrary, (e) => setError(message(e)));
  }, [adding]);
  const act = (action: Promise<unknown>) => action.then(reload, (e) => setError(message(e)));

  const words = query.trim().toLowerCase();
  const candidates = words
    ? library
        .filter((i) => i.id !== itemId && !related.some((r) => r.id === i.id))
        .filter((i) => [i.title, i.authors, i.doi ?? ""].some((text) => text.toLowerCase().includes(words)))
        .slice(0, 8)
    : [];

  return (
    <section className="files related">
      <header>
        <h2>{t("Related")}</h2>
        <button onClick={() => setAdding(!adding)}>{adding ? t("Done") : t("Add Related Item…")}</button>
      </header>
      {error && <p className="notice">{error}</p>}
      {adding && (
        <div className="related-picker">
          <input
            autoFocus
            value={query}
            placeholder={t("Titles, authors, DOIs")}
            aria-label={t("Titles, authors, DOIs")}
            onChange={(e) => setQuery(e.target.value)}
          />
          <ul>
            {candidates.map((item) => (
              <li key={item.id}>
                <button
                  className="link"
                  onClick={() => {
                    setQuery("");
                    void act(backend.relateItems(itemId, item.id));
                  }}
                >
                  {item.title}
                </button>
                <span className="muted">{[item.authors, item.year].filter(Boolean).join(" · ")}</span>
              </li>
            ))}
          </ul>
        </div>
      )}
      {related.length === 0 && !adding && <p className="muted">{t("No related items yet.")}</p>}
      {related.map((item) => (
        <div className="file-row" key={item.id}>
          <Icon name="link" size={18} />
          <button className="link file-name" onClick={() => onOpen(item)}>
            {item.title}
          </button>
          <span className="muted">{[item.authors, item.year].filter(Boolean).join(" · ")}</span>
          <button
            className="icon-button"
            title={t("Remove Relation")}
            aria-label={t("Remove Relation")}
            onClick={() => act(backend.unrelateItems(itemId, item.id))}
          >
            <Icon name="close" size={15} />
          </button>
        </div>
      ))}
    </section>
  );
}

/** A section a Zotero plugin adds to the item pane, as it drew it for this item. */
function PluginSectionView({ section, itemId, version }: { section: PluginSection; itemId: string; version: number }) {
  const [html, setHtml] = useState<string | null>(null);
  useEffect(() => {
    let current = true;
    void pluginHost.renderSection(section, itemId).then((drawn) => current && setHtml(drawn));
    return () => {
      current = false;
    };
  }, [section, itemId, version]);
  if (!html) return null;
  return (
    <section className="files">
      <header>
        <h2>{section.label}</h2>
      </header>
      <div className="plugin-block" dangerouslySetInnerHTML={{ __html: DOMPurify.sanitize(html) }} />
    </section>
  );
}

/** A summary of the paper by the AI provider set up in Settings. */
function AiSummarySection({ itemId, onOpenNote }: { itemId: string; onOpenNote: (path: string) => void }) {
  const { t, language } = useI18n();
  const [answer, setAnswer] = useState<AiAnswer | null>(null);
  const [working, setWorking] = useState(false);
  const [drafting, setDrafting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    setAnswer(null);
    setError(null);
  }, [itemId]);

  const summarize = async () => {
    setWorking(true);
    setError(null);
    try {
      setAnswer(await backend.aiSummarize(aiSettings(), itemId, language));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setWorking(false);
    }
  };

  /** A mind map of the paper, drafted by the AI provider, as a note in the vault. */
  const draftMap = async () => {
    setDrafting(true);
    setError(null);
    try {
      const drafted = await backend.aiMindmapItem(aiSettings(), itemId, language);
      const created = await backend.vaultCreateNote("", `${drafted.title} – ${t("Mind Map")}`);
      await backend.vaultWrite(created.path, drafted.text);
      onOpenNote(created.path);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setDrafting(false);
    }
  };

  return (
    <section className="files">
      <header>
        <h2>{t("AI Summary")}</h2>
        <button disabled={working} onClick={() => void summarize()}>
          {working ? t("Working…") : t("Summarize")}
        </button>
        <button disabled={drafting} onClick={() => void draftMap()}>
          {drafting ? t("Drafting…") : t("Mind Map")}
        </button>
      </header>
      {error && <p className="notice">{error}</p>}
      {answer && <AiAnswerView answer={answer} />}
    </section>
  );
}

interface ItemViewProps {
  itemId: string;
  tags: TagInfo[];
  onRead: (file: AttachmentInfo, title: string) => void;
  /** After a change the item list should show. */
  onChanged: () => void;
  /** Opens a note (the item's literature note). */
  onOpenNote: (path: string) => void;
  /** Opens another item (a related one). */
  onOpenItem: (item: ItemSummary) => void;
}

/** An item's details, tags and files. */
export function ItemView({ itemId, tags, onRead, onChanged, onOpenNote, onOpenItem }: ItemViewProps) {
  const { t } = useI18n();
  const sections = usePlugins().sections;
  const [sectionVersion, setSectionVersion] = useState(0);
  const [item, setItem] = useState<ItemDetail | null | undefined>(undefined);
  const [editing, setEditing] = useState(false);
  const [newTag, setNewTag] = useState("");
  const [error, setError] = useState<string | null>(null);
  const load = () => {
    backend.itemDetail(itemId).then(setItem, () => setItem(null));
    setSectionVersion((v) => v + 1);
  };
  useEffect(load, [itemId]);
  const change = (action: Promise<unknown>) =>
    action.then(
      () => {
        load();
        onChanged();
      },
      (e) => setError(e instanceof Error ? e.message : String(e)),
    );
  const colors = new Map(tags.map((tag) => [tag.name, tag.color]));

  if (item === undefined) return null;
  if (item === null) {
    return (
      <div className="empty">
        <h3>{t("Item Not Found")}</h3>
      </div>
    );
  }
  const rows: [string, string | null][] = [
    ["Volume", item.volume],
    ["Issue", item.issue],
    ["Pages", item.pages],
    ["Publisher", item.publisher],
    ["Place", item.place],
    ["ISBN", item.isbn],
    ["ISSN", item.issn],
    ["arXiv", item.arxivId],
    ["PMID", item.pmid],
    ["Keywords", item.keywords.length ? item.keywords.join("; ") : null],
    ["Cited by", item.citedBy != null ? item.citedBy.toLocaleString() : null],
  ];
  return (
    <article className="item-view">
      <p className="kicker">{t(ITEM_TYPE_NAMES[item.itemType] ?? "Other")}</p>
      <h1>{item.title}</h1>
      {item.authors.length > 0 && <p className="authors">{item.authors.join(", ")}</p>}
      <div className="item-actions">
        <button
          className={item.starred ? "starred" : ""}
          aria-pressed={item.starred}
          onClick={() => change(backend.setStarred(item.id, !item.starred))}
        >
          <Icon name="star" size={15} />
          <span>{item.starred ? t("Remove Star") : t("Add Star")}</span>
        </button>
        <select
          aria-label={t("Reading Status")}
          value={item.readStatus}
          onChange={(e) => change(backend.setReadStatus(item.id, e.target.value as ReadStatus))}
        >
          {READ_STATUSES.map((status) => (
            <option key={status.id} value={status.id}>
              {t(status.label)}
            </option>
          ))}
        </select>
        <button onClick={() => setEditing(true)}>
          <Icon name="edit" size={15} />
          <span>{t("Edit")}</span>
        </button>
        <button
          onClick={() =>
            backend.vaultCreateLiteratureNote(item.id).then(
              (note) => onOpenNote(note.path),
              (e) => setError(e instanceof Error ? e.message : String(e)),
            )
          }
        >
          <Icon name="note" size={15} />
          <span>{t("Literature Note")}</span>
        </button>
        <button
          onClick={() =>
            backend
              .itemLink(item.id)
              .then((link) => navigator.clipboard.writeText(link))
              .then(
                () => setError(t("Copied. Paste it into a note to link to this item.")),
                (e) => setError(e instanceof Error ? e.message : String(e)),
              )
          }
        >
          <Icon name="link" size={15} />
          <span>{t("Copy for Notes")}</span>
        </button>
      </div>
      <div className="item-tags">
        {item.tags.map((tag) => (
          <span key={tag} className="tag-editable">
            <TagChip name={tag} color={colors.get(tag) ?? null} />
            <button
              className="icon-button"
              aria-label={t("Remove")}
              title={t("Remove")}
              onClick={() => change(backend.setItemTags(item.id, item.tags.filter((x) => x !== tag)))}
            >
              <Icon name="close" size={11} />
            </button>
          </span>
        ))}
        <form
          onSubmit={(e) => {
            e.preventDefault();
            const name = newTag.trim();
            setNewTag("");
            if (name && !item.tags.includes(name)) void change(backend.setItemTags(item.id, [...item.tags, name]));
          }}
        >
          <input list="known-tags" value={newTag} placeholder={t("Add Tag")} aria-label={t("Add Tag")} onChange={(e) => setNewTag(e.target.value)} />
          <datalist id="known-tags">
            {tags
              .filter((tag) => !item.tags.includes(tag.name))
              .map((tag) => (
                <option key={tag.name} value={tag.name} />
              ))}
          </datalist>
        </form>
      </div>
      {error && <p className="notice">{error}</p>}
      {editing && (
        <EditItemDialog
          detail={item}
          onClose={() => setEditing(false)}
          onSave={async (detail) => {
            try {
              setItem(await backend.updateItem(detail));
              onChanged();
              return null;
            } catch (e) {
              return e instanceof Error ? e.message : String(e);
            }
          }}
        />
      )}
      <div className="facts">
        {item.year !== null && <span className="chip">{item.year}</span>}
        {item.containerTitle && <span className="chip">{item.containerTitle}</span>}
        {item.doi && (
          <a className="chip link-chip" href={`https://doi.org/${item.doi}`} target="_blank" rel="noreferrer">
            <Icon name="link" size={14} />
            {item.doi}
          </a>
        )}
      </div>
      {item.abstractNote && (
        <section className="abstract">
          <h2>{t("Abstract")}</h2>
          <p>{item.abstractNote}</p>
        </section>
      )}
      <dl className="fields">
        {rows
          .filter((row): row is [string, string] => !!row[1])
          .map(([label, value]) => (
            <div key={label}>
              <dt>{t(label)}</dt>
              <dd>{value}</dd>
            </div>
          ))}
      </dl>
      <FilesSection itemId={item.id} onRead={(file) => onRead(file, item.title)} />
      <NotesSection itemId={item.id} onOpenNote={onOpenNote} />
      <RelatedSection itemId={item.id} onOpen={onOpenItem} />
      <AiSummarySection itemId={item.id} onOpenNote={onOpenNote} />
      {sections.map((section) => (
        <PluginSectionView key={`${section.plugin}:${section.paneID}`} section={section} itemId={item.id} version={sectionVersion} />
      ))}
      <CitationBox itemIds={[item.id]} />
    </article>
  );
}

interface DialogProps {
  onAdd: (item: NewItem) => Promise<string | null>;
  onAddIdentifier: (input: string) => Promise<string | null>;
  onClose: () => void;
}

/** Adds an item: by identifier, with every detail looked up, or by hand. */
export function AddItemDialog({ onAdd, onAddIdentifier, onClose }: DialogProps) {
  const { t } = useI18n();
  const [form, setForm] = useState({ itemType: "journal-article", title: "", authors: "", container: "", year: "", doi: "" });
  const [doiCheck, setDoiCheck] = useState<string | null | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);
  const [identifier, setIdentifier] = useState("");
  const [kind, setKind] = useState<IdentifierKind | null>(null);
  const [lookingUp, setLookingUp] = useState(false);
  const set = (field: keyof typeof form) => (e: { target: { value: string } }) => setForm({ ...form, [field]: e.target.value });

  useEffect(() => {
    if (!form.doi.trim()) {
      setDoiCheck(undefined);
      return;
    }
    let current = true;
    backend.normalizeDoi(form.doi).then((n) => current && setDoiCheck(n));
    return () => {
      current = false;
    };
  }, [form.doi]);

  useEffect(() => {
    let current = true;
    backend.recognizeIdentifier(identifier).then((k) => current && setKind(k));
    return () => {
      current = false;
    };
  }, [identifier]);

  const lookUp = async () => {
    if (!kind || lookingUp) return;
    setLookingUp(true);
    setError(null);
    const message = await onAddIdentifier(identifier);
    setLookingUp(false);
    if (message) setError(message);
    else onClose();
  };

  const submit = async () => {
    const year = Number.parseInt(form.year.trim(), 10);
    const message = await onAdd({
      itemType: form.itemType,
      title: form.title.trim(),
      authors: form.authors
        .split(/[\n;；]/)
        .map((a) => a.trim())
        .filter(Boolean),
      containerTitle: form.container.trim() || null,
      year: Number.isFinite(year) ? year : null,
      doi: form.doi.trim() || null,
    });
    if (message) setError(message);
    else onClose();
  };

  return (
    <div className="dialog-backdrop" role="presentation" onClick={onClose}>
      <div
        className="dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="add-item-title"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && onClose()}
      >
        <form
          className="dialog-form"
          onSubmit={(e) => {
            e.preventDefault();
            void submit();
          }}
        >
          <h2 id="add-item-title">{t("Add Item")}</h2>
          <fieldset>
            <legend>{t("Add by identifier")}</legend>
            <div className="row">
              <input
                autoFocus
                value={identifier}
                placeholder={t("DOI, arXiv ID, PMID or ISBN")}
                aria-label={t("DOI, arXiv ID, PMID or ISBN")}
                onChange={(e) => setIdentifier(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    void lookUp();
                  }
                }}
              />
              <button type="button" className="primary" disabled={!kind || lookingUp} onClick={() => void lookUp()}>
                <Icon name="search" size={15} />
                <span>{lookingUp ? t("Looking up…") : t("Look Up and Add")}</span>
              </button>
            </div>
            {identifier.trim() !== "" &&
              (kind ? (
                <p className="hint ok">✓ {t("Recognized as {kind}", { kind: KIND_NAMES[kind] })}</p>
              ) : (
                <p className="hint warning">{t("Not a DOI, arXiv ID, PMID or ISBN")}</p>
              ))}
          </fieldset>
          <fieldset>
            <legend>{t("Add by hand")}</legend>
            <label>
              {t("Type")}
              <select value={form.itemType} onChange={set("itemType")}>
                {ITEM_TYPES.map((type) => (
                  <option key={type} value={type}>
                    {t(ITEM_TYPE_NAMES[type])}
                  </option>
                ))}
              </select>
            </label>
            <label>
              {t("Title")}
              <textarea rows={2} value={form.title} onChange={set("title")} />
            </label>
            <label>
              {t("Authors")}
              <textarea rows={2} value={form.authors} placeholder={t("One per line, or separated by ;")} onChange={set("authors")} />
            </label>
            <label>
              {t("Journal or book")}
              <input value={form.container} onChange={set("container")} />
            </label>
            <label>
              {t("Year")}
              <input inputMode="numeric" value={form.year} onChange={set("year")} />
            </label>
            <label>
              {t("DOI")}
              <input value={form.doi} placeholder={t("10.1038/… or https://doi.org/…")} onChange={set("doi")} />
            </label>
            {doiCheck === null && <p className="hint warning">{t("Not a valid DOI")}</p>}
          </fieldset>
          {error && <p className="hint error">{error}</p>}
          <div className="row end">
            <button type="button" onClick={onClose}>
              {t("Cancel")}
            </button>
            <button type="submit" className="primary" disabled={!form.title.trim()}>
              {t("Add")}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
