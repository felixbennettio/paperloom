import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { pickMessages } from "../../addin-messages";
import fr from "../i18n/locales/fr.json";

describe("task pane translations", () => {
  it("keeps the keys a source writes as literals", () => {
    const picked = pickMessages({ Cite: "Citer", Library: "Bibliothèque", 'Say "hi"': "x" }, 't("Cite"); t("Say \\"hi\\"")');
    expect(picked).toEqual({ Cite: "Citer", 'Say "hi"': "x" });
  });

  it("keeps every text the pane shows", () => {
    const app = readFileSync(new URL("./AddinApp.tsx", import.meta.url), "utf8");
    const picked = pickMessages(fr, app);
    for (const [, key] of app.matchAll(/\bt\("((?:[^"\\]|\\.)*)"/g)) {
      const text = JSON.parse(`"${key}"`) as string;
      if (text in fr) expect(picked[text]).toBe(fr[text as keyof typeof fr]);
    }
    expect(Object.keys(picked).length).toBeLessThan(Object.keys(fr).length / 4);
  });
});
