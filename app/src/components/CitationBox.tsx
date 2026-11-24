import { useEffect, useState } from "react";
import { useI18n } from "../i18n";
import { backend, type CitationOutput, type CitationStyle } from "../lib/api";
import { browserStorage } from "../lib/preferences";
import { CITATION_STYLES_CHANGED } from "../lib/styles";

const KEY = "paperloom.citation.style";

/** The formatted reference and in-text citation for some items, in the
 * style last chosen, with buttons to copy them. */
export function CitationBox({ itemIds }: { itemIds: string[] }) {
  const { t } = useI18n();
  const [styles, setStyles] = useState<CitationStyle[]>([]);
  const [styleId, setStyleId] = useState(() => browserStorage()?.getItem(KEY) ?? "gb-7714-2015-numeric");
  const [output, setOutput] = useState<CitationOutput | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState<"reference" | "citation" | "key" | null>(null);
  const ids = itemIds.join("\n");

  useEffect(() => {
    const load = () => backend.citationStyles().then(setStyles, () => setStyles([]));
    load();
    window.addEventListener(CITATION_STYLES_CHANGED, load);
    return () => window.removeEventListener(CITATION_STYLES_CHANGED, load);
  }, []);

  useEffect(() => {
    let current = true;
    backend.formatCitations(ids.split("\n"), styleId, false).then(
      (out) => {
        if (!current) return;
        setOutput(out);
        setError(null);
      },
      (e) => current && setError(e instanceof Error ? e.message : String(e)),
    );
    return () => {
      current = false;
    };
  }, [ids, styleId]);

  const choose = (id: string) => {
    setStyleId(id);
    try {
      browserStorage()?.setItem(KEY, id);
    } catch {
      // Remembered for this session only.
    }
  };

  const copy = async (text: string, which: "reference" | "citation" | "key") => {
    await navigator.clipboard.writeText(text);
    setCopied(which);
    setTimeout(() => setCopied((c) => (c === which ? null : c)), 2000);
  };

  return (
    <section className="citation-box card">
      <header>
        <h3>{t("Cite")}</h3>
        <select value={styleId} aria-label={t("Style")} onChange={(e) => choose(e.target.value)}>
          {styles.map((s) => (
            <option key={s.id} value={s.id}>
              {s.name}
            </option>
          ))}
        </select>
      </header>
      {error && <p className="hint error">{error}</p>}
      {output?.entries.map((entry, i) => (
        <p key={i} className="reference">
          {entry}
        </p>
      ))}
      {output?.citation && <p className="in-text">{output.citation}</p>}
      {output && output.entries.length > 0 && (
        <div className="row">
          <button onClick={() => void copy(output.entries.join("\n"), "reference")}>
            {copied === "reference" ? t("Copied") : t("Copy Reference")}
          </button>
          <button disabled={!output.citation} onClick={() => void copy(output.citation, "citation")}>
            {copied === "citation" ? t("Copied") : t("Copy In-Text Citation")}
          </button>
          {/* `[@key]`, for notes and for Word documents cited later. */}
          <button onClick={() => void backend.citationMarker(itemIds).then((marker) => copy(marker, "key"))}>
            {copied === "key" ? t("Copied") : t("Copy Citation Key")}
          </button>
        </div>
      )}
    </section>
  );
}
