//! Drawings for the apps: hand-drawn pages kept as Excalidraw scenes
//! (`.excalidraw`) in the vault. The apps draw strokes, shapes, lines and
//! text; what they do not draw stays in the file as it was.

use std::collections::HashMap;

use paperloom_vault::{DrawElement, Drawing, drawing_svg, picture_id};
use serde::{Deserialize, Serialize};

use crate::{CoreError, PaperVault};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, uniffi::Record)]
pub struct DrawingPoint {
    pub x: f64,
    pub y: f64,
}

/// One element of a drawing. Points are relative to `x`, `y`; a stroke is
/// `stroke_width × 4.25 × (0.4 + 1.2 × pressure)` wide, as Excalidraw
/// draws it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct DrawingItem {
    pub id: String,
    /// `freedraw`, `line`, `arrow`, `rectangle`, `ellipse`, `diamond` or
    /// `text`.
    pub kind: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// Radians, clockwise, about the centre.
    pub angle: f64,
    pub stroke_color: String,
    /// `transparent` or a colour.
    pub background_color: String,
    pub stroke_width: f64,
    /// 0–100.
    pub opacity: f64,
    pub points: Vec<DrawingPoint>,
    /// 0–1 for each point of a stroke; empty when the pen had none.
    pub pressures: Vec<f64>,
    pub text: String,
    pub font_size: f64,
    /// See-through, under the other strokes.
    pub highlighter: bool,
    /// An arrowhead at the end of a line.
    pub arrowhead: bool,
    /// A picture's `data:` URL; empty for anything else.
    #[serde(default)]
    #[uniffi(default = "")]
    pub image: String,
}

/// A drawing: its items, bottom first, and its paper (`blank`, `lines`,
/// `grid` or `dots`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, uniffi::Record)]
pub struct DrawingPage {
    pub items: Vec<DrawingItem>,
    pub paper: String,
}

impl From<DrawElement> for DrawingItem {
    fn from(e: DrawElement) -> Self {
        DrawingItem {
            id: e.id,
            kind: e.kind,
            x: e.x,
            y: e.y,
            width: e.width,
            height: e.height,
            angle: e.angle,
            stroke_color: e.stroke_color,
            background_color: e.background_color,
            stroke_width: e.stroke_width,
            opacity: e.opacity,
            points: e.points.into_iter().map(|(x, y)| DrawingPoint { x, y }).collect(),
            pressures: e.pressures,
            text: e.text,
            font_size: e.font_size,
            highlighter: e.highlighter,
            arrowhead: e.arrowhead,
            image: String::new(),
        }
    }
}

impl From<DrawingItem> for DrawElement {
    fn from(i: DrawingItem) -> Self {
        DrawElement {
            id: i.id,
            kind: i.kind,
            x: i.x,
            y: i.y,
            width: i.width,
            height: i.height,
            angle: i.angle,
            stroke_color: i.stroke_color,
            background_color: i.background_color,
            stroke_width: i.stroke_width,
            opacity: i.opacity,
            points: i.points.into_iter().map(|p| (p.x, p.y)).collect(),
            pressures: i.pressures,
            text: i.text,
            font_size: i.font_size,
            highlighter: i.highlighter,
            arrowhead: i.arrowhead,
            file_id: if i.image.is_empty() { String::new() } else { picture_id(&i.image) },
        }
    }
}

impl From<Drawing> for DrawingPage {
    fn from(drawing: Drawing) -> Self {
        let Drawing { elements, paper, files } = drawing;
        let items = elements
            .into_iter()
            .map(|e| {
                let image = files.get(&e.file_id).cloned().unwrap_or_default();
                DrawingItem { image, ..e.into() }
            })
            .collect();
        DrawingPage { items, paper }
    }
}

impl From<DrawingPage> for Drawing {
    fn from(page: DrawingPage) -> Self {
        let files: HashMap<String, String> = page
            .items
            .iter()
            .filter(|i| !i.image.is_empty())
            .map(|i| (picture_id(&i.image), i.image.clone()))
            .collect();
        Drawing { elements: page.items.into_iter().map(Into::into).collect(), paper: page.paper, files }
    }
}

fn distance(a: DrawingPoint, b: DrawingPoint) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

/// How far a point is from the segment between two others.
fn segment_distance(p: DrawingPoint, a: DrawingPoint, b: DrawingPoint) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let length = dx * dx + dy * dy;
    if length == 0.0 {
        return distance(p, a);
    }
    let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / length).clamp(0.0, 1.0);
    distance(p, DrawingPoint { x: a.x + t * dx, y: a.y + t * dy })
}

/// What erasing along `path` leaves of one stroke: the runs of it farther
/// than `radius` from the path, each a stroke of its own.
fn erase_stroke(item: DrawingItem, path: &[DrawingPoint], radius: f64) -> Vec<DrawingItem> {
    // The stroke on the page (turned as it is drawn), its points closer
    // together than half the eraser, so that no cut falls between them.
    let (cx, cy) = (item.x + item.width / 2.0, item.y + item.height / 2.0);
    let (sin, cos) = item.angle.sin_cos();
    let on_page = |p: &DrawingPoint| {
        let (x, y) = (item.x + p.x - cx, item.y + p.y - cy);
        DrawingPoint { x: cx + x * cos - y * sin, y: cy + x * sin + y * cos }
    };
    let pressured = item.pressures.len() == item.points.len();
    let step = (radius / 2.0).max(0.5);
    let mut points: Vec<DrawingPoint> = Vec::new();
    let mut pressures: Vec<f64> = Vec::new();
    for (i, p) in item.points.iter().enumerate() {
        let here = on_page(p);
        if let (Some(&last), Some(&last_pressure)) = (points.last(), pressures.last().or(Some(&0.0))) {
            let between = (distance(last, here) / step).floor() as usize;
            for k in 1..=between {
                let t = k as f64 / (between + 1) as f64;
                points.push(DrawingPoint {
                    x: last.x + (here.x - last.x) * t,
                    y: last.y + (here.y - last.y) * t,
                });
                if pressured {
                    pressures.push(last_pressure + (item.pressures[i] - last_pressure) * t);
                }
            }
        }
        points.push(here);
        if pressured {
            pressures.push(item.pressures[i]);
        }
    }
    let rubbed = |p: DrawingPoint| match path {
        [] => false,
        [only] => distance(p, *only) <= radius,
        _ => path.windows(2).any(|w| segment_distance(p, w[0], w[1]) <= radius),
    };
    if !points.iter().any(|&p| rubbed(p)) {
        return vec![item];
    }
    let mut runs: Vec<(Vec<DrawingPoint>, Vec<f64>)> = Vec::new();
    let mut run: (Vec<DrawingPoint>, Vec<f64>) = (Vec::new(), Vec::new());
    for (i, &p) in points.iter().enumerate() {
        if rubbed(p) {
            if !run.0.is_empty() {
                runs.push(std::mem::take(&mut run));
            }
            continue;
        }
        run.0.push(p);
        if pressured {
            run.1.push(pressures[i]);
        }
    }
    if !run.0.is_empty() {
        runs.push(run);
    }
    runs.into_iter()
        .enumerate()
        .map(|(n, (points, pressures))| {
            let left = points.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
            let top = points.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
            let right = points.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max);
            let bottom = points.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max);
            DrawingItem {
                // The first part stays the stroke it was; the others are new.
                id: if n == 0 { item.id.clone() } else { paperloom_model::ItemId::new().to_string() },
                x: left,
                y: top,
                width: right - left,
                height: bottom - top,
                angle: 0.0,
                points: points.iter().map(|p| DrawingPoint { x: p.x - left, y: p.y - top }).collect(),
                pressures,
                ..item.clone()
            }
        })
        .collect()
}

/// What erasing along a path leaves, as OneNote's point eraser rubs out:
/// the parts of each pen and highlighter stroke more than `radius` from
/// the path, each a stroke of its own (the first keeps the stroke's ID);
/// shapes, lines and text as they are.
#[uniffi::export]
pub fn drawing_erase(items: Vec<DrawingItem>, path: Vec<DrawingPoint>, radius: f64) -> Vec<DrawingItem> {
    let mut left = Vec::with_capacity(items.len());
    for item in items {
        if item.kind == "freedraw" && !item.points.is_empty() {
            left.extend(erase_stroke(item, &path, radius));
        } else {
            left.push(item);
        }
    }
    left
}

#[uniffi::export]
impl PaperVault {
    /// The vault's drawings (`.excalidraw` files), by path.
    pub fn drawings(&self) -> Result<Vec<String>, CoreError> {
        Ok(self.lock()?.drawings().cloned().collect())
    }

    pub fn read_drawing(&self, path: String) -> Result<DrawingPage, CoreError> {
        Ok(self.lock()?.read_drawing(&path)?.into())
    }

    /// Saves a drawing; what it held that this app does not draw (images,
    /// frames, fields other apps wrote) stays.
    pub fn write_drawing(&self, path: String, page: DrawingPage) -> Result<(), CoreError> {
        Ok(self.lock()?.write_drawing(&path, &page.into())?)
    }

    /// Makes an empty drawing on `paper` in `folder`; its path.
    pub fn create_drawing(&self, folder: String, name: String, paper: String) -> Result<String, CoreError> {
        Ok(self.lock()?.create_drawing(&folder, &name, &paper)?)
    }

    /// Keeps a file the app made (a recording) in `folder` as `name`, or
    /// `name 2`, … when taken; its path, for `![[…]]`.
    pub fn add_file(&self, folder: String, name: String, data: Vec<u8>) -> Result<String, CoreError> {
        Ok(self.lock()?.add_file(&folder, &name, &data)?)
    }

    /// A drawing as an SVG picture, cropped to what is drawn.
    pub fn drawing_svg(&self, path: String) -> Result<String, CoreError> {
        Ok(drawing_svg(&self.lock()?.read_drawing(&path)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drawings_go_through_the_vault() {
        let dir = tempfile::tempdir().unwrap();
        let vault = PaperVault::open(dir.path().join("Vault").display().to_string()).unwrap();
        let path = vault.create_drawing("".into(), "Apparatus".into(), "grid".into()).unwrap();
        assert_eq!(vault.drawings().unwrap(), std::slice::from_ref(&path));
        let stroke = DrawingItem {
            id: "s1".into(),
            kind: "freedraw".into(),
            x: 10.0,
            y: 10.0,
            width: 20.0,
            height: 5.0,
            angle: 0.0,
            stroke_color: "#1971c2".into(),
            background_color: "transparent".into(),
            stroke_width: 0.5,
            opacity: 100.0,
            points: vec![DrawingPoint { x: 0.0, y: 0.0 }, DrawingPoint { x: 20.0, y: 5.0 }],
            pressures: vec![0.3, 0.8],
            text: String::new(),
            font_size: 20.0,
            highlighter: false,
            arrowhead: false,
            image: String::new(),
        };
        let page = DrawingPage { items: vec![stroke], paper: "grid".into() };
        vault.write_drawing(path.clone(), page.clone()).unwrap();
        assert_eq!(vault.read_drawing(path.clone()).unwrap(), page);
        assert!(vault.drawing_svg(path.clone()).unwrap().contains("stroke=\"#1971c2\""));
        let text = std::fs::read_to_string(dir.path().join("Vault").join(&path)).unwrap();
        assert!(
            text.contains("\"type\": \"excalidraw\"") && text.contains("\"type\": \"freedraw\""),
            "{text}"
        );
        vault.delete_note(path).unwrap();
        assert!(vault.drawings().unwrap().is_empty());
    }

    fn line(id: &str, kind: &str) -> DrawingItem {
        DrawingItem {
            id: id.into(),
            kind: kind.into(),
            x: 0.0,
            y: 10.0,
            width: 100.0,
            height: 0.0,
            angle: 0.0,
            stroke_color: "#1e1e1e".into(),
            background_color: "transparent".into(),
            stroke_width: 0.6,
            opacity: 100.0,
            points: vec![DrawingPoint { x: 0.0, y: 0.0 }, DrawingPoint { x: 100.0, y: 0.0 }],
            pressures: vec![0.2, 0.8],
            text: String::new(),
            font_size: 20.0,
            highlighter: false,
            arrowhead: false,
            image: String::new(),
        }
    }

    #[test]
    fn rubs_out_parts_of_strokes() {
        // Down across the middle of a stroke from (0, 10) to (100, 10).
        let path = vec![DrawingPoint { x: 50.0, y: 0.0 }, DrawingPoint { x: 50.0, y: 20.0 }];
        let left = drawing_erase(vec![line("s", "freedraw"), line("l", "line")], path.clone(), 5.0);
        assert_eq!(left.len(), 3, "two halves of the stroke, and the line untouched");
        assert_eq!(left[0].id, "s");
        assert_ne!(left[1].id, "s");
        assert_eq!(left[2], line("l", "line"));
        assert!(left[0].x.abs() < 1e-9 && left[0].x + left[0].width < 46.0);
        assert!(left[1].x > 54.0 && (left[1].x + left[1].width - 100.0).abs() < 1e-9);
        assert_eq!(left[0].pressures.len(), left[0].points.len());
        assert!((left[0].y - 10.0).abs() < 1e-9 && left[0].points.iter().all(|p| p.y.abs() < 1e-9));

        // Away from the stroke, nothing changes; across its end, one part is left.
        let far = vec![DrawingPoint { x: 50.0, y: 60.0 }];
        assert_eq!(drawing_erase(vec![line("s", "freedraw")], far, 5.0), [line("s", "freedraw")]);
        let end = vec![DrawingPoint { x: 100.0, y: 10.0 }];
        assert_eq!(drawing_erase(vec![line("s", "freedraw")], end, 5.0).len(), 1);
        let all = vec![DrawingPoint { x: -10.0, y: 10.0 }, DrawingPoint { x: 110.0, y: 10.0 }];
        assert!(drawing_erase(vec![line("s", "freedraw")], all, 5.0).is_empty());
    }
}
