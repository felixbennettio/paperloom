import { afterEach, describe, expect, it, vi } from "vitest";
import { setActiveLanguage } from "../i18n";
import { tauriBackend } from "./api";

// The Rust side answers in English; a failed command asks Core for the
// message in the interface's language.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string, args?: Record<string, unknown>) => {
    if (command === "localize_message") return `${String(args?.language)}: ${String(args?.message)}`;
    throw "A tag needs a name.";
  }),
}));

describe("errors from commands", () => {
  afterEach(() => setActiveLanguage("en"));

  it("stay in English for English", async () => {
    await expect(tauriBackend.setItemTags("item", [""])).rejects.toBe("A tag needs a name.");
  });

  it("come back in the interface's language", async () => {
    setActiveLanguage("zh-Hans");
    await expect(tauriBackend.setItemTags("item", [""])).rejects.toBe("zh-Hans: A tag needs a name.");
  });
});
