//! What a map draws besides its branches, as XMind does: relations (a line
//! from any topic to another), summaries (a brace beside a run of sibling
//! topics, with what they come to) and boundaries (a frame around a run of
//! siblings and what is under them).
//!
//! They name topics by block ID (`- Topic ^t1`), which stays with a topic
//! when it moves; the centre is `centre`. The note keeps them in a comment
//! at its end, which Obsidian does not show:
//!
//! ```text
//! %% mindmap
//! relation t1 t2 leads to
//! summary t3 t5 Causes
//! boundary t3 t3 Early work
//! %%
//! ```

use std::ops::RangeInclusive;

use super::{MapStyle, Placed, Topic, measure, path_of, steps, topic_mut};
use crate::parse;

/// What extras call the centre.
pub const CENTRE: &str = "centre";

const OPEN: &str = "%% mindmap";
const CLOSE: &str = "%%";

/// How far a frame stands off what it takes in, and how deep a brace is.
const PAD: f64 = 8.0;
const BRACE: f64 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtraKind {
    Relation,
    Summary,
    Boundary,
}

impl ExtraKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ExtraKind::Relation => "relation",
            ExtraKind::Summary => "summary",
            ExtraKind::Boundary => "boundary",
        }
    }

    pub fn parse(word: &str) -> Option<Self> {
        match word.to_ascii_lowercase().as_str() {
            "relation" => Some(ExtraKind::Relation),
            "summary" => Some(ExtraKind::Summary),
            "boundary" => Some(ExtraKind::Boundary),
            _ => None,
        }
    }
}

/// A relation from `first` to `last`, or a summary or boundary taking in
/// the siblings from `first` to `last`; topics by block ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extra {
    pub kind: ExtraKind,
    pub first: String,
    pub last: String,
    pub text: String,
}

/// A change to a map's extras; topics by path, extras by their place in
/// the note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtraEdit {
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
    /// New text for an extra.
    Retext {
        index: usize,
        text: String,
    },
    Remove {
        index: usize,
    },
}

/// An extra placed on the map.
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedExtra {
    /// Its place among the note's extras, which edits name it by.
    pub index: usize,
    pub kind: ExtraKind,
    pub text: String,
    /// The topics it joins or takes in, by path.
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

// Reading and writing.

/// The lines of a body's extras comment, its first and last line too.
pub(super) fn block(body: &str) -> Option<RangeInclusive<usize>> {
    let mut lines = body.lines().enumerate();
    let (start, _) = lines.find(|(_, line)| line.trim() == OPEN)?;
    let end = lines.find(|(_, line)| line.trim() == CLOSE).map_or(body.lines().count() - 1, |(n, _)| n);
    Some(start..=end)
}

/// A note's relations, summaries and boundaries, in the order written.
pub fn read_extras(text: &str) -> Vec<Extra> {
    let body = &text[parse(text).body_start..];
    let Some(lines) = block(body) else { return Vec::new() };
    body.lines()
        .enumerate()
        .filter(|(n, _)| lines.contains(n) && *n != *lines.start())
        .filter_map(|(_, line)| {
            let mut parts = line.trim().splitn(4, ' ');
            let kind = ExtraKind::parse(parts.next()?)?;
            let first = parts.next()?.trim_start_matches('^');
            let last = parts.next()?.trim_start_matches('^');
            let text = parts.next().unwrap_or_default().trim();
            (!first.is_empty() && !last.is_empty()).then(|| Extra {
                kind,
                first: first.to_string(),
                last: last.to_string(),
                text: text.to_string(),
            })
        })
        .collect()
}

/// Writes the extras whose topics are on the map as the note's closing
/// comment.
pub(super) fn write(root: &Topic, extras: &[Extra], out: &mut String) {
    let known = anchors(root);
    let kept: Vec<&Extra> =
        extras.iter().filter(|e| known.contains(&e.first) && known.contains(&e.last)).collect();
    if kept.is_empty() {
        return;
    }
    out.push('\n');
    out.push_str(OPEN);
    out.push('\n');
    for extra in kept {
        let text = extra.text.split_whitespace().collect::<Vec<_>>().join(" ");
        let line = format!("{} {} {} {text}", extra.kind.as_str(), extra.first, extra.last);
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out.push_str(CLOSE);
    out.push('\n');
}

/// Every block ID on the map, the centre's too.
fn anchors(root: &Topic) -> Vec<String> {
    fn walk(topic: &Topic, out: &mut Vec<String>) {
        if !topic.anchor.is_empty() {
            out.push(topic.anchor.clone());
        }
        for child in &topic.children {
            walk(child, out);
        }
    }
    let mut out = vec![CENTRE.to_string()];
    for child in &root.children {
        walk(child, &mut out);
    }
    out
}

/// Each block ID with its topic's path.
fn paths(root: &Topic) -> Vec<(String, String)> {
    fn walk(topic: &Topic, path: String, out: &mut Vec<(String, String)>) {
        if !topic.anchor.is_empty() {
            out.push((topic.anchor.clone(), path.clone()));
        }
        for (i, child) in topic.children.iter().enumerate() {
            walk(child, format!("{path}.{i}"), out);
        }
    }
    let mut out = vec![(CENTRE.to_string(), "0".to_string())];
    for (i, child) in root.children.iter().enumerate() {
        walk(child, format!("0.{i}"), &mut out);
    }
    out
}

// Editing.

/// The block ID of the topic at a path, given one when it has none.
fn anchor_of(root: &mut Topic, path: &str) -> Result<String, String> {
    let at = steps(path)?;
    if at.is_empty() {
        return Ok(CENTRE.to_string());
    }
    let taken = anchors(root);
    let fresh = (1..).map(|n| format!("t{n}")).find(|a| !taken.contains(a)).unwrap_or_default();
    let topic = topic_mut(root, &at)?;
    if topic.anchor.is_empty() {
        topic.anchor = fresh;
    }
    Ok(topic.anchor.clone())
}

/// The block IDs of the first and last of a run of siblings, in order.
fn run(root: &mut Topic, first: &str, last: &str, what: &str) -> Result<(String, String), String> {
    let (mut a, mut b) = (steps(first)?, steps(last)?);
    if a.is_empty() || b.is_empty() || a.len() != b.len() || a[..a.len() - 1] != b[..b.len() - 1] {
        return Err(format!("A {what} takes in topics under the same parent."));
    }
    if a.last() > b.last() {
        std::mem::swap(&mut a, &mut b);
    }
    Ok((anchor_of(root, &path_of(&a))?, anchor_of(root, &path_of(&b))?))
}

/// Changes a map's extras, giving topics block IDs as they need them.
pub fn edit_extras(root: &mut Topic, extras: &mut Vec<Extra>, edit: &ExtraEdit) -> Result<(), String> {
    let line = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
    match edit {
        ExtraEdit::Relate { from, to, text } => {
            if from == to {
                return Err("A relation joins two different topics.".into());
            }
            let (first, last) = (anchor_of(root, from)?, anchor_of(root, to)?);
            extras.push(Extra { kind: ExtraKind::Relation, first, last, text: line(text) });
        }
        ExtraEdit::Summarize { first, last, text } => {
            let (first, last) = run(root, first, last, "summary")?;
            extras.push(Extra { kind: ExtraKind::Summary, first, last, text: line(text) });
        }
        ExtraEdit::Enclose { first, last, text } => {
            let (first, last) = run(root, first, last, "boundary")?;
            extras.push(Extra { kind: ExtraKind::Boundary, first, last, text: line(text) });
        }
        ExtraEdit::Retext { index, text } => {
            extras.get_mut(*index).ok_or("That is not on the map.")?.text = line(text);
        }
        ExtraEdit::Remove { index } => {
            if *index >= extras.len() {
                return Err("That is not on the map.".into());
            }
            extras.remove(*index);
        }
    }
    Ok(())
}

// Layout.

/// Where an extra goes, before the map is moved to its margin.
pub(super) struct Shape {
    index: usize,
    kind: ExtraKind,
    text: String,
    first: String,
    last: String,
    geometry: Geometry,
    /// Its text's box: left, top, width, height.
    label: (f64, f64, f64, f64),
}

enum Geometry {
    /// A curve from `start`, pulled towards `control`, to `end`.
    Curve {
        start: (f64, f64),
        control: (f64, f64),
        end: (f64, f64),
    },
    /// A brace at `at` (an x, or a y when `across`) from `from` to `to`,
    /// its point `towards` +1 or −1.
    Brace {
        at: f64,
        from: f64,
        to: f64,
        towards: f64,
        across: bool,
    },
    Frame {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
}

/// A small label's box: twelve-point text with a little room around it.
fn label_size(text: &str) -> (f64, f64) {
    if text.is_empty() {
        return (0.0, 0.0);
    }
    let width: f64 =
        text.chars().map(|c| if (c as u32) >= 0x1100 && !c.is_ascii() { 1.0 } else { 0.58 }).sum();
    (width * 12.0 + 16.0, 12.0 * 1.4 + 8.0)
}

/// Where a line from a box's centre towards a point leaves the box.
fn edge_toward(topic: &Placed, (px, py): (f64, f64)) -> (f64, f64) {
    let (cx, cy) = (topic.x + topic.width / 2.0, topic.y + topic.height / 2.0);
    let (dx, dy) = (px - cx, py - cy);
    let scale = [(topic.width / 2.0, dx), (topic.height / 2.0, dy)]
        .iter()
        .filter(|(_, d)| d.abs() > f64::EPSILON)
        .map(|(half, d)| half / d.abs())
        .fold(f64::INFINITY, f64::min);
    if scale.is_finite() { (cx + dx * scale, cy + dy * scale) } else { (cx, cy) }
}

/// The extras placed against the laid out topics; those with a topic gone,
/// folded away, or no longer siblings are left out.
pub(super) fn shapes(root: &Topic, extras: &[Extra], style: MapStyle, topics: &[Placed]) -> Vec<Shape> {
    let paths = paths(root);
    let named = |anchor: &str| paths.iter().find(|(a, _)| a == anchor).map(|(_, p)| p.as_str());
    let placed = |path: &str| topics.iter().find(|t| t.id == path);
    let mut shapes = Vec::new();
    for (index, extra) in extras.iter().enumerate() {
        let (Some(first), Some(last)) = (named(&extra.first), named(&extra.last)) else { continue };
        let (Some(a), Some(b)) = (placed(first), placed(last)) else { continue };
        let label = label_size(&extra.text);
        let (geometry, label) = match extra.kind {
            ExtraKind::Relation => {
                let centre = |t: &Placed| (t.x + t.width / 2.0, t.y + t.height / 2.0);
                let (start, end) = (edge_toward(a, centre(b)), edge_toward(b, centre(a)));
                let (dx, dy) = (end.0 - start.0, end.1 - start.1);
                let length = dx.hypot(dy);
                if length < 1.0 {
                    continue;
                }
                let bend = (length * 0.25).clamp(24.0, 90.0);
                let mid = ((start.0 + end.0) / 2.0, (start.1 + end.1) / 2.0);
                let control = (mid.0 - dy / length * bend, mid.1 + dx / length * bend);
                // The curve's own middle.
                let (lx, ly) = (
                    0.25 * start.0 + 0.5 * control.0 + 0.25 * end.0,
                    0.25 * start.1 + 0.5 * control.1 + 0.25 * end.1,
                );
                (
                    Geometry::Curve { start, control, end },
                    (lx - label.0 / 2.0, ly - label.1 / 2.0, label.0, label.1),
                )
            }
            ExtraKind::Summary | ExtraKind::Boundary => {
                let Some((parent, i)) = first.rsplit_once('.') else { continue };
                let Some((other, j)) = last.rsplit_once('.') else { continue };
                let (Ok(i), Ok(j)) = (i.parse::<usize>(), j.parse::<usize>()) else { continue };
                if parent != other {
                    continue;
                }
                // The run's topics and everything shown under them.
                let run: Vec<String> = (i.min(j)..=i.max(j)).map(|k| format!("{parent}.{k}")).collect();
                let inside = topics.iter().filter(|t| {
                    run.iter().any(|r| {
                        t.id == *r || t.id.strip_prefix(r.as_str()).is_some_and(|s| s.starts_with('.'))
                    })
                });
                let (mut x1, mut y1, mut x2, mut y2) =
                    (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
                for t in inside {
                    (x1, y1, x2, y2) =
                        (x1.min(t.x), y1.min(t.y), x2.max(t.x + t.width), y2.max(t.y + t.height));
                }
                if extra.kind == ExtraKind::Boundary {
                    let (x, y) = (x1 - PAD, y1 - PAD);
                    let frame =
                        Geometry::Frame { x, y, width: x2 - x1 + 2.0 * PAD, height: y2 - y1 + 2.0 * PAD };
                    (frame, (x, y - label.1, label.0, label.1))
                } else {
                    // A summary reads like a topic, beside the brace.
                    let size = if extra.text.is_empty() { (0.0, 0.0) } else { measure(&extra.text, 2) };
                    if style == MapStyle::Org {
                        let at = y2 + PAD;
                        let mid = (x1 + x2) / 2.0;
                        let brace = Geometry::Brace { at, from: x1, to: x2, towards: 1.0, across: true };
                        (brace, (mid - size.0 / 2.0, at + BRACE + PAD, size.0, size.1))
                    } else {
                        let left = a.side < 0 || b.side < 0;
                        let mid = (y1 + y2) / 2.0;
                        let (at, towards) = if left { (x1 - PAD, -1.0) } else { (x2 + PAD, 1.0) };
                        let x = if left { at - BRACE - PAD - size.0 } else { at + BRACE + PAD };
                        let brace = Geometry::Brace { at, from: y1, to: y2, towards, across: false };
                        (brace, (x, mid - size.1 / 2.0, size.0, size.1))
                    }
                }
            }
        };
        shapes.push(Shape {
            index,
            kind: extra.kind,
            text: extra.text.clone(),
            first: first.to_string(),
            last: last.to_string(),
            geometry,
            label,
        });
    }
    shapes
}

impl Shape {
    /// Left, top, right and bottom of the shape and its text.
    pub(super) fn bounds(&self) -> (f64, f64, f64, f64) {
        let mut points = match self.geometry {
            Geometry::Curve { start, control, end } => vec![start, control, end],
            Geometry::Brace { at, from, to, towards, across } => {
                let out = at + BRACE * towards;
                if across { vec![(from, at), (to, out)] } else { vec![(at, from), (out, to)] }
            }
            Geometry::Frame { x, y, width, height } => vec![(x, y), (x + width, y + height)],
        };
        let (lx, ly, lw, lh) = self.label;
        if lw > 0.0 {
            points.extend([(lx, ly), (lx + lw, ly + lh)]);
        }
        points.iter().fold(
            (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
            |(x1, y1, x2, y2), &(x, y)| (x1.min(x), y1.min(y), x2.max(x), y2.max(y)),
        )
    }

    /// The extra moved by `dx`, `dy`, with its path drawn.
    pub(super) fn placed(self, dx: f64, dy: f64) -> PlacedExtra {
        let path = match self.geometry {
            Geometry::Curve { start, control, end } => format!(
                "M{:.1} {:.1}Q{:.1} {:.1} {:.1} {:.1}",
                start.0 + dx,
                start.1 + dy,
                control.0 + dx,
                control.1 + dy,
                end.0 + dx,
                end.1 + dy
            ),
            Geometry::Brace { at, from, to, towards, across } => {
                let r = ((to - from) / 4.0).clamp(0.0, 8.0);
                let mid = (from + to) / 2.0;
                // A point `along` the brace and `out` from its back.
                let point = |along: f64, out: f64| {
                    let o = at + out * towards;
                    if across { (along + dx, o + dy) } else { (o + dx, along + dy) }
                };
                let half = BRACE / 2.0;
                let p = [
                    point(from, 0.0),
                    point(from, half),
                    point(from + r, half),
                    point(mid - r, half),
                    point(mid, half),
                    point(mid, BRACE),
                    point(mid + r, half),
                    point(to - r, half),
                    point(to, half),
                    point(to, 0.0),
                ];
                format!(
                    "M{:.1} {:.1}Q{:.1} {:.1} {:.1} {:.1}L{:.1} {:.1}Q{:.1} {:.1} {:.1} {:.1}Q{:.1} {:.1} {:.1} {:.1}L{:.1} {:.1}Q{:.1} {:.1} {:.1} {:.1}",
                    p[0].0,
                    p[0].1,
                    p[1].0,
                    p[1].1,
                    p[2].0,
                    p[2].1,
                    p[3].0,
                    p[3].1,
                    p[4].0,
                    p[4].1,
                    p[5].0,
                    p[5].1,
                    p[4].0,
                    p[4].1,
                    p[6].0,
                    p[6].1,
                    p[7].0,
                    p[7].1,
                    p[8].0,
                    p[8].1,
                    p[9].0,
                    p[9].1
                )
            }
            Geometry::Frame { x, y, width, height } => {
                let (x, y, r) = (x + dx, y + dy, 10.0_f64.min(width / 2.0).min(height / 2.0));
                let (right, bottom) = (x + width, y + height);
                format!(
                    "M{:.1} {y:.1}H{:.1}Q{right:.1} {y:.1} {right:.1} {:.1}V{:.1}Q{right:.1} {bottom:.1} {:.1} {bottom:.1}H{:.1}Q{x:.1} {bottom:.1} {x:.1} {:.1}V{:.1}Q{x:.1} {y:.1} {:.1} {y:.1}Z",
                    x + r,
                    right - r,
                    y + r,
                    bottom - r,
                    right - r,
                    x + r,
                    bottom - r,
                    y + r,
                    x + r
                )
            }
        };
        let (x, y, width, height) = self.label;
        PlacedExtra {
            index: self.index,
            kind: self.kind,
            text: self.text,
            first: self.first,
            last: self.last,
            path,
            x: x + dx,
            y: y + dy,
            width,
            height,
        }
    }
}
