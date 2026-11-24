import { useCallback, useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n";
import { backend, type DiffLine, type NoteVersion } from "../lib/api";

interface Props {
  path: string;
  /** Saves typing in the note first, so going back keeps it as a version. */
  beforeRestore: () => Promise<void>;
  /** The note changed on disk; show it again. */
  onRestored: () => void;
  onError: (message: string) => void;
}

/** Unchanged lines shown around each change. */
const CONTEXT = 2;

/** The lines to show: changes, with a little of what is around them. */
function around(lines: DiffLine[]): (DiffLine | { skipped: number })[] {
  const near = lines.map((_, i) =>
    lines.slice(Math.max(0, i - CONTEXT), i + CONTEXT + 1).some((line) => line.change !== "same"),
  );
  const out: (DiffLine | { skipped: number })[] = [];
  lines.forEach((line, i) => {
    if (near[i]) out.push(line);
    else {
      const last = out[out.length - 1];
      if (last && "skipped" in last) last.skipped++;
      else out.push({ skipped: 1 });
    }
  });
  return out;
}

/** A note's earlier versions: what changed since each, and going back. */
export function NoteHistory({ path, beforeRestore, onRestored, onError }: Props) {
  const { t } = useI18n();
  const [versions, setVersions] = useState<NoteVersion[]>([]);
  const [chosen, setChosen] = useState<number | null>(null);
  const [lines, setLines] = useState<DiffLine[]>([]);
  // The latest handler, without making each render a new note.
  const report = useRef(onError);
  useEffect(() => {
    report.current = onError;
  });

  const reload = useCallback(() => {
    backend.vaultVersions(path).then(setVersions, (e) => report.current(String(e)));
  }, [path]);
  useEffect(() => {
    setChosen(null);
    setLines([]);
    reload();
  }, [reload]);

  const choose = async (time: number) => {
    setChosen(time);
    try {
      setLines(await backend.vaultCompareVersion(path, time));
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    }
  };

  const restore = async (time: number) => {
    const when = new Date(time).toLocaleString();
    if (!window.confirm(t("Put back the version from {time}? What the note says now is kept as a version too.", { time: when }))) return;
    try {
      await beforeRestore();
      await backend.vaultRestoreVersion(path, time);
      setChosen(null);
      setLines([]);
      reload();
      onRestored();
    } catch (e) {
      onError(e instanceof Error ? e.message : String(e));
    }
  };

  if (versions.length === 0) {
    return <p className="muted">{t("No earlier versions yet. Before a note changes, it is kept as it was, at most every five minutes.")}</p>;
  }
  return (
    <div className="note-history">
      <ul className="versions">
        {versions.map((v) => (
          <li key={v.time}>
            <button className={chosen === v.time ? "nav-item selected" : "nav-item"} onClick={() => void choose(v.time)}>
              <span>{new Date(v.time).toLocaleString()}</span>
              <span className="muted">{t("Characters: {count}", { count: v.size })}</span>
            </button>
          </li>
        ))}
      </ul>
      {chosen !== null && (
        <>
          <p className="muted">{t("What changed since then")}</p>
          <pre className="diff">
            {around(lines).map((line, i) =>
              "skipped" in line ? (
                <span key={i} className="skipped">
                  {t("… unchanged lines: {count} …", { count: line.skipped })}
                  {"\n"}
                </span>
              ) : (
                <span key={i} className={line.change}>
                  {line.change === "added" ? "+ " : line.change === "removed" ? "- " : "  "}
                  {line.text}
                  {"\n"}
                </span>
              ),
            )}
          </pre>
          <button className="primary" onClick={() => void restore(chosen)}>
            {t("Restore This Version")}
          </button>
        </>
      )}
    </div>
  );
}
