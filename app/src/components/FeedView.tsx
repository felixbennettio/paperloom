import { useCallback, useEffect, useState } from "react";
import { useI18n } from "../i18n";
import { backend, type FeedInfo, type FeedItem } from "../lib/api";
import { Icon } from "./Icon";

interface Props {
  feed: FeedInfo;
  /** New items go here too, if a collection is picked. */
  collectionId: string | null;
  /** Opens an item of the library. */
  onOpenItem: (id: string, title: string) => void;
  /** After entries were read, added or the feed read again. */
  onChanged: () => void;
}

function message(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** What a feed announced, as Zotero shows a feed: newest first, unread in
 * bold; an entry opens to its abstract, and goes into the library with one
 * click (its full record looked up by DOI). */
export function FeedView({ feed, collectionId, onOpenItem, onChanged }: Props) {
  const { t } = useI18n();
  const [items, setItems] = useState<FeedItem[]>([]);
  const [open, setOpen] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    backend.feedItems(feed.id).then(setItems, (e) => setError(message(e)));
  }, [feed.id]);
  useEffect(load, [load]);

  const act = async (key: string, action: () => Promise<unknown>) => {
    setBusy(key);
    setError(null);
    try {
      await action();
      load();
      onChanged();
    } catch (e) {
      setError(message(e));
    } finally {
      setBusy(null);
    }
  };

  const toggle = (item: FeedItem) => {
    setOpen(open === item.id ? null : item.id);
    if (!item.read) void act(item.id, () => backend.setFeedItemRead(item.id, true));
  };

  return (
    <div className="library-view feed-view">
      <header className="view-header">
        <h2>{feed.title}</h2>
        <span className="spacer" />
        <button disabled={busy !== null} onClick={() => void act("refresh", () => backend.refreshFeeds())}>
          {busy === "refresh" ? t("Working…") : t("Refresh")}
        </button>
        <button disabled={feed.unread === 0} onClick={() => void act("read", () => backend.markFeedRead(feed.id))}>
          {t("Mark All as Read")}
        </button>
      </header>
      <p className="hint feed-address">{feed.url}</p>
      {feed.problem && <p className="hint error">{feed.problem}</p>}
      {error && <p className="hint error">{error}</p>}
      {items.length === 0 && (
        <div className="empty">
          <Icon name="globe" size={40} />
          <h3>{t("Nothing in this feed yet")}</h3>
        </div>
      )}
      <ul className="feed-items">
        {items.map((item) => (
          <li key={item.id} className={item.read ? "feed-item" : "feed-item unread"}>
            <button className="feed-title link" aria-expanded={open === item.id} onClick={() => toggle(item)}>
              {!item.read && <span className="unread-dot" aria-label={t("Unread")} />}
              {item.title}
            </button>
            <span className="muted">
              {[item.authors, item.containerTitle, item.date ?? item.year].filter(Boolean).join(" · ")}
            </span>
            {open === item.id && item.abstractNote && <p className="feed-abstract">{item.abstractNote}</p>}
            <div className="row">
              {item.libraryItemId ? (
                <button onClick={() => onOpenItem(item.libraryItemId!, item.title)}>{t("Show in Library")}</button>
              ) : (
                <button
                  className="primary"
                  disabled={busy !== null}
                  onClick={() => void act(item.id, () => backend.addFeedItem(item.id, collectionId))}
                >
                  {busy === item.id ? t("Working…") : t("Add to Library")}
                </button>
              )}
              {item.link && <button onClick={() => void backend.openUrl(item.link!).catch((e) => setError(message(e)))}>{t("Open Web Page")}</button>}
              {item.read && (
                <button className="link" onClick={() => void act(item.id, () => backend.setFeedItemRead(item.id, false))}>
                  {t("Mark as Unread")}
                </button>
              )}
            </div>
          </li>
        ))}
      </ul>
    </div>
  );
}
