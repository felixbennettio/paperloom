import { readdirSync, readFileSync } from "node:fs";
import type { Plugin } from "vite";

/** The translations a source uses: the keys written as string literals in
 * it. The locale files hold the whole app's text, most of which the task
 * pane never shows. */
export function pickMessages(messages: Record<string, string>, sources: string): Record<string, string> {
  return Object.fromEntries(Object.entries(messages).filter(([key]) => sources.includes(JSON.stringify(key))));
}

/** Builds the task pane (Word and WPS) with only the translations its own
 * code uses (src/addin, tests aside), about half its size otherwise. Runs
 * before Vite's JSON plugin, on the locale files' text. */
export function addinMessages(): Plugin {
  const folder = new URL("./src/addin/", import.meta.url);
  const sources = readdirSync(folder)
    .filter((name) => /\.tsx?$/.test(name) && !/\.test\.tsx?$/.test(name))
    .map((name) => readFileSync(new URL(name, folder), "utf8"))
    .join("\n");
  return {
    name: "addin-messages",
    enforce: "pre",
    transform(code, id) {
      if (!/[\\/]src[\\/]i18n[\\/]locales[\\/][\w-]+\.json$/.test(id)) return null;
      return { code: JSON.stringify(pickMessages(JSON.parse(code) as Record<string, string>, sources)), map: null };
    },
  };
}
