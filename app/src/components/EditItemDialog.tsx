import { useState } from "react";
import { useI18n } from "../i18n";
import { ITEM_TYPES, type ItemDetail } from "../lib/api";
import { ITEM_TYPE_NAMES } from "./LibraryView";

type TextField = {
  [K in keyof ItemDetail]-?: ItemDetail[K] extends string | null ? K : never;
}[keyof ItemDetail];

const FIELDS: { key: Exclude<TextField, "id" | "itemType" | "title" | "readStatus" | "abstractNote" | "extra">; label: string }[] = [
  { key: "containerTitle", label: "Publication" },
  { key: "containerShortTitle", label: "Short title" },
  { key: "date", label: "Date" },
  { key: "volume", label: "Volume" },
  { key: "issue", label: "Issue" },
  { key: "pages", label: "Pages" },
  { key: "publisher", label: "Publisher" },
  { key: "place", label: "Place" },
  { key: "edition", label: "Edition" },
  { key: "number", label: "Number" },
  { key: "language", label: "Language" },
  { key: "doi", label: "DOI" },
  { key: "isbn", label: "ISBN" },
  { key: "issn", label: "ISSN" },
  { key: "arxivId", label: "arXiv" },
  { key: "pmid", label: "PMID" },
  { key: "url", label: "URL" },
];

interface Props {
  detail: ItemDetail;
  /** Saves; returns why it could not, or null. */
  onSave: (detail: ItemDetail) => Promise<string | null>;
  onClose: () => void;
}

/** Edits every detail of an item. */
export function EditItemDialog({ detail, onSave, onClose }: Props) {
  const { t } = useI18n();
  const [draft, setDraft] = useState<ItemDetail>(detail);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const lines = (names: string[]) => names.join("\n");
  const split = (text: string, separators: RegExp) => text.split(separators).map((s) => s.trim());

  const save = async () => {
    setSaving(true);
    const problem = await onSave({
      ...draft,
      authors: draft.authors.filter((a) => a.trim()),
      editors: draft.editors.filter((a) => a.trim()),
      keywords: draft.keywords.filter((k) => k.trim()),
    });
    setSaving(false);
    if (problem) setError(problem);
    else onClose();
  };

  return (
    <div className="dialog-backdrop" role="presentation" onClick={onClose}>
      <div
        className="dialog wide"
        role="dialog"
        aria-modal="true"
        aria-labelledby="edit-item-title"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && onClose()}
      >
        <form
          className="dialog-form"
          onSubmit={(e) => {
            e.preventDefault();
            void save();
          }}
        >
          <h2 id="edit-item-title">{t("Edit Item")}</h2>
          <label>
            {t("Type")}
            <select value={draft.itemType} onChange={(e) => setDraft({ ...draft, itemType: e.target.value })}>
              {ITEM_TYPES.map((type) => (
                <option key={type} value={type}>
                  {t(ITEM_TYPE_NAMES[type])}
                </option>
              ))}
            </select>
          </label>
          <label>
            {t("Title")}
            <textarea rows={2} value={draft.title} onChange={(e) => setDraft({ ...draft, title: e.target.value })} />
          </label>
          <div className="field-grid">
            <label>
              {t("Authors")}
              <textarea
                rows={3}
                value={lines(draft.authors)}
                placeholder={t("One per line")}
                onChange={(e) => setDraft({ ...draft, authors: e.target.value.split("\n") })}
              />
            </label>
            <label>
              {t("Editors")}
              <textarea
                rows={3}
                value={lines(draft.editors)}
                placeholder={t("One per line")}
                onChange={(e) => setDraft({ ...draft, editors: e.target.value.split("\n") })}
              />
            </label>
            <label>
              {t("Year")}
              <input
                inputMode="numeric"
                value={draft.year ?? ""}
                onChange={(e) => {
                  const year = Number.parseInt(e.target.value, 10);
                  setDraft({ ...draft, year: Number.isFinite(year) ? year : null });
                }}
              />
            </label>
            {FIELDS.map(({ key, label }) => (
              <label key={key}>
                {t(label)}
                <input value={draft[key] ?? ""} onChange={(e) => setDraft({ ...draft, [key]: e.target.value || null } as ItemDetail)} />
              </label>
            ))}
          </div>
          <label>
            {t("Keywords")}
            <input
              value={draft.keywords.join(", ")}
              placeholder={t("Separated by commas")}
              onChange={(e) => setDraft({ ...draft, keywords: split(e.target.value, /[,;，；]/) })}
            />
          </label>
          <label>
            {t("Abstract")}
            <textarea rows={6} value={draft.abstractNote ?? ""} onChange={(e) => setDraft({ ...draft, abstractNote: e.target.value || null })} />
          </label>
          <label>
            {t("Extra")}
            <textarea rows={6} value={draft.extra ?? ""} onChange={(e) => setDraft({ ...draft, extra: e.target.value || null })} />
          </label>
          {error && <p className="hint error">{error}</p>}
          <div className="row end">
            <button type="button" onClick={onClose}>
              {t("Cancel")}
            </button>
            <button type="submit" className="primary" disabled={saving || !draft.title.trim()}>
              {t("Save")}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
