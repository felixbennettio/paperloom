import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useI18n } from "../i18n";
import { ConnectorError, type CiteConnector, type CitationStyle, type SearchItem } from "./connector";
import type { DocumentAdapter } from "./document";
import { citationHtml, type MemoryDocument } from "./memoryDocument";
import { CitationSession, DEFAULT_STYLE, type RefreshResult } from "./session";
import "./addin.css";

type Connection = "checking" | "not-running" | "not-connected" | "ready";
type Notice = { kind: "error"; error: unknown; source?: "search" } | { kind: "updated"; result: RefreshResult } | { kind: "unlinked" };

interface Props {
  client: CiteConnector;
  document: DocumentAdapter;
  clientName: string;
  preview?: MemoryDocument;
}

export function AddinApp({ client, document, clientName, preview }: Props) {
  const { t } = useI18n();
  const session = useMemo(() => new CitationSession(document, client), [document, client]);
  const [connection, setConnection] = useState<Connection>("checking");
  const [pairing, setPairing] = useState(false);
  const [styles, setStyles] = useState<CitationStyle[]>([]);
  const [styleId, setStyleId] = useState(DEFAULT_STYLE);
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchItem[]>([]);
  const [selected, setSelected] = useState<SearchItem[]>([]);
  const [page, setPage] = useState("");
  const [searching, setSearching] = useState(false);
  const [working, setWorking] = useState(false);
  const [confirmUnlink, setConfirmUnlink] = useState(false);
  const [notice, setNotice] = useState<Notice | null>(null);
  const [, redraw] = useState(0);
  const actionPending = useRef(false);
  const unlinkButton = useRef<HTMLButtonElement>(null);
  const cancelUnlink = useRef<HTMLButtonElement>(null);

  const reportError = useCallback((error: unknown, source?: "search") => {
    setNotice({ kind: "error", error, source });
    if (error instanceof ConnectorError) {
      if (error.code === "not-running") setConnection("not-running");
      if (error.code === "not-connected") setConnection("not-connected");
    }
  }, []);

  const connect = useCallback(async (pair = false, active: () => boolean = () => true) => {
    setConnection("checking");
    setPairing(pair);
    setNotice(null);
    try {
      await client.ping();
      if (!active()) return;
      if (pair) await client.pair(clientName);
      if (!active()) return;
      if (!client.hasToken()) { setConnection("not-connected"); return; }
      const [available, savedStyle] = await Promise.all([client.styles(), document.getStyleId()]);
      if (!active()) return;
      setStyles(available);
      setStyleId(savedStyle ?? DEFAULT_STYLE);
      setConnection("ready");
    } catch (error) {
      if (active()) {
        setConnection("not-connected");
        reportError(error);
      }
    } finally {
      if (active()) setPairing(false);
    }
  }, [client, clientName, document, reportError]);

  useEffect(() => {
    let active = true;
    void connect(false, () => active);
    return () => { active = false; };
  }, [connect]);

  useEffect(() => {
    if (connection !== "ready" || working) return;
    let active = true;
    setSearching(true);
    const timer = window.setTimeout(() => {
      void client.search(query, 20).then((items) => {
        if (active) {
          setResults(items);
          setSearching(false);
          setNotice((current) => current?.kind === "error" && current.source === "search" ? null : current);
        }
      }, (error: unknown) => {
        if (active) { setResults([]); setSearching(false); reportError(error, "search"); }
      });
    }, 300);
    // Ignore responses for previous queries, even if they arrive out of order.
    return () => { active = false; window.clearTimeout(timer); };
  }, [client, connection, query, working, reportError]);

  useEffect(() => {
    if (confirmUnlink) cancelUnlink.current?.focus();
  }, [confirmUnlink]);

  const perform = async (operation: () => Promise<RefreshResult | void>, inserted = false) => {
    if (actionPending.current) return;
    actionPending.current = true;
    setWorking(true);
    setNotice(null);
    try {
      const result = await operation();
      setNotice(result ? { kind: "updated", result } : { kind: "unlinked" });
      if (inserted) { setSelected([]); setPage(""); }
    } catch (error) {
      reportError(error);
    } finally {
      try {
        const savedStyle = await document.getStyleId();
        setStyleId(savedStyle ?? DEFAULT_STYLE);
      } catch (error) {
        reportError(error);
      } finally {
        redraw((value) => value + 1);
        setWorking(false);
        actionPending.current = false;
      }
    }
  };

  const errorText = (error: unknown): string => {
    if (error instanceof ConnectorError) {
      switch (error.code) {
        case "not-running": return t("Paperloom is not running on this computer");
        case "not-connected": return t("Not connected to Paperloom");
        case "declined": return t("The connection was declined in Paperloom. You can try again.");
        case "busy": return t("Another connection request is waiting in Paperloom. Try again after it is answered.");
      }
    }
    return t("Could not complete the request: {message}", { message: t(error instanceof Error ? error.message : String(error)) });
  };

  const closeConfirmation = () => {
    setConfirmUnlink(false);
    unlinkButton.current?.focus();
  };

  return (
    <main className="addin-pane" aria-busy={working || connection === "checking"}>
      <h1>{t("Paperloom Writing Plugins")}</h1>
      {connection === "checking" && <p role="status">{pairing ? t("Allow the connection in Paperloom…") : t("Checking Paperloom…")}</p>}
      {connection === "not-running" && <section>
        <h2>{t("Paperloom is not running on this computer")}</h2>
        <p>{t("Open Paperloom on this computer, then try again.")}</p>
        <button type="button" onClick={() => void connect()}>{t("Try Again")}</button>
      </section>}
      {connection === "not-connected" && <section>
        <h2>{t("Not connected to Paperloom")}</h2>
        <p>{t("Connect to search your library. Paperloom will ask you to allow this task pane.")}</p>
        <button type="button" onClick={() => void connect(true)}>{t("Connect")}</button>
      </section>}
      {connection === "ready" && <>
        <fieldset disabled={working}>
          <legend>{t("Insert Citation")}</legend>
          <label htmlFor="addin-search">{t("Search your library")}</label>
          <input id="addin-search" type="search" value={query} onChange={(event) => setQuery(event.target.value)} />
          <p role="status">{searching ? t("Searching…") : t("Search results: {count}", { count: results.length })}</p>
          <ul className="addin-results" aria-label={t("Search results")} aria-busy={searching}>
            {results.map((item) => <li key={item.id}>
              <label>
                <input type="checkbox" checked={selected.some((work) => work.id === item.id)} onChange={(event) => {
                  const checked = event.currentTarget.checked;
                  setSelected((current) => checked ? [...current, item] : current.filter((work) => work.id !== item.id));
                }} />
                <span><span className="addin-result-title">{item.title}</span><small>{item.cite}</small></span>
              </label>
            </li>)}
          </ul>
          {!searching && results.length === 0 && <p>{t("No matching works.")}</p>}
          {selected.length > 0 && <div className="addin-selected">
            <p>{t("Selected works: {count}", { count: selected.length })}</p>
            <ul>{selected.map((item) => <li key={item.id}>
              <span>{item.title}</span>
              <button type="button" aria-label={t("Deselect {title}", { title: item.title })}
                onClick={() => setSelected((current) => current.filter((work) => work.id !== item.id))}>{t("Remove")}</button>
            </li>)}</ul>
          </div>}
          <label htmlFor="addin-page">{t("Page (optional)")}</label>
          <input id="addin-page" type="text" value={page} onChange={(event) => setPage(event.target.value)} />
          <button type="button" disabled={selected.length === 0} onClick={() => void perform(() => session.insert(
            selected.map((item) => ({ itemId: item.id, ...(page.trim() ? { locator: page.trim() } : {}) })),
          ), true)}>{t("Insert Citation")}</button>
        </fieldset>
        <fieldset disabled={working}>
          <legend>{t("Document citations")}</legend>
          <label htmlFor="addin-style">{t("Style")}</label>
          <select id="addin-style" value={styleId} onChange={(event) => {
            const id = event.currentTarget.value;
            void perform(() => session.setStyle(id));
          }}>
            {!styles.some((style) => style.id === styleId) && <option value={styleId}>{styleId}</option>}
            {styles.map((style) => <option key={style.id} value={style.id}>{style.name}</option>)}
          </select>
          <button type="button" onClick={() => void perform(() => session.refresh())}>{t("Update Citations")}</button>
          <button type="button" onClick={() => void perform(() => session.insertBibliography())}>{t("Insert Reference List")}</button>
          <button type="button" onClick={() => void perform(() => session.convertMarkers())}>{t("Convert Citation Markers")}</button>
          <button ref={unlinkButton} type="button" onClick={() => setConfirmUnlink(true)}>{t("Convert to Plain Text")}</button>
          {confirmUnlink && <div role="group" aria-labelledby="addin-unlink-title" className="addin-confirm"
            onKeyDown={(event) => { if (event.key === "Escape") { event.preventDefault(); closeConfirmation(); } }}>
            <p id="addin-unlink-title">{t("Convert all citations and the reference list to plain text? They will no longer update automatically.")}</p>
            <button ref={cancelUnlink} type="button" onClick={closeConfirmation}>{t("Cancel")}</button>
            <button type="button" onClick={() => {
              closeConfirmation();
              void perform(() => session.unlink());
            }}>{t("Convert to Plain Text")}</button>
          </div>}
        </fieldset>
      </>}
      <div className="addin-status" role="status" aria-live="polite" aria-atomic="true">
        {working && <p>{t("Working…")}</p>}
        {notice?.kind === "error" && <p className="addin-error">{errorText(notice.error)}</p>}
        {notice?.kind === "updated" && <>
          <p>{t("Updated {citations} citations and {references} references.", { citations: notice.result.citations, references: notice.result.references })}</p>
          {notice.result.converted !== undefined && <p>{t("Citation markers converted: {count}.", { count: notice.result.converted })}</p>}
          {notice.result.unconverted && notice.result.unconverted.length > 0 && <p className="addin-error">
            {t("Left as written, as the library lacks a work they name: {markers}", { markers: notice.result.unconverted.join(" ") })}
          </p>}
          {notice.result.missing.length > 0 && <p className="addin-error">{t("Works missing from the library: {count}. Their saved citation text is kept when no works remain.", { count: notice.result.missing.length })}</p>}
        </>}
        {notice?.kind === "unlinked" && <p>{t("Citations and the reference list are now plain text.")}</p>}
      </div>
      {preview && <DocumentPreview document={preview} disabled={working} />}
    </main>
  );
}

function DocumentPreview({ document, disabled }: { document: MemoryDocument; disabled: boolean }) {
  const { t } = useI18n();
  const [, redraw] = useState(0);
  const runs = document.getRuns();
  return <section className="addin-preview" aria-labelledby="addin-preview-title">
    <h2 id="addin-preview-title">{t("Document preview")}</h2>
    <p>{t("This browser preview is stored in memory. Choose where to insert the next citation or reference list.")}</p>
    <label htmlFor="addin-selection">{t("Insertion position")}</label>
    <select id="addin-selection" disabled={disabled} value={document.getSelection()} onChange={(event) => {
      document.setSelection(Number(event.target.value));
      redraw((value) => value + 1);
    }}>
      {runs.map((_, index) => <option key={index} value={index}>{t("Before run {number}", { number: index + 1 })}</option>)}
      <option value={runs.length}>{t("End of document")}</option>
    </select>
    <div className="addin-preview-document">
      {runs.length === 0 && <p>{t("Your document preview appears here.")}</p>}
      {runs.map((run, index) => run.kind === "text" ? <p key={index}>{run.text}</p>
        : run.kind === "citation" ? <p key={index} dangerouslySetInnerHTML={{ __html: citationHtml(run.citation.text) }} />
          : <div key={index}>{run.entries.map((entry, number) => <p key={number} dangerouslySetInnerHTML={{ __html: citationHtml(entry) }} />)}</div>)}
    </div>
  </section>;
}
