import DOMPurify from "dompurify";
import { useEffect, useState } from "react";
import { useI18n } from "../i18n";
import { pageRanges } from "../lib/ai";
import { backend, type AiAnswer } from "../lib/api";

/** An AI answer: its Markdown shown, the pages it read, and copying it. */
export function AiAnswerView({ answer }: { answer: AiAnswer }) {
  const { t } = useI18n();
  const [html, setHtml] = useState("");
  useEffect(() => {
    let current = true;
    backend.vaultRender("AI.md", answer.text).then(
      (rendered) => current && setHtml(DOMPurify.sanitize(rendered)),
      () => current && setHtml(""),
    );
    return () => {
      current = false;
    };
  }, [answer.text]);

  return (
    <div className="ai-answer">
      {html ? <div className="ai-text" dangerouslySetInnerHTML={{ __html: html }} /> : <p className="ai-text">{answer.text}</p>}
      {answer.cutShort && <p className="hint">{t("The answer stops early: it reached the length limit.")}</p>}
      {answer.pages.length > 0 && <p className="hint">{t("Read pages {pages}", { pages: pageRanges(answer.pages) })}</p>}
      <button onClick={() => void navigator.clipboard.writeText(answer.text)}>{t("Copy")}</button>
    </div>
  );
}
