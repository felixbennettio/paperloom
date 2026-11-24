// Translations generated from i18n/strings.json by scripts/i18n.py. Keys are
// the English text, so English needs no file.

import { createContext, useContext, useEffect, useMemo, type ReactNode } from "react";
import fr from "./locales/fr.json";
import ja from "./locales/ja.json";
import ru from "./locales/ru.json";
import zhHans from "./locales/zh-Hans.json";
import zhHant from "./locales/zh-Hant.json";

export const LANGUAGES = [
  { code: "zh-Hans", name: "简体中文" },
  { code: "zh-Hant", name: "繁體中文" },
  { code: "en", name: "English" },
  { code: "fr", name: "Français" },
  { code: "ja", name: "日本語" },
  { code: "ru", name: "Русский" },
] as const;

export type Language = (typeof LANGUAGES)[number]["code"];

const MESSAGES: Record<Language, Record<string, string>> = {
  "zh-Hans": zhHans,
  "zh-Hant": zhHant,
  en: {},
  fr,
  ja,
  ru,
};

export type Vars = Record<string, string | number>;

let active: Language = "en";

/** The interface's language, for text made outside React (errors from
 * the Rust side). */
export function activeLanguage(): Language {
  return active;
}

export function setActiveLanguage(language: Language): void {
  active = language;
}

/** The first supported language among the user's preferred ones. */
export function resolveLanguage(preferred: readonly string[]): Language {
  for (const tag of preferred) {
    const lower = tag.toLowerCase();
    if (lower.startsWith("zh")) {
      const traditional = /(hant|-tw|-hk|-mo)/.test(lower);
      return traditional ? "zh-Hant" : "zh-Hans";
    }
    const base = lower.split("-")[0];
    const match = LANGUAGES.find((l) => l.code === base);
    if (match) return match.code;
  }
  return "en";
}

/** The text for `key` in `language`, falling back to English. */
export function translate(language: Language, key: string, vars?: Vars): string {
  const text = MESSAGES[language][key] ?? key;
  if (!vars) return text;
  return text.replace(/\{(\w+)\}/g, (whole, name: string) => (name in vars ? String(vars[name]) : whole));
}

/** BCP 47 tag for `Intl` and the `lang` attribute. */
export function localeTag(language: Language): string {
  return language === "zh-Hans" ? "zh-CN" : language === "zh-Hant" ? "zh-TW" : language;
}

interface I18n {
  language: Language;
  t: (key: string, vars?: Vars) => string;
}

const I18nContext = createContext<I18n>({ language: "en", t: (key, vars) => translate("en", key, vars) });

export function I18nProvider({ language, children }: { language: Language; children: ReactNode }) {
  const value = useMemo<I18n>(() => ({ language, t: (key, vars) => translate(language, key, vars) }), [language]);
  useEffect(() => {
    setActiveLanguage(language);
    document.documentElement.lang = localeTag(language);
  }, [language]);
  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>;
}

export function useI18n(): I18n {
  return useContext(I18nContext);
}
