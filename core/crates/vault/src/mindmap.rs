//! Mind maps kept as Markdown outlines. A note with a `mindmap` property is
//! a map: its first `#` heading (or its title) is the central topic, and
//! the headings and list items under it, nested as written, are the
//! branches; text under a topic is its note. Obsidian and any other editor
//! read the same note as an ordinary outline.
//!
//! Topics are named by their path from the centre: `0` is the central
//! topic, `0.2` its third branch, `0.2.0` that branch's first topic.
//!
//! The layout is worked out here so every app draws the same map: sizes
//! come from the text (wide characters count double), branches from the
//! style — `logic` (balanced left and right), `right`, `org` (top down),
//! `tree` (indented), `timeline` or `fishbone` — and the line to each topic
//! is an SVG
//! path the apps draw as it is. Relations, summaries and boundaries are
//! in `extras`.

mod extras;
mod labels;
mod links;
mod markers;
mod themes;

pub use extras::{CENTRE, Extra, ExtraEdit, ExtraKind, PlacedExtra, edit_extras, read_extras};
pub use labels::{split_labels, with_labels};
pub use links::{is_web_link, split_link, with_link};
pub use markers::{MARKERS, Marker, marker_named, split_markers, toggle_marker, with_markers};
pub use themes::{MAP_THEMES, MapTheme};

use crate::parse;

/// A topic and the topics under it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Topic {
    pub text: String,
    /// Text written under the topic.
    pub note: String,
    pub children: Vec<Topic>,
    /// Its block ID (`- Topic ^t1`), which relations, summaries and
    /// boundaries name it by; empty when it has none.
    pub anchor: String,
}

/// How the branches lie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapStyle {
    /// Branches on both sides of the centre, balanced.
    Logic,
    /// Every branch to the right.
    Right,
    /// The centre on top, branches below (an organisation chart).
    Org,
    /// An indented tree under the centre, growing down and to the right.
    Tree,
    /// The main branches along a line to the right, what is under them
    /// above and below it in turn.
    Timeline,
    /// A cause and effect diagram: the centre is the head on the right,
    /// the main branches are bones slanting off the spine above and below
    /// it in turn, and what is under them is set along their bone.
    Fishbone,
}

impl MapStyle {
    pub fn parse(value: &str) -> MapStyle {
        match value.trim().to_ascii_lowercase().as_str() {
            "right" => MapStyle::Right,
            "org" => MapStyle::Org,
            "tree" => MapStyle::Tree,
            "timeline" => MapStyle::Timeline,
            "fishbone" => MapStyle::Fishbone,
            _ => MapStyle::Logic,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            MapStyle::Logic => "logic",
            MapStyle::Right => "right",
            MapStyle::Org => "org",
            MapStyle::Tree => "tree",
            MapStyle::Timeline => "timeline",
            MapStyle::Fishbone => "fishbone",
        }
    }
}

/// A change to a map; topics are named by their path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MapEdit {
    /// A new last topic under `id`.
    AddChild {
        id: String,
        text: String,
    },
    /// A new topic right after `id`, under the same parent.
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
    /// Removes a topic and what is under it (not the centre).
    Delete {
        id: String,
    },
    /// Moves a topic (with what is under it) under `parent`, at `index`.
    Move {
        id: String,
        parent: String,
        index: usize,
    },
    /// Puts a marker (XMind's ID: `priority-1`, `task-done`…) on a topic,
    /// or takes it off.
    ToggleMarker {
        id: String,
        marker: String,
    },
    /// Links a topic to a web page or a note (by its name), or takes its
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
}

/// A topic placed on the map.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    pub id: String,
    pub parent: Option<String>,
    /// Its text as shown, links as their text.
    pub text: String,
    /// Where its link goes: a web address or a note's name; empty when it
    /// has none.
    pub link: String,
    /// Its labels, shown under its text.
    pub labels: Vec<String>,
    pub note: String,
    pub depth: usize,
    /// −1 left of the centre, 1 right of it (or below, in an org chart), 0
    /// the centre.
    pub side: i8,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub font_size: f64,
    /// How many topics are directly under it.
    pub children: usize,
    /// Its branches are folded away.
    pub folded: bool,
    /// The line from its parent, as SVG path data; empty for the centre.
    pub edge: String,
}

/// A laid out map: its topics, centre first, what it draws besides its
/// branches, and its size.
#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    pub topics: Vec<Placed>,
    pub extras: Vec<PlacedExtra>,
    pub width: f64,
    pub height: f64,
}

// Reading and writing.

/// `- item`, `* item`, `+ item` or `1. item` (with any `[ ]` box), as its
/// indent and text.
fn list_item(line: &str) -> Option<(usize, &str)> {
    let indent: usize =
        line.chars().take_while(|c| c.is_whitespace()).map(|c| if c == '\t' { 4 } else { 1 }).sum();
    let rest = line.trim_start();
    let text = if let Some(text) = rest.strip_prefix(['-', '*', '+']).filter(|t| t.starts_with(' ')) {
        text
    } else {
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        let after = rest.get(digits..)?;
        (digits > 0).then_some(())?;
        after.strip_prefix(['.', ')']).filter(|t| t.starts_with(' '))?
    };
    let text = text.trim();
    let text = ["[ ] ", "[x] ", "[X] "].iter().find_map(|b| text.strip_prefix(b)).unwrap_or(text);
    Some((indent, text.trim()))
}

/// `## Heading` as its level and text.
fn heading(line: &str) -> Option<(usize, &str)> {
    let level = line.chars().take_while(|&c| c == '#').count();
    let text = line.get(level..)?;
    ((1..=6).contains(&level) && text.starts_with(' ')).then(|| (level, text.trim()))
}

struct Node {
    text: String,
    anchor: String,
    note: Vec<String>,
    children: Vec<usize>,
}

fn build(nodes: &[Node], index: usize) -> Topic {
    let node = &nodes[index];
    Topic {
        text: node.text.clone(),
        note: node.note.join("\n").trim().to_string(),
        children: node.children.iter().map(|&c| build(nodes, c)).collect(),
        anchor: node.anchor.clone(),
    }
}

/// A topic's text and its block ID: `Topic ^t1` → `Topic`, `t1`.
fn anchored(text: &str) -> (&str, &str) {
    match text.rsplit_once(" ^") {
        Some((text, id)) if !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') => {
            (text.trim_end(), id)
        }
        _ => (text, ""),
    }
}

/// Adds a topic at a depth under the latest topic one level up (or the
/// deepest there is, when levels are skipped).
fn add(nodes: &mut Vec<Node>, latest: &mut Vec<usize>, depth: usize, text: &str) {
    let parent_depth = depth.min(latest.len()).saturating_sub(1);
    let index = nodes.len();
    // The centre is always `centre`; its heading keeps its text as written.
    let (text, anchor) = if depth == 0 { (text, "") } else { anchored(text) };
    nodes.push(Node {
        text: text.to_string(),
        anchor: anchor.to_string(),
        note: Vec::new(),
        children: Vec::new(),
    });
    if depth > 0
        && let Some(&parent) = latest.get(parent_depth)
    {
        nodes[parent].children.push(index);
    }
    latest.truncate(if depth > 0 { parent_depth + 1 } else { 0 });
    latest.push(index);
}

/// A line without up to `width` columns of leading space.
fn dedent(line: &str, width: usize) -> &str {
    let mut columns = 0;
    for (at, c) in line.char_indices() {
        if columns >= width || !c.is_whitespace() {
            return &line[at..];
        }
        columns += if c == '\t' { 4 } else { 1 };
    }
    ""
}

/// The map in a note; `title` names the centre when the note has no `#`
/// heading before its branches.
pub fn read_map(text: &str, title: &str) -> Topic {
    let body = &text[parse(text).body_start..];
    let mut nodes: Vec<Node> = Vec::new();
    // The latest topic at each depth, and the heading depth lists sit under.
    let mut latest: Vec<usize> = Vec::new();
    let mut root_level = 0;
    let mut heading_depth = 0;
    let mut indents: Vec<usize> = Vec::new();
    // How far a topic's note is indented under it.
    let mut note_indent = 0;
    let mut fence = false;
    let extras = extras::block(body);
    for (number, line) in body.lines().enumerate() {
        if extras.as_ref().is_some_and(|lines| lines.contains(&number)) {
            continue;
        }
        let fence_line = line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~");
        if !fence && !fence_line {
            if let Some((level, text)) = heading(line) {
                indents.clear();
                note_indent = 0;
                if nodes.is_empty() {
                    root_level = level;
                    add(&mut nodes, &mut latest, 0, text);
                    heading_depth = 0;
                } else {
                    let depth = level.saturating_sub(root_level).max(1);
                    add(&mut nodes, &mut latest, depth, text);
                    heading_depth = latest.len() - 1;
                }
                continue;
            }
            if let Some((indent, text)) = list_item(line) {
                if nodes.is_empty() {
                    add(&mut nodes, &mut latest, 0, title);
                }
                while indents.last().is_some_and(|&last| last >= indent) {
                    indents.pop();
                }
                let depth = heading_depth + 1 + indents.len();
                indents.push(indent);
                note_indent = indent + 2;
                add(&mut nodes, &mut latest, depth, text);
                continue;
            }
            if line.trim().is_empty() {
                if let Some(&last) = latest.last()
                    && !nodes[last].note.is_empty()
                {
                    nodes[last].note.push(String::new());
                }
                continue;
            }
        }
        if fence_line {
            fence = !fence;
        }
        if nodes.is_empty() {
            add(&mut nodes, &mut latest, 0, title);
        }
        if let Some(&last) = latest.last() {
            nodes[last].note.push(dedent(line, note_indent).trim_end().to_string());
        }
    }
    if nodes.is_empty() {
        return Topic { text: title.to_string(), ..Topic::default() };
    }
    build(&nodes, 0)
}

fn write_topic(topic: &Topic, level: usize, out: &mut String) {
    let indent = "  ".repeat(level);
    let anchor = if topic.anchor.is_empty() { String::new() } else { format!(" ^{}", topic.anchor) };
    out.push_str(&format!("{indent}- {}{anchor}\n", topic.text.replace('\n', " ")));
    for line in topic.note.lines() {
        if line.is_empty() {
            out.push('\n');
        } else {
            out.push_str(&format!("{indent}  {line}\n"));
        }
    }
    for child in &topic.children {
        write_topic(child, level + 1, out);
    }
}

/// The note with this map in place of its outline; the front matter and
/// the relations, summaries and boundaries whose topics are still on the
/// map stay.
pub fn write_map(text: &str, root: &Topic) -> String {
    write_map_with(text, root, &read_extras(text))
}

/// The note with this map and these extras in place of its own; the front
/// matter stays. Extras whose topics are gone are left out.
pub fn write_map_with(text: &str, root: &Topic, extras: &[Extra]) -> String {
    let mut out = text[..parse(text).body_start].to_string();
    out.push_str(&format!("# {}\n", root.text.replace('\n', " ")));
    if !root.note.is_empty() {
        out.push('\n');
        out.push_str(&root.note);
        out.push('\n');
    }
    if !root.children.is_empty() {
        out.push('\n');
    }
    for child in &root.children {
        write_topic(child, 0, &mut out);
    }
    extras::write(root, extras, &mut out);
    out
}

/// A new map's note.
pub fn new_map(title: &str, style: MapStyle) -> String {
    format!("---\nmindmap: {}\n---\n# {}\n", style.as_str(), title.trim())
}

// Editing.

/// The child indexes of a path, `0.2.0` → `[2, 0]`.
fn steps(id: &str) -> Result<Vec<usize>, String> {
    let mut parts = id.split('.');
    if parts.next() != Some("0") {
        return Err(format!("“{id}” is not a topic."));
    }
    parts.map(|p| p.parse().map_err(|_| format!("“{id}” is not a topic."))).collect()
}

fn path_of(steps: &[usize]) -> String {
    std::iter::once("0".to_string()).chain(steps.iter().map(usize::to_string)).collect::<Vec<_>>().join(".")
}

fn topic_mut<'a>(root: &'a mut Topic, steps: &[usize]) -> Result<&'a mut Topic, String> {
    let mut topic = root;
    for &step in steps {
        topic = topic.children.get_mut(step).ok_or("That topic is not on the map.")?;
    }
    Ok(topic)
}

/// Makes a change; the path of the topic to show next.
pub fn edit_map(root: &mut Topic, edit: &MapEdit) -> Result<String, String> {
    match edit {
        MapEdit::AddChild { id, text } => {
            let at = steps(id)?;
            let topic = topic_mut(root, &at)?;
            topic.children.push(Topic { text: text.trim().to_string(), ..Topic::default() });
            let mut new = at;
            new.push(topic.children.len() - 1);
            Ok(path_of(&new))
        }
        MapEdit::AddSibling { id, text } => {
            let mut at = steps(id)?;
            let Some(last) = at.pop() else { return Err("The centre has no siblings.".into()) };
            let parent = topic_mut(root, &at)?;
            let index = (last + 1).min(parent.children.len());
            parent.children.insert(index, Topic { text: text.trim().to_string(), ..Topic::default() });
            at.push(index);
            Ok(path_of(&at))
        }
        MapEdit::Rename { id, text } => {
            let topic = topic_mut(root, &steps(id)?)?;
            let mut text = text.trim().replace('\n', " ");
            // New text keeps the topic's link, unless it brings its own,
            // and its labels, with any it brings.
            let (shown, link) = split_link(&topic.text);
            if !link.is_empty() && split_link(&text).1.is_empty() {
                text = with_link(&text, &link);
            }
            let (_, mut labels) = split_labels(&shown);
            if !labels.is_empty() {
                labels.extend(split_labels(&text).1);
                text = with_labels(&text, &labels);
            }
            topic.text = text;
            Ok(id.clone())
        }
        MapEdit::SetLink { id, link } => {
            let topic = topic_mut(root, &steps(id)?)?;
            topic.text = with_link(&topic.text, link);
            Ok(id.clone())
        }
        MapEdit::SetLabels { id, labels } => {
            let topic = topic_mut(root, &steps(id)?)?;
            topic.text = with_labels(&topic.text, labels);
            Ok(id.clone())
        }
        MapEdit::SetNote { id, note } => {
            topic_mut(root, &steps(id)?)?.note = note.trim().to_string();
            Ok(id.clone())
        }
        MapEdit::ToggleMarker { id, marker } => {
            let topic = topic_mut(root, &steps(id)?)?;
            topic.text = toggle_marker(&topic.text, marker)?;
            Ok(id.clone())
        }
        MapEdit::Delete { id } => {
            let mut at = steps(id)?;
            let Some(last) = at.pop() else { return Err("The centre cannot be removed.".into()) };
            let parent = topic_mut(root, &at)?;
            if last >= parent.children.len() {
                return Err("That topic is not on the map.".into());
            }
            parent.children.remove(last);
            // The topic now in its place, the one before when it was the
            // last, or its parent when it was the only one.
            if !parent.children.is_empty() {
                at.push(last.min(parent.children.len() - 1));
            }
            Ok(path_of(&at))
        }
        MapEdit::Move { id, parent, index } => {
            let mut from = steps(id)?;
            let to = steps(parent)?;
            if to.starts_with(&from) {
                return Err("A topic cannot move under itself.".into());
            }
            let Some(last) = from.pop() else { return Err("The centre cannot move.".into()) };
            let old_parent = topic_mut(root, &from)?;
            if last >= old_parent.children.len() {
                return Err("That topic is not on the map.".into());
            }
            let moving = old_parent.children.remove(last);
            // Removing it shifts the later siblings' paths, the new parent's
            // perhaps among them.
            let mut to = to;
            if to.len() > from.len() && to.starts_with(&from) && to[from.len()] > last {
                to[from.len()] -= 1;
            }
            let mut index = *index;
            if to == from && index > last {
                index -= 1;
            }
            let new_parent = topic_mut(root, &to)?;
            let index = index.min(new_parent.children.len());
            new_parent.children.insert(index, moving);
            to.push(index);
            Ok(path_of(&to))
        }
    }
}

// Layout.

const GAP_X: f64 = 48.0;
const GAP_Y: f64 = 14.0;
const MARGIN: f64 = 40.0;
const MAX_TEXT: f64 = 260.0;

fn font_size(depth: usize) -> f64 {
    match depth {
        0 => 20.0,
        1 => 16.0,
        _ => 14.0,
    }
}

/// How wide text is at a size, wide characters counting as a whole em and
/// others a little over half.
pub(crate) fn text_width(text: &str, size: f64) -> f64 {
    text.chars().map(|c| if (c as u32) >= 0x1100 && !c.is_ascii() { 1.0 } else { 0.58 }).sum::<f64>() * size
}

/// The size of labels' text.
pub(crate) const LABEL_SIZE: f64 = 11.0;

/// The height of the row of labels under a topic's text, and the room
/// above it.
pub(crate) const LABEL_ROW: f64 = LABEL_SIZE * 1.4 + 6.0;

/// The width of a row of labels, each in its own box.
pub(crate) fn labels_width(labels: &[String]) -> f64 {
    labels.iter().map(|l| text_width(l, LABEL_SIZE) + 10.0).sum::<f64>()
        + 4.0 * labels.len().saturating_sub(1) as f64
}

/// The topic's box: its text wrapped at a width.
fn measure(text: &str, depth: usize) -> (f64, f64) {
    let size = font_size(depth);
    let width = text_width(text, size);
    let lines = (width / MAX_TEXT).ceil().max(1.0);
    let (pad_x, pad_y) = if depth == 0 { (24.0, 14.0) } else { (14.0, 8.0) };
    (width.clamp(size, MAX_TEXT) + 2.0 * pad_x, lines * size * 1.4 + 2.0 * pad_y)
}

struct Tree<'a> {
    id: String,
    topic: &'a Topic,
    /// The topic's text as shown, its link and its labels.
    shown: String,
    link: String,
    labels: Vec<String>,
    depth: usize,
    size: (f64, f64),
    children: Vec<Tree<'a>>,
    folded: bool,
}

fn tree<'a>(topic: &'a Topic, id: String, depth: usize, folded: &[String]) -> Tree<'a> {
    let is_folded = folded.contains(&id) && !topic.children.is_empty();
    let children = if is_folded {
        Vec::new()
    } else {
        topic
            .children
            .iter()
            .enumerate()
            .map(|(i, c)| tree(c, format!("{id}.{i}"), depth + 1, folded))
            .collect()
    };
    let (shown, link) = split_link(&topic.text);
    let (shown, labels) = split_labels(&shown);
    let (mut width, mut height) = measure(shown, depth);
    // Room for the link's mark after the text, and the labels under it.
    if !link.is_empty() {
        width += font_size(depth) * 1.2;
    }
    if !labels.is_empty() {
        let pad_x = if depth == 0 { 24.0 } else { 14.0 };
        width = width.max(labels_width(&labels) + 2.0 * pad_x);
        height += LABEL_ROW;
    }
    let shown = shown.to_string();
    Tree { size: (width, height), id, topic, shown, link, labels, depth, children, folded: is_folded }
}

/// How much room a branch takes across its direction of growth: height for
/// left and right maps, width for org charts.
fn span(tree: &Tree, org: bool) -> f64 {
    let own = if org { tree.size.0 } else { tree.size.1 };
    let gap = if org { GAP_X / 2.0 } else { GAP_Y };
    let children: f64 = tree.children.iter().map(|c| span(c, org)).sum::<f64>()
        + gap * tree.children.len().saturating_sub(1) as f64;
    own.max(children)
}

/// A topic where it goes, with no line to it yet.
fn placed(tree: &Tree, parent: Option<&str>, side: i8, x: f64, y: f64) -> Placed {
    Placed {
        id: tree.id.clone(),
        parent: parent.map(str::to_string),
        text: tree.shown.clone(),
        link: tree.link.clone(),
        labels: tree.labels.clone(),
        note: tree.topic.note.clone(),
        depth: tree.depth,
        side,
        x,
        y,
        width: tree.size.0,
        height: tree.size.1,
        font_size: font_size(tree.depth),
        children: tree.topic.children.len(),
        folded: tree.folded,
        edge: String::new(),
    }
}

/// Places a branch: `at` is where it grows from (the parent's edge), `top`
/// where its span starts.
fn place(tree: &Tree, parent: Option<&str>, side: i8, at: f64, top: f64, org: bool, out: &mut Vec<Placed>) {
    let total = span(tree, org);
    let (w, h) = tree.size;
    let (x, y) = if org {
        (top + (total - w) / 2.0, at)
    } else {
        let x = if side < 0 { at - w } else { at };
        (x, top + (total - h) / 2.0)
    };
    out.push(placed(tree, parent, side, x, y));
    let gap = if org { GAP_X / 2.0 } else { GAP_Y };
    let children: f64 = tree.children.iter().map(|c| span(c, org)).sum::<f64>()
        + gap * tree.children.len().saturating_sub(1) as f64;
    let mut start = top + (total - children) / 2.0;
    let next = if org {
        y + h + GAP_X
    } else if side < 0 {
        x - GAP_X
    } else {
        x + w + GAP_X
    };
    for child in &tree.children {
        place(child, Some(&tree.id), side, next, start, org, out);
        start += span(child, org) + gap;
    }
}

/// How far an indented tree's topics are set in from their parent's.
const INDENT: f64 = 28.0;

/// How tall a branch is laid out as an indented tree.
fn tree_height(tree: &Tree) -> f64 {
    tree.size.1 + tree.children.iter().map(|c| GAP_Y + tree_height(c)).sum::<f64>()
}

/// How wide a branch is laid out as an indented tree.
fn tree_width(tree: &Tree) -> f64 {
    tree.children.iter().map(|c| INDENT + tree_width(c)).fold(tree.size.0, f64::max)
}

/// Places a branch as an indented tree, the topic at `x`, `y` and what is
/// under it below and to the right; where the next branch starts.
fn place_tree(tree: &Tree, parent: &str, x: f64, y: f64, out: &mut Vec<Placed>) -> f64 {
    out.push(placed(tree, Some(parent), 1, x, y));
    let mut next = y + tree.size.1 + GAP_Y;
    for child in &tree.children {
        next = place_tree(child, &tree.id, x + INDENT, next, out);
    }
    next
}

/// How far a fishbone's bone runs across for each point it climbs.
const SLANT: f64 = 0.5;

/// A fishbone's main topic: the edge of its box that faces the spine, and
/// where its bone leaves the spine.
fn bone(topic: &Placed, axis: f64) -> (f64, f64) {
    let edge = if topic.y + topic.height <= axis { topic.y + topic.height } else { topic.y };
    (edge, topic.x + topic.width / 2.0 + (axis - edge).abs() * SLANT)
}

/// The line from a parent to a topic, as SVG path data.
fn edge_path(style: MapStyle, parent: &Placed, child: &Placed, previous: Option<&Placed>) -> String {
    let cy = child.y + child.height / 2.0;
    match style {
        MapStyle::Org => {
            let (x1, y1) = (parent.x + parent.width / 2.0, parent.y + parent.height);
            let (x2, y2) = (child.x + child.width / 2.0, child.y);
            let mid = (y1 + y2) / 2.0;
            format!("M{x1:.1} {y1:.1}C{x1:.1} {mid:.1} {x2:.1} {mid:.1} {x2:.1} {y2:.1}")
        }
        MapStyle::Logic | MapStyle::Right => {
            let right = child.side >= 0;
            let x1 = if right { parent.x + parent.width } else { parent.x };
            let y1 = parent.y + parent.height / 2.0;
            let x2 = if right { child.x } else { child.x + child.width };
            let mid = (x1 + x2) / 2.0;
            format!("M{x1:.1} {y1:.1}C{mid:.1} {y1:.1} {mid:.1} {cy:.1} {x2:.1} {cy:.1}")
        }
        // The line of a timeline, from the topic before.
        MapStyle::Timeline if child.depth == 1 => {
            let from = previous.unwrap_or(parent);
            let axis = parent.y + parent.height / 2.0;
            format!("M{:.1} {axis:.1}L{:.1} {axis:.1}", from.x + from.width, child.x)
        }
        // A bone, and the spine up to it from the head or the bones before.
        MapStyle::Fishbone if child.depth == 1 => {
            let axis = parent.y + parent.height / 2.0;
            let (edge, start) = bone(child, axis);
            let tip = child.x + child.width / 2.0;
            let bone = format!("M{start:.1} {axis:.1}L{tip:.1} {edge:.1}");
            let index = child.id.rsplit_once('.').and_then(|(_, n)| n.parse::<usize>().ok()).unwrap_or(0);
            if index % 2 == 1 {
                return bone;
            }
            let from = previous.map_or(parent.x, |p| self::bone(p, axis).1);
            format!("M{from:.1} {axis:.1}L{start:.1} {axis:.1}{bone}")
        }
        // From the bone across to the topic.
        MapStyle::Fishbone if child.depth == 2 => {
            let cy = child.y + child.height / 2.0;
            let edge = if cy > parent.y + parent.height { parent.y + parent.height } else { parent.y };
            let x = parent.x + parent.width / 2.0 + (cy - edge).abs() * SLANT;
            format!("M{x:.1} {cy:.1}L{:.1} {cy:.1}", child.x + child.width)
        }
        MapStyle::Tree | MapStyle::Timeline | MapStyle::Fishbone => {
            let x = parent.x + (parent.width / 2.0).min(16.0);
            let y = if cy < parent.y { parent.y } else { parent.y + parent.height };
            format!("M{x:.1} {y:.1}L{x:.1} {cy:.1}L{:.1} {cy:.1}", child.x)
        }
    }
}

/// Where every topic goes, folded topics hiding what is under them, and
/// where the extras go.
pub fn layout_map(root: &Topic, extras: &[Extra], style: MapStyle, folded: &[String]) -> Layout {
    let tree = tree(root, "0".to_string(), 0, folded);
    let (w, h) = tree.size;
    let mut topics = vec![placed(&tree, None, 0, -w / 2.0, -h / 2.0)];
    match style {
        MapStyle::Org => {
            let children: f64 = tree.children.iter().map(|c| span(c, true)).sum::<f64>()
                + GAP_X / 2.0 * tree.children.len().saturating_sub(1) as f64;
            let mut start = -children / 2.0;
            for child in &tree.children {
                place(child, Some("0"), 1, h / 2.0 + GAP_X, start, true, &mut topics);
                start += span(child, true) + GAP_X / 2.0;
            }
        }
        MapStyle::Tree => {
            let mut y = h / 2.0 + GAP_Y;
            for child in &tree.children {
                y = place_tree(child, "0", -w / 2.0 + INDENT, y, &mut topics);
            }
        }
        MapStyle::Timeline => {
            let mut x = w / 2.0 + GAP_X;
            for (i, child) in tree.children.iter().enumerate() {
                let (cw, ch) = child.size;
                topics.push(placed(child, Some("0"), 1, x, -ch / 2.0));
                // In turn above the line and below it.
                let stack: f64 = child.children.iter().map(|c| tree_height(c) + GAP_Y).sum();
                let mut y = if i % 2 == 1 { ch / 2.0 + GAP_Y } else { -ch / 2.0 - stack };
                for grandchild in &child.children {
                    y = place_tree(grandchild, &child.id, x + INDENT, y, &mut topics);
                }
                let reach = child.children.iter().map(|c| INDENT + tree_width(c)).fold(cw, f64::max);
                x += reach + GAP_X;
            }
        }
        MapStyle::Fishbone => {
            // The pairs of bones go left along the spine, each pair clear
            // of everything to its right.
            let mut reach = -w / 2.0;
            for pair in tree.children.chunks(2) {
                // How far each bone climbs: what is set along it, and a little.
                let heights: Vec<f64> = pair
                    .iter()
                    .map(|b| {
                        (b.children.iter().map(|c| tree_height(c) + GAP_Y).sum::<f64>() + GAP_Y).max(40.0)
                    })
                    .collect();
                // A main topic wider than its bone's run would reach back.
                let overhang =
                    pair.iter().zip(&heights).map(|(b, h)| b.size.0 / 2.0 - h * SLANT).fold(0.0, f64::max);
                let start = reach - GAP_X - overhang;
                let mut left = start;
                for (i, (branch, &height)) in pair.iter().zip(&heights).enumerate() {
                    let up = i == 0;
                    let (bw, bh) = branch.size;
                    let tip = start - height * SLANT;
                    let edge = if up { -height } else { height };
                    topics.push(placed(
                        branch,
                        Some("0"),
                        -1,
                        tip - bw / 2.0,
                        if up { edge - bh } else { edge },
                    ));
                    left = left.min(tip - bw / 2.0);
                    // Along the bone from the main topic towards the spine,
                    // each clear of the bone where it comes nearest.
                    let mut cursor = if up { edge + GAP_Y } else { edge - GAP_Y };
                    for sub in &branch.children {
                        let height = tree_height(sub);
                        let nearest = tip + (cursor - edge).abs() * SLANT;
                        let x = nearest - 12.0 - tree_width(sub);
                        place_tree(
                            sub,
                            &branch.id,
                            x,
                            if up { cursor } else { cursor - height },
                            &mut topics,
                        );
                        left = left.min(x);
                        cursor += if up { height + GAP_Y } else { -height - GAP_Y };
                    }
                }
                reach = left;
            }
            for topic in topics.iter_mut().skip(1) {
                topic.side = -1;
            }
        }
        MapStyle::Right | MapStyle::Logic => {
            // Each branch goes to the side with less on it so far.
            let mut sides: [Vec<&Tree>; 2] = [Vec::new(), Vec::new()];
            let mut heights = [0.0, 0.0];
            for child in &tree.children {
                let side = if style == MapStyle::Right || heights[1] <= heights[0] { 1 } else { 0 };
                heights[side] += span(child, false) + GAP_Y;
                sides[side].push(child);
            }
            for (side, branches) in sides.iter().enumerate() {
                let direction: i8 = if side == 1 { 1 } else { -1 };
                let total: f64 = branches.iter().map(|c| span(c, false)).sum::<f64>()
                    + GAP_Y * branches.len().saturating_sub(1) as f64;
                let mut start = -total / 2.0;
                let at = if direction > 0 { w / 2.0 + GAP_X } else { -w / 2.0 - GAP_X };
                for child in branches {
                    place(child, Some("0"), direction, at, start, false, &mut topics);
                    start += span(child, false) + GAP_Y;
                }
            }
        }
    }
    // Moved so the map, with its extras, starts at the margin.
    let shapes = extras::shapes(root, extras, style, &topics);
    let boxes = topics
        .iter()
        .map(|t| (t.x, t.y, t.x + t.width, t.y + t.height))
        .chain(shapes.iter().map(|s| s.bounds()));
    let (mut left, mut top, mut right, mut bottom) =
        (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for (x1, y1, x2, y2) in boxes {
        (left, top, right, bottom) = (left.min(x1), top.min(y1), right.max(x2), bottom.max(y2));
    }
    let (dx, dy) = (MARGIN - left, MARGIN - top);
    for topic in &mut topics {
        topic.x += dx;
        topic.y += dy;
    }
    let snapshot = topics.clone();
    let find = |id: &str| snapshot.iter().find(|t| t.id == id);
    for topic in &mut topics {
        let Some(parent) = topic.parent.as_deref().and_then(find) else { continue };
        // A timeline's main topics follow one another along the line.
        let previous = topic
            .id
            .rsplit_once('.')
            .and_then(|(_, n)| n.parse::<usize>().ok())
            .filter(|&n| n > 0 && topic.depth == 1)
            .and_then(|n| find(&format!("0.{}", n - 1)));
        topic.edge = edge_path(style, parent, topic, previous);
    }
    let extras = shapes.into_iter().map(|shape| shape.placed(dx, dy)).collect();
    Layout { topics, extras, width: right - left + 2.0 * MARGIN, height: bottom - top + 2.0 * MARGIN }
}

#[cfg(test)]
mod tests;
