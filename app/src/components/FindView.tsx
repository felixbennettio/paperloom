import DOMPurify from "dompurify";
import { useState, type FormEvent } from "react";
import { useI18n } from "../i18n";
import { aiSettings } from "../lib/ai";
import { backend, type AiLibraryAnswer, type AiSource, type FullTextHit, type FullTextKind } from "../lib/api";
import { AiAnswerView } from "./AiAnswer";
import { Icon, type IconName } from "./Icon";

const KINDS: { id: FullTextKind; label: string; icon: IconName }[] = [
  { id: "item", label: "Items", icon: "library" },
  { id: "pdf", label: "PDF Text", icon: "doc" },
  { id: "annotation", label: "Annotations", icon: "edit" },
  { id: "note", label: "Notes", icon: "note" },
];

interface Props {
  onOpenItem: (itemId: string, title: string) => void;
  onOpenPdf: (attachmentId: string, title: string, page: number | null) => void;
  onOpenNote: (path: string) => void;
}

/** Searches the whole library at once: items, PDF text, annotations and notes. */
export function FindView({ onOpenItem, onOpenPdf, onOpenNote }: Props) {
  const { t, language } = useI18n();
  const [query, setQuery] = useState("");
  const [answer, setAnswer] = useState<AiLibraryAnswer | null>(null);
  const [asking, setAsking] = useState(false);
  const [kinds, setKinds] = useState<FullTextKind[]>([]);
  const [hits, setHits] = useState<FullTextHit[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const run = async (words: string, only: FullTextKind[]) => {
    if (!words.trim()) return;
    try {
      setHits(await backend.searchEverything(words, only, 200));
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const toggle = (kind: FullTextKind) => {
    const next = kinds.includes(kind) ? kinds.filter((k) => k !== kind) : [...kinds, kind];
    setKinds(next);
    void run(query, next);
  };

  /** An answer from the whole library, by the AI provider set up in Settings. */
  const ask = async () => {
    if (!query.trim()) return;
    setAsking(true);
    setError(null);
    try {
      setAnswer(await backend.aiAskLibrary(aiSettings(), query, language));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setAsking(false);
    }
  };

  const openSource = (source: AiSource) => {
    if (source.kind === "note" && source.notePath) onOpenNote(source.notePath);
    else if ((source.kind === "pdf" || source.kind === "annotation") && source.attachmentId) onOpenPdf(source.attachmentId, source.title, source.pageIndex);
    else if (source.itemId) onOpenItem(source.itemId, source.title);
  };

  const open = (hit: FullTextHit) => {
    if (hit.kind === "note" && hit.notePath) onOpenNote(hit.notePath);
    else if ((hit.kind === "pdf" || hit.kind === "annotation") && hit.attachmentId) onOpenPdf(hit.attachmentId, hit.title, hit.pageIndex);
    else if (hit.itemId) onOpenItem(hit.itemId, hit.title);
  };

  return (
    <div className="find-view">
      <header className="view-header">
        <h2>{t("Search Everything")}</h2>
      </header>
      <form
        className="search-form"
        onSubmit={(e: FormEvent) => {
          e.preventDefault();
          void run(query, kinds);
        }}
      >
        <label className="search-field wide">
          <Icon name="search" size={15} />
          <input
            type="search"
            autoFocus
            value={query}
            placeholder={t("Words in titles, PDFs, annotations and notes")}
            onChange={(e) => setQuery(e.target.value)}
          />
        </label>
        <button type="button" disabled={asking || !query.trim()} onClick={() => void ask()}>
          {asking ? t("Asking…") : t("Ask the Library")}
        </button>
        <div className="segmented" role="group" aria-label={t("Show")}>
          {KINDS.map((kind) => (
            <button key={kind.id} type="button" aria-pressed={kinds.includes(kind.id)} className={kinds.includes(kind.id) ? "selected" : ""} onClick={() => toggle(kind.id)}>
              {t(kind.label)}
            </button>
          ))}
        </div>
      </form>
      {error && <p className="notice">{error}</p>}
      {answer && (
        <section className="library-answer">
          <AiAnswerView answer={{ text: answer.text, cutShort: answer.cutShort, pages: [] }} />
          <h3>{t("Sources")}</h3>
          <ol className="sources">
            {answer.sources.map((source) => (
              <li key={source.number}>
                <button className="link" onClick={() => openSource(source)}>
                  [{source.number}] {source.title || t("Untitled")}
                  {source.pageIndex !== null && ` · ${t("Page {page}", { page: source.pageIndex + 1 })}`}
                </button>
              </li>
            ))}
          </ol>
        </section>
      )}
      {hits === null ? (
        <div className="empty">
          <Icon name="search" size={40} />
          <p>{t("Find words anywhere: in titles and abstracts, inside PDFs, in your annotations and notes.")}</p>
          <p className="muted">{t("All words must appear; use \"quotes\" for a phrase and -word to leave one out.")}</p>
        </div>
      ) : hits.length === 0 ? (
        <div className="empty">
          <p>{t("No results for “{query}”.", { query })}</p>
        </div>
      ) : (
        <ul className="hits">
          {hits.map((hit, n) => {
            const kind = KINDS.find((k) => k.id === hit.kind);
            return (
              <li key={`${hit.kind}:${hit.itemId ?? ""}:${hit.notePath ?? ""}:${hit.annotationId ?? ""}:${hit.pageIndex ?? ""}:${n}`} className="hit">
                <Icon name={kind?.icon ?? "doc"} size={18} />
                <div className="hit-body">
                  <button className="hit-title" onClick={() => open(hit)}>
                    {hit.title || t("Untitled")}
                  </button>
                  <p className="hit-line muted">
                    {t(kind?.label ?? "Items")}
                    {hit.pageIndex !== null && ` · ${t("Page {page}", { page: hit.pageIndex + 1 })}`}
                    {hit.notePath && ` · ${hit.notePath}`}
                  </p>
                  {hit.snippetHtml && (
                    <p className="hit-abstract" dangerouslySetInnerHTML={{ __html: DOMPurify.sanitize(hit.snippetHtml, { ALLOWED_TAGS: ["b"] }) }} />
                  )}
                </div>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}
