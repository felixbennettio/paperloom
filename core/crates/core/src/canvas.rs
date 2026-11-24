//! Canvases for the apps: cards and arrows on a board, kept as JSON Canvas
//! files in the vault (Obsidian opens them too).

use paperloom_vault::{Canvas, CanvasEdge, CanvasNode};
use serde::{Deserialize, Serialize};

use crate::{CoreError, PaperVault};

/// A card on a canvas: `text` (Markdown), `file` (a note or other file in
/// the vault), `link` (a web page) or `group` (a labelled area).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct CanvasCard {
    pub id: String,
    pub kind: String,
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
    /// `1`–`6` (red, orange, yellow, green, cyan, purple) or `#rrggbb`.
    pub color: Option<String>,
    pub text: Option<String>,
    /// A vault path.
    pub file: Option<String>,
    pub subpath: Option<String>,
    pub url: Option<String>,
    pub label: Option<String>,
}

/// An arrow between cards. Sides are `top`, `right`, `bottom` or `left`;
/// ends `none` or `arrow` (by default an arrow at the far end only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct CanvasArrow {
    pub id: String,
    pub from_node: String,
    pub from_side: Option<String>,
    pub from_end: Option<String>,
    pub to_node: String,
    pub to_side: Option<String>,
    pub to_end: Option<String>,
    pub color: Option<String>,
    pub label: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct CanvasBoard {
    pub cards: Vec<CanvasCard>,
    pub arrows: Vec<CanvasArrow>,
}

impl From<Canvas> for CanvasBoard {
    fn from(canvas: Canvas) -> Self {
        let cards = canvas
            .nodes
            .into_iter()
            .map(|n| CanvasCard {
                id: n.id,
                kind: n.kind,
                x: n.x,
                y: n.y,
                width: n.width,
                height: n.height,
                color: n.color,
                text: n.text,
                file: n.file,
                subpath: n.subpath,
                url: n.url,
                label: n.label,
            })
            .collect();
        let arrows = canvas
            .edges
            .into_iter()
            .map(|e| CanvasArrow {
                id: e.id,
                from_node: e.from_node,
                from_side: e.from_side,
                from_end: e.from_end,
                to_node: e.to_node,
                to_side: e.to_side,
                to_end: e.to_end,
                color: e.color,
                label: e.label,
            })
            .collect();
        CanvasBoard { cards, arrows }
    }
}

impl From<CanvasBoard> for Canvas {
    fn from(board: CanvasBoard) -> Self {
        let nodes = board
            .cards
            .into_iter()
            .map(|c| CanvasNode {
                id: c.id,
                kind: c.kind,
                x: c.x,
                y: c.y,
                width: c.width,
                height: c.height,
                color: c.color,
                text: c.text,
                file: c.file,
                subpath: c.subpath,
                url: c.url,
                label: c.label,
                other: Default::default(),
            })
            .collect();
        let edges = board
            .arrows
            .into_iter()
            .map(|a| CanvasEdge {
                id: a.id,
                from_node: a.from_node,
                from_side: a.from_side,
                from_end: a.from_end,
                to_node: a.to_node,
                to_side: a.to_side,
                to_end: a.to_end,
                color: a.color,
                label: a.label,
                other: Default::default(),
            })
            .collect();
        Canvas { nodes, edges, other: Default::default() }
    }
}

#[uniffi::export]
impl PaperVault {
    /// The vault's canvases, by path.
    pub fn canvases(&self) -> Result<Vec<String>, CoreError> {
        Ok(self.lock()?.canvases().cloned().collect())
    }

    pub fn read_canvas(&self, path: String) -> Result<CanvasBoard, CoreError> {
        Ok(self.lock()?.read_canvas(&path)?.into())
    }

    /// Saves a canvas; what it held that this app does not show (fields
    /// other apps wrote) stays.
    pub fn write_canvas(&self, path: String, board: CanvasBoard) -> Result<(), CoreError> {
        Ok(self.lock()?.write_canvas(&path, board.into())?)
    }

    /// Makes an empty canvas in `folder`; its path.
    pub fn create_canvas(&self, folder: String, name: String) -> Result<String, CoreError> {
        Ok(self.lock()?.create_canvas(&folder, &name)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boards_go_through_the_vault() {
        let dir = tempfile::tempdir().unwrap();
        let vault = PaperVault::open(dir.path().join("Vault").display().to_string()).unwrap();
        let path = vault.create_canvas("".into(), "Ideas".into()).unwrap();
        assert_eq!(vault.canvases().unwrap(), std::slice::from_ref(&path));
        let card = |id: &str, x: i64| CanvasCard {
            id: id.into(),
            kind: "text".into(),
            x,
            y: 0,
            width: 240,
            height: 120,
            color: None,
            text: Some(format!("Card {id}")),
            file: None,
            subpath: None,
            url: None,
            label: None,
        };
        let board = CanvasBoard {
            cards: vec![card("a", 0), card("b", 400)],
            arrows: vec![CanvasArrow {
                id: "e".into(),
                from_node: "a".into(),
                from_side: Some("right".into()),
                from_end: None,
                to_node: "b".into(),
                to_side: Some("left".into()),
                to_end: Some("arrow".into()),
                color: Some("4".into()),
                label: None,
            }],
        };
        vault.write_canvas(path.clone(), board.clone()).unwrap();
        assert_eq!(vault.read_canvas(path.clone()).unwrap(), board);
        let text = std::fs::read_to_string(dir.path().join("Vault").join(&path)).unwrap();
        assert!(text.contains("\"fromNode\": \"a\"") && text.contains("\"type\": \"text\""), "{text}");
        vault.delete_note(path.clone()).unwrap();
        assert!(vault.canvases().unwrap().is_empty());
        assert!(dir.path().join("Vault/.trash/Ideas.canvas").exists());
    }
}
