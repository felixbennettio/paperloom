import { describe, expect, it } from "vitest";
import {
  DEFAULT_PREFERENCES,
  NOTES_WIDTH,
  SIDEBAR_WIDTH,
  SPLIT_RATIO,
  clamp,
  loadPreferences,
  savePreferences,
} from "./preferences";

function memoryStorage(initial?: string) {
  const data = new Map<string, string>(initial ? [["paperloom.preferences", initial]] : []);
  return {
    getItem: (key: string) => data.get(key) ?? null,
    setItem: (key: string, value: string) => void data.set(key, value),
  };
}

describe("preferences", () => {
  it("defaults when nothing is saved", () => {
    expect(loadPreferences(null)).toEqual(DEFAULT_PREFERENCES);
    expect(loadPreferences(memoryStorage())).toEqual(DEFAULT_PREFERENCES);
    expect(loadPreferences(memoryStorage("not json"))).toEqual(DEFAULT_PREFERENCES);
  });

  it("round-trips", () => {
    const storage = memoryStorage();
    const prefs = {
      tabStyle: "vertical-right",
      notesEdge: "left",
      showNotes: false,
      language: "ja",
      syncTarget: "webdav",
      syncService: "https://sync.example",
      syncFolder: "/Users/me/Paperloom",
      webdavAddress: "https://dav.jianguoyun.com/dav/Paperloom",
      webdavUser: "me@example.com",
      seafileServer: "https://cloud.example.com",
      seafileAccount: "me@example.com",
      seafileLibrary: "中文资料库",
      seafileFolder: "中文路径",
      s3Endpoint: "https://abc.r2.cloudflarestorage.com",
      s3Region: "auto",
      s3Bucket: "papers",
      s3Prefix: "paperloom",
      s3AccessKeyId: "AKID",
      aiProtocol: "openai",
      aiBaseUrl: "http://localhost:11434/v1",
      aiModel: "local",
      aiEmbeddingModel: "nomic-embed-text",
      autoBackup: "weekly",
      backupFolder: "/Volumes/Backup",
      sidebarWidth: 320,
      notesWidth: 420,
      splitRatio: 0.65,
    } as const;
    savePreferences(prefs, storage);
    expect(loadPreferences(storage)).toEqual(prefs);
  });

  it("replaces invalid values with defaults", () => {
    const storage = memoryStorage(
      JSON.stringify({
        tabStyle: "diagonal",
        showNotes: "yes",
        language: "xx",
        syncTarget: "ftp",
        syncService: "javascript:alert(1)",
        webdavAddress: "file:///etc",
        s3Endpoint: "ftp://s3.example",
        seafileServer: "file:///etc",
        aiProtocol: "smoke-signals",
        aiBaseUrl: "javascript:void(0)",
        autoBackup: "hourly",
      }),
    );
    expect(loadPreferences(storage)).toEqual(DEFAULT_PREFERENCES);
  });

  it("keeps dragged widths within their ranges", () => {
    const prefs = loadPreferences(memoryStorage(JSON.stringify({ sidebarWidth: 9999, notesWidth: "wide", splitRatio: 0.01 })));
    expect(prefs.sidebarWidth).toBe(SIDEBAR_WIDTH.max);
    expect(prefs.notesWidth).toBe(NOTES_WIDTH.initial);
    expect(prefs.splitRatio).toBe(SPLIT_RATIO.min);
    expect(clamp(100, SIDEBAR_WIDTH)).toBe(SIDEBAR_WIDTH.min);
  });

  it("keeps Seafile locations across launches and discards saved passwords", () => {
    const storage = memoryStorage(JSON.stringify({
      syncTarget: "seafile",
      seafileServer: "https://cloud.example.com",
      seafileAccount: "me@example.com",
      seafileLibrary: "中文资料库",
      seafileFolder: "",
      seafilePassword: "secret",
    }));
    const prefs = loadPreferences(storage);
    expect(prefs.syncTarget).toBe("seafile");
    expect(prefs.seafileLibrary).toBe("中文资料库");
    expect(prefs.seafileFolder).toBe("");
    savePreferences(prefs, storage);
    expect(loadPreferences(storage)).toEqual(prefs);
    expect(storage.getItem("paperloom.preferences")).not.toContain("seafilePassword");
  });
});
