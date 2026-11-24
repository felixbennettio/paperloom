import { useI18n } from "../i18n";
import type { WriterQuestion } from "../lib/api";

/** A writing plugin (Word, WPS) asks to use the library through the
 * connector; the connector waits for this answer. */
export function WriterQuestionDialog({ question, onAnswer }: { question: WriterQuestion; onAnswer: (allow: boolean) => void }) {
  const { t } = useI18n();
  return (
    <div className="dialog-backdrop" role="presentation">
      <div
        className="dialog"
        role="alertdialog"
        aria-modal="true"
        aria-labelledby="writer-question-title"
        aria-describedby="writer-question-text"
        onKeyDown={(e) => e.key === "Escape" && onAnswer(false)}
      >
        <h2 id="writer-question-title">{t("Allow {client} to use your library?", { client: question.client })}</h2>
        <p id="writer-question-text">
          {t("It can then search your library and format the citations in your documents. It cannot change the library.")}
        </p>
        {question.origin && <p className="hint path">{t("Asked from {origin}", { origin: question.origin })}</p>}
        <div className="row end">
          <button type="button" onClick={() => onAnswer(false)}>
            {t("Don't Allow")}
          </button>
          <button type="button" className="primary" onClick={() => onAnswer(true)}>
            {t("Allow")}
          </button>
        </div>
      </div>
    </div>
  );
}
