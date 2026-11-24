//! Canvases: boards of cards (text, notes and other files, web pages,
//! groups) joined by arrows, kept as JSON Canvas files (`.canvas`), the way
//! Obsidian writes them. Fields this app does not know are kept as they
//! were.

use std::fs;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Map, Value};

use crate::{Error, Result, Vault, checked, io_error};

/// A whole canvas.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Canvas {
    #[serde(default)]
    pub nodes: Vec<CanvasNode>,
    #[serde(default)]
    pub edges: Vec<CanvasEdge>,
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

/// A card: `text` (Markdown), `file` (a note or other file in the vault),
/// `link` (a web page) or `group` (a labelled area around others).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CanvasNode {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(deserialize_with = "whole")]
    pub x: i64,
    #[serde(deserialize_with = "whole")]
    pub y: i64,
    #[serde(deserialize_with = "whole")]
    pub width: i64,
    #[serde(deserialize_with = "whole")]
    pub height: i64,
    /// `1`–`6` (Obsidian's colours) or `#rrggbb`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// A vault path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    /// `#heading` or `#^block` in the file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subpath: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// A group's name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

/// An arrow between two cards; sides are `top`, `right`, `bottom` or
/// `left`, ends `none` or `arrow`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanvasEdge {
    pub id: String,
    pub from_node: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_side: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_end: Option<String>,
    pub to_node: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_side: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to_end: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(flatten)]
    pub other: Map<String, Value>,
}

/// A position or size: whole pixels, though some apps write fractions.
fn whole<'de, D: Deserializer<'de>>(deserializer: D) -> std::result::Result<i64, D::Error> {
    let number = f64::deserialize(deserializer)?;
    Ok(number.round() as i64)
}

impl Canvas {
    pub fn parse(text: &str) -> std::result::Result<Canvas, String> {
        if text.trim().is_empty() {
            return Ok(Canvas::default());
        }
        serde_json::from_str(text).map_err(|e| e.to_string())
    }

    /// The canvas as JSON Canvas, indented with tabs as Obsidian writes it.
    pub fn to_json(&self) -> String {
        let mut out = Vec::new();
        let mut writer = serde_json::Serializer::with_formatter(
            &mut out,
            serde_json::ser::PrettyFormatter::with_indent(b"\t"),
        );
        // Writing plain data into memory cannot fail.
        let _ = self.serialize(&mut writer);
        String::from_utf8(out).unwrap_or_default()
    }

    /// This canvas with what `older` knew beyond the known fields: the
    /// unknown fields of the canvas, and of each card and arrow still here.
    pub fn keeping_unknown(mut self, older: &Canvas) -> Canvas {
        if self.other.is_empty() {
            self.other = older.other.clone();
        }
        for node in &mut self.nodes {
            if node.other.is_empty()
                && let Some(old) = older.nodes.iter().find(|n| n.id == node.id)
            {
                node.other = old.other.clone();
            }
        }
        for edge in &mut self.edges {
            if edge.other.is_empty()
                && let Some(old) = older.edges.iter().find(|e| e.id == edge.id)
            {
                edge.other = old.other.clone();
            }
        }
        self
    }

    /// The vault paths of the files on the canvas.
    pub fn files(&self) -> impl Iterator<Item = &str> {
        self.nodes.iter().filter_map(|n| n.file.as_deref())
    }
}

/// `Board.canvas` for `Board`, and any path ending `.canvas` as it is.
fn checked_canvas(path: &str) -> Result<String> {
    let path = checked(path)?;
    Ok(if path.to_lowercase().ends_with(".canvas") { path } else { format!("{path}.canvas") })
}

impl Vault {
    /// The canvases in the vault, by path.
    pub fn canvases(&self) -> impl Iterator<Item = &String> {
        self.files.iter().filter(|f| f.to_lowercase().ends_with(".canvas"))
    }

    pub fn read_canvas(&self, path: &str) -> Result<Canvas> {
        let path = checked_canvas(path)?;
        let full = self.full(&path);
        let text = fs::read_to_string(&full).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Error::NotFound(path.clone()),
            _ => io_error(&full, e),
        })?;
        Canvas::parse(&text).map_err(|message| Error::Invalid(format!("{path}: {message}")))
    }

    /// Saves a canvas (creating it and its folders if needed), keeping the
    /// fields of the one there that this app does not know. Replaced in one
    /// step, like notes.
    pub fn write_canvas(&mut self, path: &str, canvas: Canvas) -> Result<()> {
        let path = checked_canvas(path)?;
        let canvas = match self.read_canvas(&path) {
            Ok(older) => canvas.keeping_unknown(&older),
            Err(_) => canvas,
        };
        let full = self.full(&path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
            self.add_folders(&path);
        }
        let temporary = full.with_extension("canvas.paperloom-tmp");
        fs::write(&temporary, canvas.to_json()).map_err(|e| io_error(&temporary, e))?;
        fs::rename(&temporary, &full).map_err(|e| io_error(&full, e))?;
        self.files.insert(path);
        Ok(())
    }

    /// Makes an empty canvas in `folder` named `name` (or `name 2`, … when
    /// taken); its path.
    pub fn create_canvas(&mut self, folder: &str, name: &str) -> Result<String> {
        let name = name.trim().trim_end_matches(".canvas");
        let name = if name.is_empty() { "Untitled" } else { name };
        let folder = folder.trim_matches('/');
        let mut n = 1;
        let path = loop {
            let file = if n == 1 { format!("{name}.canvas") } else { format!("{name} {n}.canvas") };
            let path = if folder.is_empty() { file } else { format!("{folder}/{file}") };
            if !self.files.contains(&path) {
                break path;
            }
            n += 1;
        };
        self.write_canvas(&path, Canvas::default())?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OBSIDIAN: &str = r##"{
	"nodes":[
		{"id":"a1","type":"text","text":"# Idea\nNitrogen-vacancy centres","x":-120,"y":-40.4,"width":250,"height":60,"color":"4","styleAttributes":{}},
		{"id":"b2","type":"file","file":"Papers/Kucsko.md","subpath":"#Results","x":200,"y":0,"width":400,"height":300},
		{"id":"c3","type":"group","label":"Reading","x":-200,"y":-100,"width":900,"height":500,"background":"bg.png"}
	],
	"edges":[
		{"id":"e1","fromNode":"a1","fromSide":"right","toNode":"b2","toSide":"left","toEnd":"arrow","label":"see"}
	],
	"metadata":{"version":"1.0"}
}"##;

    #[test]
    fn reads_and_writes_json_canvas() {
        let canvas = Canvas::parse(OBSIDIAN).unwrap();
        assert_eq!(canvas.nodes.len(), 3);
        let text = &canvas.nodes[0];
        assert_eq!((text.kind.as_str(), text.y, text.color.as_deref()), ("text", -40, Some("4")));
        assert_eq!(canvas.nodes[1].subpath.as_deref(), Some("#Results"));
        assert_eq!(canvas.edges[0].to_end.as_deref(), Some("arrow"));
        assert_eq!(canvas.files().collect::<Vec<_>>(), ["Papers/Kucsko.md"]);
        let again = Canvas::parse(&canvas.to_json()).unwrap();
        assert_eq!(again, canvas, "unknown fields survive a round trip");
        assert!(canvas.to_json().contains("\n\t\"nodes\""), "tab-indented like Obsidian");
        assert_eq!(Canvas::parse("  ").unwrap(), Canvas::default());
        assert!(Canvas::parse("{\"nodes\": 3}").is_err());
    }

    #[test]
    fn keeps_what_it_does_not_know_when_saving() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = Vault::open(dir.path()).unwrap();
        let path = vault.create_canvas("Boards", "Ideas").unwrap();
        assert_eq!(path, "Boards/Ideas.canvas");
        assert_eq!(vault.create_canvas("Boards", "Ideas.canvas").unwrap(), "Boards/Ideas 2.canvas");
        assert_eq!(vault.read_canvas("Boards/Ideas").unwrap(), Canvas::default());
        assert_eq!(vault.canvases().collect::<Vec<_>>(), ["Boards/Ideas 2.canvas", "Boards/Ideas.canvas"]);

        vault.write_canvas(&path, Canvas::parse(OBSIDIAN).unwrap()).unwrap();
        // An app that knows only the known fields moves a card.
        let mut moved = vault.read_canvas(&path).unwrap();
        for node in &mut moved.nodes {
            node.other.clear();
        }
        moved.other.clear();
        moved.nodes[0].x = 10;
        vault.write_canvas(&path, moved).unwrap();
        let kept = vault.read_canvas(&path).unwrap();
        assert_eq!(kept.nodes[0].x, 10);
        assert!(kept.nodes[0].other.contains_key("styleAttributes"));
        assert_eq!(kept.nodes[2].other.get("background"), Some(&Value::String("bg.png".into())));
        assert!(kept.other.contains_key("metadata"));
        assert!(matches!(vault.read_canvas("Missing"), Err(Error::NotFound(_))));
        assert!(vault.write_canvas("../Outside.canvas", Canvas::default()).is_err());
    }
}
