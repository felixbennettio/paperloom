import { createRoot } from "react-dom/client";
import { I18nProvider, resolveLanguage, translate } from "../i18n";
import { AddinApp } from "./AddinApp";
import { ConnectorClient } from "./connector";
import { createHost } from "./hosts";

const root = createRoot(document.getElementById("root")!);

async function start() {
  try {
    const { tokens, displayLanguage, ...host } = await createHost();
    const client = new ConnectorClient(tokens);
    const language = resolveLanguage(displayLanguage ? [displayLanguage] : navigator.languages);
    root.render(
      <I18nProvider language={language}>
        <AddinApp client={client} {...host} />
      </I18nProvider>,
    );
  } catch (error) {
    const displayLanguage = typeof Office !== "undefined" ? Office.context?.displayLanguage : undefined;
    const language = resolveLanguage(displayLanguage ? [displayLanguage] : navigator.languages);
    root.render(<main className="addin-pane"><p role="alert">{translate(language, "Could not complete the request: {message}", {
      message: translate(language, error instanceof Error ? error.message : String(error)),
    })}</p></main>);
  }
}

void start();
