//! A page as the apps draw it: each shape and edge on the page, in page
//! coordinates, with its outline, line, arrowheads, colours and label, in
//! the order they stack.

use std::collections::HashMap;

use super::model::{Cell, Page};
use super::route::{End, arrowhead, line_path, marker_inset, route, trim_line};
use super::shapes::{Rect, kind, outline};
use crate::xml::decode;

/// A shape or an edge, ready to draw.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Item {
    pub id: String,
    pub edge: bool,
    /// The shape that holds it (a group or container), if any.
    pub parent: String,
    /// Its box; an edge's is around its line.
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// Degrees clockwise about its box's middle.
    pub rotation: f64,
    /// The outline, or the edge's line, as SVG path data.
    pub path: String,
    /// Lines drawn inside a shape (a cylinder's rim, a process's bars).
    pub inner: String,
    /// Arrowheads and solid details in the stroke colour, and open markers.
    pub arrows: String,
    pub open_arrows: String,
    /// Colours as `#rrggbb`; empty for none.
    pub fill: String,
    pub stroke: String,
    pub stroke_width: f64,
    pub dashed: bool,
    /// From 0 to 1.
    pub opacity: f64,
    /// The label as plain text, lines split by `\n`.
    pub text: String,
    /// The box the label goes in.
    pub text_x: f64,
    pub text_y: f64,
    pub text_width: f64,
    pub text_height: f64,
    pub font_size: f64,
    pub font_color: String,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    /// `left`, `center` or `right`.
    pub align: String,
    /// `top`, `middle` or `bottom`.
    pub vertical_align: String,
    /// The colour behind the label (edge labels), empty for none.
    pub label_background: String,
    /// Whether the label wraps to its box.
    pub wrap: bool,
    /// An edge's ends, by id (empty for a loose end).
    pub source: String,
    pub target: String,
    /// An edge's line, point by point.
    pub points: Vec<(f64, f64)>,
    /// Whether it holds other shapes (a group, a container or a swimlane).
    pub container: bool,
    /// A picture's address (a `data:` URI or a web address), as an SVG
    /// `<image>` takes it; empty for other shapes.
    pub image: String,
    /// Whether the picture keeps its proportions in its box.
    pub image_aspect: bool,
}

/// What the page draws, and how far it reaches.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Scene {
    pub items: Vec<Item>,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// The page's background colour, empty for none.
    pub background: String,
    /// The grid's spacing, 0 when it is off.
    pub grid: f64,
}

/// A label as plain text: HTML labels lose their tags, and their breaks
/// and blocks become new lines.
pub fn label_text(value: &str, html: bool) -> String {
    if !html {
        return value.to_string();
    }
    let mut text = String::new();
    let mut rest = value;
    while let Some(open) = rest.find('<') {
        text.push_str(&rest[..open]);
        let Some(close) = rest[open..].find('>') else {
            rest = &rest[open..];
            break;
        };
        let tag = rest[open + 1..open + close].trim().to_ascii_lowercase();
        let name = tag.trim_start_matches('/').split([' ', '/']).next().unwrap_or_default();
        let block = matches!(name, "div" | "p" | "li" | "h1" | "h2" | "h3" | "tr");
        // A break, or a block starting or ending after some text.
        if name == "br" || (block && !text.is_empty() && !text.ends_with('\n')) {
            text.push('\n');
        }
        rest = &rest[open + close + 1..];
    }
    text.push_str(rest);
    decode(&text).replace('\u{a0}', " ").trim_end_matches('\n').to_string()
}

/// A colour as `#rrggbb`, or empty for none (`none`, `default` and the
/// like take `fallback`).
/// A picture's address as an SVG `<image>` takes it: draw.io keeps data
/// without its `;base64` (a `;` would end the style), so it goes back.
pub fn image_href(image: &str) -> String {
    match image.strip_prefix("data:").and_then(|rest| rest.split_once(',')) {
        Some((kind, data)) if !kind.ends_with(";base64") => format!("data:{kind};base64,{data}"),
        _ => image.to_string(),
    }
}

fn colour(value: Option<&str>, fallback: &str) -> String {
    match value.map(str::trim) {
        Some("none") => String::new(),
        Some(v) if v.starts_with('#') => v.to_string(),
        Some(v) if v.chars().all(|c| c.is_ascii_hexdigit()) && v.len() == 6 => format!("#{v}"),
        Some(v)
            if !v.is_empty() && !matches!(v, "default" | "inherit") && v.chars().all(char::is_alphabetic) =>
        {
            v.to_string()
        }
        _ => fallback.to_string(),
    }
}

/// Whether a cell is hidden: by itself, by its layer, or inside a folded
/// container.
fn hidden(page: &Page, cell: &Cell) -> bool {
    let mut current = Some(cell);
    for _ in 0..64 {
        let Some(c) = current else { return false };
        if c.attr("visible") == Some("0") || (c.id != cell.id && c.attr("collapsed") == Some("1")) {
            return true;
        }
        current = page.cell(&c.parent);
    }
    false
}

/// Where a cell's coordinates count from: the shapes it sits in.
pub(super) fn origin(page: &Page, parent: &str) -> (f64, f64) {
    let (mut x, mut y) = (0.0, 0.0);
    let mut current = page.cell(parent);
    for _ in 0..64 {
        let Some(c) = current else { break };
        if !c.vertex {
            break;
        }
        if let Some(g) = &c.geometry {
            x += g.x;
            y += g.y;
        }
        current = page.cell(&c.parent);
    }
    (x, y)
}

/// A shape's box on the page.
fn shape_box(page: &Page, cell: &Cell) -> Option<Rect> {
    let g = cell.geometry.as_ref()?;
    let (ox, oy) = origin(page, &cell.parent);
    Some(Rect { x: ox + g.x, y: oy + g.y, width: g.width, height: g.height })
}

/// The point at a share of the way along a line, from −1 (its start) to 1
/// (its end), and the way the line runs there.
fn along(points: &[(f64, f64)], share: f64) -> ((f64, f64), (f64, f64)) {
    let lengths: Vec<f64> = points.windows(2).map(|p| (p[1].0 - p[0].0).hypot(p[1].1 - p[0].1)).collect();
    let total: f64 = lengths.iter().sum();
    let mut left = total * (share.clamp(-1.0, 1.0) + 1.0) / 2.0;
    for (i, length) in lengths.iter().enumerate() {
        if left <= *length || i == lengths.len() - 1 {
            let t = if *length > 0.0 { (left / length).min(1.0) } else { 0.0 };
            let (a, b) = (points[i], points[i + 1]);
            let way = if *length > 0.0 { ((b.0 - a.0) / length, (b.1 - a.1) / length) } else { (1.0, 0.0) };
            return ((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t), way);
        }
        left -= length;
    }
    (points.first().copied().unwrap_or_default(), (1.0, 0.0))
}

fn styled(item: &mut Item, cell: &Cell) {
    let style = &cell.style;
    item.stroke_width = style.number("strokeWidth", 1.0);
    item.dashed = style.flag("dashed", false);
    item.opacity = style.number("opacity", 100.0).clamp(0.0, 100.0) / 100.0;
    item.rotation = style.number("rotation", 0.0);
    item.font_size = style.number("fontSize", 12.0);
    item.font_color = colour(style.get("fontColor"), "#000000");
    let font = style.number("fontStyle", 0.0) as u32;
    (item.bold, item.italic, item.underline) = (font & 1 != 0, font & 2 != 0, font & 4 != 0);
    item.align = style.get("align").unwrap_or("center").to_string();
    item.vertical_align = style.get("verticalAlign").unwrap_or("middle").to_string();
    item.wrap = style.get("whiteSpace") == Some("wrap");
    item.text = label_text(&cell.value, style.flag("html", false));
}

/// A shape on the page.
fn shape_item(page: &Page, cell: &Cell, r: Rect) -> Item {
    let style = &cell.style;
    let shape = outline(style, r);
    let mut item = Item {
        id: cell.id.clone(),
        parent: if page.cell(&cell.parent).is_some_and(|p| p.vertex) {
            cell.parent.clone()
        } else {
            String::new()
        },
        x: r.x,
        y: r.y,
        width: r.width,
        height: r.height,
        path: shape.path,
        inner: shape.inner,
        arrows: shape.solid,
        container: kind(style) == "swimlane"
            || style.has("group")
            || style.flag("container", false)
            || page.cells.iter().any(|c| c.parent == cell.id && c.vertex),
        ..Item::default()
    };
    styled(&mut item, cell);
    if kind(style) == "bpmnEnd" {
        item.stroke_width *= 3.0;
    }
    let text_shape = matches!(kind(style), "text" | "group");
    item.fill =
        if shape.filled && !text_shape { colour(style.get("fillColor"), "#ffffff") } else { String::new() };
    item.stroke = if text_shape { String::new() } else { colour(style.get("strokeColor"), "#000000") };
    // A picture has no outline or fill of its own unless asked.
    if kind(style) == "image" {
        item.image = style.get("image").map(image_href).unwrap_or_default();
        item.image_aspect = style.flag("imageAspect", true);
        item.fill = colour(style.get("imageBackground"), "");
        item.stroke = colour(style.get("imageBorder"), "");
    }
    let spacing = style.number("spacing", 2.0);
    let (mut tx, mut ty, mut tw, mut th) =
        (r.x + spacing, r.y + spacing, r.width - 2.0 * spacing, r.height - 2.0 * spacing);
    match style.get("verticalLabelPosition") {
        Some("bottom") => (ty, th) = (r.bottom() + spacing, item.font_size * 1.6),
        Some("top") => (ty, th) = (r.y - item.font_size * 1.6 - spacing, item.font_size * 1.6),
        _ => {}
    }
    match style.get("labelPosition") {
        Some("left") => (tx, tw) = (r.x - r.width - spacing, r.width),
        Some("right") => (tx, tw) = (r.right() + spacing, r.width),
        _ => {}
    }
    if kind(style) == "umlActor" && style.get("verticalLabelPosition").is_none() {
        (ty, th) = (r.bottom() + spacing, item.font_size * 1.6);
        item.vertical_align = "top".into();
    }
    if kind(style) == "swimlane" {
        let head = style.number("startSize", 23.0);
        if style.flag("horizontal", true) {
            th = head - 2.0 * spacing;
        } else {
            tw = head - 2.0 * spacing;
        }
        item.vertical_align = "middle".into();
    }
    if kind(style) == "umlLifeline" {
        th = style.number("size", 40.0).clamp(0.0, r.height) - 2.0 * spacing;
    }
    if kind(style) == "umlFrame" {
        tw = style.number("width", 60.0).clamp(0.0, r.width) - 2.0 * spacing;
        th = style.number("height", 30.0).clamp(0.0, r.height) - 2.0 * spacing;
    }
    (item.text_x, item.text_y, item.text_width, item.text_height) = (tx, ty, tw.max(0.0), th.max(0.0));
    item
}

/// The page as the apps draw it.
pub fn scene(page: &Page) -> Scene {
    let boxes: HashMap<&str, Rect> = page
        .cells
        .iter()
        .filter(|c| c.vertex)
        .filter_map(|c| Some((c.id.as_str(), shape_box(page, c)?)))
        .collect();
    let mut lines: HashMap<&str, Vec<(f64, f64)>> = HashMap::new();
    for cell in page.cells.iter().filter(|c| c.edge) {
        let Some(g) = &cell.geometry else { continue };
        let (ox, oy) = origin(page, &cell.parent);
        let end = |id: &str, loose: Option<(f64, f64)>| match (page.cell(id), boxes.get(id)) {
            (Some(c), Some(r)) if c.vertex && !hidden(page, c) => Some(End::Shape(*r, &c.style)),
            _ => loose.map(|(x, y)| End::Point(ox + x, oy + y)),
        };
        let (Some(source), Some(target)) =
            (end(&cell.source, g.source_point), end(&cell.target, g.target_point))
        else {
            continue;
        };
        let bends: Vec<(f64, f64)> = g.points.iter().map(|&(x, y)| (ox + x, oy + y)).collect();
        lines.insert(cell.id.as_str(), route(&cell.style, &source, &target, &bends));
    }
    let mut items = Vec::new();
    for cell in &page.cells {
        if hidden(page, cell) {
            continue;
        }
        if cell.vertex {
            // A label on an edge sits along its line.
            if let (Some(line), Some(g)) = (lines.get(cell.parent.as_str()), &cell.geometry)
                && g.relative
            {
                let ((x, y), _) = along(line, g.x);
                let (dx, dy) = g.offset.unwrap_or_default();
                let mut item = Item { id: cell.id.clone(), parent: cell.parent.clone(), ..Item::default() };
                styled(&mut item, cell);
                let width = (item.text.chars().count() as f64 * item.font_size * 0.6).clamp(20.0, 240.0);
                let height = item.font_size * 1.4 * item.text.lines().count().max(1) as f64;
                (item.x, item.y, item.width, item.height) =
                    (x + dx - width / 2.0, y + dy - height / 2.0, width, height);
                (item.text_x, item.text_y, item.text_width, item.text_height) =
                    (item.x, item.y, width, height);
                item.label_background = colour(cell.style.get("labelBackgroundColor"), "#ffffff");
                items.push(item);
                continue;
            }
            if let Some(r) = boxes.get(cell.id.as_str()) {
                items.push(shape_item(page, cell, *r));
            }
        } else if cell.edge
            && let Some(line) = lines.get(cell.id.as_str())
        {
            items.push(edge_item(page, cell, line));
        }
    }
    let (mut left, mut top, mut right, mut bottom) =
        (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for item in &items {
        (left, top) = (left.min(item.x), top.min(item.y));
        (right, bottom) = (right.max(item.x + item.width), bottom.max(item.y + item.height));
        if !item.text.is_empty() {
            (left, top) = (left.min(item.text_x), top.min(item.text_y));
            right = right.max(item.text_x + item.text_width);
            bottom = bottom.max(item.text_y + item.text_height);
        }
    }
    if items.is_empty() {
        (left, top, right, bottom) = (0.0, 0.0, 0.0, 0.0);
    }
    let model = |key: &str| page.model.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
    Scene {
        items,
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
        background: colour(model("background"), ""),
        grid: if model("grid") == Some("0") {
            0.0
        } else {
            model("gridSize").and_then(|g| g.parse().ok()).unwrap_or(10.0)
        },
    }
}

/// An edge on the page: its line, arrowheads and label.
fn edge_item(page: &Page, cell: &Cell, line: &[(f64, f64)]) -> Item {
    let style = &cell.style;
    let mut item = Item {
        id: cell.id.clone(),
        edge: true,
        parent: if page.cell(&cell.parent).is_some_and(|p| p.vertex) {
            cell.parent.clone()
        } else {
            String::new()
        },
        path: line_path(line, style.flag("curved", false)),
        source: cell.source.clone(),
        target: cell.target.clone(),
        points: line.to_vec(),
        ..Item::default()
    };
    styled(&mut item, cell);
    item.rotation = 0.0;
    item.stroke = colour(style.get("strokeColor"), "#000000");
    let ends = [
        (
            style.get("endArrow").unwrap_or("classic"),
            style.flag("endFill", true),
            line.len().checked_sub(1),
            "endSize",
        ),
        (style.get("startArrow").unwrap_or("none"), style.flag("startFill", true), Some(0), "startSize"),
    ];
    let mut insets = [0.0; 2];
    let mut marker_padding: f64 = 0.0;
    for (i, (arrow, filled, tip, size_key)) in ends.into_iter().enumerate() {
        let Some(tip) = tip.filter(|_| line.len() >= 2) else { continue };
        let from = if tip == 0 { line[1] } else { line[tip - 1] };
        let raw_size = style.number(size_key, 6.0).max(1.0);
        let size = if arrow.starts_with("ER") { raw_size } else { raw_size * 1.6 + item.stroke_width };
        if arrow.starts_with("ER") {
            marker_padding = marker_padding.max(size + item.stroke_width + 1.0);
        }
        insets[i] = marker_inset(arrow, size, item.stroke_width);
        if let Some((path, filled)) = arrowhead(arrow, filled, size, line[tip], from, item.stroke_width) {
            let into = if filled { &mut item.arrows } else { &mut item.open_arrows };
            into.push_str(&path);
        }
    }
    if insets.iter().any(|inset| *inset > 0.0) {
        item.path = line_path(&trim_line(line, insets[1], insets[0]), style.flag("curved", false));
    }
    let (mut left, mut top, mut right, mut bottom) =
        (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for &(x, y) in line {
        (left, top, right, bottom) = (left.min(x), top.min(y), right.max(x), bottom.max(y));
    }
    if marker_padding > 0.0 {
        left -= marker_padding;
        top -= marker_padding;
        right += marker_padding;
        bottom += marker_padding;
    }
    (item.x, item.y, item.width, item.height) = (left, top, right - left, bottom - top);
    if !item.text.is_empty() {
        let g = cell.geometry.clone().unwrap_or_default();
        let ((x, y), _) = along(line, if g.relative { g.x } else { 0.0 });
        let (dx, dy) = g.offset.unwrap_or_default();
        let width =
            (item.text.lines().map(|l| l.chars().count()).max().unwrap_or(0) as f64 * item.font_size * 0.6)
                .clamp(16.0, 240.0);
        let height = item.font_size * 1.4 * item.text.lines().count().max(1) as f64;
        (item.text_x, item.text_y, item.text_width, item.text_height) =
            (x + dx - width / 2.0, y + dy - height / 2.0, width, height);
        item.label_background = colour(style.get("labelBackgroundColor"), "#ffffff");
    }
    item
}
