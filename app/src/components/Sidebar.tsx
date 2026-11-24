import { useState, type ReactElement } from "react";
import { useI18n } from "../i18n";
import type { CollectionInfo, FeedInfo, SavedSearch, TagInfo } from "../lib/api";
import { MARK_COLORS } from "../lib/pdf";
import { sameScope, type Scope } from "../lib/scope";
import { Icon, type IconName } from "./Icon";

interface Props {
  collections: CollectionInfo[];
  searches: SavedSearch[];
  feeds: FeedInfo[];
  tags: TagInfo[];
  scope: Scope;
  onScope: (scope: Scope) => void;
  onCreate: (name: string, parentId: string | null) => void;
  onTagColor: (name: string, color: string | null) => void;
  onRenameTag: (name: string, newName: string) => void;
  onDeleteTag: (name: string) => void;
  /** Opens the editor for a new saved search, or for this one. */
  onEditSearch: (search: SavedSearch | null) => void;
  /** Follows a feed; why not, or null. */
  onAddFeed: (url: string) => Promise<string | null>;
  onRenameFeed: (id: string, title: string) => void;
  onRemoveFeed: (id: string) => void;
}

/** The library's views, the collection tree, and tags. */
export function Sidebar({
  collections,
  searches,
  feeds,
  tags,
  scope,
  onScope,
  onCreate,
  onTagColor,
  onRenameTag,
  onDeleteTag,
  onEditSearch,
  onAddFeed,
  onRenameFeed,
  onRemoveFeed,
}: Props) {
  const { t } = useI18n();
  const [naming, setNaming] = useState(false);
  const [name, setName] = useState("");
  const [editingTag, setEditingTag] = useState<string | null>(null);
  const [tagName, setTagName] = useState("");
  const [addingFeed, setAddingFeed] = useState(false);
  const [feedUrl, setFeedUrl] = useState("");
  const [feedProblem, setFeedProblem] = useState<string | null>(null);
  const [feedBusy, setFeedBusy] = useState(false);
  const [editingFeed, setEditingFeed] = useState<string | null>(null);
  const [feedName, setFeedName] = useState("");

  const submitFeed = async () => {
    setFeedBusy(true);
    setFeedProblem(null);
    const problem = await onAddFeed(feedUrl);
    setFeedBusy(false);
    if (problem) {
      setFeedProblem(problem);
    } else {
      setAddingFeed(false);
      setFeedUrl("");
    }
  };
  const selected = scope.kind === "collection" ? scope.id : null;
  const parent = collections.find((c) => c.id === selected) ?? null;
  const views: { scope: Scope; icon: IconName; label: string }[] = [
    { scope: { kind: "all" }, icon: "library", label: t("All Items") },
    { scope: { kind: "starred" }, icon: "star", label: t("Starred") },
    { scope: { kind: "unread" }, icon: "doc", label: t("Unread") },
    { scope: { kind: "duplicates" }, icon: "duplicate", label: t("Duplicate Items") },
    { scope: { kind: "trash" }, icon: "trash", label: t("Trash") },
  ];

  const submit = () => {
    if (name.trim()) onCreate(name, parent?.id ?? null);
    setNaming(false);
    setName("");
  };

  const tree = (parentId: string | null, depth: number): ReactElement[] =>
    collections
      .filter((c) => c.parentId === parentId)
      .sort((a, b) => a.position - b.position)
      .flatMap((c) => [
        <li key={c.id}>
          <button
            className={c.id === selected ? "nav-item selected" : "nav-item"}
            style={{ paddingInlineStart: 12 + depth * 16 }}
            onClick={() => onScope({ kind: "collection", id: c.id })}
          >
            <Icon name="folder" size={16} />
            <span>{c.name}</span>
          </button>
        </li>,
        ...tree(c.id, depth + 1),
      ]);

  return (
    <nav className="sidebar-content">
      <p className="brand" aria-hidden="true">
        {t("Paperloom")}
      </p>
      <h2 className="section-title">{t("Library")}</h2>
      <ul>
        {views.map((view) => (
          <li key={view.scope.kind}>
            <button className={sameScope(view.scope, scope) ? "nav-item selected" : "nav-item"} onClick={() => onScope(view.scope)}>
              <Icon name={view.icon} size={16} />
              <span>{view.label}</span>
            </button>
          </li>
        ))}
      </ul>
      <div className="section-header">
        <h2 className="section-title">{t("Collections")}</h2>
        <button className="icon-button" title={t("New Collection")} aria-label={t("New Collection")} onClick={() => setNaming(true)}>
          <Icon name="plus" size={15} />
        </button>
      </div>
      {naming && (
        <form
          className="inline-form"
          onSubmit={(e) => {
            e.preventDefault();
            submit();
          }}
        >
          <input
            autoFocus
            value={name}
            placeholder={t("Name")}
            aria-label={t("Name")}
            onChange={(e) => setName(e.target.value)}
            onKeyDown={(e) => e.key === "Escape" && setNaming(false)}
          />
          <small>{parent ? t("Inside “{name}”.", { name: parent.name }) : t("At the top level.")}</small>
          <div className="row">
            <button type="button" onClick={() => setNaming(false)}>
              {t("Cancel")}
            </button>
            <button type="submit" className="primary">
              {t("Create")}
            </button>
          </div>
        </form>
      )}
      <ul>{tree(null, 0)}</ul>
      <div className="section-header">
        <h2 className="section-title">{t("Saved Searches")}</h2>
        <button className="icon-button" title={t("New Saved Search")} aria-label={t("New Saved Search")} onClick={() => onEditSearch(null)}>
          <Icon name="plus" size={15} />
        </button>
      </div>
      <ul>
        {searches.map((search) => (
          <li key={search.id}>
            <button
              className={scope.kind === "search" && scope.id === search.id ? "nav-item selected" : "nav-item"}
              onClick={() => onScope({ kind: "search", id: search.id })}
              onContextMenu={(e) => {
                e.preventDefault();
                onEditSearch(search);
              }}
              onDoubleClick={() => onEditSearch(search)}
              title={t("Double-click to edit")}
            >
              <Icon name="search" size={16} />
              <span>{search.name}</span>
            </button>
          </li>
        ))}
      </ul>
      <div className="section-header">
        <h2 className="section-title">{t("Feeds")}</h2>
        <button className="icon-button" title={t("Add Feed")} aria-label={t("Add Feed")} onClick={() => setAddingFeed(true)}>
          <Icon name="plus" size={15} />
        </button>
      </div>
      {addingFeed && (
        <form
          className="inline-form"
          onSubmit={(e) => {
            e.preventDefault();
            void submitFeed();
          }}
        >
          <input
            autoFocus
            value={feedUrl}
            placeholder="https://"
            aria-label={t("A journal's RSS or Atom address, or its web page")}
            onChange={(e) => setFeedUrl(e.target.value)}
            onKeyDown={(e) => e.key === "Escape" && setAddingFeed(false)}
          />
          <small>{feedProblem ?? t("A journal's RSS or Atom address, or its web page")}</small>
          <div className="row">
            <button type="button" onClick={() => setAddingFeed(false)}>
              {t("Cancel")}
            </button>
            <button type="submit" className="primary" disabled={feedBusy || !feedUrl.trim()}>
              {feedBusy ? t("Working…") : t("Add")}
            </button>
          </div>
        </form>
      )}
      <ul>
        {feeds.map((feed) => (
          <li key={feed.id}>
            {editingFeed === feed.id ? (
              <form
                className="inline-form"
                onSubmit={(e) => {
                  e.preventDefault();
                  if (feedName.trim()) onRenameFeed(feed.id, feedName);
                  setEditingFeed(null);
                }}
              >
                <input autoFocus value={feedName} aria-label={t("Name")} onChange={(e) => setFeedName(e.target.value)} />
                <small>{feed.url}</small>
                <div className="row">
                  <button
                    type="button"
                    className="danger"
                    onClick={() => {
                      setEditingFeed(null);
                      onRemoveFeed(feed.id);
                    }}
                  >
                    {t("Unsubscribe")}
                  </button>
                  <span className="spacer" />
                  <button type="button" onClick={() => setEditingFeed(null)}>
                    {t("Cancel")}
                  </button>
                  <button type="submit" className="primary">
                    {t("Done")}
                  </button>
                </div>
              </form>
            ) : (
              <button
                className={scope.kind === "feed" && scope.id === feed.id ? "nav-item selected" : "nav-item"}
                onClick={() => onScope({ kind: "feed", id: feed.id })}
                onContextMenu={(e) => {
                  e.preventDefault();
                  setFeedName(feed.title);
                  setEditingFeed(feed.id);
                }}
                onDoubleClick={() => {
                  setFeedName(feed.title);
                  setEditingFeed(feed.id);
                }}
                title={feed.problem ?? t("Double-click to edit")}
              >
                <Icon name="globe" size={16} />
                <span>{feed.title}</span>
                {feed.unread > 0 && <span className="count">{feed.unread}</span>}
              </button>
            )}
          </li>
        ))}
      </ul>
      {tags.length > 0 && <h2 className="section-title">{t("Tags")}</h2>}
      <ul>
        {tags.map((tag) => (
          <li key={tag.name}>
            {editingTag === tag.name ? (
              <form
                className="inline-form"
                onSubmit={(e) => {
                  e.preventDefault();
                  if (tagName.trim()) onRenameTag(tag.name, tagName);
                  setEditingTag(null);
                }}
              >
                <input autoFocus value={tagName} aria-label={t("Name")} onChange={(e) => setTagName(e.target.value)} />
                <div className="swatches" role="radiogroup" aria-label={t("Color")}>
                  <button
                    type="button"
                    className={tag.color === null ? "swatch plain selected" : "swatch plain"}
                    aria-label={t("None")}
                    title={t("None")}
                    onClick={() => onTagColor(tag.name, null)}
                  />
                  {MARK_COLORS.map((c) => (
                    <button
                      type="button"
                      key={c.hex}
                      className={tag.color === c.hex ? "swatch selected" : "swatch"}
                      style={{ background: c.hex }}
                      aria-label={t(c.name)}
                      title={t(c.name)}
                      onClick={() => onTagColor(tag.name, c.hex)}
                    />
                  ))}
                </div>
                <small>{t("Renaming onto an existing tag merges the two.")}</small>
                <div className="row">
                  <button type="button" className="danger" onClick={() => onDeleteTag(tag.name)}>
                    {t("Delete Tag")}
                  </button>
                  <span className="spacer" />
                  <button type="button" onClick={() => setEditingTag(null)}>
                    {t("Cancel")}
                  </button>
                  <button type="submit" className="primary">
                    {t("Done")}
                  </button>
                </div>
              </form>
            ) : (
              <button
                className={scope.kind === "tag" && scope.name === tag.name ? "nav-item selected" : "nav-item"}
                onClick={() => onScope({ kind: "tag", name: tag.name })}
                onContextMenu={(e) => {
                  e.preventDefault();
                  setTagName(tag.name);
                  setEditingTag(tag.name);
                }}
                onDoubleClick={() => {
                  setTagName(tag.name);
                  setEditingTag(tag.name);
                }}
                title={t("Double-click to edit")}
              >
                <span className="tag-dot" style={{ background: tag.color ?? "transparent" }} />
                <span>{tag.name}</span>
                <span className="count">{tag.count}</span>
              </button>
            )}
          </li>
        ))}
      </ul>
    </nav>
  );
}
