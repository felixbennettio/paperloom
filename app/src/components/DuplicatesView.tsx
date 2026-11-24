import { useCallback, useEffect, useState } from "react";
import { useI18n } from "../i18n";
import { backend, type DuplicateGroup, type ItemSummary } from "../lib/api";
import { Icon } from "./Icon";

interface Props {
  onOpen: (item: ItemSummary) => void;
  /** Called after items were merged, so the library list can reload. */
  onMerged: () => void;
}

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** Items that look like the same work, as Zotero's Duplicate Items shows
 * them: each group with the item to keep picked (the oldest, at first) and
 * merged into it with one click. */
export function DuplicatesView({ onOpen, onMerged }: Props) {
  const { t } = useI18n();
  const [groups, setGroups] = useState<DuplicateGroup[] | null>(null);
  /** The item each group keeps, by the group's first item. */
  const [kept, setKept] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    backend.duplicateGroups().then(setGroups, (e: unknown) => setError(message(e)));
  }, []);

  useEffect(load, [load]);

  const merge = async (group: DuplicateGroup) => {
    const key = group.items[0].id;
    const master = kept[key] ?? group.items[group.items.length - 1].id;
    setBusy(key);
    try {
      await backend.mergeItems(
        master,
        group.items.map((i) => i.id).filter((id) => id !== master),
      );
      setError(null);
      onMerged();
      load();
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(null);
    }
  };

  const date = (ms: number) => new Date(ms).toLocaleDateString();

  return (
    <div className="library-view duplicates-view">
      <header className="view-header">
        <h2>{t("Duplicate Items")}</h2>
      </header>
      <p className="hint duplicates-hint">
        {t("Items with the same DOI, ISBN, arXiv or PubMed ID, or the same title, close years and an author in common. Merging keeps the item you pick, fills its gaps from the others and brings their files, tags and collections to it; the others go to the trash.")}
      </p>
      {error && <p className="hint error">{error}</p>}
      {groups && groups.length === 0 && (
        <div className="empty">
          <Icon name="duplicate" size={40} />
          <h3>{t("No Duplicates")}</h3>
        </div>
      )}
      {groups?.map((group) => {
        const key = group.items[0].id;
        const master = kept[key] ?? group.items[group.items.length - 1].id;
        return (
          <section key={key} className="card duplicate-group">
            <ul>
              {group.items.map((item) => (
                <li key={item.id}>
                  <label>
                    <input
                      type="radio"
                      name={`keep-${key}`}
                      checked={item.id === master}
                      onChange={() => setKept((k) => ({ ...k, [key]: item.id }))}
                    />
                    <span className="duplicate-title">{item.title}</span>
                  </label>
                  <span className="duplicate-meta">
                    {[item.authors, item.year, item.containerTitle, item.doi].filter(Boolean).join(" · ")}
                    {item.hasPdf ? " · PDF" : ""} · {t("Added {date}", { date: date(item.dateAdded) })}
                  </span>
                  <button className="link" onClick={() => onOpen(item)}>
                    {t("Open")}
                  </button>
                </li>
              ))}
            </ul>
            <button className="primary" disabled={busy !== null} onClick={() => void merge(group)}>
              {t("Merge {count} Items", { count: group.items.length })}
            </button>
          </section>
        );
      })}
    </div>
  );
}
