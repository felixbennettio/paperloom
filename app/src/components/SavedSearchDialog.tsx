import { useEffect, useMemo, useState } from "react";
import { useI18n } from "../i18n";
import {
  ITEM_TYPES,
  backend,
  type CollectionInfo,
  type SavedSearch,
  type SearchCondition,
  type SearchField,
  type TagInfo,
} from "../lib/api";
import { FIELD_NAMES, OP_NAMES, conditionFor, withField, type Choices } from "../lib/searches";
import { Icon } from "./Icon";
import { ITEM_TYPE_NAMES } from "./LibraryView";

const READ_STATUS_NAMES: Record<string, string> = { unread: "Unread", reading: "In Progress", read: "Finished" };

interface Props {
  search: SavedSearch;
  fields: SearchField[];
  collections: CollectionInfo[];
  tags: TagInfo[];
  /** Called with the search as saved. */
  onSaved: (search: SavedSearch) => void;
  /** Deletes the search; offered once it is saved. */
  onDelete: (search: SavedSearch) => void;
  onClose: () => void;
}

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** Edits a saved search, as Zotero's search dialog does: a name, whether
 * items must pass all conditions or any, and the conditions, with how many
 * items they find as they stand. */
export function SavedSearchDialog({ search, fields, collections, tags, onSaved, onDelete, onClose }: Props) {
  const { t } = useI18n();
  const [draft, setDraft] = useState<SavedSearch>(search);
  const [count, setCount] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const choices: Choices = useMemo(
    () => ({ tags: tags.map((tag) => tag.name), collections: collections.map((c) => c.id) }),
    [tags, collections],
  );

  // How many items the conditions find, a moment after they change.
  useEffect(() => {
    let current = true;
    const timer = setTimeout(() => {
      backend.runSearch(draft).then(
        (items) => current && setCount(items.length),
        () => current && setCount(null),
      );
    }, 250);
    return () => {
      current = false;
      clearTimeout(timer);
    };
  }, [draft]);

  const change = (index: number, condition: SearchCondition) =>
    setDraft((d) => ({ ...d, conditions: d.conditions.map((c, i) => (i === index ? condition : c)) }));

  const save = async () => {
    setSaving(true);
    try {
      onSaved(await backend.saveSearch(draft));
    } catch (e) {
      setError(message(e));
    } finally {
      setSaving(false);
    }
  };

  const valueInput = (condition: SearchCondition, index: number, field: SearchField) => {
    const set = (value: string) => change(index, { ...condition, value });
    const label = t("Value");
    switch (field.kind) {
      case "itemType":
        return (
          <select aria-label={label} value={condition.value} onChange={(e) => set(e.target.value)}>
            {ITEM_TYPES.map((type) => (
              <option key={type} value={type}>
                {t(ITEM_TYPE_NAMES[type])}
              </option>
            ))}
          </select>
        );
      case "readStatus":
        return (
          <select aria-label={label} value={condition.value} onChange={(e) => set(e.target.value)}>
            {Object.entries(READ_STATUS_NAMES).map(([status, name]) => (
              <option key={status} value={status}>
                {t(name)}
              </option>
            ))}
          </select>
        );
      case "collection":
        return (
          <select aria-label={label} value={condition.value} onChange={(e) => set(e.target.value)}>
            {collections.map((c) => (
              <option key={c.id} value={c.id}>
                {c.name}
              </option>
            ))}
          </select>
        );
      case "flag":
        return (
          <select aria-label={label} value={condition.value} onChange={(e) => set(e.target.value)}>
            <option value="true">{t("Yes")}</option>
            <option value="false">{t("No")}</option>
          </select>
        );
      case "tag":
        return (
          <input aria-label={label} list="saved-search-tags" value={condition.value} onChange={(e) => set(e.target.value)} />
        );
      case "days":
        return (
          <span className="condition-days">
            <input aria-label={label} inputMode="numeric" value={condition.value} onChange={(e) => set(e.target.value)} />
            {t("days")}
          </span>
        );
      case "year":
      case "text":
        return (
          <input
            aria-label={label}
            inputMode={field.kind === "year" ? "numeric" : undefined}
            value={condition.value}
            onChange={(e) => set(e.target.value)}
          />
        );
    }
  };

  return (
    <div className="dialog-backdrop" role="presentation" onClick={onClose}>
      <div
        className="dialog wide saved-search"
        role="dialog"
        aria-modal="true"
        aria-labelledby="saved-search-title"
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
          <h2 id="saved-search-title">{search.id ? t("Edit Saved Search") : t("New Saved Search")}</h2>
          <label>
            {t("Name")}
            <input autoFocus value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} />
          </label>
          <select
            aria-label={t("Match All Conditions")}
            value={draft.matchAll ? "all" : "any"}
            onChange={(e) => setDraft({ ...draft, matchAll: e.target.value === "all" })}
          >
            <option value="all">{t("Match All Conditions")}</option>
            <option value="any">{t("Match Any Condition")}</option>
          </select>
          <datalist id="saved-search-tags">
            {tags.map((tag) => (
              <option key={tag.name} value={tag.name} />
            ))}
          </datalist>
          <ul className="conditions">
            {draft.conditions.map((condition, index) => {
              const field = fields.find((f) => f.id === condition.field);
              return (
                <li key={index} className="condition-row">
                  <select
                    aria-label={t("Field")}
                    value={condition.field}
                    onChange={(e) => {
                      const next = fields.find((f) => f.id === e.target.value);
                      if (next) change(index, withField(condition, next, choices, fields));
                    }}
                  >
                    {fields.map((f) => (
                      <option key={f.id} value={f.id}>
                        {t(FIELD_NAMES[f.id] ?? f.id)}
                      </option>
                    ))}
                  </select>
                  <select
                    aria-label={t("Comparison")}
                    value={condition.op}
                    onChange={(e) => change(index, { ...condition, op: e.target.value })}
                  >
                    {(field?.ops ?? [condition.op]).map((op) => (
                      <option key={op} value={op}>
                        {t(OP_NAMES[op] ?? op)}
                      </option>
                    ))}
                  </select>
                  {field ? valueInput(condition, index, field) : <input aria-label={t("Value")} value={condition.value} readOnly />}
                  <button
                    type="button"
                    className="icon-button"
                    title={t("Remove Condition")}
                    aria-label={t("Remove Condition")}
                    onClick={() => setDraft((d) => ({ ...d, conditions: d.conditions.filter((_, i) => i !== index) }))}
                  >
                    <Icon name="close" size={14} />
                  </button>
                </li>
              );
            })}
          </ul>
          <div className="row">
            <button
              type="button"
              disabled={fields.length === 0}
              onClick={() => setDraft((d) => ({ ...d, conditions: [...d.conditions, conditionFor(fields[0], choices)] }))}
            >
              <Icon name="plus" size={14} /> {t("Add Condition")}
            </button>
            <span className="spacer" />
            {count !== null && <span className="hint">{t("Matching items: {count}", { count })}</span>}
          </div>
          {error && <p className="hint error">{error}</p>}
          <div className="row">
            {search.id && (
              <button type="button" className="danger" onClick={() => onDelete(search)}>
                {t("Delete Saved Search")}
              </button>
            )}
            <span className="spacer" />
            <button type="button" onClick={onClose}>
              {t("Cancel")}
            </button>
            <button type="submit" className="primary" disabled={saving || !draft.name.trim()}>
              {t("Save")}
            </button>
          </div>
        </form>
      </div>
    </div>
  );
}
