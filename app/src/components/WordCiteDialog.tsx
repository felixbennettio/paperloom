import { useEffect, useState } from "react";
import { useI18n } from "../i18n";
import { backend, type CitationStyle, type CitedDocument } from "../lib/api";
import { browserStorage } from "../lib/preferences";

/** The style the Cite box last used, which this dialog shares. */
const KEY = "paperloom.citation.style";

/** Formats the `[@citations]` in a Word, WPS or LibreOffice document: the
 * person picks a style, then the document, then where its copy goes. */
export function WordCiteDialog({ onClose }: { onClose: () => void }) {
  const { t } = useI18n();
  const [styles, setStyles] = useState<CitationStyle[]>([]);
  const [styleId, setStyleId] = useState(() => browserStorage()?.getItem(KEY) ?? "gb-7714-2015-numeric");
  const [working, setWorking] = useState(false);
  const [done, setDone] = useState<CitedDocument | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    backend.citationStyles().then(setStyles, () => setStyles([]));
  }, []);

  const choose = (id: string) => {
    setStyleId(id);
    try {
      browserStorage()?.setItem(KEY, id);
    } catch {
      // Remembered for this session only.
    }
  };

  const cite = async () => {
    setWorking(true);
    setError(null);
    try {
      const result = await backend.citeWordDocument(styleId, t("{name} (cited)"));
      if (result) setDone(result);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setWorking(false);
    }
  };

  return (
    <div className="dialog-backdrop" role="presentation" onClick={onClose}>
      <div
        className="dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="word-cite-title"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && onClose()}
      >
        <h2 id="word-cite-title">{t("Cite in Word Document")}</h2>
        <p className="hint">
          {t(
            "Write citations in Word, WPS or LibreOffice as in notes: [@citation key], [@DOI, p. 3] or [@key1; @key2]. Put {bibliography} where the reference list goes, or it goes at the end. Paperloom saves a copy with the citations formatted; your document stays as it is, so run this again after editing.",
          )}
        </p>
        <label>
          {t("Style")}
          <select value={styleId} onChange={(e) => choose(e.target.value)}>
            {styles.map((s) => (
              <option key={s.id} value={s.id}>
                {s.name}
              </option>
            ))}
          </select>
        </label>
        {done && (
          <div className="word-cite-result" role="status">
            <p>
              {t("Citations formatted: {citations}; references: {references}. Saved as {path}.", {
                citations: done.citations,
                references: done.references,
                path: done.path,
              })}
            </p>
            {done.missing.length > 0 && (
              <p className="hint error">{t("Not in the library: {keys}", { keys: done.missing.join(", ") })}</p>
            )}
          </div>
        )}
        {error && <p className="hint error">{error}</p>}
        <div className="row end">
          <button type="button" onClick={onClose}>
            {done ? t("Done") : t("Cancel")}
          </button>
          <button type="button" className="primary" disabled={working || !styleId} onClick={() => void cite()}>
            {working ? t("Working…") : t("Choose Document…")}
          </button>
        </div>
      </div>
    </div>
  );
}
