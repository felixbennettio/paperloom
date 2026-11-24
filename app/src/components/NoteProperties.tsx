import { useEffect, useState } from "react";
import { useI18n } from "../i18n";
import { backend, type NoteProperty } from "../lib/api";
import { Icon } from "./Icon";

interface Props {
  path: string;
  properties: NoteProperty[];
  /** Saves typing in the note first, so the properties go onto it. */
  beforeSave: () => Promise<void>;
  /** The note changed on disk; show it again. */
  onSaved: () => void;
  onError: (message: string) => void;
}

/** A note's properties (its front matter) as fields to edit. */
export function NoteProperties({ path, properties, beforeSave, onSaved, onError }: Props) {
  const { t } = useI18n();
  const [rows, setRows] = useState<NoteProperty[]>(properties);
  const [saving, setSaving] = useState(false);
  // Saving the note's text reads its properties again; only a real change
  // replaces what is being typed here.
  const written = JSON.stringify(properties);
  useEffect(() => setRows(JSON.parse(written) as NoteProperty[]), [path, written]);

  const dirty = JSON.stringify(rows) !== written;
  const change = (index: number, patch: Partial<NoteProperty>) =>
    setRows((all) => all.map((row, i) => (i === index ? { ...row, ...patch } : row)));

  const save = async () => {
    setSaving(true);
    try {
      await beforeSave();
      await backend.vaultSetProperties(path, rows);
      onSaved();
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="note-properties">
      <h3>{t("Properties")}</h3>
      {rows.map((row, i) => (
        <div className="property-row" key={i}>
          <input value={row.key} aria-label={t("Property")} placeholder={t("Property")} spellCheck={false} onChange={(e) => change(i, { key: e.target.value })} />
          <input value={row.value} aria-label={t("Value")} placeholder={t("Value")} onChange={(e) => change(i, { value: e.target.value })} />
          <button className="icon-button" aria-label={t("Remove")} title={t("Remove")} onClick={() => setRows((all) => all.filter((_, j) => j !== i))}>
            <Icon name="close" size={12} />
          </button>
        </div>
      ))}
      <div className="property-actions">
        <button className="link" onClick={() => setRows((all) => [...all, { key: "", value: "" }])}>
          {t("Add Property")}
        </button>
        {dirty && (
          <button className="primary" disabled={saving} onClick={() => void save()}>
            {t("Save")}
          </button>
        )}
      </div>
    </div>
  );
}
