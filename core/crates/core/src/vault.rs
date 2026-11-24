//! The Markdown vault, for both apps: notes as plain files, their outline,
//! links and backlinks, the quick switcher, search and the reading view;
//! properties, templates and daily notes; earlier versions; the graph.

use std::sync::{Arc, Mutex, MutexGuard};

use paperloom_vault::{self as vault, Note, Vault};
use serde::{Deserialize, Serialize};

use crate::{CoreError, PaperLibrary};

impl From<vault::Error> for CoreError {
    fn from(err: vault::Error) -> Self {
        let message = err.to_string();
        match err {
            vault::Error::NotFound(_) => CoreError::NotFound { message },
            vault::Error::InvalidPath(_) | vault::Error::Exists(_) | vault::Error::Invalid(_) => {
                CoreError::InvalidInput { message }
            }
            vault::Error::Io { .. } => CoreError::Storage { message },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct NoteInfo {
    /// `folder/Note.md`, relative to the vault.
    pub path: String,
    pub title: String,
    pub folder: String,
    /// Milliseconds since 1970.
    pub modified: i64,
    pub size: u64,
}

impl From<&Note> for NoteInfo {
    fn from(note: &Note) -> Self {
        Self {
            path: note.path.clone(),
            title: note.title.clone(),
            folder: note.folder().to_string(),
            modified: note.modified,
            size: note.size,
        }
    }
}

/// A line of a note that links to, mentions or matches something.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct NoteMention {
    pub path: String,
    pub title: String,
    /// From 0.
    pub line: u32,
    pub context: String,
}

/// A note or library item a note is about too but does not link to yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct LinkSuggestion {
    pub title: String,
    /// What goes in the note: `[[Note]]`, or `[[@10.1038/…|Kucsko 2013]]`.
    pub insert: String,
    /// The note's path, for a note.
    pub path: Option<String>,
    /// The item's ID, for an item.
    pub item_id: Option<String>,
    /// The passage that matched, as HTML with the words in `<b>`.
    pub snippet_html: String,
}

impl From<vault::Mention> for NoteMention {
    fn from(m: vault::Mention) -> Self {
        Self { path: m.path, title: m.title, line: m.line as u32, context: m.context }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct NoteHeading {
    pub level: u8,
    pub text: String,
    pub line: u32,
    /// The `id` the reading view gives it.
    pub anchor: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct NoteLinkInfo {
    pub target: String,
    pub subpath: Option<String>,
    pub alias: Option<String>,
    pub embed: bool,
    pub line: u32,
    /// The note or file it leads to, if it exists.
    pub resolved: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct NoteProperty {
    pub key: String,
    pub value: String,
}

/// Everything the side panels show about a note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct NoteOutline {
    pub headings: Vec<NoteHeading>,
    pub tags: Vec<String>,
    pub aliases: Vec<String>,
    pub properties: Vec<NoteProperty>,
    pub links: Vec<NoteLinkInfo>,
    pub backlinks: Vec<NoteMention>,
    pub unlinked_mentions: Vec<NoteMention>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct VaultTag {
    pub name: String,
    pub count: u32,
}

/// What `[[` autocompletion offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct LinkCompletion {
    /// What to put between `[[` and `]]`.
    pub insert: String,
    pub title: String,
    pub path: String,
}

/// A local date and time from the app, for templates and daily notes (the
/// core does not know the time zone).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct LocalTime {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl From<LocalTime> for vault::Moment {
    fn from(t: LocalTime) -> Self {
        Self { year: t.year, month: t.month, day: t.day, hour: t.hour, minute: t.minute, second: t.second }
    }
}

/// How the vault makes daily notes and fills templates; kept where
/// Obsidian keeps them, so both apps share them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct VaultSettings {
    /// Empty for the vault's top folder.
    pub daily_folder: String,
    /// A Moment.js format: `YYYY-MM-DD`, `YYYY/MM/YYYY-MM-DD`…
    pub daily_format: String,
    /// The note a daily note starts from; empty for none.
    pub daily_template: String,
    pub templates_folder: String,
    pub date_format: String,
    pub time_format: String,
    /// The style `[@citations]` in notes show in, one of `citation_styles()`.
    pub citation_style: String,
}

impl From<vault::Settings> for VaultSettings {
    fn from(s: vault::Settings) -> Self {
        Self {
            daily_folder: s.daily_folder,
            daily_format: s.daily_format,
            daily_template: s.daily_template,
            templates_folder: s.templates_folder,
            date_format: s.date_format,
            time_format: s.time_format,
            citation_style: s.citation_style,
        }
    }
}

impl From<VaultSettings> for vault::Settings {
    fn from(s: VaultSettings) -> Self {
        Self {
            daily_folder: s.daily_folder,
            daily_format: s.daily_format,
            daily_template: s.daily_template,
            templates_folder: s.templates_folder,
            date_format: s.date_format,
            time_format: s.time_format,
            citation_style: s.citation_style,
        }
    }
}

/// An earlier version of a note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct NoteVersion {
    /// When it was kept, in milliseconds since 1970: the note read like
    /// this until then.
    pub time: i64,
    pub size: u64,
}

/// A line in a comparison: `same`, `added` or `removed`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct DiffLine {
    pub change: String,
    pub text: String,
}

/// A note (`note`), a link to a note not written yet (`missing`), or a
/// paper in the library (`item`), placed for drawing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct GraphNodeInfo {
    /// The note's path, or the link as written.
    pub id: String,
    pub title: String,
    pub kind: String,
    pub item_id: Option<String>,
    pub degree: u32,
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdge {
    pub from: u32,
    pub to: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct NoteGraph {
    pub nodes: Vec<GraphNodeInfo>,
    pub edges: Vec<GraphEdge>,
}

/// A plugin's note HTML, at most 1 MiB.
fn check_note_html(html: &str) -> Result<(), CoreError> {
    if html.len() > 1 << 20 {
        return Err(CoreError::InvalidInput { message: "A note may hold at most 1 MiB of HTML.".into() });
    }
    Ok(())
}

/// A note title from its first line of text, safe as a file name.
fn note_title(markdown: &str) -> String {
    let line =
        markdown.lines().map(|l| l.trim_start_matches('#').trim()).find(|l| !l.is_empty()).unwrap_or("");
    let title: String = line
        .chars()
        .filter(|c| {
            !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '[' | ']' | '#' | '^')
        })
        .take(80)
        .collect();
    if title.trim().is_empty() { "Note".into() } else { title.trim().to_string() }
}

/// A vault folder. Safe to share between threads.
#[derive(uniffi::Object)]
pub struct PaperVault {
    inner: Mutex<Vault>,
    /// The library whose full-text index holds the notes, once connected.
    library: Mutex<Option<Arc<PaperLibrary>>>,
}

impl PaperVault {
    pub(crate) fn lock(&self) -> Result<MutexGuard<'_, Vault>, CoreError> {
        self.inner
            .lock()
            .map_err(|_| CoreError::Storage { message: "The vault is unavailable after a crash.".into() })
    }

    pub(crate) fn library(&self) -> Option<Arc<PaperLibrary>> {
        self.library.lock().ok().and_then(|l| l.clone())
    }

    /// Indexes some notes (by path), or all of them.
    fn index(&self, paths: Option<&[String]>) {
        let Some(library) = self.library() else { return };
        let notes: Vec<(String, String, String)> = match self.lock() {
            Ok(vault) => vault
                .notes()
                .filter(|n| paths.is_none_or(|p| p.contains(&n.path)))
                .map(|n| (n.path.clone(), n.title.clone(), n.text.clone()))
                .collect(),
            Err(_) => return,
        };
        library.index_notes(&notes, paths.is_none());
    }
}

#[uniffi::export]
impl PaperVault {
    /// Opens a vault folder (an existing Obsidian vault works), creating it
    /// if needed.
    #[uniffi::constructor]
    pub fn open(path: String) -> Result<Arc<Self>, CoreError> {
        Ok(Arc::new(Self { inner: Mutex::new(Vault::open(path)?), library: Mutex::new(None) }))
    }

    /// Makes the notes findable through the library's
    /// `search_everything`, and keeps them up to date there.
    pub fn connect_library(&self, library: Arc<PaperLibrary>) {
        if let Ok(mut slot) = self.library.lock() {
            *slot = Some(library);
        }
        self.index(None);
    }

    pub fn root(&self) -> Result<String, CoreError> {
        Ok(self.lock()?.root().display().to_string())
    }

    /// Picks up edits made by other apps; true if anything changed.
    pub fn refresh(&self) -> Result<bool, CoreError> {
        let changed = self.lock()?.refresh()?;
        if changed {
            self.index(None);
        }
        Ok(changed)
    }

    /// Every note, by path.
    pub fn notes(&self) -> Result<Vec<NoteInfo>, CoreError> {
        Ok(self.lock()?.notes().map(NoteInfo::from).collect())
    }

    pub fn folders(&self) -> Result<Vec<String>, CoreError> {
        Ok(self.lock()?.folders().cloned().collect())
    }

    pub fn read_note(&self, path: String) -> Result<String, CoreError> {
        let vault = self.lock()?;
        let note =
            vault.note(&path).ok_or_else(|| CoreError::NotFound { message: format!("{path} not found") })?;
        Ok(note.text.clone())
    }

    /// Saves a note, creating it (and its folders) if needed.
    pub fn write_note(&self, path: String, text: String) -> Result<NoteInfo, CoreError> {
        let info = {
            let mut vault = self.lock()?;
            vault.write(&path, &text)?;
            let saved = if path.to_lowercase().ends_with(".md") { path } else { format!("{path}.md") };
            let note = vault
                .note(saved.trim_matches('/'))
                .ok_or_else(|| CoreError::NotFound { message: saved.clone() })?;
            NoteInfo::from(note)
        };
        self.index(Some(std::slice::from_ref(&info.path)));
        Ok(info)
    }

    /// Creates an empty note; the title gets ` 2`, ` 3`… if taken.
    pub fn create_note(&self, folder: String, title: String) -> Result<NoteInfo, CoreError> {
        let info = {
            let mut vault = self.lock()?;
            let path = vault.create_note(&folder, &title)?;
            NoteInfo::from(vault.note(&path).expect("just created"))
        };
        self.index(Some(std::slice::from_ref(&info.path)));
        Ok(info)
    }

    pub fn create_folder(&self, path: String) -> Result<(), CoreError> {
        Ok(self.lock()?.create_folder(&path)?)
    }

    /// Renames or moves a note and updates links to it; returns how many
    /// other notes changed.
    pub fn rename_note(&self, path: String, new_path: String) -> Result<u32, CoreError> {
        let changed = self.lock()?.rename(&path, &new_path)?;
        if let Some(library) = self.library() {
            library.unindex_keys(std::slice::from_ref(&path));
        }
        // The renamed note and every note whose links changed.
        self.index(None);
        Ok(changed as u32)
    }

    /// Moves a note to the vault's `.trash` folder.
    pub fn delete_note(&self, path: String) -> Result<(), CoreError> {
        self.lock()?.delete(&path)?;
        if let Some(library) = self.library() {
            library.unindex_keys(std::slice::from_ref(&path));
        }
        Ok(())
    }

    pub fn outline(&self, path: String) -> Result<NoteOutline, CoreError> {
        let vault = self.lock()?;
        let note =
            vault.note(&path).ok_or_else(|| CoreError::NotFound { message: format!("{path} not found") })?;
        let parsed = &note.parsed;
        Ok(NoteOutline {
            headings: parsed
                .headings
                .iter()
                .map(|h| NoteHeading {
                    level: h.level,
                    text: h.text.clone(),
                    line: h.line as u32,
                    anchor: vault::heading_id(&h.text),
                })
                .collect(),
            tags: parsed.tags.clone(),
            aliases: parsed.aliases.clone(),
            properties: parsed
                .properties
                .iter()
                .map(|(key, value)| NoteProperty { key: key.clone(), value: value.clone() })
                .collect(),
            links: parsed
                .links
                .iter()
                .map(|l| NoteLinkInfo {
                    target: l.target.clone(),
                    subpath: l.subpath.clone(),
                    alias: l.alias.clone(),
                    embed: l.embed,
                    line: l.line as u32,
                    resolved: vault.resolve(&l.target, &path),
                })
                .collect(),
            backlinks: vault.backlinks(&path).into_iter().map(NoteMention::from).collect(),
            unlinked_mentions: vault.unlinked_mentions(&path).into_iter().map(NoteMention::from).collect(),
        })
    }

    /// Notes for the quick switcher, best match first.
    pub fn switcher(&self, query: String, limit: u32) -> Result<Vec<NoteInfo>, CoreError> {
        Ok(self.lock()?.switcher(&query, limit as usize).into_iter().map(NoteInfo::from).collect())
    }

    /// What `[[query` could complete to, written so it resolves from `from`.
    pub fn link_completions(
        &self,
        from: String,
        query: String,
        limit: u32,
    ) -> Result<Vec<LinkCompletion>, CoreError> {
        let vault = self.lock()?;
        Ok(vault
            .switcher(&query, limit as usize)
            .into_iter()
            .map(|note| LinkCompletion {
                insert: vault.link_target_for(&note.path, &from),
                title: note.title.clone(),
                path: note.path.clone(),
            })
            .collect())
    }

    pub fn tags(&self) -> Result<Vec<VaultTag>, CoreError> {
        Ok(self
            .lock()?
            .tags()
            .into_iter()
            .map(|(name, count)| VaultTag { name, count: count as u32 })
            .collect())
    }

    /// Lines matching every word; `tag:x`, `path:x` and "quoted phrases"
    /// work too.
    pub fn search(&self, query: String, limit: u32) -> Result<Vec<NoteMention>, CoreError> {
        Ok(self.lock()?.search(&query, limit as usize).into_iter().map(NoteMention::from).collect())
    }

    /// A note (as currently typed) as HTML for the reading view. With a
    /// connected library, `[[@paper]]` links and embedded highlights show.
    pub fn render(&self, path: String, text: String) -> Result<String, CoreError> {
        let library = self.library();
        let hook = |link: &paperloom_vault::Link| library.as_ref().and_then(|l| l.render_note_link(link));
        let style = self.lock()?.settings().citation_style;
        let cited = library
            .as_ref()
            .map(|l| l.cite_note(&text, &style, paperloom_citation::Format::Html))
            .unwrap_or_default();
        let cite = |citation: &paperloom_vault::Citation| {
            cited
                .citations
                .get(&citation.start)
                .map(|html| format!(r#"<span class="citation">{html}</span>"#))
        };
        let rows = |query: &paperloom_vault::Query| {
            library.as_ref().map(|l| l.query_rows(query)).unwrap_or_default()
        };
        let hooks = paperloom_vault::Hooks {
            link: &hook,
            cite: &cite,
            library: library.as_ref().map(|_| &rows as &paperloom_vault::LibraryRows<'_>),
        };
        let mut html = self.lock()?.render_with_hooks(&path, &text, &hooks);
        if !cited.entries.is_empty() {
            let heading = if style.starts_with("gb-") { "参考文献" } else { "References" };
            let entries: String =
                cited.entries.iter().map(|entry| format!(r#"<p class="reference">{entry}</p>"#)).collect();
            html.push_str(&format!(r#"<section class="references"><h2>{heading}</h2>{entries}</section>"#));
        }
        Ok(html)
    }

    /// Makes (once) a literature note for an item in `Literature/`: its
    /// details, a link to it, the abstract and every highlight.
    pub fn create_literature_note(&self, item_id: String) -> Result<NoteInfo, CoreError> {
        let library = self.library().ok_or_else(|| CoreError::InvalidInput {
            message: "The vault is not connected to a library.".into(),
        })?;
        let (path, text) = crate::notelinks::literature_note(&library, &item_id)?;
        if let Some(note) = self.lock()?.note(&path) {
            return Ok(NoteInfo::from(note));
        }
        self.write_note(path, text)
    }

    /// The notes Zotero plugins see, as Zotero note items (JSON): those with
    /// a `paperloom-note-id`, children of the item named by
    /// `paperloom-parent-item` when the library has it, else standalone.
    /// A repeated id keeps its first note; a note over 1 MiB of HTML is
    /// left out rather than cut.
    pub fn zotero_notes(&self) -> Result<String, CoreError> {
        use crate::notelinks::{NOTE_ID, NOTE_PARENT};
        let library = self.library();
        // Read under the lock, render after it: rendering takes the lock again.
        let notes: Vec<Note> = self.lock()?.notes().cloned().collect();
        let mut seen = std::collections::HashSet::new();
        let mut items = Vec::new();
        for Note { path, title, text, modified, parsed, .. } in notes {
            let (properties, tags) = (parsed.properties, parsed.tags);
            let property =
                |key: &str| properties.iter().find(|(k, _)| k == key).map(|(_, v)| v.trim().to_string());
            let Some(id) = property(NOTE_ID).filter(|id| {
                !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            }) else {
                continue;
            };
            // A repeated id, or one an item already has, would make two records one.
            let taken = library.as_ref().is_some_and(|l| matches!(l.item_detail(id.clone()), Ok(Some(_))));
            if taken || !seen.insert(id.clone()) {
                continue;
            }
            let parent = property(NOTE_PARENT).filter(|item| {
                library.as_ref().is_some_and(|l| matches!(l.item_detail(item.clone()), Ok(Some(_))))
            });
            let note = format!(r#"<div data-schema-version="9">{}</div>"#, self.render(path, text)?);
            if note.len() > 1 << 20 {
                continue;
            }
            let date = crate::zotero_plugins::sql_date(modified);
            let made_by_plugin = property(crate::notelinks::NOTE_SOURCE).as_deref() == Some("plugin");
            let mut record = serde_json::json!({
                "key": id, "itemType": "note", "title": title, "note": note,
                "dateAdded": date, "dateModified": date,
                "tags": tags.iter().map(|tag| serde_json::json!({ "tag": tag })).collect::<Vec<_>>(),
            });
            if let Some(parent) = parent {
                record["parentItem"] = serde_json::json!(parent);
            }
            if made_by_plugin {
                record["paperloomPluginNote"] = serde_json::json!(true);
            }
            items.push(record);
        }
        Ok(serde_json::Value::Array(items).to_string())
    }

    /// A Zotero plugin's new note (HTML) as a Markdown note in
    /// `Zotero Notes/`, under an item when the library has it; its id.
    pub fn zotero_create_note(&self, parent_item: Option<String>, html: String) -> Result<String, CoreError> {
        use crate::notelinks::{NOTE_ID, NOTE_PARENT, NOTE_SOURCE};
        check_note_html(&html)?;
        let parent = parent_item.filter(|item| {
            self.library().is_some_and(|l| matches!(l.item_detail(item.clone()), Ok(Some(_))))
        });
        let body = crate::zotero::note_markdown(&html);
        let id = uuid::Uuid::now_v7().to_string();
        let mut front = format!("---\n{NOTE_ID}: {id}\n");
        if let Some(parent) = &parent {
            front.push_str(&format!("{NOTE_PARENT}: {parent}\n"));
        }
        front.push_str(&format!("{NOTE_SOURCE}: plugin\n---\n\n"));
        let created = self.create_note("Zotero Notes".into(), note_title(&body))?;
        self.write_note(created.path, front + &body)?;
        Ok(id)
    }

    /// A Zotero plugin's change to a note it made (HTML): the body is
    /// replaced, the front matter kept. Other notes are the person's own,
    /// and plugins do not change them.
    pub fn zotero_update_note(&self, key: String, html: String) -> Result<(), CoreError> {
        use crate::notelinks::{NOTE_ID, NOTE_SOURCE};
        check_note_html(&html)?;
        let found = self.lock()?.notes().find_map(|note| {
            let property =
                |name: &str| note.parsed.properties.iter().find(|(k, _)| k == name).map(|(_, v)| v.trim());
            (property(NOTE_ID) == Some(key.as_str()))
                .then(|| (note.path.clone(), note.text.clone(), property(NOTE_SOURCE) == Some("plugin")))
        });
        let (path, text, made_by_plugin) =
            found.ok_or_else(|| CoreError::not_found(format!("note {key}")))?;
        if !made_by_plugin {
            return Err(CoreError::InvalidInput {
                message: "Plugins change only the notes they made.".into(),
            });
        }
        let front = text
            .strip_prefix("---\n")
            .and_then(|rest| rest.find("\n---\n").map(|end| &text[..end + 4 + "\n---\n".len()]))
            .unwrap_or("");
        let body = crate::zotero::note_markdown(&html);
        self.write_note(path, format!("{front}\n{body}"))?;
        Ok(())
    }

    /// Lines of notes that link to an item or embed its highlights; none
    /// without a connected library.
    pub fn item_mentions(&self, item_id: String) -> Result<Vec<NoteMention>, CoreError> {
        let Some(library) = self.library() else { return Ok(Vec::new()) };
        let wanted = library.links_to_item(&item_id)?;
        Ok(self.lock()?.links_where(&wanted).into_iter().map(NoteMention::from).collect())
    }

    /// Notes and library items about what a note is about that it does not
    /// link to or cite yet, most related first, found by the words that set
    /// the note apart. None without a connected library (which keeps the
    /// search index).
    pub fn link_suggestions(&self, path: String, limit: u32) -> Result<Vec<LinkSuggestion>, CoreError> {
        let Some(library) = self.library() else { return Ok(Vec::new()) };
        let (text, mut skip, references) = {
            let vault = self.lock()?;
            let note = vault
                .note(&path)
                .ok_or_else(|| CoreError::NotFound { message: format!("{path} not found") })?;
            let mut notes = vec![path.clone()];
            let mut references: Vec<String> = Vec::new();
            for link in &note.parsed.links {
                match link.target.strip_prefix('@') {
                    Some(reference) => references.push(reference.to_string()),
                    None => notes.extend(vault.resolve(&link.target, &path)),
                }
            }
            references
                .extend(note.parsed.citations.iter().flat_map(|c| c.works.iter().map(|w| w.key.clone())));
            (note.text.clone(), notes, references)
        };
        skip.extend(references.into_iter().filter_map(|r| library.resolve_item_reference(r).ok().flatten()));
        let kinds = [paperloom_fulltext::Kind::Note, paperloom_fulltext::Kind::Item];
        let hits = library.fulltext.related(&text, &kinds, &skip, limit as usize * 2)?;
        let vault = self.lock()?;
        let mut found = Vec::new();
        for hit in hits {
            let suggestion = match hit.kind {
                paperloom_fulltext::Kind::Note => vault.note(&hit.key).map(|note| LinkSuggestion {
                    title: note.title.clone(),
                    insert: format!("[[{}]]", vault.link_target_for(&note.path, &path)),
                    path: Some(note.path.clone()),
                    item_id: None,
                    snippet_html: hit.snippet,
                }),
                _ => library.item_to_link(&hit.key).map(|(title, insert)| LinkSuggestion {
                    title,
                    insert,
                    path: None,
                    item_id: Some(hit.key),
                    snippet_html: hit.snippet,
                }),
            };
            found.extend(suggestion);
            if found.len() >= limit as usize {
                break;
            }
        }
        Ok(found)
    }

    /// The note or file a link leads to from a note.
    pub fn resolve(&self, target: String, from: String) -> Result<Option<String>, CoreError> {
        Ok(self.lock()?.resolve(&target, &from))
    }

    /// The file on disk for a vault path, for showing images.
    pub fn file_path(&self, path: String) -> Result<String, CoreError> {
        Ok(self.lock()?.file_path(&path)?.display().to_string())
    }

    /// Writes a note's properties (front matter), keeping the rest of it.
    /// Keys written as lists stay lists; no properties, no front matter.
    pub fn set_properties(&self, path: String, properties: Vec<NoteProperty>) -> Result<NoteInfo, CoreError> {
        let text = self.read_note(path.clone())?;
        let pairs: Vec<(String, String)> = properties.into_iter().map(|p| (p.key, p.value)).collect();
        self.write_note(path, vault::with_properties(&text, &pairs))
    }

    pub fn settings(&self) -> Result<VaultSettings, CoreError> {
        Ok(self.lock()?.settings().into())
    }

    pub fn set_settings(&self, settings: VaultSettings) -> Result<(), CoreError> {
        Ok(self.lock()?.set_settings(&settings.into())?)
    }

    /// The notes in the templates folder.
    pub fn templates(&self) -> Result<Vec<NoteInfo>, CoreError> {
        Ok(self.lock()?.templates().into_iter().map(NoteInfo::from).collect())
    }

    /// A template filled in for a note named `title`, for putting into a
    /// note: `{{title}}`, `{{date}}`, `{{time}}`, `{{date:YYYY-MM-DD}}`.
    pub fn template_text(
        &self,
        template: String,
        title: String,
        now: LocalTime,
    ) -> Result<String, CoreError> {
        Ok(self.lock()?.fill_template(&template, &title, &now.into())?)
    }

    /// Creates a note from a template, or empty without one.
    pub fn create_note_from_template(
        &self,
        folder: String,
        title: String,
        template: Option<String>,
        now: LocalTime,
    ) -> Result<NoteInfo, CoreError> {
        let info = {
            let mut vault = self.lock()?;
            let path = vault.create_from_template(&folder, &title, template.as_deref(), &now.into())?;
            NoteInfo::from(vault.note(&path).expect("just created"))
        };
        self.index(Some(std::slice::from_ref(&info.path)));
        Ok(info)
    }

    /// Today's daily note (by the app's clock), made from the daily
    /// template the first time.
    pub fn daily_note(&self, now: LocalTime) -> Result<NoteInfo, CoreError> {
        let (info, new) = {
            let mut vault = self.lock()?;
            let (path, new) = vault.daily_note(&now.into())?;
            (NoteInfo::from(vault.note(&path).expect("just made")), new)
        };
        if new {
            self.index(Some(std::slice::from_ref(&info.path)));
        }
        Ok(info)
    }

    /// A note's earlier versions, newest first.
    pub fn note_versions(&self, path: String) -> Result<Vec<NoteVersion>, CoreError> {
        let versions = self.lock()?.versions(&path);
        Ok(versions.into_iter().map(|v| NoteVersion { time: v.time, size: v.size }).collect())
    }

    pub fn note_version(&self, path: String, time: i64) -> Result<String, CoreError> {
        Ok(self.lock()?.version(&path, time)?)
    }

    /// What changed from an earlier version to the note now, line by line.
    pub fn compare_version(&self, path: String, time: i64) -> Result<Vec<DiffLine>, CoreError> {
        let vault = self.lock()?;
        let old = vault.version(&path, time)?;
        let now = vault.note(&path).map(|n| n.text.clone()).unwrap_or_default();
        Ok(vault::diff_lines(&old, &now)
            .into_iter()
            .map(|(change, text)| {
                let change = match change {
                    vault::Change::Same => "same",
                    vault::Change::Added => "added",
                    vault::Change::Removed => "removed",
                };
                DiffLine { change: change.into(), text }
            })
            .collect())
    }

    /// Puts an earlier version back; what was there is kept as a version.
    pub fn restore_version(&self, path: String, time: i64) -> Result<NoteInfo, CoreError> {
        let info = {
            let mut vault = self.lock()?;
            vault.restore(&path, time)?;
            NoteInfo::from(vault.note(&path).ok_or_else(|| CoreError::NotFound { message: path.clone() })?)
        };
        self.index(Some(std::slice::from_ref(&info.path)));
        Ok(info)
    }

    /// The graph of notes and links, laid out; around `center` (within
    /// `depth` links) when given. `[[@paper]]` links are papers from the
    /// connected library.
    pub fn graph(&self, center: Option<String>, depth: u32) -> Result<NoteGraph, CoreError> {
        let graph = self.lock()?.graph(center.as_deref(), depth as usize);
        let library = self.library();
        let nodes = graph
            .nodes
            .into_iter()
            .map(|node| {
                let item_id = match (&node.kind, node.id.strip_prefix('@'), &library) {
                    (vault::NodeKind::Missing, Some(reference), Some(library)) => {
                        library.resolve_item_reference(reference.to_string()).ok().flatten()
                    }
                    _ => None,
                };
                let kind = match (node.kind, &item_id) {
                    (_, Some(_)) => "item",
                    (vault::NodeKind::Note, None) => "note",
                    (vault::NodeKind::Missing, None) => "missing",
                };
                GraphNodeInfo {
                    id: node.id,
                    title: node.title,
                    kind: kind.into(),
                    item_id,
                    degree: node.degree,
                    x: node.x,
                    y: node.y,
                }
            })
            .collect();
        let edges = graph.edges.into_iter().map(|(from, to)| GraphEdge { from, to }).collect();
        Ok(NoteGraph { nodes, edges })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn works_through_the_api() {
        let dir = tempfile::tempdir().unwrap();
        let vault = PaperVault::open(dir.path().join("Vault").display().to_string()).unwrap();
        let a = vault.create_note(String::new(), "Reading list".into()).unwrap();
        assert_eq!((a.path.as_str(), a.title.as_str()), ("Reading list.md", "Reading list"));
        vault.write_note("papers/Transformer".into(), "# Transformer\n## Method\n#ml".into()).unwrap();
        vault.write_note(a.path.clone(), "Read [[Transformer#Method]] next.".into()).unwrap();
        let outline = vault.outline("papers/Transformer.md".into()).unwrap();
        assert_eq!(outline.headings[1].anchor, "method");
        assert_eq!(outline.tags, ["ml"]);
        assert_eq!(outline.backlinks[0].path, "Reading list.md");
        let completions = vault.link_completions(a.path.clone(), "trans".into(), 5).unwrap();
        assert_eq!(completions[0].insert, "Transformer");
        assert_eq!(
            vault.rename_note("papers/Transformer.md".into(), "papers/Attention.md".into()).unwrap(),
            1
        );
        assert_eq!(vault.read_note(a.path.clone()).unwrap(), "Read [[Attention#Method]] next.");
        assert!(matches!(vault.read_note("nope.md".into()), Err(CoreError::NotFound { .. })));
        assert!(matches!(
            vault.write_note("../x".into(), String::new()),
            Err(CoreError::InvalidInput { .. })
        ));
        assert_eq!(vault.tags().unwrap(), [VaultTag { name: "ml".into(), count: 1 }]);
    }

    #[test]
    fn renders_citations_with_a_reference_list() {
        let dir = tempfile::tempdir().unwrap();
        let library = PaperLibrary::new(dir.path().join("library.sqlite").display().to_string()).unwrap();
        let item = library
            .add_item(crate::NewItem {
                item_type: String::new(),
                title: "Nanometre-scale thermometry in a living cell".into(),
                authors: vec!["Georg Kucsko".into()],
                container_title: Some("Nature".into()),
                year: Some(2013),
                doi: Some("10.1038/nature12373".into()),
            })
            .unwrap();
        let found = library.citation_completions("thermometry KUCSKO".into(), 5).unwrap();
        assert_eq!(found.len(), 1);
        assert!(found[0].key.starts_with("kucsko2013"), "{found:?}");
        assert!(library.citation_completions("curie".into(), 5).unwrap().is_empty());
        let key = found[0].key.clone();
        let vault = PaperVault::open(dir.path().join("Vault").display().to_string()).unwrap();
        vault.connect_library(library);
        let by_key = vault.render("Keyed.md".into(), format!("[@{key}]")).unwrap();
        assert!(by_key.contains(r#"<span class="citation">"#), "a completed key cites: {by_key}");
        let text = "As shown [@10.1038/nature12373, p. 55], and [@nobody] is not here.".to_string();
        let html = vault.render("Note.md".into(), text.clone()).unwrap();
        assert!(html.contains(r#"<span class="citation">"#), "{html}");
        assert!(html.contains("55") && html.contains("Kucsko"), "{html}");
        assert!(html.contains("[@nobody]"), "a work the library lacks stays as written: {html}");
        assert!(html.contains(r#"<section class="references"><h2>References</h2>"#), "{html}");
        assert!(html.contains("Nanometre-scale thermometry"), "{html}");
        let settings =
            VaultSettings { citation_style: "gb-7714-2015-numeric".into(), ..vault.settings().unwrap() };
        vault.set_settings(settings).unwrap();
        let html = vault.render("Note.md".into(), text).unwrap();
        assert!(html.contains("<h2>参考文献</h2>") && html.contains("[1]"), "{html}");
        assert_eq!(item.title, "Nanometre-scale thermometry in a living cell");
    }

    #[test]
    fn suggests_links_by_what_notes_are_about() {
        let dir = tempfile::tempdir().unwrap();
        let library = PaperLibrary::new(dir.path().join("library.sqlite").display().to_string()).unwrap();
        let item = library
            .add_item(crate::NewItem {
                title: "Nanometre-scale thermometry in a living cell".into(),
                authors: vec!["Georg Kucsko".into()],
                year: Some(2013),
                doi: Some("10.1038/nature12373".into()),
                ..crate::NewItem::default()
            })
            .unwrap();
        let vault = PaperVault::open(dir.path().join("Vault").display().to_string()).unwrap();
        vault.connect_library(library);
        vault
            .write_note(
                "Diamond.md".into(),
                "Nitrogen-vacancy centres measure temperature: thermometry.".into(),
            )
            .unwrap();
        vault.write_note("Sensors.md".into(), "Nitrogen-vacancy thermometry sensors.".into()).unwrap();
        vault.write_note("Bread.md".into(), "Sourdough needs patience.".into()).unwrap();
        let draft = "Thermometry with nitrogen-vacancy centres, see [[Sensors]].";
        vault.write_note("Draft.md".into(), draft.into()).unwrap();

        let found = vault.link_suggestions("Draft.md".into(), 10).unwrap();
        let titles: Vec<&str> = found.iter().map(|s| s.title.as_str()).collect();
        assert!(titles.contains(&"Diamond") && titles.contains(&item.title.as_str()), "{titles:?}");
        assert!(
            !titles.contains(&"Sensors") && !titles.contains(&"Bread") && !titles.contains(&"Draft"),
            "{titles:?}"
        );
        let diamond = found.iter().find(|s| s.title == "Diamond").unwrap();
        assert_eq!((diamond.insert.as_str(), diamond.path.as_deref()), ("[[Diamond]]", Some("Diamond.md")));
        assert!(diamond.snippet_html.contains("<b>"), "{}", diamond.snippet_html);
        let paper = found.iter().find(|s| s.item_id.as_deref() == Some(item.id.as_str())).unwrap();
        assert!(paper.insert.starts_with("[[@10.1038/nature12373|"), "{}", paper.insert);

        vault.write_note("Draft.md".into(), format!("{draft} [@10.1038/nature12373]")).unwrap();
        let found = vault.link_suggestions("Draft.md".into(), 10).unwrap();
        assert!(found.iter().all(|s| s.item_id.is_none()), "a cited work is not suggested: {found:?}");
    }

    #[test]
    fn renders_library_queries() {
        let dir = tempfile::tempdir().unwrap();
        let vault = PaperVault::open(dir.path().join("Vault").display().to_string()).unwrap();
        let query =
            "```query\nfrom: library #physics\nwhere: year < 2020\nshow: table title, authors, year\n```";
        let html = vault.render("Q.md".into(), query.into()).unwrap();
        assert!(html.contains("not connected to a library"), "{html}");
        let library = PaperLibrary::new(dir.path().join("library.sqlite").display().to_string()).unwrap();
        let mut ids = Vec::new();
        for (title, year) in
            [("Old cell thermometry", 2013), ("New cell thermometry", 2024), ("Untagged", 2001)]
        {
            let item = library
                .add_item(crate::NewItem {
                    item_type: String::new(),
                    title: title.into(),
                    authors: vec!["Georg Kucsko".into()],
                    container_title: None,
                    year: Some(year),
                    doi: None,
                })
                .unwrap();
            ids.push(item.id);
        }
        library.set_item_tags(ids[0].clone(), vec!["physics/quantum".into()]).unwrap();
        library.set_item_tags(ids[1].clone(), vec!["Physics".into()]).unwrap();
        vault.connect_library(library);
        let html = vault.render("Q.md".into(), query.into()).unwrap();
        assert!(html.contains("<th>authors</th>"), "{html}");
        assert!(
            html.contains(&format!(r#"href="paperloom:item/{}">Old cell thermometry</a>"#, ids[0])),
            "{html}"
        );
        assert!(!html.contains("New cell") && !html.contains("Untagged"), "{html}");
        let html = vault.render("Q.md".into(), "```query\nfrom: library \"No such\"\n```".into()).unwrap();
        assert!(html.contains("Nothing matches."), "{html}");
        let html = vault.render("Q.md".into(), "```query\nshow: chart\n```".into()).unwrap();
        assert!(html.contains(r#"<p class="query-error">"#), "{html}");
    }

    #[test]
    fn keeps_properties_templates_daily_notes_and_versions() {
        let dir = tempfile::tempdir().unwrap();
        let vault = PaperVault::open(dir.path().display().to_string()).unwrap();
        let now = LocalTime { year: 2026, month: 10, day: 12, hour: 8, minute: 5, second: 0 };
        let note = vault.write_note("Paper.md".into(), "Body".into()).unwrap();
        let props = vec![NoteProperty { key: "status".into(), value: "reading".into() }];
        vault.set_properties(note.path.clone(), props).unwrap();
        assert_eq!(vault.read_note(note.path.clone()).unwrap(), "---\nstatus: reading\n---\nBody");
        assert_eq!(vault.outline(note.path.clone()).unwrap().properties[0].value, "reading");

        vault.write_note("Templates/Day.md".into(), "# {{date:YYYY年M月D日}}".into()).unwrap();
        let settings = VaultSettings {
            daily_folder: "Daily".into(),
            daily_template: "Templates/Day".into(),
            ..vault.settings().unwrap()
        };
        vault.set_settings(settings).unwrap();
        let day = vault.daily_note(now).unwrap();
        assert_eq!(day.path, "Daily/2026-10-12.md");
        assert_eq!(vault.read_note(day.path).unwrap(), "# 2026年10月12日");
        assert_eq!(vault.templates().unwrap().len(), 1);
        let made =
            vault.create_note_from_template(String::new(), "New".into(), Some("Day".into()), now).unwrap();
        assert_eq!(vault.read_note(made.path).unwrap(), "# 2026年10月12日");

        let versions = vault.note_versions(note.path.clone()).unwrap();
        assert_eq!(versions.len(), 1, "the note before its properties");
        let diff = vault.compare_version(note.path.clone(), versions[0].time).unwrap();
        assert_eq!(diff.iter().filter(|d| d.change == "added").count(), 3);
        assert_eq!(diff.last().unwrap(), &DiffLine { change: "same".into(), text: "Body".into() });
        vault.restore_version(note.path.clone(), versions[0].time).unwrap();
        assert_eq!(vault.read_note(note.path.clone()).unwrap(), "Body");

        let graph = vault.graph(None, 0).unwrap();
        assert_eq!(graph.nodes.len(), 4);
        assert!(graph.nodes.iter().all(|n| n.kind == "note"));
    }
}
