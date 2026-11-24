import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { useI18n } from "../i18n";
import { translatorHost } from "../lib/translators";

type Selection = { items: Record<string, string>; finish: (keys: string[]) => void };

/** The host's item checklist stays available across workbench tabs. */
export function TranslatorPicker() {
  const { t } = useI18n();
  const [selection, setSelection] = useState<Selection | null>(null);
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const dialog = useRef<HTMLDialogElement>(null);
  const cancel = useRef<(() => void) | null>(null);
  useEffect(() => {
    translatorHost.setPicker((items, signal) => new Promise((resolve) => {
      cancel.current?.();
      const finish = (keys: string[]) => {
        signal.removeEventListener("abort", aborted);
        cancel.current = null;
        setSelection(null);
        resolve(keys);
      };
      const aborted = () => finish([]);
      if (signal.aborted) { resolve([]); return; }
      signal.addEventListener("abort", aborted, { once: true });
      cancel.current = aborted;
      setChosen(new Set(Object.keys(items)));
      setSelection({ items, finish });
    }));
    return () => { cancel.current?.(); translatorHost.setPicker(async () => []); };
  }, []);
  useEffect(() => {
    if (selection) dialog.current?.showModal();
  }, [selection]);
  if (!selection) return null;
  return createPortal(
    <dialog className="dialog wide" ref={dialog} aria-labelledby="translator-picker-title" onCancel={(event) => { event.preventDefault(); selection.finish([]); }}>
      <h2 id="translator-picker-title">{t("Choose items to save")}</h2>
      <div className="translator-selection">
        {Object.entries(selection.items).map(([key, title]) => (
          <label key={key}>
            <input type="checkbox" checked={chosen.has(key)} onChange={() => setChosen((old) => {
              const next = new Set(old);
              if (!next.delete(key)) next.add(key);
              return next;
            })} />
            <span>{title}</span>
          </label>
        ))}
      </div>
      <div className="dialog-actions">
        <button onClick={() => selection.finish([])}>{t("Cancel")}</button>
        <button className="primary" disabled={!chosen.size} onClick={() => selection.finish([...chosen])}>{t("Save")}</button>
      </div>
    </dialog>, document.body,
  );
}
