//! The Markdown vault: notes are plain `.md` files in a folder (an existing
//! Obsidian vault works as is), indexed in memory for links, backlinks,
//! tags, the quick switcher and search.
//!
//! Paths are relative to the vault and use `/`. Folders and files whose
//! name starts with `.` (`.obsidian`, `.trash`, `.paperloom`) are left
//! alone.
//!
//! Beyond notes: properties written back as front matter, templates and
//! daily notes (sharing Obsidian's settings), each note's earlier versions,
//! the graph of links, `query` blocks that list notes or library items,
//! canvases, mind maps (with OPML, FreeMind, XMind and SVG files),
//! drawings kept as Excalidraw scenes, and draw.io diagrams.

mod canvas;
mod daily;
mod diagram;
mod diff;
mod drawing;
mod graph;
mod history;
mod mapfiles;
mod mindmap;
mod note_html;
mod parse;
mod plugins;
mod properties;
mod query;
mod render;
mod template;
mod xml;

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

pub use canvas::{Canvas, CanvasEdge, CanvasNode};
pub use daily::Settings;
pub use diagram::{
    Cell as DiagramCell, Diagram, DiagramEdit, EDGE_STYLE as DIAGRAM_EDGE_STYLE, EMPTY_DIAGRAM,
    Geometry as DiagramGeometry, Item as DiagramItem, PALETTE as DIAGRAM_PALETTE, Page as DiagramPage,
    PaletteRow as DiagramPaletteRow, PaletteShape as DiagramPaletteShape, Rect as DiagramRect,
    Scene as DiagramScene, Style as DiagramStyle, diagram_picture, diagram_xml,
    edit_page as edit_diagram_page, is_diagram, label_text as diagram_label_text, picture_holding,
    scene as diagram_scene, scene_svg as diagram_svg,
};
pub use diff::{Change, diff_lines};
pub use drawing::{DrawElement, Drawing, drawing_svg, picture_id, stroke_diameter};
pub use graph::{Graph, GraphNode, NodeKind};
pub use history::Version;
pub use mapfiles::{
    map_from_freemind, map_from_opml, map_from_xmind_json, map_from_xmind_xml, map_svg, map_to_freemind,
    map_to_opml, map_to_xmind_json,
};
pub use mindmap::{
    CENTRE as MAP_CENTRE, Extra as MapExtra, ExtraEdit as MapExtraEdit, ExtraKind as MapExtraKind,
    Layout as MapLayout, MAP_THEMES, MARKERS as MAP_MARKERS, MapEdit, MapStyle, MapTheme,
    Marker as MapMarker, Placed as PlacedTopic, PlacedExtra as PlacedMapExtra, Topic,
    edit_extras as edit_map_extras, edit_map, is_web_link, layout_map, new_map,
    read_extras as read_map_extras, read_map, split_link as split_topic_link, with_link as with_topic_link,
    write_map, write_map_with,
};
pub use parse::{Citation, CitedWork, Heading, Link, Parsed, parse, plain_citations};
pub use plugins::{InstalledPlugin, PluginCode, PluginManifest};
pub use properties::with_properties;
pub use query::{Filter, Op, Query, Row, date_of, has_tag, parse_query, render_rows, select};
pub use render::heading_id;
pub use template::{Moment, fill as fill_template, format as format_date};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{path}: {source}")]
    Io { path: String, source: io::Error },
    #[error("“{0}” is not a path inside the vault")]
    InvalidPath(String),
    #[error("{0} not found")]
    NotFound(String),
    #[error("{0} already exists")]
    Exists(String),
    /// A file the vault reads is not what it should be (a broken canvas).
    #[error("{0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, Error>;

fn io_error(path: &Path, source: io::Error) -> Error {
    Error::Io { path: path.display().to_string(), source }
}

/// A note and what the index knows about it.
#[derive(Debug, Clone)]
pub struct Note {
    /// `folder/Note.md`.
    pub path: String,
    /// The file name without `.md`.
    pub title: String,
    /// Milliseconds since 1970.
    pub modified: i64,
    pub size: u64,
    pub text: String,
    pub parsed: Parsed,
}

impl Note {
    pub fn folder(&self) -> &str {
        self.path.rsplit_once('/').map_or("", |(folder, _)| folder)
    }
}

/// Gives the library items a `from: library` query starts from (in its
/// collection, under its tag); the vault filters, sorts and cuts them.
pub type LibraryRows<'a> = dyn Fn(&Query) -> Vec<Row> + 'a;

/// What an app adds to the reading view.
#[derive(Clone, Copy)]
pub struct Hooks<'a> {
    /// Shows some links itself: library items, embedded PDF highlights.
    pub link: &'a dyn Fn(&Link) -> Option<External>,
    /// HTML in place of one of the note's Pandoc citations.
    pub cite: &'a dyn Fn(&Citation) -> Option<String>,
    /// None when no library is connected.
    pub library: Option<&'a LibraryRows<'a>>,
}

/// HTML a render hook puts in place of a link; `block` content (a quoted
/// highlight) stands on its own rather than inside a line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct External {
    pub html: String,
    pub block: bool,
}

/// Where a note is linked to or mentioned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mention {
    pub path: String,
    pub title: String,
    /// From 0.
    pub line: usize,
    /// The line, trimmed and shortened.
    pub context: String,
}

pub struct Vault {
    root: PathBuf,
    notes: BTreeMap<String, Note>,
    /// Other files (images, PDFs…), for embeds.
    files: BTreeSet<String>,
    folders: BTreeSet<String>,
}

impl Vault {
    /// Opens a vault folder, creating it if needed.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root).map_err(|e| io_error(&root, e))?;
        let mut vault =
            Self { root, notes: BTreeMap::new(), files: BTreeSet::new(), folders: BTreeSet::new() };
        vault.refresh()?;
        Ok(vault)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Picks up changes made outside the app. Unchanged notes (same size
    /// and time) are not read again. Returns true if anything changed.
    pub fn refresh(&mut self) -> Result<bool> {
        let mut found_notes = BTreeMap::new();
        let mut files = BTreeSet::new();
        let mut folders = BTreeSet::new();
        self.scan(&self.root.clone(), "", &mut found_notes, &mut files, &mut folders)?;
        let mut changed = files != self.files || folders != self.folders;
        let mut notes = BTreeMap::new();
        for (path, (modified, size)) in found_notes {
            match self.notes.remove(&path) {
                Some(note) if note.modified == modified && note.size == size => {
                    notes.insert(path, note);
                }
                _ => {
                    changed = true;
                    let text = fs::read_to_string(self.full(&path)).unwrap_or_default();
                    notes.insert(path.clone(), make_note(path, text, modified, size));
                }
            }
        }
        changed |= !self.notes.is_empty();
        self.notes = notes;
        self.files = files;
        self.folders = folders;
        Ok(changed)
    }

    fn scan(
        &self,
        dir: &Path,
        prefix: &str,
        notes: &mut BTreeMap<String, (i64, u64)>,
        files: &mut BTreeSet<String>,
        folders: &mut BTreeSet<String>,
    ) -> Result<()> {
        let entries = fs::read_dir(dir).map_err(|e| io_error(dir, e))?;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let path = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                folders.insert(path.clone());
                self.scan(&entry.path(), &path, notes, files, folders)?;
            } else if name.to_lowercase().ends_with(".md") {
                notes.insert(path, (modified_ms(&meta), meta.len()));
            } else {
                files.insert(path);
            }
        }
        Ok(())
    }

    fn full(&self, path: &str) -> PathBuf {
        path.split('/').fold(self.root.clone(), |p, part| p.join(part))
    }

    pub fn notes(&self) -> impl Iterator<Item = &Note> {
        self.notes.values()
    }

    pub fn note(&self, path: &str) -> Option<&Note> {
        self.notes.get(path)
    }

    pub fn folders(&self) -> impl Iterator<Item = &String> {
        self.folders.iter()
    }

    pub fn files(&self) -> impl Iterator<Item = &String> {
        self.files.iter()
    }

    /// The file on disk for a vault path (for showing images).
    pub fn file_path(&self, path: &str) -> Result<PathBuf> {
        Ok(self.full(&checked(path)?))
    }

    /// Saves a note (creating it and its folders if needed). The file is
    /// replaced in one step, so a crash never leaves half a note.
    pub fn write(&mut self, path: &str, text: &str) -> Result<()> {
        let path = checked_note(path)?;
        if let Some(old) = self.notes.get(&path)
            && old.text != text
        {
            self.keep_version(&path, &old.text, history::now_ms(), false);
        }
        let full = self.full(&path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
            self.add_folders(&path);
        }
        let temporary = full.with_extension("md.paperloom-tmp");
        fs::write(&temporary, text).map_err(|e| io_error(&temporary, e))?;
        fs::rename(&temporary, &full).map_err(|e| io_error(&full, e))?;
        let meta = fs::metadata(&full).map_err(|e| io_error(&full, e))?;
        self.notes.insert(path.clone(), make_note(path, text.to_string(), modified_ms(&meta), meta.len()));
        Ok(())
    }

    /// Keeps a file the apps made (a recording) in `folder` as `name`, or
    /// `name 2`, … when that is taken; its path.
    pub fn add_file(&mut self, folder: &str, name: &str, data: &[u8]) -> Result<String> {
        let name = name.trim();
        let (stem, extension) = match name.rsplit_once('.') {
            Some((stem, extension)) if !stem.is_empty() => (stem, format!(".{extension}")),
            _ => (name, String::new()),
        };
        let stem = if stem.is_empty() { "Untitled" } else { stem };
        let folder = folder.trim_matches('/');
        let mut n = 1;
        let path = loop {
            let file = if n == 1 { format!("{stem}{extension}") } else { format!("{stem} {n}{extension}") };
            let path = if folder.is_empty() { file } else { format!("{folder}/{file}") };
            if !self.files.iter().any(|f| f.eq_ignore_ascii_case(&path)) {
                break path;
            }
            n += 1;
        };
        let path = checked(&path)?;
        let full = self.full(&path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
            self.add_folders(&path);
        }
        fs::write(&full, data).map_err(|e| io_error(&full, e))?;
        self.files.insert(path.clone());
        Ok(path)
    }

    fn add_folders(&mut self, path: &str) {
        let mut folder = String::new();
        let parts: Vec<&str> = path.split('/').collect();
        for part in &parts[..parts.len().saturating_sub(1)] {
            if !folder.is_empty() {
                folder.push('/');
            }
            folder.push_str(part);
            self.folders.insert(folder.clone());
        }
    }

    /// Creates an empty note named `title` (or “Untitled”) in a folder,
    /// adding ` 2`, ` 3`… if the name is taken. Returns its path.
    pub fn create_note(&mut self, folder: &str, title: &str) -> Result<String> {
        let folder = if folder.is_empty() { String::new() } else { checked(folder)? };
        let base = clean_title(title);
        let path_for = |n: usize| {
            let name = if n == 1 { format!("{base}.md") } else { format!("{base} {n}.md") };
            if folder.is_empty() { name } else { format!("{folder}/{name}") }
        };
        let path = (1..).map(path_for).find(|p| !self.exists(p)).expect("a free name");
        self.write(&path, "")?;
        Ok(path)
    }

    fn exists(&self, path: &str) -> bool {
        self.notes.keys().chain(self.files.iter()).any(|p| p.eq_ignore_ascii_case(path))
            || self.full(path).exists()
    }

    pub fn create_folder(&mut self, path: &str) -> Result<()> {
        let path = checked(path)?;
        let full = self.full(&path);
        fs::create_dir_all(&full).map_err(|e| io_error(&full, e))?;
        self.add_folders(&format!("{path}/x"));
        Ok(())
    }

    /// Moves a note (or a canvas or other file) to the vault's `.trash`
    /// folder, as Obsidian does.
    pub fn delete(&mut self, path: &str) -> Result<()> {
        let path = checked(path)?;
        let note = self.notes.contains_key(&path);
        if !note && !self.files.contains(&path) {
            return Err(Error::NotFound(path));
        }
        let trash = self.root.join(".trash");
        fs::create_dir_all(&trash).map_err(|e| io_error(&trash, e))?;
        let name = path.rsplit('/').next().unwrap_or(&path).to_string();
        let (stem, extension) = match name.rsplit_once('.') {
            Some((stem, extension)) => (stem, format!(".{extension}")),
            None => (name.as_str(), String::new()),
        };
        let mut target = trash.join(&name);
        let mut n = 2;
        while target.exists() {
            target = trash.join(format!("{stem} {n}{extension}"));
            n += 1;
        }
        let full = self.full(&path);
        fs::rename(&full, &target).map_err(|e| io_error(&full, e))?;
        if note {
            self.notes.remove(&path);
        } else {
            self.files.remove(&path);
        }
        Ok(())
    }

    /// Renames or moves a note and updates every link to it. Returns how
    /// many other notes were changed.
    pub fn rename(&mut self, from: &str, to: &str) -> Result<usize> {
        let from = checked(from)?;
        let to = checked_note(to)?;
        if from == to {
            return Ok(0);
        }
        if !self.notes.contains_key(&from) {
            return Err(Error::NotFound(from));
        }
        if self.notes.contains_key(&to) || (self.full(&to).exists() && !from.eq_ignore_ascii_case(&to)) {
            return Err(Error::Exists(to));
        }
        // Links pointing at the note, found before anything moves.
        let mut pointing: Vec<(String, Vec<Link>)> = Vec::new();
        for note in self.notes.values() {
            let links: Vec<Link> = note
                .parsed
                .links
                .iter()
                .filter(|l| {
                    !l.target.is_empty() && self.resolve(&l.target, &note.path).as_deref() == Some(&from)
                })
                .cloned()
                .collect();
            if !links.is_empty() {
                pointing.push((note.path.clone(), links));
            }
        }
        let source = self.full(&from);
        let target = self.full(&to);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
        }
        fs::rename(&source, &target).map_err(|e| io_error(&source, e))?;
        self.move_history(&from, &to);
        let mut note = self.notes.remove(&from).expect("checked above");
        note.title = title_of(&to);
        note.path = to.clone();
        self.notes.insert(to.clone(), note);
        self.add_folders(&to);

        let mut changed = 0;
        for (path, links) in pointing {
            let path = if path == from { to.clone() } else { path };
            let Some(note) = self.notes.get(&path) else { continue };
            let new_target = self.link_target_for(&to, &path);
            let mut text = note.text.clone();
            for link in links.iter().rev() {
                text.replace_range(link.start..link.end, &link_text(link, &new_target));
            }
            self.write(&path, &text)?;
            if path != to {
                changed += 1;
            }
        }
        Ok(changed)
    }

    /// How a note at `from` should name `target`: its title when that is
    /// unambiguous, else its path without `.md`.
    pub fn link_target_for(&self, target: &str, from: &str) -> String {
        let title = title_of(target);
        if self.resolve(&title, from).as_deref() == Some(target) {
            title
        } else {
            target.trim_end_matches(".md").to_string()
        }
    }

    /// The note (or file) a link names, the way Obsidian finds it: by path
    /// if it has a `/`, else by name, preferring the linking note's folder,
    /// then the shortest path.
    pub fn resolve(&self, target: &str, from: &str) -> Option<String> {
        let target = target.trim();
        if target.is_empty() {
            return self.notes.contains_key(from).then(|| from.to_string());
        }
        let wanted = target.to_lowercase();
        let wanted_note = if wanted.ends_with(".md") { wanted.clone() } else { format!("{wanted}.md") };
        if wanted.contains('/') {
            let by_path = |path: &&String| {
                let lower = path.to_lowercase();
                lower == wanted_note || lower.ends_with(&format!("/{wanted_note}"))
            };
            if let Some(path) = self.notes.keys().filter(by_path).min_by_key(|p| p.len()) {
                return Some(path.clone());
            }
            let by_file = |path: &&String| {
                let lower = path.to_lowercase();
                lower == wanted || lower.ends_with(&format!("/{wanted}"))
            };
            return self.files.iter().filter(by_file).min_by_key(|p| p.len()).cloned();
        }
        let folder = from.rsplit_once('/').map_or("", |(f, _)| f);
        let mut candidates: Vec<&Note> =
            self.notes.values().filter(|n| format!("{}.md", n.title.to_lowercase()) == wanted_note).collect();
        if !candidates.is_empty() {
            candidates.sort_by_key(|n| (n.folder() != folder, n.path.len(), n.path.clone()));
            return Some(candidates[0].path.clone());
        }
        self.files
            .iter()
            .filter(|p| p.rsplit('/').next().is_some_and(|name| name.to_lowercase() == wanted))
            .min_by_key(|p| p.len())
            .cloned()
    }

    /// Notes linking to a note.
    pub fn backlinks(&self, path: &str) -> Vec<Mention> {
        let mut found = Vec::new();
        for note in self.notes.values().filter(|n| n.path != path) {
            for link in &note.parsed.links {
                if !link.target.is_empty() && self.resolve(&link.target, &note.path).as_deref() == Some(path)
                {
                    found.push(mention(note, link.line));
                }
            }
        }
        found
    }

    /// Lines of notes with a link that `wanted` picks out, once a line.
    pub fn links_where(&self, wanted: &dyn Fn(&Link) -> bool) -> Vec<Mention> {
        let mut found = Vec::new();
        for note in self.notes.values() {
            for link in note.parsed.links.iter().filter(|l| wanted(l)) {
                found.push(mention(note, link.line));
            }
            found.dedup();
        }
        found
    }

    /// Notes that name a note (its title or an alias) without linking.
    pub fn unlinked_mentions(&self, path: &str) -> Vec<Mention> {
        let Some(target) = self.notes.get(path) else { return Vec::new() };
        let mut names: Vec<String> =
            std::iter::once(target.title.clone()).chain(target.parsed.aliases.iter().cloned()).collect();
        names.retain(|n| n.chars().count() >= 2);
        let names: Vec<String> = names.iter().map(|n| n.to_lowercase()).collect();
        let mut found = Vec::new();
        for note in self.notes.values().filter(|n| n.path != path) {
            let mut offset = 0;
            for (number, line) in note.text.split_inclusive('\n').enumerate() {
                let start = offset;
                offset += line.len();
                if start < note.parsed.body_start {
                    continue;
                }
                let lower = line.to_lowercase();
                let named = names.iter().any(|name| {
                    lower.match_indices(name.as_str()).any(|(at, _)| {
                        // Not inside a link on this line (byte offsets match
                        // for ASCII; CJK lowercases to itself).
                        let at = start + at;
                        !note.parsed.links.iter().any(|l| l.start <= at && at < l.end)
                    })
                });
                if named {
                    found.push(mention(note, number));
                }
            }
        }
        found
    }

    /// Notes for the quick switcher: fuzzy matches on title, alias and
    /// path, best first.
    pub fn switcher(&self, query: &str, limit: usize) -> Vec<&Note> {
        let query = query.trim().to_lowercase();
        let mut scored: Vec<(i64, &Note)> = self
            .notes
            .values()
            .filter_map(|note| {
                if query.is_empty() {
                    return Some((note.modified, note));
                }
                let candidates = std::iter::once(note.title.to_lowercase())
                    .chain(note.parsed.aliases.iter().map(|a| a.to_lowercase()))
                    .chain(std::iter::once(note.path.to_lowercase()));
                candidates.filter_map(|text| fuzzy_score(&query, &text)).max().map(|s| (s, note))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.modified.cmp(&a.1.modified)));
        scored.into_iter().take(limit).map(|(_, note)| note).collect()
    }

    /// Every tag with how many notes carry it, by name.
    pub fn tags(&self) -> Vec<(String, usize)> {
        let mut counts: BTreeMap<String, (String, usize)> = BTreeMap::new();
        for note in self.notes.values() {
            for tag in &note.parsed.tags {
                counts.entry(tag.to_lowercase()).or_insert_with(|| (tag.clone(), 0)).1 += 1;
            }
        }
        counts.into_values().collect()
    }

    /// Lines matching every word of a query. `tag:x` and `path:x` narrow
    /// the notes; quoted text is one word.
    pub fn search(&self, query: &str, limit: usize) -> Vec<Mention> {
        let mut words = Vec::new();
        let mut tag_filter = Vec::new();
        let mut path_filter = Vec::new();
        for word in split_query(query) {
            let lower = word.to_lowercase();
            if let Some(tag) = lower.strip_prefix("tag:") {
                tag_filter.push(tag.trim_start_matches('#').to_string());
            } else if let Some(path) = lower.strip_prefix("path:") {
                path_filter.push(path.to_string());
            } else if !lower.is_empty() {
                words.push(lower);
            }
        }
        let mut found = Vec::new();
        for note in self.notes.values() {
            let tags: Vec<String> = note.parsed.tags.iter().map(|t| t.to_lowercase()).collect();
            if !tag_filter.iter().all(|t| tags.iter().any(|x| x == t || x.starts_with(&format!("{t}/")))) {
                continue;
            }
            let path = note.path.to_lowercase();
            if !path_filter.iter().all(|p| path.contains(p.as_str())) {
                continue;
            }
            let text = note.text.to_lowercase();
            let title = note.title.to_lowercase();
            if !words.iter().all(|w| text.contains(w.as_str()) || title.contains(w.as_str())) {
                continue;
            }
            let lines: Vec<usize> = if words.is_empty() {
                vec![0]
            } else {
                note.text
                    .lines()
                    .enumerate()
                    .filter(|(_, line)| {
                        let line = line.to_lowercase();
                        words.iter().any(|w| line.contains(w.as_str()))
                    })
                    .map(|(n, _)| n)
                    .take(3)
                    .collect()
            };
            if lines.is_empty() {
                found.push(mention(note, 0));
            }
            for line in lines {
                found.push(mention(note, line));
            }
            if found.len() >= limit {
                break;
            }
        }
        found.truncate(limit);
        found
    }

    /// The note as HTML for the reading view: links point at notes
    /// (`data-note`), tags at the tag view (`data-tag`), embeds of other
    /// notes are shown inline once, images keep their vault path
    /// (`data-src`).
    pub fn render(&self, path: &str, text: &str) -> String {
        self.render_with(path, text, &|_| None)
    }

    /// Renders with a hook that may show some links itself (links to
    /// library items, embedded PDF highlights).
    pub fn render_with(&self, path: &str, text: &str, hook: &dyn Fn(&Link) -> Option<External>) -> String {
        self.render_with_hooks(path, text, &Hooks { link: hook, cite: &|_| None, library: None })
    }

    /// Like `render_with`, with HTML for the note's own Pandoc citations and
    /// library items for its queries.
    pub fn render_with_hooks(&self, path: &str, text: &str, hooks: &Hooks) -> String {
        render::render(self, path, text, 0, hooks)
    }
}

fn make_note(path: String, text: String, modified: i64, size: u64) -> Note {
    let parsed = parse(&text);
    Note { title: title_of(&path), path, modified, size, text, parsed }
}

fn title_of(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.strip_suffix(".md").or_else(|| name.strip_suffix(".MD")).unwrap_or(name).to_string()
}

fn modified_ms(meta: &fs::Metadata) -> i64 {
    meta.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as i64)
}

fn mention(note: &Note, line: usize) -> Mention {
    let text = note.text.lines().nth(line).unwrap_or("").trim();
    let context: String = text.chars().take(200).collect();
    Mention { path: note.path.clone(), title: note.title.clone(), line, context }
}

/// A link rewritten to name `target`, keeping its heading and alias.
fn link_text(link: &Link, target: &str) -> String {
    let mut text = String::new();
    if link.embed {
        text.push('!');
    }
    text.push_str("[[");
    text.push_str(target);
    if let Some(sub) = &link.subpath {
        text.push('#');
        text.push_str(sub);
    }
    if let Some(alias) = &link.alias {
        text.push('|');
        text.push_str(alias);
    }
    text.push_str("]]");
    text
}

/// A relative vault path without `..`, `.` or empty parts.
fn checked(path: &str) -> Result<String> {
    let cleaned = path.trim().replace('\\', "/");
    if cleaned.starts_with('/') || cleaned.contains(':') {
        return Err(Error::InvalidPath(path.to_string()));
    }
    let cleaned = cleaned.trim_end_matches('/');
    let parts: Vec<&str> = cleaned.split('/').collect();
    let valid = !cleaned.is_empty()
        && parts.iter().all(|p| !p.is_empty() && *p != "." && *p != ".." && !p.starts_with('.'))
        && Path::new(cleaned).components().all(|c| matches!(c, Component::Normal(_)));
    if valid { Ok(cleaned.to_string()) } else { Err(Error::InvalidPath(path.to_string())) }
}

fn checked_note(path: &str) -> Result<String> {
    let path = checked(path)?;
    Ok(if path.to_lowercase().ends_with(".md") { path } else { format!("{path}.md") })
}

/// A note title safe as a file name everywhere.
fn clean_title(title: &str) -> String {
    let cleaned: String = title
        .trim()
        .chars()
        .map(|c| {
            if c.is_control()
                || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' | '#' | '^' | '[' | ']')
            {
                ' '
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let cleaned = cleaned.trim_start_matches('.');
    if cleaned.is_empty() { "Untitled".to_string() } else { cleaned.chars().take(120).collect() }
}

fn split_query(query: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in query.chars() {
        match c {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    words.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

/// How well `query` matches `text` as a subsequence: more for a prefix,
/// word starts and runs of consecutive letters; `None` if it does not.
fn fuzzy_score(query: &str, text: &str) -> Option<i64> {
    if text.starts_with(query) {
        return Some(1000 - text.chars().count() as i64);
    }
    if let Some(at) = text.find(query) {
        return Some(700 - at as i64);
    }
    let mut score = 0;
    let mut chars = text.chars().peekable();
    let mut previous: Option<char> = None;
    let mut run = 0;
    for q in query.chars() {
        loop {
            let c = chars.next()?;
            let word_start = previous.is_none_or(|p| !p.is_alphanumeric());
            previous = Some(c);
            if c == q {
                run += 1;
                score += 10 + run * 5 + if word_start { 20 } else { 0 };
                break;
            }
            run = 0;
            score -= 1;
        }
    }
    Some(score)
}

#[cfg(test)]
mod tests;
