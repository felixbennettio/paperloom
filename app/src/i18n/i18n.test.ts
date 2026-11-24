import { describe, expect, it } from "vitest";
import fr from "./locales/fr.json";
import ja from "./locales/ja.json";
import ru from "./locales/ru.json";
import zhHans from "./locales/zh-Hans.json";
import zhHant from "./locales/zh-Hant.json";
import { LANGUAGES, localeTag, resolveLanguage, translate } from "./index";

describe("i18n", () => {
  it("picks the first supported preferred language", () => {
    expect(resolveLanguage(["zh-CN"])).toBe("zh-Hans");
    expect(resolveLanguage(["zh-Hans-SG"])).toBe("zh-Hans");
    expect(resolveLanguage(["zh-TW"])).toBe("zh-Hant");
    expect(resolveLanguage(["zh-Hant-HK"])).toBe("zh-Hant");
    expect(resolveLanguage(["de-DE", "fr-CA", "en"])).toBe("fr");
    expect(resolveLanguage(["ja-JP"])).toBe("ja");
    expect(resolveLanguage(["ru"])).toBe("ru");
    expect(resolveLanguage(["de", "it"])).toBe("en");
    expect(resolveLanguage([])).toBe("en");
  });

  it("translates with placeholders and falls back to English", () => {
    expect(translate("zh-Hans", "Library")).toBe("文库");
    expect(translate("zh-Hant", "Library")).toBe("文庫");
    expect(translate("en", "Library")).toBe("Library");
    expect(translate("ja", "Items: {count}", { count: 3 })).toBe("文献：3");
    expect(translate("ru", "Not a key at all")).toBe("Not a key at all");
    expect(translate("en", "Hello {who}", {})).toBe("Hello {who}");
  });

  it("has every language complete and in step", () => {
    const keys = Object.keys(zhHans).sort();
    for (const messages of [zhHant, fr, ja, ru]) {
      expect(Object.keys(messages).sort()).toEqual(keys);
    }
    expect(LANGUAGES.map((l) => l.code)).toEqual(["zh-Hans", "zh-Hant", "en", "fr", "ja", "ru"]);
    expect(localeTag("zh-Hant")).toBe("zh-TW");
  });
});
