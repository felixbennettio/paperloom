import { useState } from "react";
import { useI18n } from "../i18n";
import type { SearchHit } from "../lib/api";
import { Icon } from "./Icon";
import { ITEM_TYPE_NAMES } from "./LibraryView";

/** `Vaswani, Shazeer, Parmar et al.` */
function authorLine(authors: string[]): string {
  return authors.length > 3 ? `${authors.slice(0, 3).join(", ")} et al.` : authors.join(", ");
}

/** The hits after adding `keys`, each marked as in the library. Results
 * that could be added were not there before, so each key got the next added
 * ID; if some were duplicates the IDs cannot be matched up, and the results
 * are only marked. */
export function markAdded(hits: SearchHit[], keys: string[], addedIds: string[]): SearchHit[] {
  const ids = addedIds.length === keys.length ? addedIds : keys.map(() => "");
  const added = new Map(keys.map((key, i) => [key, ids[i]]));
  return hits.map((hit) => (added.has(hit.key) ? { ...hit, libraryItemId: hit.libraryItemId ?? added.get(hit.key) ?? "" } : hit));
}

interface Props {
  hits: SearchHit[];
  chosen: ReadonlySet<string>;
  onToggle: (key: string) => void;
  onAdd: (keys: string[]) => void;
  onOpenItem: (id: string, title: string) => void;
  onOpenUrl: (url: string) => void;
}

/** Works found online or on a page, to pick and add. */
export function HitList({ hits, chosen, onToggle, onAdd, onOpenItem, onOpenUrl }: Props) {
  const { t } = useI18n();
  const [expanded, setExpanded] = useState<string | null>(null);
  return (
    <ul className="hits">
      {hits.map((hit) => {
        const d = hit.detail;
        const inLibrary = hit.libraryItemId !== null;
        const facts = [d.containerTitle, d.year, t(ITEM_TYPE_NAMES[d.itemType] ?? "Other")].filter(Boolean);
        return (
          <li key={hit.key} className={chosen.has(hit.key) ? "hit chosen" : "hit"}>
            <input type="checkbox" aria-label={d.title} checked={chosen.has(hit.key)} disabled={inLibrary} onChange={() => onToggle(hit.key)} />
            <div className="hit-body">
              <button className="hit-title" onClick={() => setExpanded(expanded === hit.key ? null : hit.key)}>
                {d.title}
              </button>
              {d.authors.length > 0 && <p className="hit-line">{authorLine(d.authors)}</p>}
              <p className="hit-line muted">{facts.join(" · ")}</p>
              <div className="facts">
                {inLibrary && (
                  <button className="chip in-library" disabled={!hit.libraryItemId} onClick={() => hit.libraryItemId && onOpenItem(hit.libraryItemId, d.title)}>
                    ✓ {t("In Library")}
                  </button>
                )}
                {d.doi && <span className="chip doi">{d.doi}</span>}
                {hit.citedBy !== null && <span className="chip">{t("Cited by {count}", { count: hit.citedBy })}</span>}
                {hit.pdfUrl && (
                  <button className="chip link-chip" onClick={() => onOpenUrl(hit.pdfUrl!)}>
                    PDF
                  </button>
                )}
                {d.url && (
                  <button className="chip link-chip" onClick={() => onOpenUrl(d.url!)}>
                    <Icon name="link" size={13} />
                    {t("Web Page")}
                  </button>
                )}
              </div>
              {expanded === hit.key && d.abstractNote && <p className="hit-abstract">{d.abstractNote}</p>}
            </div>
            {!inLibrary && (
              <button className="hit-add" onClick={() => onAdd([hit.key])}>
                <Icon name="plus" size={14} />
                <span>{t("Add")}</span>
              </button>
            )}
          </li>
        );
      })}
    </ul>
  );
}
