//! Mind maps for the apps: a note's outline laid out as a map, and changes
//! to it written back as the note's text. The apps keep the note as usual
//! (`write_note`); these functions only read and change text. Maps also
//! come in from and go out to other apps' files: OPML, FreeMind (`.mm`),
//! XMind, and SVG pictures.

use std::path::Path;

use paperloom_vault::{
    MAP_MARKERS, MAP_THEMES, MapEdit, MapExtra, MapExtraEdit, MapStyle, MapTheme, Topic, edit_map,
    edit_map_extras, is_web_link, layout_map, map_from_freemind, map_from_opml, map_from_xmind_json,
    map_from_xmind_xml, map_svg, map_to_freemind, map_to_opml, map_to_xmind_json, new_map, read_map,
    read_map_extras, write_map, write_map_with,
};
use serde::{Deserialize, Serialize};

use crate::backup::ZipWriter;
use crate::unzip;
use crate::{CoreError, NoteInfo, PaperVault};

/// A topic placed on the map, in points from the map's top left.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct MindTopic {
    /// Its path from the centre: `0`, `0.2`, `0.2.0`.
    pub id: String,
    pub parent: Option<String>,
    /// Its text as shown: a link as its text.
    pub text: String,
    pub note: String,
    /// Where its link goes, as XMind links topics: a web address, or a
    /// note's name (as `[[…]]` names it); empty when it has none.
    #[uniffi(default = "")]
    pub link: String,
    /// The link goes to a web page (or an app), not a note.
    #[uniffi(default = false)]
    pub web_link: bool,
    /// Its labels, as XMind puts them under a topic (tags at the end of
    /// its text in the note).
    #[uniffi(default = [])]
    pub labels: Vec<String>,
    pub depth: u32,
    /// −1 left of the centre, 1 right of it (below, in an org chart), 0 the
    /// centre.
    pub side: i32,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub font_size: f64,
    /// Topics directly under it.
    pub children: u32,
    /// Its branches are folded away.
    pub folded: bool,
    /// The line from its parent, as SVG path data; empty for the centre.
    pub edge: String,
}

/// A relation, summary or boundary placed on the map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct MindExtra {
    /// Its place among the note's extras, which edits name it by.
    pub index: u32,
    /// `relation`, `summary` or `boundary`.
    pub kind: String,
    pub text: String,
    /// The topics it joins (a relation, from `first` to `last`) or takes
    /// in (the first and last of a run of siblings), by path.
    pub first: String,
    pub last: String,
    /// The relation's curve, the summary's brace or the boundary's frame,
    /// as SVG path data.
    pub path: String,
    /// The box its text goes in; empty when it has no text.
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// How a map is coloured (`#rrggbb`), as XMind's themes colour it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct MindMapTheme {
    /// `classic`, `ocean`, `forest`, `sunset`, `mono` or `dark`.
    pub id: String,
    pub background: String,
    /// The centre topic's fill and text.
    pub centre: String,
    pub centre_text: String,
    /// Main topics filled with their branch's colour (text in
    /// `centre_text`), or outlined in it on `topic`.
    pub filled: bool,
    pub topic: String,
    /// Text on outlined topics and under them.
    pub text: String,
    /// Relations' and summaries' lines.
    pub line: String,
    /// The main branches' colours, in turn: branch `0.n` takes the n-th.
    pub branches: Vec<String>,
}

impl From<&MapTheme> for MindMapTheme {
    fn from(t: &MapTheme) -> Self {
        Self {
            id: t.id.to_string(),
            background: t.background.to_string(),
            centre: t.centre.to_string(),
            centre_text: t.centre_text.to_string(),
            filled: t.filled,
            topic: t.topic.to_string(),
            text: t.text.to_string(),
            line: t.line.to_string(),
            branches: t.branches.iter().map(|b| (*b).to_string()).collect(),
        }
    }
}

/// A marker a topic can carry: XMind's ID for it, the emoji a note keeps,
/// and its group (one of a group per topic).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct MindMarker {
    pub id: String,
    pub emoji: String,
    /// `priority`, `task`, `flag`, `star` or `symbol`.
    pub group: String,
}

/// The markers topics can carry, in the order they show.
#[uniffi::export]
pub fn mindmap_markers() -> Vec<MindMarker> {
    MAP_MARKERS
        .iter()
        .map(|m| MindMarker { id: m.id.to_string(), emoji: m.emoji.to_string(), group: m.group.to_string() })
        .collect()
}

/// The themes a map can take, the default first.
#[uniffi::export]
pub fn mindmap_themes() -> Vec<MindMapTheme> {
    MAP_THEMES.iter().map(MindMapTheme::from).collect()
}

/// The theme a note asks for in its `mindmap-theme` property.
fn theme_of(text: &str) -> &'static MapTheme {
    let parsed = paperloom_vault::parse(text);
    let named = parsed.properties.iter().find(|(key, _)| key.eq_ignore_ascii_case("mindmap-theme"));
    MapTheme::named(named.map_or("", |(_, value)| value.as_str()))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct MindMapLayout {
    /// The centre first.
    pub topics: Vec<MindTopic>,
    /// Relations, summaries and boundaries.
    pub extras: Vec<MindExtra>,
    pub width: f64,
    pub height: f64,
    /// `logic`, `right`, `org`, `tree`, `timeline` or `fishbone`.
    pub style: String,
    /// The colours, from the note's `mindmap-theme` property.
    pub theme: MindMapTheme,
}

/// A change to a map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Enum)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum MindMapEdit {
    AddChild {
        id: String,
        text: String,
    },
    AddSibling {
        id: String,
        text: String,
    },
    Rename {
        id: String,
        text: String,
    },
    SetNote {
        id: String,
        note: String,
    },
    Delete {
        id: String,
    },
    Move {
        id: String,
        parent: String,
        index: u32,
    },
    /// A line from one topic to another.
    Relate {
        from: String,
        to: String,
        text: String,
    },
    /// A summary beside the siblings from `first` to `last`.
    Summarize {
        first: String,
        last: String,
        text: String,
    },
    /// A frame around the siblings from `first` to `last`.
    Enclose {
        first: String,
        last: String,
        text: String,
    },
    /// Puts a marker (`priority-1`, `task-done`…, see `mindmap_markers`)
    /// on a topic, or takes it off.
    ToggleMarker {
        id: String,
        marker: String,
    },
    /// Links a topic to a web page or a note (by its name), or takes the
    /// link off when `link` is empty.
    SetLink {
        id: String,
        link: String,
    },
    /// Puts these labels on a topic in place of its own.
    SetLabels {
        id: String,
        labels: Vec<String>,
    },
    /// New text for a relation, summary or boundary, by its `index`.
    RetextExtra {
        index: u32,
        text: String,
    },
    RemoveExtra {
        index: u32,
    },
}

/// A note after a change, and the topic to show next (empty: the one shown
/// stays).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct MindMapChange {
    pub text: String,
    pub focus: String,
}

/// The style a note asks for in its `mindmap` property; none when the note
/// is not a map.
fn style_of(text: &str) -> Option<MapStyle> {
    paperloom_vault::parse(text)
        .properties
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case("mindmap"))
        .map(|(_, value)| MapStyle::parse(value))
}

/// Whether a note is a mind map (it has a `mindmap` property).
#[uniffi::export]
pub fn is_mindmap(text: String) -> bool {
    style_of(&text).is_some()
}

/// A note's outline laid out as a map: `style` (`logic`, `right`, `org`,
/// `tree`, `timeline`, `fishbone`) or,
/// empty, the note's own; the topics in `folded` hide their branches.
#[uniffi::export]
pub fn mindmap_layout(text: String, title: String, style: String, folded: Vec<String>) -> MindMapLayout {
    let style = if style.trim().is_empty() {
        style_of(&text).unwrap_or(MapStyle::Logic)
    } else {
        MapStyle::parse(&style)
    };
    let map = read_map(&text, &title);
    let layout = layout_map(&map, &read_map_extras(&text), style, &folded);
    MindMapLayout {
        topics: layout
            .topics
            .into_iter()
            .map(|t| MindTopic {
                id: t.id,
                parent: t.parent,
                text: t.text,
                note: t.note,
                web_link: is_web_link(&t.link),
                link: t.link,
                labels: t.labels,
                depth: t.depth as u32,
                side: i32::from(t.side),
                x: t.x,
                y: t.y,
                width: t.width,
                height: t.height,
                font_size: t.font_size,
                children: t.children as u32,
                folded: t.folded,
                edge: t.edge,
            })
            .collect(),
        extras: layout
            .extras
            .into_iter()
            .map(|e| MindExtra {
                index: e.index as u32,
                kind: e.kind.as_str().to_string(),
                text: e.text,
                first: e.first,
                last: e.last,
                path: e.path,
                x: e.x,
                y: e.y,
                width: e.width,
                height: e.height,
            })
            .collect(),
        width: layout.width,
        height: layout.height,
        style: style.as_str().to_string(),
        theme: MindMapTheme::from(theme_of(&text)),
    }
}

/// A change to the topics, or to the relations, summaries and boundaries
/// (with the topic to show next).
enum Change {
    Topics(MapEdit),
    Extras(MapExtraEdit, String),
}

impl From<MindMapEdit> for Change {
    fn from(edit: MindMapEdit) -> Self {
        match edit {
            MindMapEdit::AddChild { id, text } => Change::Topics(MapEdit::AddChild { id, text }),
            MindMapEdit::AddSibling { id, text } => Change::Topics(MapEdit::AddSibling { id, text }),
            MindMapEdit::Rename { id, text } => Change::Topics(MapEdit::Rename { id, text }),
            MindMapEdit::SetNote { id, note } => Change::Topics(MapEdit::SetNote { id, note }),
            MindMapEdit::Delete { id } => Change::Topics(MapEdit::Delete { id }),
            MindMapEdit::Move { id, parent, index } => {
                Change::Topics(MapEdit::Move { id, parent, index: index as usize })
            }
            MindMapEdit::ToggleMarker { id, marker } => Change::Topics(MapEdit::ToggleMarker { id, marker }),
            MindMapEdit::SetLink { id, link } => Change::Topics(MapEdit::SetLink { id, link }),
            MindMapEdit::SetLabels { id, labels } => Change::Topics(MapEdit::SetLabels { id, labels }),
            MindMapEdit::Relate { from, to, text } => {
                Change::Extras(MapExtraEdit::Relate { from: from.clone(), to, text }, from)
            }
            MindMapEdit::Summarize { first, last, text } => {
                Change::Extras(MapExtraEdit::Summarize { first: first.clone(), last, text }, first)
            }
            MindMapEdit::Enclose { first, last, text } => {
                Change::Extras(MapExtraEdit::Enclose { first: first.clone(), last, text }, first)
            }
            MindMapEdit::RetextExtra { index, text } => {
                Change::Extras(MapExtraEdit::Retext { index: index as usize, text }, String::new())
            }
            MindMapEdit::RemoveExtra { index } => {
                Change::Extras(MapExtraEdit::Remove { index: index as usize }, String::new())
            }
        }
    }
}

/// Changes a map; the note's new text and the topic to show next.
#[uniffi::export]
pub fn mindmap_edit(text: String, title: String, edit: MindMapEdit) -> Result<MindMapChange, CoreError> {
    let mut map = read_map(&text, &title);
    let invalid = |message| CoreError::InvalidInput { message };
    match Change::from(edit) {
        Change::Topics(edit) => {
            let focus = edit_map(&mut map, &edit).map_err(invalid)?;
            Ok(MindMapChange { text: write_map(&text, &map), focus })
        }
        Change::Extras(edit, focus) => {
            let mut extras = read_map_extras(&text);
            edit_map_extras(&mut map, &mut extras, &edit).map_err(invalid)?;
            Ok(MindMapChange { text: write_map_with(&text, &map, &extras), focus })
        }
    }
}

/// The text of a new map note with this title (a `logic` map).
#[uniffi::export]
pub fn new_mindmap_text(title: String) -> String {
    new_map(&title, MapStyle::Logic)
}

/// A map note as an SVG picture, laid out as `mindmap_layout` would.
#[uniffi::export]
pub fn mindmap_svg(text: String, title: String, style: String, folded: Vec<String>) -> String {
    let style = if style.trim().is_empty() {
        style_of(&text).unwrap_or(MapStyle::Logic)
    } else {
        MapStyle::parse(&style)
    };
    map_svg(&read_map(&text, &title), &read_map_extras(&text), style, &folded, theme_of(&text))
}

fn extension(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase()
}

/// The map in another app's file, by its extension, its layout, and (from
/// XMind) its relations, summaries and boundaries.
fn read_map_file(path: &Path) -> Result<(Topic, MapStyle, Vec<MapExtra>), CoreError> {
    let bytes = std::fs::read(path)
        .map_err(|e| CoreError::Storage { message: format!("Reading {}: {e}", path.display()) })?;
    let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default();
    let text = || String::from_utf8_lossy(&bytes).into_owned();
    let read = match extension(path).as_str() {
        "xmind" => match unzip::file(&bytes, "content.json") {
            Some(json) => map_from_xmind_json(&String::from_utf8_lossy(&json)),
            None => unzip::file(&bytes, "content.xml")
                .and_then(|xml| map_from_xmind_xml(&String::from_utf8_lossy(&xml))),
        },
        "mm" => map_from_freemind(&text()).map(|map| (map, MapStyle::Logic, Vec::new())),
        _ => map_from_opml(&text(), name).map(|map| (map, MapStyle::Logic, Vec::new())),
    };
    let (mut map, style, extras) = read.ok_or_else(|| CoreError::InvalidInput {
        message: format!("{} is not a mind map Paperloom can read", path.display()),
    })?;
    if map.text.trim().is_empty() {
        map.text = name.to_string();
    }
    Ok((map, style, extras))
}

/// Writes a map note as another app's file, by the destination's
/// extension: `.opml`, `.mm` (FreeMind), `.xmind` or `.svg`.
#[uniffi::export]
pub fn export_mindmap(text: String, title: String, destination: String) -> Result<(), CoreError> {
    let map = read_map(&text, &title);
    let style = style_of(&text).unwrap_or(MapStyle::Logic);
    let path = Path::new(&destination);
    let failed = |e: std::io::Error| CoreError::Storage { message: format!("Saving {destination}: {e}") };
    match extension(path).as_str() {
        "xmind" => {
            let mut zip = ZipWriter::create(path).map_err(failed)?;
            let json = map_to_xmind_json(&map, &read_map_extras(&text), style);
            zip.add_bytes("content.json", json.as_bytes()).map_err(failed)?;
            zip.add_bytes("metadata.json", br#"{"creator":{"name":"Paperloom"}}"#).map_err(failed)?;
            zip.add_bytes("manifest.json", br#"{"file-entries":{"content.json":{},"metadata.json":{}}}"#)
                .map_err(failed)?;
            zip.finish().map_err(failed)?;
            Ok(())
        }
        "mm" => std::fs::write(path, map_to_freemind(&map)).map_err(failed),
        "opml" | "xml" => std::fs::write(path, map_to_opml(&map)).map_err(failed),
        "svg" => std::fs::write(path, map_svg(&map, &read_map_extras(&text), style, &[], theme_of(&text)))
            .map_err(failed),
        other => {
            Err(CoreError::InvalidInput { message: format!("Mind maps can't be saved as .{other} files") })
        }
    }
}

#[uniffi::export]
impl PaperVault {
    /// Makes a map note in `folder` from an OPML, FreeMind (`.mm`) or XMind
    /// file, named after its central topic.
    pub fn import_mindmap(&self, folder: String, source: String) -> Result<NoteInfo, CoreError> {
        let (map, style, extras) = read_map_file(Path::new(&source))?;
        let created = self.create_note(folder, map.text.clone())?;
        self.write_note(created.path, write_map_with(&new_map(&map.text, style), &map, &extras))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_and_lays_out_a_note() {
        let text = new_mindmap_text("Thermometry".into());
        assert!(is_mindmap(text.clone()) && !is_mindmap("# Plain".into()));
        let change = mindmap_edit(
            text,
            "Ignored".into(),
            MindMapEdit::AddChild { id: "0".into(), text: "Methods".into() },
        )
        .unwrap();
        assert_eq!(change.focus, "0.0");
        assert!(change.text.ends_with("# Thermometry\n\n- Methods\n"), "{}", change.text);
        let layout = mindmap_layout(change.text.clone(), String::new(), String::new(), vec![]);
        assert_eq!(layout.style, "logic");
        assert_eq!(
            layout.topics.iter().map(|t| t.text.as_str()).collect::<Vec<_>>(),
            ["Thermometry", "Methods"]
        );
        assert_eq!(mindmap_layout(change.text.clone(), String::new(), "org".into(), vec![]).style, "org");
        assert_eq!(layout.theme.id, "classic");
        let dark = change.text.replace("mindmap: logic", "mindmap: logic\nmindmap-theme: dark");
        let themed = mindmap_layout(dark.clone(), String::new(), String::new(), vec![]);
        assert_eq!((themed.theme.id.as_str(), themed.theme.branches.len()), ("dark", 6));
        assert!(
            mindmap_svg(dark.clone(), String::new(), String::new(), vec![])
                .contains(&themed.theme.background)
        );
        assert_eq!(mindmap_themes().len(), 6);
        let marked = mindmap_edit(
            dark,
            String::new(),
            MindMapEdit::ToggleMarker { id: "0.0".into(), marker: "task-done".into() },
        )
        .unwrap();
        assert!(marked.text.contains("- ✅ Methods"), "{}", marked.text);
        let linked = mindmap_edit(
            marked.text.clone(),
            String::new(),
            MindMapEdit::SetLink { id: "0.0".into(), link: "https://example.com".into() },
        )
        .unwrap();
        assert!(linked.text.contains("- ✅ [Methods](https://example.com)"), "{}", linked.text);
        let layout = mindmap_layout(linked.text.clone(), String::new(), String::new(), vec![]);
        let topic = &layout.topics[1];
        assert_eq!(
            (topic.text.as_str(), topic.link.as_str(), topic.web_link),
            ("✅ Methods", "https://example.com", true)
        );
        let labelled = mindmap_edit(
            linked.text,
            String::new(),
            MindMapEdit::SetLabels { id: "0.0".into(), labels: vec!["to read".into()] },
        )
        .unwrap();
        assert!(labelled.text.contains("- ✅ [Methods](https://example.com) #to-read"), "{}", labelled.text);
        let layout = mindmap_layout(labelled.text, String::new(), String::new(), vec![]);
        let topic = &layout.topics[1];
        assert_eq!((topic.text.as_str(), topic.labels.clone()), ("✅ Methods", vec!["to-read".to_string()]));
        assert_eq!(mindmap_markers()[0].id, "priority-1");
        assert!(matches!(
            mindmap_edit(change.text, String::new(), MindMapEdit::Delete { id: "0".into() }),
            Err(CoreError::InvalidInput { .. })
        ));
    }

    #[test]
    fn reads_deflated_xmind_files() {
        let branch = Topic { text: "Branch".into(), ..Topic::default() };
        let map = Topic { text: "Centre".into(), children: vec![branch], ..Topic::default() };
        let json = map_to_xmind_json(&map, &[], MapStyle::Right);
        let zip = unzip::tests::deflated_zip(&[("content.json", json.as_bytes())]);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Plan.xmind");
        std::fs::write(&path, &zip).unwrap();
        assert_eq!(read_map_file(&path).unwrap(), (map, MapStyle::Right, vec![]));
    }

    #[test]
    fn exports_and_imports_other_apps_files() {
        let dir = tempfile::tempdir().unwrap();
        let vault = PaperVault::open(dir.path().join("Vault").display().to_string()).unwrap();
        let add = MindMapEdit::AddChild { id: "0".into(), text: "Step & go".into() };
        let text = mindmap_edit(new_mindmap_text("Plan".into()), String::new(), add).unwrap().text;
        let text = text.replace("mindmap: logic", "mindmap: org");
        for name in ["plan.opml", "plan.mm", "plan.xmind"] {
            let file = dir.path().join(name).display().to_string();
            export_mindmap(text.clone(), String::new(), file.clone()).unwrap();
            let note = vault.import_mindmap(String::new(), file).unwrap();
            let imported = vault.read_note(note.path).unwrap();
            assert!(is_mindmap(imported.clone()), "{name}: {imported}");
            assert!(imported.ends_with("# Plan\n\n- Step & go\n"), "{name}: {imported}");
            // Only XMind keeps the layout.
            assert_eq!(imported.contains("mindmap: org"), name.ends_with(".xmind"), "{name}: {imported}");
        }
        let svg = dir.path().join("plan.svg");
        export_mindmap(text.clone(), String::new(), svg.display().to_string()).unwrap();
        assert!(std::fs::read_to_string(&svg).unwrap().contains(">Step &amp; go</tspan>"));
        assert!(mindmap_svg(text.clone(), String::new(), String::new(), vec![]).contains("<path "));
        let pdf = dir.path().join("plan.pdf").display().to_string();
        assert!(matches!(export_mindmap(text, String::new(), pdf), Err(CoreError::InvalidInput { .. })));
    }
}
