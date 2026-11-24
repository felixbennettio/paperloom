import { useEffect, useState, type FormEvent } from "react";
import { useI18n } from "../i18n";
import { backend, type CaptureSite, type SearchHit } from "../lib/api";
import { HitList, markAdded } from "./HitList";
import { translatorHost, type DetectedTranslator } from "../lib/translators";
import { MenuButton } from "./MenuButton";
import { Icon } from "./Icon";

/** What the in-app browser shows and what was read from it; kept by the
 * workbench, since the browser window lives on when this tab closes. */
export interface BrowserState {
  url: string;
  title: string;
  reading: boolean;
  translator: DetectedTranslator | null;
  /** The works read from the page, once it was read. */
  hits: SearchHit[] | null;
  notice: string | null;
}

export const NO_BROWSER: BrowserState = { url: "", title: "", reading: false, translator: null, hits: null, notice: null };

/** Scholarly sites worth starting from besides those the rules read. */
const MORE_SITES: CaptureSite[] = [
  { id: "scholar", name: "Google Scholar", home: "https://scholar.google.com/" },
  { id: "baidu", name: "Baidu Scholar", home: "https://xueshu.baidu.com/" },
];

interface Props {
  state: BrowserState;
  onState: (change: (state: BrowserState) => BrowserState) => void;
  collectionId: string | null;
  collectionName: string | null;
  onAdded: () => void;
  onOpenItem: (id: string, title: string) => void;
}

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** Drives the in-app browser, a window of its own where a person browses
 * with their own access (CNKI, Wanfang, VIP, publishers), and lists what it
 * read from a page for adding. */
export function BrowserView({ state, onState, collectionId, collectionName, onAdded, onOpenItem }: Props) {
  const { t } = useI18n();
  const [sites, setSites] = useState<CaptureSite[]>([]);
  const [address, setAddress] = useState(state.url);
  const [saving, setSaving] = useState(false);
  const [chosen, setChosen] = useState<ReadonlySet<string>>(new Set());

  useEffect(() => {
    backend.captureSites().then((known) => setSites([...known, ...MORE_SITES]), () => setSites(MORE_SITES));
  }, []);
  useEffect(() => setAddress(state.url), [state.url]);
  useEffect(() => setChosen(new Set()), [state.hits]);

  const notice = (text: string | null) => onState((s) => ({ ...s, notice: text }));
  const visit = (where: string) => {
    if (where.trim()) backend.browserOpen(where).catch((e) => notice(message(e)));
  };
  const submit = (event: FormEvent) => {
    event.preventDefault();
    visit(address);
  };
  const read = () => {
    onState((s) => ({ ...s, reading: true, notice: null }));
    backend.browserRead().catch((e) => onState((s) => ({ ...s, reading: false, notice: message(e) })));
  };
  const save = async () => {
    const url = state.url;
    const translator = state.translator;
    if (!translator) return;
    setSaving(true);
    notice(null);
    try {
      const items = await translatorHost.save();
      const page = await backend.readTranslatedItems(JSON.stringify(items), url);
      onState((s) => s.url === url && s.translator === translator ? { ...s, hits: page.hits } : s);
    } catch (error) {
      onState((s) => s.url === url && s.translator === translator ? { ...s, notice: message(error) } : s);
    } finally { setSaving(false); }
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
      onState((s) => ({ ...s, hits: s.hits && markAdded(s.hits, keys, report.addedIds) }));
      setChosen(new Set());
      notice(
        t("Added {added}, {duplicates} already in the library, {skipped} skipped.", {
          added: report.added,
          duplicates: report.duplicates,
          skipped: report.skipped,
        }),
      );
      onAdded();
    } catch (e) {
      notice(message(e));
    }
  };

  return (
    <div className="search-view browser-view">
      <header className="view-header">
        <h2>{t("Browser")}</h2>
      </header>
      <form className="search-form" onSubmit={submit}>
        <div className="browser-nav">
          <button type="button" className="icon-button" aria-label={t("Back")} title={t("Back")} onClick={() => void backend.browserGo("back")}>
            <Icon name="back" size={16} />
          </button>
          <button type="button" className="icon-button" aria-label={t("Forward")} title={t("Forward")} onClick={() => void backend.browserGo("forward")}>
            <Icon name="forward" size={16} />
          </button>
          <button type="button" className="icon-button" aria-label={t("Reload")} title={t("Reload")} onClick={() => void backend.browserGo("reload")}>
            <Icon name="restore" size={16} />
          </button>
        </div>
        <label className="search-field wide">
          <Icon name="globe" size={15} />
          <input type="url" value={address} spellCheck={false} placeholder={t("Address or site")} onChange={(e) => setAddress(e.target.value)} />
        </label>
        <button className="primary" type="submit" disabled={!address.trim()}>
          {t("Open")}
        </button>
      </form>
      <div className="browser-sites">
        {sites.map((site) => (
          <button key={site.id} className="chip link-chip" onClick={() => visit(site.home)}>
            {t(site.name)}
          </button>
        ))}
      </div>
      {state.url && (
        <div className="results-bar browser-capture-bar">
          <span className="muted browser-page" title={state.url}>
            {state.title || state.url}
          </span>
          <span className="spacer" />
          {state.translator ? (
            <>
              <button className="primary translator-save" disabled={saving || state.reading} onClick={() => void save()} title={t("Save with {translator}", { translator: state.translator.label })}>
                {saving ? t("Saving…") : t("Save with {translator}", { translator: state.translator.label })}
              </button>
              <MenuButton label={t("More")} icon={<span aria-hidden="true">⋯</span>} iconOnly disabled={saving} items={[
                { label: state.reading ? t("Reading…") : t("Read This Page"), disabled: state.reading, onSelect: read },
              ]} />
            </>
          ) : (
            <button className="primary" disabled={state.reading || saving} onClick={read}>
              {state.reading ? t("Reading…") : t("Read This Page")}
            </button>
          )}
        </div>
      )}
      {state.notice && (
        <p className="notice" role="status">
          {state.notice}
          <button className="icon-button" aria-label={t("OK")} onClick={() => notice(null)}>
            <Icon name="close" size={12} />
          </button>
        </p>
      )}
      {state.hits === null ? (
        <div className="empty">
          <Icon name="compass" size={40} />
          <h3>{t("Browse with your own access, and save what you find.")}</h3>
          <p>
            {t(
              "Sign in to your library's databases in the browser window. On a list of results or a paper's page, press ＋ Paperloom or Read This Page to pick what to add. PDFs you download are added too.",
            )}
          </p>
        </div>
      ) : state.hits.length === 0 ? (
        <div className="empty">
          <Icon name="compass" size={40} />
          <p>{t("Nothing on this page could be read as a work.")}</p>
        </div>
      ) : (
        <>
          <div className="results-bar">
            <span className="muted">{t("Results: {count}", { count: state.hits.length })}</span>
            <span className="spacer" />
            {collectionName && chosen.size > 0 && <span className="muted">{t("Adds to “{name}”", { name: collectionName })}</span>}
            <button className="primary" disabled={chosen.size === 0} onClick={() => void add([...chosen])}>
              {t("Add {count} Selected", { count: chosen.size })}
            </button>
          </div>
          <HitList
            hits={state.hits}
            chosen={chosen}
            onToggle={toggle}
            onAdd={(keys) => void add(keys)}
            onOpenItem={onOpenItem}
            onOpenUrl={visit}
          />
        </>
      )}
    </div>
  );
}
