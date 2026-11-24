import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { useI18n } from "../i18n";
import { pluginHost, type PluginCommand, type PluginRibbon } from "../lib/plugins";

/** What the plugins have added to the app, kept up to date. */
export function usePlugins() {
  return useSyncExternalStore(pluginHost.subscribe, pluginHost.snapshot);
}

interface Props {
  /** Opens a note a plugin names. */
  onOpenLink: (link: string) => void;
}

type Entry = { kind: "command"; command: PluginCommand } | { kind: "ribbon"; ribbon: PluginRibbon };

/** The plugins' frames, a plugin's settings or modal over the app, their
 * notices, and the command palette (⌘P / Ctrl+P) with their commands. */
export function PluginLayer({ onOpenLink }: Props) {
  const { t } = useI18n();
  const state = usePlugins();
  const layer = useRef<HTMLDivElement>(null);
  const [palette, setPalette] = useState(false);
  const [query, setQuery] = useState("");
  const [index, setIndex] = useState(0);

  useEffect(() => {
    pluginHost.setLayer(layer.current);
    void pluginHost.reload();
    return () => pluginHost.setLayer(null);
  }, []);

  useEffect(() => {
    pluginHost.openNote = onOpenLink;
    pluginHost.allowSite = (plugin, site) => window.confirm(t("{plugin} wants to reach {site}. Allow it?", { plugin, site }));
  });

  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && !event.shiftKey && event.key.toLowerCase() === "p") {
        event.preventDefault();
        setQuery("");
        setIndex(0);
        setPalette(true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  const entries: Entry[] = [
    ...state.commands.map((command): Entry => ({ kind: "command", command })),
    ...state.ribbon.map((ribbon): Entry => ({ kind: "ribbon", ribbon })),
  ].filter((entry) => {
    const name = (entry.kind === "command" ? entry.command.name : entry.ribbon.title).toLowerCase();
    return words.every((word) => name.includes(word));
  });

  const choose = (i: number) => {
    const entry = entries[i];
    setPalette(false);
    if (entry?.kind === "command") pluginHost.run(entry.command);
    else if (entry?.kind === "ribbon") pluginHost.clickRibbon(entry.ribbon);
  };

  return (
    <>
      <div ref={layer} className="plugin-layer" style={{ zIndex: 1001 }} />
      {state.showing && (
        <div className="plugin-backdrop" role="presentation" onClick={() => pluginHost.closeShown()}>
          <button className="plugin-close" aria-label={t("Close")} title={t("Close")} onClick={() => pluginHost.closeShown()}>
            ×
          </button>
        </div>
      )}
      {state.notices.length > 0 && (
        <div className="plugin-notices" role="status">
          {state.notices.map((notice) => (
            <button key={notice.id} className="plugin-notice" onClick={() => pluginHost.dismissNotice(notice.id)}>
              {notice.text}
            </button>
          ))}
        </div>
      )}
      {palette && (
        <div className="dialog-backdrop" role="presentation" onClick={() => setPalette(false)}>
          <div className="dialog switcher" role="dialog" aria-modal="true" aria-label={t("Commands")} onClick={(e) => e.stopPropagation()}>
            <input
              autoFocus
              value={query}
              placeholder={t("Run a command…")}
              onChange={(e) => {
                setQuery(e.target.value);
                setIndex(0);
              }}
              onKeyDown={(e) => {
                if (e.key === "Escape") setPalette(false);
                else if (e.key === "ArrowDown") setIndex((i) => Math.min(i + 1, entries.length - 1));
                else if (e.key === "ArrowUp") setIndex((i) => Math.max(i - 1, 0));
                else if (e.key === "Enter") choose(index);
              }}
            />
            <ul>
              {entries.map((entry, i) => (
                <li key={entry.kind === "command" ? entry.command.id : `${entry.ribbon.plugin}:${entry.ribbon.index}`}>
                  <button className={i === index ? "selected" : ""} onMouseEnter={() => setIndex(i)} onClick={() => choose(i)}>
                    {entry.kind === "command" ? entry.command.name : entry.ribbon.title}
                  </button>
                </li>
              ))}
              {entries.length === 0 && (
                <li className="muted">{state.running.length ? t("No matching commands.") : t("Turn on Obsidian plugins in Settings to add commands.")}</li>
              )}
            </ul>
          </div>
        </div>
      )}
    </>
  );
}
