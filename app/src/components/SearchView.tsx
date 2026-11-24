import { useEffect, useState, type FormEvent } from "react";
import { useI18n } from "../i18n";
import { backend, type SearchHit, type SearchSource, type SruCatalogue } from "../lib/api";
import { CATALOGUES_CHANGED, DATABASE_KEYS_CHANGED, loadCatalogues } from "../lib/databases";
import { HitList, markAdded } from "./HitList";
import { Icon } from "./Icon";

interface Props {
  /** Where added results go: the selected collection, if any. */
  collectionId: string | null;
  collectionName: string | null;
  /** Called after results were added, so the library list can reload. */
  onAdded: () => void;
  onOpenItem: (id: string, title: string) => void;
}

interface SearchState {
  sourceId: string;
  text: string;
  page: number;
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Searches the open scholarly databases (and those the person has a key
 * for, and the library catalogues they added) and adds the results a
 * person picks. */
export function SearchView({ collectionId, collectionName, onAdded, onOpenItem }: Props) {
  const { t } = useI18n();
  const [allSources, setSources] = useState<SearchSource[]>([]);
  const [keyed, setKeyed] = useState<string[]>([]);
  const [catalogues, setCatalogues] = useState<SruCatalogue[]>(() => loadCatalogues());
  const [sourceId, setSourceId] = useState("crossref");
  const [text, setText] = useState("");
  const [search, setSearch] = useState<SearchState | null>(null);
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [total, setTotal] = useState<number | null>(null);
  const [hasMore, setHasMore] = useState(false);
  const [busy, setBusy] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [chosen, setChosen] = useState<ReadonlySet<string>>(new Set());

  useEffect(() => {
    backend.searchSources().then(setSources, () => setSources([]));
    const loadKeys = () => backend.databaseKeysKnown().then(setKeyed, () => setKeyed([]));
    void loadKeys();
    window.addEventListener(DATABASE_KEYS_CHANGED, loadKeys);
    const reloadCatalogues = () => setCatalogues(loadCatalogues());
    window.addEventListener(CATALOGUES_CHANGED, reloadCatalogues);
    return () => {
      window.removeEventListener(DATABASE_KEYS_CHANGED, loadKeys);
      window.removeEventListener(CATALOGUES_CHANGED, reloadCatalogues);
    };
  }, []);
  // A database that needs a key is offered once there is one; the
  // catalogues the person added come last.
  const sources: SearchSource[] = [
    ...allSources.filter((s) => !s.needsKey || keyed.includes(s.id)),
    ...catalogues.map((c) => ({ id: c.id, name: c.name, homepage: c.url, needsKey: false })),
  ];
  const missingKeys = allSources.filter((s) => s.needsKey && !keyed.includes(s.id));

  const run = async (next: SearchState) => {
    setBusy(true);
    setNotice(null);
    try {
      const catalogue = catalogues.find((c) => c.id === next.sourceId);
      const page = catalogue
        ? await backend.searchCatalogue(catalogue, next.text, next.page)
        : await backend.searchOnline(next.sourceId, next.text, next.page);
      setHits((shown) =>
        next.page === 0 ? page.hits : [...shown, ...page.hits.filter((hit) => !shown.some((s) => s.key === hit.key))],
      );
      if (next.page === 0) setChosen(new Set());
      setTotal(page.total);
      setHasMore(page.hasMore);
      setSearch(next);
    } catch (e) {
      setNotice(message(e));
    } finally {
      setBusy(false);
    }
  };

  const submit = (event: FormEvent) => {
    event.preventDefault();
    const query = text.trim();
    if (query && !busy) void run({ sourceId, text: query, page: 0 });
  };

  const toggle = (key: string) =>
    setChosen((keys) => {
      const next = new Set(keys);
      if (!next.delete(key)) next.add(key);
      return next;
    });

  const add = async (keys: string[]) => {
    try {
      const report = await backend.addSearchResults(keys, collectionId);
      setHits((shown) => markAdded(shown, keys, report.addedIds));
      setChosen(new Set());
      setNotice(
        t("Added {added}, {duplicates} already in the library, {skipped} skipped.", {
          added: report.added,
          duplicates: report.duplicates,
          skipped: report.skipped,
        }),
      );
      onAdded();
    } catch (e) {
      setNotice(message(e));
    }
  };

  const open = (url: string) => {
    backend.openUrl(url).catch((e) => setNotice(message(e)));
  };

  return (
    <div className="search-view">
      <header className="view-header">
        <h2>{t("Online Search")}</h2>
      </header>
      <form className="search-form" onSubmit={submit}>
        <div className="segmented sources" role="radiogroup" aria-label={t("Database")}>
          {sources.map((source) => (
            <button
              key={source.id}
              type="button"
              role="radio"
              aria-checked={source.id === sourceId}
              className={source.id === sourceId ? "selected" : ""}
              onClick={() => {
                setSourceId(source.id);
                if (search && search.text === text.trim()) void run({ sourceId: source.id, text: search.text, page: 0 });
              }}
            >
              {source.name}
            </button>
          ))}
        </div>
        <label className="search-field wide">
          <Icon name="search" size={15} />
          <input
            type="search"
            value={text}
            autoFocus
            placeholder={t("Title, authors, keywords")}
            onChange={(e) => setText(e.target.value)}
          />
        </label>
        <button className="primary" type="submit" disabled={busy || !text.trim()}>
          {busy ? t("Searching…") : t("Search")}
        </button>
      </form>
      {notice && (
        <p className="notice" role="status">
          {notice}
          <button className="icon-button" aria-label={t("OK")} onClick={() => setNotice(null)}>
            <Icon name="close" size={12} />
          </button>
        </p>
      )}
      {search === null ? (
        <div className="empty">
          <Icon name="search" size={40} />
          <h3>{t("Search Crossref, OpenAlex, arXiv, PubMed and Semantic Scholar.")}</h3>
          {missingKeys.length > 0 && (
            <p>
              {t("Add an API key in Settings to search {names} too.", {
                names: missingKeys.map((s) => s.name).join(", "),
              })}
            </p>
          )}
        </div>
      ) : hits.length === 0 ? (
        <div className="empty">
          <Icon name="search" size={40} />
          <p>{t("No results for “{query}”.", { query: search.text })}</p>
        </div>
      ) : (
        <>
          <div className="results-bar">
            <span className="muted">{total !== null && t("Results: {count}", { count: total.toLocaleString() })}</span>
            <span className="spacer" />
            {collectionName && chosen.size > 0 && <span className="muted">{t("Adds to “{name}”", { name: collectionName })}</span>}
            <button className="primary" disabled={chosen.size === 0} onClick={() => void add([...chosen])}>
              {t("Add {count} Selected", { count: chosen.size })}
            </button>
          </div>
          <HitList
            hits={hits}
            chosen={chosen}
            onToggle={toggle}
            onAdd={(keys) => void add(keys)}
            onOpenItem={onOpenItem}
            onOpenUrl={open}
          />
          {hasMore && (
            <div className="load-more">
              <button disabled={busy} onClick={() => search && void run({ ...search, page: search.page + 1 })}>
                {busy ? t("Searching…") : t("Load More")}
              </button>
            </div>
          )}
        </>
      )}
    </div>
  );
}
