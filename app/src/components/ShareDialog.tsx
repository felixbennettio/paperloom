import { useEffect, useState } from "react";
import { useI18n } from "../i18n";
import { backend, type ImportReport, type SearchHit, type Shared } from "../lib/api";
import { planSharedText } from "../lib/share";
import { HitList, markAdded } from "./HitList";

function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

interface Props {
  shared: Shared;
  collectionId: string | null;
  collectionName: string | null;
  /** Adds PDFs as if picked; says how it went. */
  onAddPdfs: (paths: string[]) => Promise<string>;
  /** After the library changed. */
  onAdded: () => void;
  onOpenItem: (id: string, title: string) => void;
  onClose: () => void;
}

/** What another app shared (Android's share sheet) or the files opened with
 * Paperloom (Windows, Linux): PDFs and reference files are added at once, identifiers looked up and added, and the works
 * on a shared web page listed to pick from. */
export function ShareDialog({ shared, collectionId, collectionName, onAddPdfs, onAdded, onOpenItem, onClose }: Props) {
  const { t } = useI18n();
  const [working, setWorking] = useState(true);
  const [lines, setLines] = useState<string[]>([]);
  const [hits, setHits] = useState<SearchHit[]>([]);
  const [chosen, setChosen] = useState<ReadonlySet<string>>(new Set());
  const summary = (r: ImportReport) =>
    t("Added {added}, {duplicates} already in the library, {skipped} skipped.", {
      added: r.added,
      duplicates: r.duplicates,
      skipped: r.skipped,
    });

  useEffect(() => {
    let current = true;
    const say = (line: string) => current && setLines((all) => [...all, line]);
    const importText = async (text: string, name: string | null) => {
      try {
        const r = await backend.importReferences(text, collectionId);
        say(name ? `${name}: ${summary(r)}` : summary(r));
      } catch (e) {
        say(name ? `${name}: ${message(e)}` : message(e));
      }
    };
    void (async () => {
      let changed = false;
      if (shared.pdfs.length) {
        say(await onAddPdfs(shared.pdfs));
        changed = true;
      }
      for (const file of shared.references) {
        await importText(file.text, file.name);
        changed = true;
      }
      for (const text of shared.texts) {
        const plan = planSharedText(text);
        if (plan.kind === "references") {
          await importText(plan.text, null);
          changed = true;
        } else if (plan.kind === "lookup") {
          let identifier: string | null = null;
          for (const candidate of plan.candidates) {
            if (await backend.recognizeIdentifier(candidate).catch(() => null)) {
              identifier = candidate;
              break;
            }
          }
          try {
            if (identifier) {
              const item = await backend.addByIdentifier(identifier, collectionId);
              say(t("Added “{title}”.", { title: item.title }));
              changed = true;
            } else if (plan.page) {
              const page = await backend.readWebPage(plan.page);
              if (!current) return;
              if (page.hits.length === 0) say(t("No works were found on {url}.", { url: plan.page }));
              setHits((all) => [...all, ...page.hits]);
              const open = page.hits.filter((h) => h.libraryItemId === null);
              // One work on the page: most likely the one meant.
              if (page.hits.length === 1 && open.length === 1) setChosen((keys) => new Set([...keys, open[0].key]));
            } else {
              say(t("Nothing to add was found in what was shared."));
            }
          } catch (e) {
            say(`${plan.page ?? identifier ?? text.text}: ${message(e)}`);
          }
        }
      }
      for (const name of shared.unsupported) say(t("{name} cannot be imported.", { name }));
      if (changed && current) onAdded();
      if (current) setWorking(false);
    })();
    return () => {
      current = false;
    };
    // Runs once for each share; the dialog is keyed by it.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const toggle = (key: string) =>
    setChosen((keys) => {
      const next = new Set(keys);
      if (!next.delete(key)) next.add(key);
      return next;
    });
  const add = async (keys: string[]) => {
    try {
      const r = await backend.addSearchResults(keys, collectionId);
      setHits((all) => markAdded(all, keys, r.addedIds));
      setChosen(new Set());
      setLines((all) => [...all, summary(r)]);
      onAdded();
    } catch (e) {
      setLines((all) => [...all, message(e)]);
    }
  };

  return (
    <div className="dialog-backdrop" role="presentation" onClick={onClose}>
      <div
        className="dialog share-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="share-title"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && onClose()}
      >
        <h2 id="share-title">{shared.opened ? t("Opened in Paperloom") : t("Shared to Paperloom")}</h2>
        {working && <p className="hint">{shared.opened ? t("Reading the files…") : t("Reading what was shared…")}</p>}
        {lines.map((line, i) => (
          <p key={i} className="hint share-line">
            {line}
          </p>
        ))}
        {hits.length > 0 && (
          <HitList
            hits={hits}
            chosen={chosen}
            onToggle={toggle}
            onAdd={(keys) => void add(keys)}
            onOpenItem={(id, title) => {
              onOpenItem(id, title);
              onClose();
            }}
            onOpenUrl={(url) => void backend.openUrl(url).catch((e) => setLines((all) => [...all, message(e)]))}
          />
        )}
        <div className="row end">
          {collectionName && chosen.size > 0 && <span className="muted">{t("Adds to “{name}”", { name: collectionName })}</span>}
          {hits.length > 0 && (
            <button className="primary" disabled={chosen.size === 0} onClick={() => void add([...chosen])}>
              {t("Add {count} Selected", { count: chosen.size })}
            </button>
          )}
          <button onClick={onClose}>{t("Done")}</button>
        </div>
      </div>
    </div>
  );
}
