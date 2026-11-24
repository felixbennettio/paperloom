import { useEffect, useState } from "react";
import { LANGUAGES, useI18n } from "../i18n";
import {
  backend,
  type AccountUser,
  type AiModel,
  type AiSemanticStatus,
  type CitationStyle,
  type CommunityPlugin,
  type ConnectedWriter,
  type LibraryInfo,
  type ObsidianPlugin,
  type ZoteroPlugin,
  type TranslatorStatus,
  type SearchSource,
  type SruCatalogue,
  type StyleListing,
  type SyncStatus,
  type SyncTarget,
  type VaultSettings,
  type WordAddin,
  type WpsAddin,
} from "../lib/api";
import {
  DATABASE_KEYS_CHANGED,
  KEY_PAGES,
  isCatalogueAddress,
  loadCatalogues,
  newCatalogueId,
  saveCatalogues,
} from "../lib/databases";
import { TRANSLATORS_CHANGED } from "../lib/translators";
import { pluginHost } from "../lib/plugins";
import { usePlugins } from "./PluginLayer";
import { CITATION_STYLES_CHANGED, STYLE_FORMATS } from "../lib/styles";
import { ANTHROPIC_ADDRESS, type AiProtocol, type AutoBackup, type Preferences, type SyncKind } from "../lib/preferences";

interface Props {
  preferences: Preferences;
  onChange: (preferences: Preferences) => void;
  info: LibraryInfo | null;
  /** After a sync changed the library. */
  onSynced: () => void;
}

export function SettingsView({ preferences, onChange, info, onSynced }: Props) {
  const { t } = useI18n();
  const set = <K extends keyof Preferences>(key: K, value: Preferences[K]) => onChange({ ...preferences, [key]: value });
  const setMany = (patch: Partial<Preferences>) => onChange({ ...preferences, ...patch });
  const [user, setUser] = useState<AccountUser | null>(null);
  const [showNotices, setShowNotices] = useState(false);

  return (
    <div className="settings-view">
      <h2>{t("Settings")}</h2>
      <AccountSection onUser={setUser} />
      <SyncCard preferences={preferences} set={set} signedIn={user !== null} onSynced={onSynced} />
      <AiCard preferences={preferences} setMany={setMany} />
      <DatabasesCard />
      <CataloguesCard />
      <NotesCard />
      <StylesCard />
      <WritingPluginsCard />
      <PluginsCard />
      <ZoteroPluginsCard />
      <TranslatorsCard />
      <BackupCard preferences={preferences} set={set} />
      <section className="card">
        <h3>{t("Workbench")}</h3>
        <label className="setting">
          <span>{t("Tabs")}</span>
          <select value={preferences.tabStyle} onChange={(e) => set("tabStyle", e.target.value as Preferences["tabStyle"])}>
            <option value="horizontal">{t("Horizontal tabs")}</option>
            <option value="vertical-left">{t("Vertical tabs on the left")}</option>
            <option value="vertical-right">{t("Vertical tabs on the right")}</option>
          </select>
        </label>
        <label className="setting">
          <span>{t("Notes panel")}</span>
          <select value={preferences.notesEdge} onChange={(e) => set("notesEdge", e.target.value as Preferences["notesEdge"])}>
            <option value="left">{t("Left")}</option>
            <option value="right">{t("Right")}</option>
          </select>
        </label>
        <label className="setting">
          <span>{t("Show notes panel")}</span>
          <input type="checkbox" checked={preferences.showNotes} onChange={(e) => set("showNotes", e.target.checked)} />
        </label>
        <label className="setting">
          <span>{t("Language")}</span>
          <select value={preferences.language} onChange={(e) => set("language", e.target.value as Preferences["language"])}>
            <option value="system">{t("Follow system")}</option>
            {LANGUAGES.map((l) => (
              <option key={l.code} value={l.code} lang={l.code}>
                {l.name}
              </option>
            ))}
          </select>
        </label>
      </section>
      <section className="card">
        <h3>{t("About")}</h3>
        <dl>
          <dt>{t("Version")}</dt>
          <dd>{__APP_VERSION__}</dd>
          <dt>{t("Core")}</dt>
          <dd>{info?.coreVersion ?? "…"}</dd>
          <dt>{t("Library")}</dt>
          <dd className="path">{info?.location ?? "…"}</dd>
        </dl>
        <div className="row">
          <button type="button" onClick={() => setShowNotices(true)}>
            {t("Third-Party Notices")}
          </button>
        </div>
        {showNotices && <NoticesDialog onClose={() => setShowNotices(false)} />}
      </section>
    </div>
  );
}

/** Sign in with the Paperloom account (system browser, then back through
 * the app's link). */
function AccountSection({ onUser }: { onUser: (user: AccountUser | null) => void }) {
  const { t } = useI18n();
  const [available, setAvailable] = useState(false);
  const [user, setUserHere] = useState<AccountUser | null>(null);
  const setUser = (changed: AccountUser | null) => {
    setUserHere(changed);
    onUser(changed);
  };
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    backend.accountAvailable().then(setAvailable, () => setAvailable(false));
    backend.accountStatus().then(setUser, () => setUser(null));
    let unsubscribe: (() => void) | undefined;
    let active = true;
    backend
      .onAccountChange((changed, failure) => {
        setWorking(false);
        if (failure) setError(failure);
        else {
          setUser(changed);
          setError(null);
        }
      })
      .then((off) => (active ? (unsubscribe = off) : off()));
    return () => {
      active = false;
      unsubscribe?.();
    };
  }, []);

  const signIn = async () => {
    setWorking(true);
    setError(null);
    try {
      await backend.accountSignIn();
    } catch (e) {
      setWorking(false);
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <section className="card">
      <h3>{t("Account")}</h3>
      {user ? (
        <div className="setting">
          <span>
            <strong>{user.displayName}</strong>
            {user.email && <small className="muted"> {user.email}</small>}
          </span>
          <button onClick={() => void backend.accountSignOut()}>{t("Sign Out")}</button>
        </div>
      ) : (
        <div className="setting">
          <span className="muted">
            {available ? t("Sign in to sync your library across devices with Paperloom Cloud.") : t("Signing in is not set up in this build yet.")}
          </span>
          <button className="primary" disabled={!available || working} onClick={() => void signIn()}>
            {t("Sign In")}
          </button>
        </div>
      )}
      {error && <p className="hint error">{error}</p>}
    </section>
  );
}

type SetPreference = <K extends keyof Preferences>(key: K, value: Preferences[K]) => void;

interface SyncCardProps {
  preferences: Preferences;
  set: SetPreference;
  signedIn: boolean;
  onSynced: () => void;
}

/** The library syncs with Paperloom Cloud, through a shared folder, or through
 * WebDAV, S3 or Seafile. This card shows its status and starts sync. */
function SyncCard({ preferences, set, signedIn, onSynced }: SyncCardProps) {
  const { t } = useI18n();
  const [status, setStatus] = useState<SyncStatus | null>(null);
  const [syncing, setSyncing] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [password, setPassword] = useState("");
  const [passwordKept, setPasswordKept] = useState(false);
  const [seafilePassword, setSeafilePassword] = useState("");
  const [seafilePasswordKept, setSeafilePasswordKept] = useState(false);
  const [secret, setSecret] = useState("");
  const [secretKept, setSecretKept] = useState(false);
  const [phrase, setPhrase] = useState("");
  const [sealed, setSealed] = useState(false);
  const [sealing, setSealing] = useState(false);
  const refresh = () => {
    backend.syncStatus().then(setStatus, () => setStatus(null));
  };
  useEffect(refresh, []);
  useEffect(() => {
    backend.webdavPasswordKnown().then(setPasswordKept, () => setPasswordKept(false));
    backend.seafilePasswordKnown().then(setSeafilePasswordKept, () => setSeafilePasswordKept(false));
    backend.s3SecretKnown().then(setSecretKept, () => setSecretKept(false));
    backend.syncPassphraseKnown().then(setSealed, () => setSealed(false));
  }, []);

  const kind = preferences.syncTarget;
  const target: SyncTarget | null =
    kind === "cloud"
      ? signedIn && preferences.syncService.trim()
        ? { kind, service: preferences.syncService.trim() }
        : null
      : kind === "folder"
        ? preferences.syncFolder
          ? { kind, path: preferences.syncFolder }
          : null
        : kind === "webdav"
          ? preferences.webdavAddress.trim() && preferences.webdavUser.trim() && passwordKept
            ? { kind, address: preferences.webdavAddress.trim(), username: preferences.webdavUser.trim() }
            : null
          : kind === "seafile"
            ? preferences.seafileServer.trim() && preferences.seafileAccount.trim() && preferences.seafileLibrary.trim() && seafilePasswordKept
              ? {
                  kind,
                  server: preferences.seafileServer.trim(),
                  account: preferences.seafileAccount.trim(),
                  library: preferences.seafileLibrary.trim(),
                  folder: preferences.seafileFolder.trim(),
                }
              : null
            : preferences.s3Endpoint.trim() && preferences.s3Bucket.trim() && preferences.s3AccessKeyId.trim() && secretKept
              ? {
                  kind,
                  endpoint: preferences.s3Endpoint.trim(),
                  region: preferences.s3Region.trim(),
                  bucket: preferences.s3Bucket.trim(),
                  prefix: preferences.s3Prefix.trim(),
                  accessKeyId: preferences.s3AccessKeyId.trim(),
                }
              : null;

  const keepPassword = async () => {
    if (!password) return;
    try {
      await backend.webdavPasswordSet(password);
      setPassword("");
      setPasswordKept(true);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const keepSeafilePassword = async () => {
    if (!seafilePassword) return;
    try {
      await backend.seafilePasswordSet(seafilePassword);
      setSeafilePassword("");
      setSeafilePasswordKept(true);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  /** Seals sync with a passphrase, or (empty) stops sealing. */
  const keepPhrase = async (passphrase: string) => {
    setSealing(true);
    try {
      await backend.syncPassphraseSet(passphrase);
      setPhrase("");
      setSealed(passphrase !== "");
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSealing(false);
    }
  };

  const keepSecret = async () => {
    if (!secret) return;
    try {
      await backend.s3SecretSet(secret);
      setSecret("");
      setSecretKept(true);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const chooseFolder = async () => {
    try {
      const folder = await backend.syncPickFolder();
      if (folder) set("syncFolder", folder);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const syncNow = async () => {
    if (!target) return;
    setSyncing(true);
    setNotice(null);
    setError(null);
    try {
      const report = await backend.syncNow(target);
      setNotice(
        t("Changes sent: {sent}; received: {received}. Files downloaded: {downloaded}.", {
          sent: report.sent,
          received: report.received,
          downloaded: report.downloaded,
        }),
      );
      if (report.changed > 0 || report.downloaded > 0) onSynced();
      // Open notes reload what sync wrote.
      if (report.notes > 0) window.dispatchEvent(new Event("paperloom:synced"));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSyncing(false);
      refresh();
    }
  };

  const last = status?.lastSynced ? new Date(status.lastSynced).toLocaleString() : null;
  return (
    <section className="card">
      <h3>{t("Sync")}</h3>
      <label className="setting">
        <span>{t("Sync with")}</span>
        <select value={kind} onChange={(e) => set("syncTarget", e.target.value as SyncKind)}>
          <option value="cloud">{t("Paperloom Cloud")}</option>
          <option value="folder">{t("A Folder")}</option>
          <option value="webdav">{t("WebDAV")}</option>
          <option value="s3">{t("S3 Storage")}</option>
          <option value="seafile">{t("Seafile")}</option>
        </select>
      </label>
      {kind === "cloud" &&
        (signedIn ? (
          <label className="setting">
            <span>{t("Sync service")}</span>
            <input type="url" value={preferences.syncService} spellCheck={false} onChange={(e) => set("syncService", e.target.value)} />
          </label>
        ) : (
          <p className="hint">{t("Sign in under Account first to sync with Paperloom Cloud.")}</p>
        ))}
      {kind === "folder" && (
        <>
          <div className="setting">
            <span>{t("Folder")}</span>
            <button className="setting-path" title={preferences.syncFolder} onClick={() => void chooseFolder()}>{preferences.syncFolder || t("Choose…")}</button>
          </div>
          <p className="hint">
            {t("iCloud Drive, a synced Nextcloud, Seafile or Syncthing folder, or a NAS share. Each library needs a folder of its own.")}
          </p>
        </>
      )}
      {kind === "webdav" && (
        <>
          <label className="setting">
            <span>{t("WebDAV address")}</span>
            <input
              type="url"
              value={preferences.webdavAddress}
              placeholder="https://dav.jianguoyun.com/dav/Paperloom"
              spellCheck={false}
              onChange={(e) => set("webdavAddress", e.target.value)}
            />
          </label>
          <label className="setting">
            <span>{t("User name")}</span>
            <input value={preferences.webdavUser} spellCheck={false} onChange={(e) => set("webdavUser", e.target.value)} />
          </label>
          <label className="setting">
            <span>{t("Password")}</span>
            <input
              type="password"
              value={password}
              placeholder={passwordKept ? "••••••••" : ""}
              onChange={(e) => setPassword(e.target.value)}
              onBlur={() => void keepPassword()}
            />
          </label>
          <p className="hint">{t("For 坚果云 and Nextcloud, use an app password.")}</p>
        </>
      )}
      {kind === "seafile" && (
        <>
          <label className="setting">
            <span>{t("Seafile address")}</span>
            <input
              type="url"
              value={preferences.seafileServer}
              placeholder="https://cloud.example.com"
              spellCheck={false}
              onChange={(e) => set("seafileServer", e.target.value)}
            />
          </label>
          <label className="setting">
            <span>{t("Seafile account")}</span>
            <input value={preferences.seafileAccount} spellCheck={false} onChange={(e) => set("seafileAccount", e.target.value)} />
          </label>
          <label className="setting">
            <span>{t("Seafile library")}</span>
            <input value={preferences.seafileLibrary} placeholder="Paperloom" spellCheck={false} onChange={(e) => set("seafileLibrary", e.target.value)} />
          </label>
          <label className="setting">
            <span>{t("Folder in the library")}</span>
            <input value={preferences.seafileFolder} spellCheck={false} onChange={(e) => set("seafileFolder", e.target.value)} />
          </label>
          <label className="setting">
            <span>{t("Password or API token")}</span>
            <input
              type="password"
              value={seafilePassword}
              placeholder={seafilePasswordKept ? "••••••••" : ""}
              onChange={(e) => setSeafilePassword(e.target.value)}
              onBlur={() => void keepSeafilePassword()}
            />
          </label>
          <p className="hint">{t("The library is created if missing. Leave the folder empty for its top. A credential of exactly 40 hexadecimal characters is used as an account API token; otherwise it is a password.")}</p>
        </>
      )}
      {kind === "s3" && (
        <>
          <label className="setting">
            <span>{t("S3 address")}</span>
            <input
              type="url"
              value={preferences.s3Endpoint}
              placeholder="https://s3.us-east-1.amazonaws.com"
              spellCheck={false}
              onChange={(e) => set("s3Endpoint", e.target.value)}
            />
          </label>
          <label className="setting">
            <span>{t("Region")}</span>
            <input value={preferences.s3Region} placeholder="us-east-1" spellCheck={false} onChange={(e) => set("s3Region", e.target.value)} />
          </label>
          <label className="setting">
            <span>{t("Bucket")}</span>
            <input value={preferences.s3Bucket} spellCheck={false} onChange={(e) => set("s3Bucket", e.target.value)} />
          </label>
          <label className="setting">
            <span>{t("Folder in the bucket")}</span>
            <input value={preferences.s3Prefix} placeholder="paperloom" spellCheck={false} onChange={(e) => set("s3Prefix", e.target.value)} />
          </label>
          <label className="setting">
            <span>{t("Access key ID")}</span>
            <input value={preferences.s3AccessKeyId} spellCheck={false} onChange={(e) => set("s3AccessKeyId", e.target.value)} />
          </label>
          <label className="setting">
            <span>{t("Secret key")}</span>
            <input
              type="password"
              value={secret}
              placeholder={secretKept ? "••••••••" : ""}
              onChange={(e) => setSecret(e.target.value)}
              onBlur={() => void keepSecret()}
            />
          </label>
          <p className="hint">{t("AWS S3, Cloudflare R2 (region auto), MinIO, 阿里云 OSS or 腾讯云 COS. The bucket must exist; give the keys access to it alone.")}</p>
        </>
      )}
      <div className="setting">
        <span>{t("End-to-end encryption")}</span>
        {sealed ? (
          <>
            <span className="muted">{t("On")}</span>
            <button disabled={sealing} onClick={() => void keepPhrase("")}>
              {t("Turn Off")}
            </button>
          </>
        ) : (
          <>
            <input
              type="password"
              value={phrase}
              placeholder={t("Passphrase")}
              aria-label={t("Passphrase")}
              onChange={(e) => setPhrase(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && phrase && void keepPhrase(phrase)}
            />
            <button disabled={sealing || !phrase} onClick={() => void keepPhrase(phrase)}>
              {sealing ? t("Working…") : t("Turn On")}
            </button>
          </>
        )}
      </div>
      <p className="hint">
        {t("Changes and files are sealed on this device before they sync. Every device needs the same passphrase, and nobody can recover it.")}
      </p>
      <div className="setting">
        <span className="muted">
          {last ? t("Last synced {time}", { time: last }) : t("Not synced yet")}
          {status && status.waiting > 0 ? ` · ${t("Changes waiting: {count}", { count: status.waiting })}` : ""}
        </span>
        <button className="primary" disabled={syncing || !target} onClick={() => void syncNow()}>
          {syncing ? t("Syncing…") : t("Sync Now")}
        </button>
      </div>
      {notice && <p className="hint">{notice}</p>}
      {error && <p className="hint error">{error}</p>}
    </section>
  );
}

/** OpenAI-style providers people use, by name. */
const AI_PRESETS: [string, string][] = [
  ["OpenAI", "https://api.openai.com/v1"],
  ["DeepSeek", "https://api.deepseek.com/v1"],
  ["Kimi", "https://api.moonshot.cn/v1"],
  ["Qwen", "https://dashscope.aliyuncs.com/compatible-mode/v1"],
  ["GLM", "https://open.bigmodel.cn/api/paas/v4"],
  ["Ollama", "http://localhost:11434/v1"],
];

/** The AI provider: its API, address, key (kept on this device) and model. */
/** Backing the library up to one `.zip`, and putting a backup back. */
function BackupCard({ preferences, set }: { preferences: Preferences; set: SetPreference }) {
  const { t } = useI18n();
  const [working, setWorking] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);
  const [restartable, setRestartable] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const run = async (task: () => Promise<void>) => {
    setWorking(true);
    setNotice(null);
    setError(null);
    try {
      await task();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setWorking(false);
    }
  };
  const chooseFolder = () =>
    run(async () => {
      const folder = await backend.syncPickFolder();
      if (folder) set("backupFolder", folder);
    });
  const backUp = () =>
    run(async () => {
      const day = new Date().toISOString().slice(0, 10);
      const report = await backend.backUpLibrary(`Paperloom ${day}.zip`);
      if (report) {
        const size = (report.bytes / 1024 / 1024).toFixed(1);
        setNotice(t("Backed up to {path}. Files: {files} ({size} MB).", { files: report.files, size, path: report.path }));
      }
    });
  const restore = () =>
    run(async () => {
      if (!window.confirm(t("Put a backup back? The library as it is now is kept aside, and notes it replaces are kept as versions."))) return;
      const report = await backend.restoreLibrary();
      if (report) {
        setNotice(t("Notes put back: {notes}. Restart Paperloom to bring back the library and its files ({attachments}).", { notes: report.notes, attachments: report.attachments }));
        setRestartable(true);
      }
    });

  return (
    <section className="card">
      <h3>{t("Backup")}</h3>
      <div className="setting">
        <span className="setting-description">{t("The library, its files and the vault, in one .zip.")}</span>
        <button disabled={working} onClick={() => void backUp()}>
          {working ? t("Working…") : t("Back Up Now…")}
        </button>
      </div>
      <div className="setting">
        <span>{t("Put a backup back")}</span>
        <button disabled={working} onClick={() => void restore()}>
          {t("Restore from Backup…")}
        </button>
      </div>
      <label className="setting">
        <span>{t("Back up automatically")}</span>
        <select value={preferences.autoBackup} onChange={(e) => set("autoBackup", e.target.value as AutoBackup)}>
          <option value="off">{t("Off")}</option>
          <option value="daily">{t("Every day")}</option>
          <option value="weekly">{t("Every week")}</option>
        </select>
      </label>
      {preferences.autoBackup !== "off" && (
        <div className="setting">
          <span>{t("Backup folder")}</span>
          <button className="setting-path" title={preferences.backupFolder} onClick={() => void chooseFolder()}>{preferences.backupFolder || t("Beside the library")}</button>
          {preferences.backupFolder && <button onClick={() => set("backupFolder", "")}>{t("Reset")}</button>}
        </div>
      )}
      {preferences.autoBackup !== "off" && <p className="hint">{t("The newest 10 automatic backups are kept.")}</p>}
      {notice && <p className="hint">{notice}</p>}
      {restartable && (
        <button className="primary" onClick={() => void backend.restartApp()}>
          {t("Restart Now")}
        </button>
      )}
      {error && <p className="hint error">{error}</p>}
    </section>
  );
}

/** Daily notes and templates; kept in the vault's `.obsidian` folder, so
 * Obsidian uses the same ones. */
function NotesCard() {
  const { t } = useI18n();
  const [settings, setSettings] = useState<VaultSettings | null>(null);
  const [styles, setStyles] = useState<CitationStyle[]>([]);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    backend.vaultSettings().then(setSettings, (e) => setError(e instanceof Error ? e.message : String(e)));
    const load = () => backend.citationStyles().then(setStyles, () => setStyles([]));
    load();
    window.addEventListener(CITATION_STYLES_CHANGED, load);
    return () => window.removeEventListener(CITATION_STYLES_CHANGED, load);
  }, []);
  if (!settings) return null;

  const keep = (next: VaultSettings) => {
    setSettings(next);
    backend.vaultSetSettings(next).then(
      () => setError(null),
      (e) => setError(e instanceof Error ? e.message : String(e)),
    );
  };
  const field = (key: keyof VaultSettings, label: string, placeholder = "") => (
    <label className="setting">
      <span>{t(label)}</span>
      <input
        value={settings[key]}
        placeholder={placeholder}
        spellCheck={false}
        onChange={(e) => setSettings({ ...settings, [key]: e.target.value })}
        onBlur={() => keep(settings)}
      />
    </label>
  );

  return (
    <section className="card">
      <h3>{t("Notes")}</h3>
      {field("dailyFolder", "Daily notes folder", t("The vault's top folder"))}
      {field("dailyFormat", "Daily note name", "YYYY-MM-DD")}
      {field("dailyTemplate", "Daily note template", "Templates/Daily")}
      {field("templatesFolder", "Templates folder", "Templates")}
      {field("dateFormat", "Date format", "YYYY-MM-DD")}
      {field("timeFormat", "Time format", "HH:mm")}
      <label className="setting">
        <span>{t("Citations in notes")}</span>
        <select value={settings.citationStyle} onChange={(e) => keep({ ...settings, citationStyle: e.target.value })}>
          {styles.map((style) => (
            <option key={style.id} value={style.id}>
              {style.name}
            </option>
          ))}
        </select>
      </label>
      <p className="hint">{t("Formats are Moment.js patterns, as in Obsidian, which shares these settings with this vault.")}</p>
      {error && <p className="hint error">{error}</p>}
    </section>
  );
}

/** Styles beyond the built-in ones: from the CSL style repository or a
 * `.csl` file. */
function StylesCard() {
  const { t } = useI18n();
  const [added, setAdded] = useState<CitationStyle[]>([]);
  const [query, setQuery] = useState("");
  const [found, setFound] = useState<StyleListing[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const load = () => backend.citationStyles().then((all) => setAdded(all.filter((s) => s.added)), () => setAdded([]));
  useEffect(() => {
    void load();
  }, []);

  const run = async (what: string, action: () => Promise<unknown>) => {
    setBusy(what);
    try {
      await action();
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  };
  const changed = async () => {
    await load();
    window.dispatchEvent(new Event(CITATION_STYLES_CHANGED));
  };
  const search = () =>
    query.trim() && run("search", async () => setFound(await backend.searchCitationStyles(query, 30)));
  const remove = (id: string) =>
    run(id, async () => {
      await backend.removeCitationStyle(id);
      setFound((list) => list?.map((s) => (s.id === id ? { ...s, available: false } : s)) ?? null);
      await changed();
    });
  const download = (id: string) =>
    run(id, async () => {
      await backend.downloadCitationStyle(id);
      setFound((list) => list?.map((s) => (s.id === id ? { ...s, available: true } : s)) ?? null);
      await changed();
    });

  return (
    <section className="card">
      <h3>{t("Citation Styles")}</h3>
      {added.map((style) => (
        <div className="setting" key={style.id}>
          <span>{style.name}</span>
          <button disabled={busy !== null} onClick={() => void remove(style.id)}>
            {t("Remove")}
          </button>
        </div>
      ))}
      <div className="setting">
        <span>{t("Search")}</span>
        <input
          type="search"
          value={query}
          placeholder={t("Search the style repository")}
          aria-label={t("Search the style repository")}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && void search()}
        />
        <div className="setting-actions">
          <button disabled={busy !== null || !query.trim()} onClick={() => void search()}>
            {busy === "search" ? t("Searching…") : t("Search")}
          </button>
          <button onClick={() => void run("file", async () => (await backend.addCitationStyleFile()) && (await changed()))}>
            {t("Add from File…")}
          </button>
        </div>
      </div>
      {found?.length === 0 && <p className="hint">{t("No style matches.")}</p>}
      {found?.map((style) => (
        <div className="setting" key={style.id}>
          <span>
            {style.name}
            <small className="hint"> · {STYLE_FORMATS[style.format] ? t(STYLE_FORMATS[style.format]) : style.format}</small>
          </span>
          <button disabled={style.available || busy !== null} onClick={() => void download(style.id)}>
            {style.available ? t("Added") : t("Add")}
          </button>
        </div>
      ))}
      <p className="hint">
        {t("Styles come from the CSL style repository, the one Zotero uses, and are kept beside the library on this device.")}
      </p>
      {error && <p className="hint error">{error}</p>}
    </section>
  );
}

/** Word and WPS: the task panes allowed to use the library, the Word
 * add-in this app gives Word (Windows) and the WPS add-in it puts in WPS
 * (Linux, Windows). */
function WritingPluginsCard() {
  const { t } = useI18n();
  const [writers, setWriters] = useState<ConnectedWriter[]>([]);
  const [word, setWord] = useState<WordAddin | null>(null);
  const [wps, setWps] = useState<WpsAddin | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    void backend.connectedWriters().then(setWriters, () => setWriters([]));
    void backend.wordAddin().then(setWord, () => setWord(null));
    void backend.wpsAddin().then(setWps, () => setWps(null));
  }, []);

  const run = async (action: () => Promise<void>) => {
    setBusy(true);
    try {
      await action();
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section className="card">
      <h3>{t("Writing Plugins")}</h3>
      {word?.available && (
        <>
          <div className="setting">
            <span>{t("Word add-in")}</span>
            <div className="setting-actions">
              <button disabled={busy} onClick={() => void run(async () => setWord(await backend.installWordAddin()))}>
                {!word.installed ? t("Install") : word.installed === word.version ? t("Reinstall") : t("Update")}
              </button>
              {word.installed && (
                <button disabled={busy} onClick={() => void run(async () => setWord(await backend.removeWordAddin()))}>
                  {t("Remove")}
                </button>
              )}
            </div>
          </div>
          <p className="hint">
            {word.installed ? t("Version {version} installed", { version: word.installed }) : t("Not installed")}
            {" · "}
            {t("Restart Word after installing. Paperloom's Cite button is then on Word's Home tab.")}
          </p>
        </>
      )}
      {wps?.available && (
        <>
          <div className="setting">
            <span>{t("WPS add-in")}</span>
            <div className="setting-actions">
              <button disabled={busy} onClick={() => void run(async () => setWps(await backend.installWpsAddin()))}>
                {!wps.installed ? t("Install") : wps.installed === wps.version ? t("Reinstall") : t("Update")}
              </button>
              {wps.installed && (
                <button disabled={busy} onClick={() => void run(async () => setWps(await backend.removeWpsAddin()))}>
                  {t("Remove")}
                </button>
              )}
            </div>
          </div>
          <p className="hint">
            {wps.installed ? t("Version {version} installed", { version: wps.installed }) : t("Not installed")}
            {" · "}
            {t("Restart WPS after installing. The first time, WPS asks whether to allow and trust the Paperloom add-in.")}
          </p>
        </>
      )}
      {writers.map((writer) => (
        <div className="setting" key={`${writer.client} ${writer.origin} ${writer.added}`}>
          <span>
            {writer.client}
            <small className="hint">
              {" · "}
              {writer.origin || t("this computer")} · {new Date(writer.added * 1000).toLocaleDateString()}
            </small>
          </span>
        </div>
      ))}
      {writers.length > 0 ? (
        <div className="setting">
          <span className="hint">{t("Connected writing plugins can search the library and format citations.")}</span>
          <button
            disabled={busy}
            onClick={() =>
              void run(async () => {
                await backend.forgetConnectedWriters();
                setWriters([]);
              })
            }
          >
            {t("Disconnect All")}
          </button>
        </div>
      ) : (
        <p className="hint">{t("No writing plugin is connected. The Word and WPS add-ins ask here the first time they connect.")}</p>
      )}
      {error && <p className="hint error">{error}</p>}
    </section>
  );
}

/** The Obsidian plugins in the vault: turning them on and off (for
 * Obsidian too), their options, and what of them works here. */
function PluginsCard() {
  const { t } = useI18n();
  const running = usePlugins().running;
  const [plugins, setPlugins] = useState<ObsidianPlugin[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [catalog, setCatalog] = useState<CommunityPlugin[] | null>(null);
  const [query, setQuery] = useState("");
  const [installing, setInstalling] = useState<string | null>(null);
  const load = () => backend.obsidianPlugins().then(setPlugins, () => setPlugins([]));
  useEffect(() => {
    void load();
  }, []);

  const toggle = async (plugin: ObsidianPlugin) => {
    try {
      await backend.setObsidianPluginEnabled(plugin.id, !plugin.enabled);
      await load();
      await pluginHost.reload();
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const browse = async () => {
    try {
      setCatalog(await backend.communityPlugins());
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const install = async (plugin: CommunityPlugin) => {
    setInstalling(plugin.id);
    try {
      await backend.installObsidianPlugin(plugin.repo);
      await load();
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setInstalling(null);
    }
  };

  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  const matches = (catalog ?? [])
    .filter((p) => words.every((w) => `${p.name} ${p.author} ${p.description}`.toLowerCase().includes(w)))
    .slice(0, 40);

  return (
    <section className="card">
      <h3>{t("Obsidian Plugins")}</h3>
      {plugins.length === 0 && <p className="hint">{t("No plugins in this vault yet. Plugins installed in Obsidian (in the vault's .obsidian/plugins folder) show here.")}</p>}
      {plugins.map((plugin) => {
        const live = running.find((r) => r.id === plugin.id);
        return (
          <div className="setting plugin-setting" key={plugin.id}>
            <span className="setting-description">
              <strong>{plugin.name}</strong> <small className="hint">{plugin.version}{plugin.author ? ` · ${plugin.author}` : ""}</small>
              {plugin.description && <small className="hint plugin-description">{plugin.description}</small>}
              {live?.error && <small className="hint error">{live.error}</small>}
              {live && live.partial.length > 0 && (
                <small className="hint">{t("Partly works: {what} are not available here yet.", { what: live.partial.join(", ") })}</small>
              )}
            </span>
            <div className="setting-actions">
              {live?.hasSettings && <button onClick={() => pluginHost.showSettings(plugin.id)}>{t("Options")}</button>}
              <label className="switch">
                <input type="checkbox" checked={plugin.enabled} onChange={() => void toggle(plugin)} />
                {plugin.enabled ? t("On") : t("Off")}
              </label>
            </div>
          </div>
        );
      })}
      {catalog === null ? (
        <div className="setting">
          <span className="setting-description">{t("Find plugins in Obsidian's community list and install them from their releases.")}</span>
          <button onClick={() => void browse()}>{t("Browse Community Plugins")}</button>
        </div>
      ) : (
        <>
          <div className="setting">
            <span>{t("Search")}</span>
            <input type="search" value={query} placeholder={t("Search community plugins")} aria-label={t("Search community plugins")} onChange={(e) => setQuery(e.target.value)} />
          </div>
          {matches.map((plugin) => {
            const installed = plugins.some((p) => p.id === plugin.id);
            return (
              <div className="setting plugin-setting" key={plugin.id}>
                <span className="setting-description">
                  <strong>{plugin.name}</strong> <small className="hint">{plugin.author}</small>
                  <small className="hint plugin-description">{plugin.description}</small>
                </span>
                <button disabled={installing !== null} onClick={() => void install(plugin)}>
                  {installing === plugin.id ? t("Installing…") : installed ? t("Update") : t("Install")}
                </button>
              </div>
            );
          })}
        </>
      )}
      <p className="hint">
        {t("Each plugin runs apart from the app and asks before it reaches a website. Turning one on or off here does the same in Obsidian.")}
      </p>
      {error && <p className="hint error">{error}</p>}
    </section>
  );
}

/** Zotero 7 plugins for the library: installing them from their .xpi files,
 * turning them on and off, their preferences, and what of them works here. */
function ZoteroPluginsCard() {
  const { t } = useI18n();
  const running = usePlugins().running;
  const [plugins, setPlugins] = useState<ZoteroPlugin[]>([]);
  const [error, setError] = useState<string | null>(null);
  const load = () => backend.zoteroPlugins().then(setPlugins, () => setPlugins([]));
  useEffect(() => {
    void load();
  }, []);

  const run = async (action: () => Promise<unknown>) => {
    try {
      await action();
      await load();
      await pluginHost.reload();
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <section className="card">
      <h3>{t("Zotero Plugins")}</h3>
      {plugins.length === 0 && <p className="hint">{t("No Zotero plugins yet. Zotero 7 plugins come as .xpi files.")}</p>}
      {plugins.map((plugin) => {
        const live = running.find((r) => r.id === `zotero:${plugin.id}`);
        return (
          <div className="setting plugin-setting" key={plugin.id}>
            <span className="setting-description">
              <strong>{plugin.name}</strong> <small className="hint">{plugin.version}{plugin.author ? ` · ${plugin.author}` : ""}</small>
              {plugin.description && <small className="hint plugin-description">{plugin.description}</small>}
              {live?.error && <small className="hint error">{live.error}</small>}
              {live && live.partial.length > 0 && (
                <small className="hint">{t("Partly works: {what} are not available here yet.", { what: live.partial.join(", ") })}</small>
              )}
            </span>
            <div className="setting-actions">
              {live?.hasSettings && <button onClick={() => pluginHost.showSettings(`zotero:${plugin.id}`)}>{t("Options")}</button>}
              <label className="switch">
                <input type="checkbox" checked={plugin.enabled} onChange={() => void run(() => backend.setZoteroPluginEnabled(plugin.id, !plugin.enabled))} />
                {plugin.enabled ? t("On") : t("Off")}
              </label>
            </div>
          </div>
        );
      })}
      <div className="setting">
        <span className="setting-description">{t("Each plugin runs apart from the app and asks before it reaches a website.")}</span>
        <button onClick={() => void run(() => backend.installZoteroPlugin())}>{t("Install from File…")}</button>
      </div>
      {error && <p className="hint error">{error}</p>}
    </section>
  );
}

/** Keys for the databases that need one, each kept on this device. */
function DatabasesCard() {
  const { t } = useI18n();
  const [sources, setSources] = useState<SearchSource[]>([]);
  const [known, setKnown] = useState<string[]>([]);
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);
  useEffect(() => {
    backend.searchSources().then((all) => setSources(all.filter((s) => s.needsKey)), () => setSources([]));
    backend.databaseKeysKnown().then(setKnown, () => setKnown([]));
  }, []);

  const keep = async (id: string, key: string) => {
    try {
      await backend.databaseKeySet(id, key);
      setDrafts((all) => ({ ...all, [id]: "" }));
      setKnown(await backend.databaseKeysKnown());
      setError(null);
      window.dispatchEvent(new Event(DATABASE_KEYS_CHANGED));
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <section className="card">
      <h3>{t("Databases")}</h3>
      {sources.map((source) => {
        const kept = known.includes(source.id);
        const draft = drafts[source.id] ?? "";
        return (
          <div className="setting" key={source.id}>
            <span>{source.name}</span>
            <input
              type="password"
              value={draft}
              aria-label={t("{name} API key", { name: source.name })}
              placeholder={kept ? "••••••••" : t("API key")}
              onChange={(e) => setDrafts((all) => ({ ...all, [source.id]: e.target.value }))}
              onBlur={() => draft.trim() && void keep(source.id, draft)}
            />
            {kept ? (
              <button onClick={() => void keep(source.id, "")}>{t("Forget")}</button>
            ) : (
              KEY_PAGES[source.id] && (
                <button className="link" onClick={() => void backend.openUrl(KEY_PAGES[source.id])}>
                  {t("Get a Key")}
                </button>
              )
            )}
          </div>
        );
      })}
      <p className="hint">
        {t("Keys stay on this device and go only to their database. What you find depends on your subscription.")}
      </p>
      {error && <p className="hint error">{error}</p>}
    </section>
  );
}

/** Library catalogues searched from Online Search: any that answers SRU,
 * from a national library's preset or typed in. */
function CataloguesCard() {
  const { t } = useI18n();
  const [catalogues, setCatalogues] = useState<SruCatalogue[]>(() => loadCatalogues());
  const [presets, setPresets] = useState<SruCatalogue[]>([]);
  const [draft, setDraft] = useState<SruCatalogue | null>(null);
  useEffect(() => {
    backend.cataloguePresets().then(setPresets, () => setPresets([]));
  }, []);

  const store = (next: SruCatalogue[]) => {
    setCatalogues(next);
    saveCatalogues(next);
  };
  const start = (presetId: string) => {
    const preset = presets.find((p) => p.id === presetId);
    setDraft({ ...(preset ?? { name: "", url: "", index: "", schema: "" }), id: newCatalogueId() });
  };
  const edit = (patch: Partial<SruCatalogue>) => setDraft((d) => (d ? { ...d, ...patch } : d));
  const ready = draft !== null && draft.name.trim() !== "" && isCatalogueAddress(draft.url);
  const add = () => {
    if (!draft || !ready) return;
    const trimmed = { ...draft, name: draft.name.trim(), url: draft.url.trim(), index: draft.index.trim(), schema: draft.schema.trim() };
    store([...catalogues, trimmed]);
    setDraft(null);
  };

  return (
    <section className="card">
      <h3>{t("Library Catalogues")}</h3>
      {catalogues.map((catalogue) => (
        <div className="setting" key={catalogue.id}>
          <span>
            {catalogue.name}
            <span className="hint"> · {catalogue.url}</span>
          </span>
          <button onClick={() => store(catalogues.filter((c) => c.id !== catalogue.id))}>{t("Remove")}</button>
        </div>
      ))}
      {draft ? (
        <>
          <label className="setting">
            <span>{t("Name")}</span>
            <input value={draft.name} onChange={(e) => edit({ name: e.target.value })} />
          </label>
          <label className="setting">
            <span>{t("Address (SRU or Z39.50)")}</span>
            <input value={draft.url} placeholder="https://… / z3950://…" spellCheck={false} onChange={(e) => edit({ url: e.target.value })} />
          </label>
          <label className="setting">
            <span>{t("Search Index")}</span>
            <input
              value={draft.index}
              placeholder={t("The server's choice")}
              spellCheck={false}
              onChange={(e) => edit({ index: e.target.value })}
            />
          </label>
          <label className="setting">
            <span>{t("Record Format")}</span>
            <input value={draft.schema} placeholder="marcxml" spellCheck={false} onChange={(e) => edit({ schema: e.target.value })} />
          </label>
          <div className="setting">
            <span />
            <span className="setting-actions">
              <button onClick={() => setDraft(null)}>{t("Cancel")}</button>{" "}
              <button className="primary" disabled={!ready} onClick={add}>
                {t("Add")}
              </button>
            </span>
          </div>
        </>
      ) : (
        <label className="setting">
          <span>{t("Add a Catalogue…")}</span>
          <select value="" onChange={(e) => e.target.value && start(e.target.value)}>
            <option value="" />
            {presets.map((preset) => (
              <option key={preset.id} value={preset.id}>
                {preset.name}
              </option>
            ))}
            <option value="other">{t("Another Catalogue…")}</option>
          </select>
        </label>
      )}
      <p className="hint">
        {t("Any library catalogue that answers SRU or Z39.50 can be searched from Online Search. A Z39.50 address is z3950://host:port/database. Records come as MARC 21, UNIMARC (CNMARC) or Dublin Core (dc).")}
      </p>
    </section>
  );
}

function AiCard({ preferences, setMany }: { preferences: Preferences; setMany: (patch: Partial<Preferences>) => void }) {
  const { t } = useI18n();
  const [key, setKey] = useState("");
  const [keyKept, setKeyKept] = useState(false);
  const [models, setModels] = useState<AiModel[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [semantic, setSemantic] = useState<AiSemanticStatus | null>(null);
  const [indexing, setIndexing] = useState(false);
  useEffect(() => {
    backend.aiKeyKnown().then(setKeyKept, () => setKeyKept(false));
  }, []);
  const settings = {
    protocol: preferences.aiProtocol,
    baseUrl: preferences.aiBaseUrl,
    model: preferences.aiModel,
    embeddingModel: preferences.aiProtocol === "openai" ? preferences.aiEmbeddingModel.trim() : "",
  };
  // How much of the library the semantic index covers, for this model.
  useEffect(() => {
    setSemantic(null);
    if (!settings.embeddingModel) return;
    let current = true;
    backend.aiSemanticStatus(settings).then((status) => current && setSemantic(status), () => undefined);
    return () => {
      current = false;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [settings.protocol, settings.baseUrl, settings.embeddingModel]);

  /** Embeds the documents not yet indexed, a batch at a time, showing progress. */
  const buildIndex = async () => {
    setIndexing(true);
    setError(null);
    try {
      for (;;) {
        const status = await backend.aiSemanticIndex(settings, 64);
        setSemantic(status);
        if (status.indexed >= status.total) break;
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setIndexing(false);
    }
  };

  const keepKey = async () => {
    if (!key.trim()) return;
    try {
      await backend.aiKeySet(key);
      setKey("");
      setKeyKept(true);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const loadModels = async () => {
    setLoading(true);
    setError(null);
    try {
      const found = await backend.aiModels(settings);
      setModels(found);
      if (found.length && !found.some((m) => m.id === preferences.aiModel)) setMany({ aiModel: found[0].id });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  };

  const protocol = (value: AiProtocol) =>
    setMany({ aiProtocol: value, aiBaseUrl: value === "anthropic" ? ANTHROPIC_ADDRESS : "", aiModel: "" });

  return (
    <section className="card">
      <h3>{t("AI")}</h3>
      <label className="setting">
        <span>{t("Provider")}</span>
        <select value={preferences.aiProtocol} onChange={(e) => protocol(e.target.value as AiProtocol)}>
          <option value="anthropic">Anthropic</option>
          <option value="openai">{t("OpenAI-compatible")}</option>
        </select>
      </label>
      {preferences.aiProtocol === "openai" && (
        <label className="setting">
          <span>{t("Preset")}</span>
          <select value="" onChange={(e) => e.target.value && setMany({ aiBaseUrl: e.target.value, aiModel: "" })}>
            <option value="">{t("Choose…")}</option>
            {AI_PRESETS.map(([name, url]) => (
              <option key={url} value={url}>
                {name}
              </option>
            ))}
          </select>
        </label>
      )}
      <label className="setting">
        <span>{t("Address")}</span>
        <input type="url" value={preferences.aiBaseUrl} spellCheck={false} onChange={(e) => setMany({ aiBaseUrl: e.target.value })} />
      </label>
      <label className="setting">
        <span>{t("API key")}</span>
        <input
          type="password"
          value={key}
          placeholder={keyKept ? "••••••••" : ""}
          onChange={(e) => setKey(e.target.value)}
          onBlur={() => void keepKey()}
        />
      </label>
      <div className="setting">
        <span>{t("Model")}</span>
        {models.length > 0 ? (
          <select value={preferences.aiModel} aria-label={t("Model")} onChange={(e) => setMany({ aiModel: e.target.value })}>
            {models.map((m) => (
              <option key={m.id} value={m.id}>
                {m.name}
              </option>
            ))}
          </select>
        ) : (
          <input value={preferences.aiModel} aria-label={t("Model")} spellCheck={false} onChange={(e) => setMany({ aiModel: e.target.value })} />
        )}
        <button disabled={loading} onClick={() => void loadModels()}>
          {loading ? t("Working…") : t("Load Models")}
        </button>
      </div>
      {preferences.aiProtocol === "openai" && (
        <>
          <label className="setting">
            <span>{t("Embedding model")}</span>
            <input
              value={preferences.aiEmbeddingModel}
              spellCheck={false}
              placeholder="text-embedding-3-small"
              onChange={(e) => setMany({ aiEmbeddingModel: e.target.value })}
            />
          </label>
          {settings.embeddingModel && (
            <div className="setting">
              <span>{t("Semantic search")}</span>
              <span className="hint">
                {semantic ? t("Indexed {indexed} of {total}", { indexed: semantic.indexed, total: semantic.total }) : ""}
              </span>
              <button disabled={indexing} onClick={() => void buildIndex()}>
                {indexing ? t("Working…") : t("Build Semantic Index")}
              </button>
            </div>
          )}
          <p className="hint">
            {t("With an embedding model, Ask the Library also finds passages by meaning. Building the index sends the text of your papers and notes to the provider.")}
          </p>
        </>
      )}
      <p className="hint">{t("The key stays on this device. Papers are sent to the provider only when you ask.")}</p>
      {error && <p className="hint error">{error}</p>}
    </section>
  );
}

/** Translators are downloaded only when the person asks for them. */
function TranslatorsCard() {
  const { t } = useI18n();
  const [status, setStatus] = useState<TranslatorStatus>({ count: 0, downloaded: null });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  useEffect(() => { void backend.translatorsStatus().then(setStatus, (error: unknown) => setError(error instanceof Error ? error.message : String(error))); }, []);
  const run = async (action: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    try {
      await action();
      setStatus(await backend.translatorsStatus());
      window.dispatchEvent(new Event(TRANSLATORS_CHANGED));
    } catch (error) { setError(error instanceof Error ? error.message : String(error)); }
    finally { setBusy(false); }
  };
  return (
    <section className="card">
      <h3>{t("Zotero Translators")}</h3>
      <div className="setting">
        <span className="setting-description">
          {status.count ? t("Translators installed: {count}", { count: status.count }) : t("No translators installed")}
          {status.downloaded && <small className="hint plugin-description">{t("Downloaded {date}", { date: status.downloaded })}</small>}
        </span>
        <div className="setting-actions">
          <button disabled={busy} onClick={() => void run(() => backend.downloadTranslators())}>{busy ? t("Working…") : status.count ? t("Update") : t("Download")}</button>
          <button disabled={busy || !status.count} onClick={() => void run(() => backend.removeTranslators())}>{t("Remove")}</button>
        </div>
      </div>
      <p className="hint">{t("These are Zotero's translators, downloaded from GitHub on request and kept apart from the app.")}</p>
      {error && <p className="hint error">{error}</p>}
    </section>
  );
}

/** The open-source software in the app and its licences, written into the
 * web UI at build time (scripts/licenses.py). */
function NoticesDialog({ onClose }: { onClose: () => void }) {
  const { t } = useI18n();
  const [text, setText] = useState<string | null>(null);

  useEffect(() => {
    let current = true;
    fetch(`${import.meta.env.BASE_URL}THIRD_PARTY_NOTICES.txt`)
      .then((response) => (response.ok ? response.text() : ""))
      .catch(() => "")
      .then((body) => {
        if (current) setText(body.trim());
      });
    return () => {
      current = false;
    };
  }, []);

  return (
    <div className="dialog-backdrop" role="presentation" onClick={onClose}>
      <div
        className="dialog wide notices"
        role="dialog"
        aria-modal="true"
        aria-labelledby="notices-title"
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.key === "Escape" && onClose()}
      >
        <h2 id="notices-title">{t("Third-Party Notices")}</h2>
        <pre className="notices-text">{text === null ? "…" : text || t("This build has no list of third-party notices.")}</pre>
        <div className="row end">
          <button type="button" className="primary" onClick={onClose}>
            {t("Done")}
          </button>
        </div>
      </div>
    </div>
  );
}
