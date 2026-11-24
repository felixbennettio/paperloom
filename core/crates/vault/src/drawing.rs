//! Drawings: hand-drawn pages kept as Excalidraw scenes (`.excalidraw`),
//! which Excalidraw and Obsidian's Excalidraw plugin open as well. The apps
//! draw pen and highlighter strokes, lines, arrows, rectangles, ellipses,
//! diamonds, text and pictures (kept in the scene's `files`, as Excalidraw
//! keeps them); other elements (frames, embeds) and every field this app
//! does not know are kept as they were.
//!
//! A stroke is as wide as Excalidraw draws it: `strokeWidth × 4.25` at half
//! pressure, from 0.4 to 1.6 times that as the pressure goes from 0 to 1.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::fs;

use serde_json::{Map, Value, json};

use crate::history::now_ms;
use crate::{Error, Result, Vault, checked, io_error};

/// The kinds of element the apps draw.
pub const DRAWN_KINDS: [&str; 8] =
    ["freedraw", "line", "arrow", "rectangle", "ellipse", "diamond", "text", "image"];

/// One element of a drawing. Points are relative to `x`, `y`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DrawElement {
    pub id: String,
    /// `freedraw`, `line`, `arrow`, `rectangle`, `ellipse`, `diamond` or
    /// `text`.
    pub kind: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// Radians, clockwise, about the element's centre.
    pub angle: f64,
    pub stroke_color: String,
    /// `transparent` or a colour.
    pub background_color: String,
    pub stroke_width: f64,
    /// 0–100.
    pub opacity: f64,
    pub points: Vec<(f64, f64)>,
    /// 0–1 for each point of a stroke; empty when the pen had none.
    pub pressures: Vec<f64>,
    pub text: String,
    pub font_size: f64,
    /// A highlighter stroke: see-through, under the other strokes.
    pub highlighter: bool,
    /// An arrowhead at the end of a line.
    pub arrowhead: bool,
    /// A picture's file, in the drawing's `files`.
    pub file_id: String,
}

/// A drawing: its elements, bottom first, its paper, and its pictures.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Drawing {
    pub elements: Vec<DrawElement>,
    /// `blank`, `lines`, `grid` or `dots`.
    pub paper: String,
    /// Pictures as `data:` URLs, by file ID.
    pub files: HashMap<String, String>,
}

/// A file ID for a picture: the same picture, the same ID (FNV-1a, in hex).
pub fn picture_id(data_url: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in data_url.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

fn number(value: &Value, key: &str, default: f64) -> f64 {
    value.get(key).and_then(Value::as_f64).unwrap_or(default)
}

fn string(value: &Value, key: &str, default: &str) -> String {
    value.get(key).and_then(Value::as_str).unwrap_or(default).to_string()
}

fn is_deleted(value: &Value) -> bool {
    value.get("isDeleted").and_then(Value::as_bool) == Some(true)
}

fn kind_of(value: &Value) -> &str {
    value.get("type").and_then(Value::as_str).unwrap_or_default()
}

fn element_from(value: &Value, index: usize) -> Option<DrawElement> {
    let kind = kind_of(value);
    if !DRAWN_KINDS.contains(&kind) || is_deleted(value) {
        return None;
    }
    let points = value
        .get("points")
        .and_then(Value::as_array)
        .map(|points| {
            points.iter().filter_map(|p| Some((p.get(0)?.as_f64()?, p.get(1)?.as_f64()?))).collect()
        })
        .unwrap_or_default();
    let pressures = if value.get("simulatePressure").and_then(Value::as_bool) == Some(true) {
        Vec::new()
    } else {
        value
            .get("pressures")
            .and_then(Value::as_array)
            .map(|p| p.iter().filter_map(Value::as_f64).collect())
            .unwrap_or_default()
    };
    let id = string(value, "id", "");
    Some(DrawElement {
        id: if id.is_empty() { format!("paperloom-{index}") } else { id },
        kind: kind.to_string(),
        x: number(value, "x", 0.0),
        y: number(value, "y", 0.0),
        width: number(value, "width", 0.0),
        height: number(value, "height", 0.0),
        angle: number(value, "angle", 0.0),
        stroke_color: string(value, "strokeColor", "#1e1e1e"),
        background_color: string(value, "backgroundColor", "transparent"),
        stroke_width: number(value, "strokeWidth", 2.0),
        opacity: number(value, "opacity", 100.0),
        points,
        pressures,
        text: string(value, "text", ""),
        font_size: number(value, "fontSize", 20.0),
        highlighter: value.pointer("/customData/paperloom").and_then(Value::as_str) == Some("highlighter"),
        arrowhead: value.get("endArrowhead").is_some_and(|head| !head.is_null()),
        file_id: string(value, "fileId", ""),
    })
}

/// Equal as Excalidraw means it: `10` and `10.0` are the same number.
fn same(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(a, b)| same(a, b)),
        _ => a == b,
    }
}

/// Sets a field unless it already holds the same value (as written).
fn set(map: &mut Map<String, Value>, key: &str, value: Value) {
    if !map.get(key).is_some_and(|old| same(old, &value)) {
        map.insert(key.to_string(), value);
    }
}

/// Sets a field Excalidraw needs, if the element has none.
fn ensure(map: &mut Map<String, Value>, key: &str, value: Value) {
    map.entry(key).or_insert(value);
}

/// A number from an id, as Excalidraw's seeds are (FNV-1a, below 2³¹).
fn seed(id: &str, salt: i64) -> i64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64 ^ salt.unsigned_abs();
    for byte in id.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    (hash % 0x7fff_ffff) as i64
}

/// An element as Excalidraw keeps it, over the element it was. Its
/// version goes up when it reads differently from before.
fn element_json(element: &DrawElement, older: Option<&Value>, now: i64) -> Value {
    let older = older.filter(|v| v.is_object());
    let changed = older.and_then(|old| element_from(old, 0)).as_ref() != Some(element);
    let mut value = older.cloned().unwrap_or_else(|| json!({}));
    let Some(map) = value.as_object_mut() else { return value };
    set(map, "id", json!(element.id));
    set(map, "type", json!(element.kind));
    for (key, number) in [
        ("x", element.x),
        ("y", element.y),
        ("width", element.width),
        ("height", element.height),
        ("angle", element.angle),
        ("strokeWidth", element.stroke_width),
        ("opacity", element.opacity),
    ] {
        set(map, key, json!(number));
    }
    set(map, "strokeColor", json!(element.stroke_color));
    set(map, "backgroundColor", json!(element.background_color));
    for (key, default) in [
        ("fillStyle", json!("solid")),
        ("strokeStyle", json!("solid")),
        ("roughness", json!(0)),
        ("groupIds", json!([])),
        ("frameId", Value::Null),
        ("roundness", Value::Null),
        ("seed", json!(seed(&element.id, 0))),
        ("version", json!(1)),
        ("versionNonce", json!(seed(&element.id, 1))),
        ("isDeleted", json!(false)),
        ("boundElements", Value::Null),
        ("updated", json!(now)),
        ("link", Value::Null),
        ("locked", json!(false)),
    ] {
        ensure(map, key, default);
    }
    let points = || json!(element.points.iter().map(|&(x, y)| [x, y]).collect::<Vec<_>>());
    match element.kind.as_str() {
        "freedraw" => {
            set(map, "points", points());
            set(map, "pressures", json!(element.pressures));
            set(map, "simulatePressure", json!(element.pressures.is_empty()));
            ensure(map, "lastCommittedPoint", Value::Null);
        }
        "line" | "arrow" => {
            set(map, "points", points());
            set(map, "endArrowhead", if element.arrowhead { json!("arrow") } else { Value::Null });
            for key in ["lastCommittedPoint", "startBinding", "endBinding", "startArrowhead"] {
                ensure(map, key, Value::Null);
            }
        }
        "image" => {
            set(map, "fileId", json!(element.file_id));
            ensure(map, "status", json!("saved"));
            ensure(map, "scale", json!([1, 1]));
        }
        "text" => {
            set(map, "text", json!(element.text));
            set(map, "originalText", json!(element.text));
            set(map, "fontSize", json!(element.font_size));
            for (key, default) in [
                ("fontFamily", json!(2)),
                ("textAlign", json!("left")),
                ("verticalAlign", json!("top")),
                ("containerId", Value::Null),
                ("autoResize", json!(true)),
                ("lineHeight", json!(1.25)),
            ] {
                ensure(map, key, default);
            }
        }
        _ => {}
    }
    let marked = map.get("customData").and_then(|c| c.get("paperloom")).is_some();
    if element.highlighter && !marked {
        let custom = map.entry("customData").or_insert_with(|| json!({}));
        if !custom.is_object() {
            *custom = json!({});
        }
        custom["paperloom"] = json!("highlighter");
    } else if !element.highlighter
        && marked
        && let Some(custom) = map.get_mut("customData").and_then(Value::as_object_mut)
    {
        custom.remove("paperloom");
    }
    if changed && older.is_some() {
        let version = map.get("version").and_then(Value::as_i64).unwrap_or(0) + 1;
        map.insert("version".into(), json!(version));
        map.insert("versionNonce".into(), json!(seed(&element.id, version)));
        map.insert("updated".into(), json!(now));
    }
    value
}

impl Drawing {
    pub fn parse(text: &str) -> std::result::Result<Drawing, String> {
        let scene: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let elements = scene.get("elements").and_then(Value::as_array).ok_or("not an Excalidraw drawing")?;
        let elements: Vec<DrawElement> =
            elements.iter().enumerate().filter_map(|(i, e)| element_from(e, i)).collect();
        let files = elements
            .iter()
            .filter(|e| e.kind == "image")
            .filter_map(|e| {
                let url = scene.get("files")?.get(&e.file_id)?.get("dataURL")?.as_str()?;
                Some((e.file_id.clone(), url.to_string()))
            })
            .collect();
        Ok(Drawing {
            elements,
            paper: scene
                .pointer("/appState/paperloomPaper")
                .and_then(Value::as_str)
                .unwrap_or("blank")
                .to_string(),
            files,
        })
    }

    /// The scene as Excalidraw writes it, over `older` (the file as it
    /// was): elements keep the fields this app does not know, and those it
    /// does not draw stay, beneath the rest.
    pub fn to_json(&self, older: Option<&str>) -> String {
        let mut scene: Value = older
            .and_then(|text| serde_json::from_str(text).ok())
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}));
        let old_elements = scene.get("elements").and_then(Value::as_array).cloned().unwrap_or_default();
        let by_id: HashMap<&str, &Value> =
            old_elements.iter().filter_map(|e| Some((e.get("id")?.as_str()?, e))).collect();
        let now = now_ms();
        let mut elements: Vec<Value> = old_elements
            .iter()
            .filter(|e| !DRAWN_KINDS.contains(&kind_of(e)) && !is_deleted(e))
            .cloned()
            .collect();
        elements
            .extend(self.elements.iter().map(|e| element_json(e, by_id.get(e.id.as_str()).copied(), now)));
        let mut app_state = scene
            .get("appState")
            .cloned()
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({ "viewBackgroundColor": "#ffffff" }));
        app_state["paperloomPaper"] =
            json!(if self.paper.is_empty() { "blank" } else { self.paper.as_str() });
        scene["type"] = json!("excalidraw");
        scene["version"] = json!(2);
        if scene.get("source").is_none() {
            scene["source"] = json!("paperloom");
        }
        scene["elements"] = Value::Array(elements);
        scene["appState"] = app_state;
        if !scene.get("files").is_some_and(Value::is_object) {
            scene["files"] = json!({});
        }
        // The pictures its images show, as Excalidraw keeps them.
        for element in self.elements.iter().filter(|e| e.kind == "image") {
            let Some(url) = self.files.get(&element.file_id) else { continue };
            if scene["files"].get(&element.file_id).is_some() {
                continue;
            }
            let mime = url.strip_prefix("data:").and_then(|u| u.split(';').next()).unwrap_or("image/png");
            scene["files"][&element.file_id] = json!({
                "mimeType": mime,
                "id": element.file_id,
                "dataURL": url,
                "created": now,
                "lastRetrieved": now,
            });
        }
        serde_json::to_string_pretty(&scene).unwrap_or_default()
    }
}

/// `Sketch.excalidraw` for `Sketch`, and a path ending `.excalidraw` as it is.
fn checked_drawing(path: &str) -> Result<String> {
    let path = checked(path)?;
    Ok(if path.to_lowercase().ends_with(".excalidraw") { path } else { format!("{path}.excalidraw") })
}

impl Vault {
    /// The drawings in the vault, by path.
    pub fn drawings(&self) -> impl Iterator<Item = &String> {
        self.files.iter().filter(|f| f.to_lowercase().ends_with(".excalidraw"))
    }

    fn drawing_text(&self, path: &str) -> Result<String> {
        let full = self.full(path);
        fs::read_to_string(&full).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Error::NotFound(path.to_string()),
            _ => io_error(&full, e),
        })
    }

    pub fn read_drawing(&self, path: &str) -> Result<Drawing> {
        let path = checked_drawing(path)?;
        Drawing::parse(&self.drawing_text(&path)?)
            .map_err(|message| Error::Invalid(format!("{path}: {message}")))
    }

    /// Saves a drawing (creating it and its folders if needed) over the one
    /// there, keeping what this app does not draw. Replaced in one step.
    pub fn write_drawing(&mut self, path: &str, drawing: &Drawing) -> Result<()> {
        let path = checked_drawing(path)?;
        let older = self.drawing_text(&path).ok();
        let full = self.full(&path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
            self.add_folders(&path);
        }
        let temporary = full.with_extension("excalidraw.paperloom-tmp");
        fs::write(&temporary, drawing.to_json(older.as_deref())).map_err(|e| io_error(&temporary, e))?;
        fs::rename(&temporary, &full).map_err(|e| io_error(&full, e))?;
        self.files.insert(path);
        Ok(())
    }

    /// Makes an empty drawing on `paper` in `folder` named `name` (or
    /// `name 2`, … when taken); its path.
    pub fn create_drawing(&mut self, folder: &str, name: &str, paper: &str) -> Result<String> {
        let name = name.trim().trim_end_matches(".excalidraw");
        let name = if name.is_empty() { "Untitled" } else { name };
        let folder = folder.trim_matches('/');
        let mut n = 1;
        let path = loop {
            let file = if n == 1 { format!("{name}.excalidraw") } else { format!("{name} {n}.excalidraw") };
            let path = if folder.is_empty() { file } else { format!("{folder}/{file}") };
            if !self.files.iter().any(|f| f.eq_ignore_ascii_case(&path)) {
                break path;
            }
            n += 1;
        };
        self.write_drawing(
            &path,
            &Drawing { elements: Vec::new(), paper: paper.to_string(), files: HashMap::new() },
        )?;
        Ok(path)
    }
}

// Pictures.

/// How wide a stroke is at a pressure, as Excalidraw draws it.
pub fn stroke_diameter(stroke_width: f64, pressure: f64) -> f64 {
    stroke_width * 4.25 * (0.4 + 1.2 * pressure.clamp(0.0, 1.0))
}

/// The area an element covers, roughly: left, top, right, bottom.
fn bounds(element: &DrawElement) -> (f64, f64, f64, f64) {
    if element.points.is_empty() {
        let (x2, y2) = (element.x + element.width, element.y + element.height);
        return (element.x.min(x2), element.y.min(y2), element.x.max(x2), element.y.max(y2));
    }
    let reach = if element.kind == "freedraw" { stroke_diameter(element.stroke_width, 1.0) } else { 12.0 };
    let xs = element.points.iter().map(|p| element.x + p.0);
    let ys = element.points.iter().map(|p| element.y + p.1);
    (
        xs.clone().fold(f64::INFINITY, f64::min) - reach,
        ys.clone().fold(f64::INFINITY, f64::min) - reach,
        xs.fold(f64::NEG_INFINITY, f64::max) + reach,
        ys.fold(f64::NEG_INFINITY, f64::max) + reach,
    )
}

fn svg_escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// A pen stroke: runs of points of about the same width, each a path.
fn stroke_svg(element: &DrawElement, color: &str, out: &mut String) {
    let points: Vec<(f64, f64)> = element.points.iter().map(|p| (element.x + p.0, element.y + p.1)).collect();
    let widths: Vec<f64> = (0..points.len())
        .map(|i| stroke_diameter(element.stroke_width, element.pressures.get(i).copied().unwrap_or(0.5)))
        .collect();
    let mut start = 0;
    while start < points.len() {
        // Widths are kept to quarter pixels so a run is a few points long.
        let bucket = (widths[start] * 4.0).round() as i64;
        let mut end = start + 1;
        while end < points.len() && (widths[end] * 4.0).round() as i64 == bucket {
            end += 1;
        }
        let run = &points[start.saturating_sub(1)..end.min(points.len())];
        let mut d = String::new();
        for (i, (x, y)) in run.iter().enumerate() {
            let _ = write!(d, "{}{x:.1} {y:.1}", if i == 0 { "M" } else { "L" });
        }
        if run.len() == 1 {
            d.push_str(" l0 0");
        }
        let _ = writeln!(
            out,
            "<path d=\"{d}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"{:.2}\" stroke-linecap=\"round\" \
             stroke-linejoin=\"round\"/>",
            bucket as f64 / 4.0
        );
        start = end;
    }
}

fn element_svg(element: &DrawElement, files: &HashMap<String, String>, out: &mut String) {
    let color = svg_escape(&element.stroke_color);
    let fill = if element.background_color == "transparent" {
        "none".to_string()
    } else {
        svg_escape(&element.background_color)
    };
    let (cx, cy) = (element.x + element.width / 2.0, element.y + element.height / 2.0);
    let _ = write!(out, "<g opacity=\"{:.2}\"", element.opacity.clamp(0.0, 100.0) / 100.0);
    if element.angle.abs() > 1e-9 {
        let _ = write!(out, " transform=\"rotate({:.2} {cx:.1} {cy:.1})\"", element.angle.to_degrees());
    }
    out.push_str(">\n");
    let (x, y, w, h, sw) = (element.x, element.y, element.width, element.height, element.stroke_width);
    match element.kind.as_str() {
        "freedraw" => stroke_svg(element, &color, out),
        "line" | "arrow" => {
            let points: Vec<(f64, f64)> = element.points.iter().map(|p| (x + p.0, y + p.1)).collect();
            let mut d = String::new();
            for (i, (px, py)) in points.iter().enumerate() {
                let _ = write!(d, "{}{px:.1} {py:.1}", if i == 0 { "M" } else { "L" });
            }
            let _ = writeln!(
                out,
                "<path d=\"{d}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"{sw}\" stroke-linecap=\"round\" \
                 stroke-linejoin=\"round\"/>"
            );
            if element.arrowhead
                && let &[.., (ax, ay), (bx, by)] = points.as_slice()
            {
                let angle = (by - ay).atan2(bx - ax);
                let size = 8.0 + 4.0 * sw;
                let mut head = String::new();
                for turn in [-0.45f64, 0.45] {
                    let (hx, hy) = (bx - size * (angle + turn).cos(), by - size * (angle + turn).sin());
                    let _ = write!(head, "M{hx:.1} {hy:.1}L{bx:.1} {by:.1}");
                }
                let _ = writeln!(
                    out,
                    "<path d=\"{head}\" fill=\"none\" stroke=\"{color}\" stroke-width=\"{sw}\" stroke-linecap=\"round\"/>"
                );
            }
        }
        "rectangle" => {
            let _ = writeln!(
                out,
                "<rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{w:.1}\" height=\"{h:.1}\" fill=\"{fill}\" stroke=\"{color}\" \
                 stroke-width=\"{sw}\"/>"
            );
        }
        "ellipse" => {
            let _ = writeln!(
                out,
                "<ellipse cx=\"{cx:.1}\" cy=\"{cy:.1}\" rx=\"{:.1}\" ry=\"{:.1}\" fill=\"{fill}\" stroke=\"{color}\" \
                 stroke-width=\"{sw}\"/>",
                w / 2.0,
                h / 2.0
            );
        }
        "diamond" => {
            let _ = writeln!(
                out,
                "<polygon points=\"{cx:.1},{y:.1} {:.1},{cy:.1} {cx:.1},{:.1} {x:.1},{cy:.1}\" fill=\"{fill}\" \
                 stroke=\"{color}\" stroke-width=\"{sw}\" stroke-linejoin=\"round\"/>",
                x + w,
                y + h
            );
        }
        "text" => {
            let size = element.font_size;
            let _ = write!(out, "<text font-size=\"{size}\" fill=\"{color}\">");
            for (i, line) in element.text.lines().enumerate() {
                let _ = write!(
                    out,
                    "<tspan x=\"{x:.1}\" y=\"{:.1}\">{}</tspan>",
                    y + size * (1.25 * i as f64 + 1.0),
                    svg_escape(line)
                );
            }
            out.push_str("</text>\n");
        }
        "image" => {
            if let Some(url) = files.get(&element.file_id) {
                let _ = writeln!(
                    out,
                    "<image href=\"{}\" x=\"{x:.1}\" y=\"{y:.1}\" width=\"{w:.1}\" height=\"{h:.1}\" \
                     preserveAspectRatio=\"none\"/>",
                    svg_escape(url)
                );
            }
        }
        _ => {}
    }
    out.push_str("</g>\n");
}

/// A drawing as an SVG picture, cropped to what is drawn; highlighter
/// strokes lie under the rest.
pub fn drawing_svg(drawing: &Drawing) -> String {
    let (mut left, mut top, mut right, mut bottom) =
        (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for element in &drawing.elements {
        let (l, t, r, b) = bounds(element);
        (left, top, right, bottom) = (left.min(l), top.min(t), right.max(r), bottom.max(b));
    }
    if !left.is_finite() {
        (left, top, right, bottom) = (0.0, 0.0, 320.0, 160.0);
    }
    let pad = 16.0;
    let (x, y, w, h) = (left - pad, top - pad, right - left + 2.0 * pad, bottom - top + 2.0 * pad);
    let mut out = String::new();
    let _ = writeln!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"{x:.1} {y:.1} {w:.1} {h:.1}\" width=\"{w:.0}\" \
         height=\"{h:.0}\" font-family=\"-apple-system, 'Segoe UI', 'PingFang SC', 'Microsoft YaHei', sans-serif\">"
    );
    let _ = writeln!(
        out,
        "<rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{w:.1}\" height=\"{h:.1}\" fill=\"#ffffff\"/>"
    );
    for element in drawing.elements.iter().filter(|e| e.highlighter) {
        element_svg(element, &drawing.files, &mut out);
    }
    for element in drawing.elements.iter().filter(|e| !e.highlighter) {
        element_svg(element, &drawing.files, &mut out);
    }
    out.push_str("</svg>\n");
    out
}

#[cfg(test)]
mod tests;
