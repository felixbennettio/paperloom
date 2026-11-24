// Layout and language preferences, kept in localStorage.

import { LANGUAGES, type Language } from "../i18n";
import type { Edge, TabStyle } from "./workbench";

export interface Preferences {
  tabStyle: TabStyle;
  notesEdge: Edge;
  showNotes: boolean;
  language: Language | "system";
  /** Where the library syncs. */
  syncTarget: SyncKind;
  /** The Paperloom Cloud address to sync with. */
  syncService: string;
  /** The folder to sync through (desktop). */
  syncFolder: string;
  webdavAddress: string;
  webdavUser: string;
  /** The Seafile location is kept here; the app keeps its password separately. */
  seafileServer: string;
  seafileAccount: string;
  seafileLibrary: string;
  seafileFolder: string;
  /** An S3 bucket (or a folder in it) to sync through; the secret key is
   * kept by the app. */
  s3Endpoint: string;
  s3Region: string;
  s3Bucket: string;
  s3Prefix: string;
  s3AccessKeyId: string;
  /** The AI provider's API: Anthropic's, or the OpenAI-style chat API. */
  aiProtocol: AiProtocol;
  aiBaseUrl: string;
  /** Chosen from the provider's own list. */
  aiModel: string;
  /** An OpenAI-style embedding model for semantic search; empty for none. */
  aiEmbeddingModel: string;
  /** How often the library is backed up by itself. */
  autoBackup: AutoBackup;
  /** Where automatic backups go; empty for `backups/` beside the library. */
  backupFolder: string;
  /** Widths the person dragged: the sidebar's and the notes panel's in
   * pixels, and the left pane's share of a split view. */
  sidebarWidth: number;
  notesWidth: number;
  splitRatio: number;
}

/** How far each divider can be dragged, and where double-clicking puts it. */
export interface Range {
  min: number;
  max: number;
  initial: number;
}
export const SIDEBAR_WIDTH: Range = { min: 180, max: 480, initial: 250 };
export const NOTES_WIDTH: Range = { min: 260, max: 720, initial: 340 };
export const SPLIT_RATIO: Range = { min: 0.2, max: 0.8, initial: 0.5 };

export function clamp(value: number, range: Pick<Range, "min" | "max">): number {
  return Math.min(range.max, Math.max(range.min, value));
}

/** A saved size, or the range's initial one when it is missing or not a number. */
function size(value: unknown, range: Range): number {
  return typeof value === "number" && Number.isFinite(value) ? clamp(value, range) : range.initial;
}

export type AutoBackup = "off" | "daily" | "weekly";

export type SyncKind = "cloud" | "folder" | "webdav" | "s3" | "seafile";
export type AiProtocol = "anthropic" | "openai";
export const ANTHROPIC_ADDRESS = "https://api.anthropic.com";

export const DEFAULT_SYNC_SERVICE = "https://sync.paperloom.routeforge.zone";

export const DEFAULT_PREFERENCES: Preferences = {
  tabStyle: "horizontal",
  notesEdge: "right",
  showNotes: true,
  language: "system",
  syncTarget: "cloud",
  syncService: DEFAULT_SYNC_SERVICE,
  syncFolder: "",
  webdavAddress: "",
  webdavUser: "",
  seafileServer: "",
  seafileAccount: "",
  seafileLibrary: "",
  seafileFolder: "",
  s3Endpoint: "",
  s3Region: "",
  s3Bucket: "",
  s3Prefix: "",
  s3AccessKeyId: "",
  aiProtocol: "anthropic",
  aiBaseUrl: ANTHROPIC_ADDRESS,
  aiModel: "",
  aiEmbeddingModel: "",
  autoBackup: "off",
  backupFolder: "",
  sidebarWidth: SIDEBAR_WIDTH.initial,
  notesWidth: NOTES_WIDTH.initial,
  splitRatio: SPLIT_RATIO.initial,
};

const KEY = "paperloom.preferences";

function oneOf<T>(value: unknown, allowed: readonly T[], fallback: T): T {
  return allowed.includes(value as T) ? (value as T) : fallback;
}

/** Reads the saved preferences; anything missing or invalid gets its default. */
export function loadPreferences(storage: Pick<Storage, "getItem"> | null): Preferences {
  let saved: Record<string, unknown> = {};
  try {
    const parsed: unknown = JSON.parse(storage?.getItem(KEY) ?? "{}");
    if (parsed && typeof parsed === "object") saved = parsed as Record<string, unknown>;
  } catch {
    // Corrupt value: start from the defaults.
  }
  const d = DEFAULT_PREFERENCES;
  return {
    tabStyle: oneOf<TabStyle>(saved.tabStyle, ["horizontal", "vertical-left", "vertical-right"], d.tabStyle),
    notesEdge: oneOf<Edge>(saved.notesEdge, ["left", "right"], d.notesEdge),
    showNotes: typeof saved.showNotes === "boolean" ? saved.showNotes : d.showNotes,
    language: oneOf<Language | "system">(saved.language, ["system", ...LANGUAGES.map((l) => l.code)], d.language),
    syncTarget: oneOf<SyncKind>(saved.syncTarget, ["cloud", "folder", "webdav", "s3", "seafile"], d.syncTarget),
    syncService:
      typeof saved.syncService === "string" && /^https?:\/\/\S+$/.test(saved.syncService) ? saved.syncService : d.syncService,
    syncFolder: typeof saved.syncFolder === "string" ? saved.syncFolder : d.syncFolder,
    webdavAddress:
      typeof saved.webdavAddress === "string" && /^https?:\/\/\S+$/.test(saved.webdavAddress) ? saved.webdavAddress : d.webdavAddress,
    webdavUser: typeof saved.webdavUser === "string" ? saved.webdavUser : d.webdavUser,
    seafileServer:
      typeof saved.seafileServer === "string" && /^https?:\/\/\S+$/.test(saved.seafileServer) ? saved.seafileServer : d.seafileServer,
    seafileAccount: typeof saved.seafileAccount === "string" ? saved.seafileAccount : d.seafileAccount,
    seafileLibrary: typeof saved.seafileLibrary === "string" ? saved.seafileLibrary : d.seafileLibrary,
    seafileFolder: typeof saved.seafileFolder === "string" ? saved.seafileFolder : d.seafileFolder,
    s3Endpoint:
      typeof saved.s3Endpoint === "string" && /^https?:\/\/\S+$/.test(saved.s3Endpoint) ? saved.s3Endpoint : d.s3Endpoint,
    s3Region: typeof saved.s3Region === "string" ? saved.s3Region : d.s3Region,
    s3Bucket: typeof saved.s3Bucket === "string" ? saved.s3Bucket : d.s3Bucket,
    s3Prefix: typeof saved.s3Prefix === "string" ? saved.s3Prefix : d.s3Prefix,
    s3AccessKeyId: typeof saved.s3AccessKeyId === "string" ? saved.s3AccessKeyId : d.s3AccessKeyId,
    aiProtocol: oneOf<AiProtocol>(saved.aiProtocol, ["anthropic", "openai"], d.aiProtocol),
    aiBaseUrl:
      typeof saved.aiBaseUrl === "string" && (saved.aiBaseUrl === "" || /^https?:\/\/\S+$/.test(saved.aiBaseUrl))
        ? saved.aiBaseUrl
        : d.aiBaseUrl,
    aiModel: typeof saved.aiModel === "string" ? saved.aiModel : d.aiModel,
    aiEmbeddingModel: typeof saved.aiEmbeddingModel === "string" ? saved.aiEmbeddingModel : d.aiEmbeddingModel,
    autoBackup: oneOf<AutoBackup>(saved.autoBackup, ["off", "daily", "weekly"], d.autoBackup),
    backupFolder: typeof saved.backupFolder === "string" ? saved.backupFolder : d.backupFolder,
    sidebarWidth: size(saved.sidebarWidth, SIDEBAR_WIDTH),
    notesWidth: size(saved.notesWidth, NOTES_WIDTH),
    splitRatio: size(saved.splitRatio, SPLIT_RATIO),
  };
}

export function savePreferences(preferences: Preferences, storage: Pick<Storage, "setItem"> | null): void {
  try {
    storage?.setItem(KEY, JSON.stringify(preferences));
  } catch {
    // Storage full or unavailable: the preferences last for this session.
  }
}

/** localStorage, or null where it is unavailable. */
export function browserStorage(): Storage | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage;
  } catch {
    return null;
  }
}
