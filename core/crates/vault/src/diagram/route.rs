//! The line an edge takes: from its source shape's outline, through its
//! bends, to its target's; straight, or in horizontal and vertical runs
//! (draw.io's orthogonal and elbow styles), with the arrowheads at its
//! ends.

use super::model::num;
use super::shapes::{Rect, perimeter};
use super::style::Style;

/// How far an orthogonal line runs straight out of a fixed connection
/// point before it turns.
const STUB: f64 = 20.0;

/// One end of an edge: a shape (its box and style), or a loose point.
pub enum End<'a> {
    Shape(Rect, &'a Style),
    Point(f64, f64),
}

impl End<'_> {
    fn centre(&self) -> (f64, f64) {
        match self {
            End::Shape(r, _) => r.centre(),
            End::Point(x, y) => (*x, *y),
        }
    }
}

/// Whether an edge style runs in horizontal and vertical lines.
pub fn orthogonal(style: &Style) -> bool {
    matches!(
        style.get("edgeStyle").unwrap_or_default(),
        "orthogonalEdgeStyle"
            | "elbowEdgeStyle"
            | "entityRelationEdgeStyle"
            | "segmentEdgeStyle"
            | "isometricEdgeStyle"
    )
}

/// A fixed connection point on a shape (`exitX`/`exitY` or
/// `entryX`/`entryY`), and the way out of the shape there.
fn fixed(style: &Style, prefix: &str, end: &End) -> Option<((f64, f64), (f64, f64))> {
    let End::Shape(r, _) = end else { return None };
    let fx = style.get(&format!("{prefix}X"))?.parse::<f64>().ok()?;
    let fy = style.get(&format!("{prefix}Y"))?.parse::<f64>().ok()?;
    let dx = style.number(&format!("{prefix}Dx"), 0.0);
    let dy = style.number(&format!("{prefix}Dy"), 0.0);
    let at = (r.x + r.width * fx + dx, r.y + r.height * fy + dy);
    // Out through the nearest side.
    let distances = [(fx, (-1.0, 0.0)), (1.0 - fx, (1.0, 0.0)), (fy, (0.0, -1.0)), (1.0 - fy, (0.0, 1.0))];
    let way = distances
        .iter()
        .fold((f64::INFINITY, (1.0, 0.0)), |best, &(d, w)| if d < best.0 { (d, w) } else { best })
        .1;
    Some((at, way))
}

/// Where a line from an end towards `toward` starts.
fn anchor(end: &End, toward: (f64, f64)) -> (f64, f64) {
    match end {
        End::Shape(r, style) => perimeter(style, *r, toward),
        End::Point(x, y) => (*x, *y),
    }
}

/// Points to the hundredth, with repeats and straight-through middles
/// taken out.
fn simplify(points: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    let mut out: Vec<(f64, f64)> = Vec::with_capacity(points.len());
    for (x, y) in points {
        let p = ((x * 100.0).round() / 100.0, (y * 100.0).round() / 100.0);
        if out.last().is_some_and(|q| (q.0 - p.0).abs() < 0.01 && (q.1 - p.1).abs() < 0.01) {
            continue;
        }
        if out.len() >= 2 {
            let (a, b) = (out[out.len() - 2], out[out.len() - 1]);
            let straight = ((a.0 - b.0).abs() < 0.01 && (b.0 - p.0).abs() < 0.01)
                || ((a.1 - b.1).abs() < 0.01 && (b.1 - p.1).abs() < 0.01);
            if straight {
                out.pop();
            }
        }
        out.push(p);
    }
    out
}

/// From `a` to `b` in a horizontal then vertical then horizontal run (or
/// the other way round), turning halfway.
fn zigzag(a: (f64, f64), b: (f64, f64), horizontal: bool) -> Vec<(f64, f64)> {
    if horizontal {
        let mx = (a.0 + b.0) / 2.0;
        vec![a, (mx, a.1), (mx, b.1), b]
    } else {
        let my = (a.1 + b.1) / 2.0;
        vec![a, (a.0, my), (b.0, my), b]
    }
}

/// The side of a box facing a point, as the way out of it, and the middle
/// of that side.
fn facing(r: Rect, toward: (f64, f64), horizontal: bool) -> ((f64, f64), (f64, f64)) {
    let (cx, cy) = r.centre();
    if horizontal {
        if toward.0 >= cx { ((r.right(), cy), (1.0, 0.0)) } else { ((r.x, cy), (-1.0, 0.0)) }
    } else if toward.1 >= cy {
        ((cx, r.bottom()), (0.0, 1.0))
    } else {
        ((cx, r.y), (0.0, -1.0))
    }
}

/// The edge's line, point by point.
pub fn route(style: &Style, source: &End, target: &End, bends: &[(f64, f64)]) -> Vec<(f64, f64)> {
    let first_toward = bends.first().copied().unwrap_or_else(|| target.centre());
    let last_toward = bends.last().copied().unwrap_or_else(|| source.centre());
    let exit = fixed(style, "exit", source);
    let entry = fixed(style, "entry", target);
    if !orthogonal(style) {
        let start = exit.map_or_else(|| anchor(source, first_toward), |(at, _)| at);
        let end = entry.map_or_else(|| anchor(target, last_toward), |(at, _)| at);
        let mut points = vec![start];
        points.extend_from_slice(bends);
        points.push(end);
        return simplify(points);
    }
    if !bends.is_empty() {
        // Through each bend, turning at corners.
        let start = exit.map_or_else(|| anchor(source, first_toward), |(at, _)| at);
        let end = entry.map_or_else(|| anchor(target, last_toward), |(at, _)| at);
        let mut stops = vec![start];
        stops.extend_from_slice(bends);
        stops.push(end);
        let mut points = vec![start];
        for pair in stops.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            if (a.0 - b.0).abs() > 0.01 && (a.1 - b.1).abs() > 0.01 {
                points.push((b.0, a.1));
            }
            points.push(b);
        }
        return simplify(points);
    }
    let (sc, tc) = (source.centre(), target.centre());
    // Facing each other across the wider gap.
    let horizontal = match (source, target) {
        (End::Shape(a, _), End::Shape(b, _)) => {
            let gap_x = (b.x - a.right()).max(a.x - b.right());
            let gap_y = (b.y - a.bottom()).max(a.y - b.bottom());
            gap_x >= gap_y
        }
        _ => (tc.0 - sc.0).abs() >= (tc.1 - sc.1).abs(),
    };
    if exit.is_none() && entry.is_none() {
        let start = match source {
            End::Shape(r, _) => facing(*r, tc, horizontal).0,
            End::Point(x, y) => (*x, *y),
        };
        let end = match target {
            End::Shape(r, _) => facing(*r, sc, horizontal).0,
            End::Point(x, y) => (*x, *y),
        };
        return simplify(zigzag(start, end, horizontal));
    }
    // A fixed point runs straight out a little before it turns.
    let out = |end: &End, fixed: Option<((f64, f64), (f64, f64))>, toward: (f64, f64)| match (fixed, end) {
        (Some(f), _) => f,
        (None, End::Shape(r, _)) => facing(*r, toward, horizontal),
        (None, End::Point(x, y)) => ((*x, *y), (0.0, 0.0)),
    };
    let (start, way_out) = out(source, exit, tc);
    let (end, way_in) = out(target, entry, sc);
    let a = (start.0 + way_out.0 * STUB, start.1 + way_out.1 * STUB);
    let b = (end.0 + way_in.0 * STUB, end.1 + way_in.1 * STUB);
    let mut points = vec![start];
    let loose = way_out.0.abs() + way_out.1.abs() < 0.5;
    let across = way_out.0.abs() > 0.5 || (loose && horizontal);
    // A target behind the way out is reached round the side.
    let behind = if across { (b.0 - a.0) * way_out.0 < 0.0 } else { (b.1 - a.1) * way_out.1 < 0.0 };
    points.extend(zigzag(a, b, across != behind));
    points.push(end);
    simplify(points)
}

/// The line through the points as SVG path data: straight runs, or (when
/// `curved`) a smooth curve through the middles of the runs.
pub fn line_path(points: &[(f64, f64)], curved: bool) -> String {
    let Some(&(x, y)) = points.first() else { return String::new() };
    let mut path = format!("M{} {}", num(x), num(y));
    if curved && points.len() > 2 {
        // Each bend pulls the curve, which runs through the runs' middles.
        for (k, pair) in points[1..].windows(2).enumerate() {
            let ((cx, cy), (nx, ny)) = (pair[0], pair[1]);
            let last = k + 2 == points.len() - 1;
            let (mx, my) = if last { (nx, ny) } else { ((cx + nx) / 2.0, (cy + ny) / 2.0) };
            path.push_str(&format!("Q{} {} {} {}", num(cx), num(cy), num(mx), num(my)));
        }
    } else {
        for &(px, py) in &points[1..] {
            path.push_str(&format!("L{} {}", num(px), num(py)));
        }
    }
    path
}

/// Leaves a gap for the open circles on optional ER ends.
pub fn marker_inset(kind: &str, size: f64, stroke: f64) -> f64 {
    if matches!(kind, "ERzeroToMany" | "ERzeroToOne") {
        2.0 * (size + stroke + 1.0) - stroke / 2.0
    } else {
        0.0
    }
}

/// Trims along the runs, keeping the original anchors for editing.
pub fn trim_line(points: &[(f64, f64)], start: f64, end: f64) -> Vec<(f64, f64)> {
    let total: f64 = points.windows(2).map(|p| (p[1].0 - p[0].0).hypot(p[1].1 - p[0].1)).sum();
    if total < 1e-9 || start + end < 1e-9 {
        return points.to_vec();
    }
    let scale = (total / (start + end)).min(1.0);
    let (first, last) = (start * scale, total - end * scale);
    let mut out = Vec::new();
    let mut distance = 0.0;
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let length = (b.0 - a.0).hypot(b.1 - a.1);
        if length > 1e-9 && distance + length >= first && distance <= last {
            for at in [first.max(distance), last.min(distance + length)] {
                let t = (at - distance) / length;
                let p = (a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t);
                if out.last() != Some(&p) {
                    out.push(p);
                }
            }
        }
        distance += length;
    }
    out
}

/// An arrowhead at `tip`, coming from `from`: its outline, and whether it
/// is filled. `none` and unknown kinds draw nothing.
pub fn arrowhead(
    kind: &str,
    filled: bool,
    size: f64,
    tip: (f64, f64),
    from: (f64, f64),
    stroke: f64,
) -> Option<(String, bool)> {
    let (dx, dy) = (tip.0 - from.0, tip.1 - from.1);
    let length = dx.hypot(dy);
    if length < 1e-9 {
        return None;
    }
    let (ux, uy) = (dx / length, dy / length);
    // Back along the line, and out to either side.
    let at = |back: f64, side: f64| (tip.0 - ux * back - uy * side, tip.1 - uy * back + ux * side);
    if matches!(kind, "ERone" | "ERmandOne" | "ERmany" | "ERoneToMany" | "ERzeroToMany" | "ERzeroToOne") {
        let span = size + stroke + 1.0;
        let mut path = String::new();
        let mut segment = |a: (f64, f64), b: (f64, f64)| {
            path.push_str(&line_path(&[a, b], false));
        };
        if matches!(kind, "ERone" | "ERmandOne" | "ERzeroToOne") {
            segment(at(span / 2.0, span / 2.0), at(span / 2.0, -span / 2.0));
        }
        if matches!(kind, "ERmandOne" | "ERoneToMany") {
            segment(at(span, span / 2.0), at(span, -span / 2.0));
        }
        if matches!(kind, "ERmany" | "ERoneToMany" | "ERzeroToMany") {
            segment(at(0.0, -span / 2.0), at(span, 0.0));
            segment(at(span, 0.0), at(0.0, span / 2.0));
        }
        if matches!(kind, "ERzeroToMany" | "ERzeroToOne") {
            // The detached stem reaches the anchor without crossing the circle.
            let stem = if kind == "ERzeroToOne" { span + stroke / 2.0 } else { span };
            segment(at(stem, 0.0), tip);
            let centre = at(span * 1.5, 0.0);
            let circle = super::shapes::outline(
                &Style::parse("ellipse;"),
                Rect { x: centre.0 - size / 2.0, y: centre.1 - size / 2.0, width: size, height: size },
            );
            path.push_str(&circle.path);
        }
        return Some((path, false));
    }
    let points: Vec<(f64, f64)> = match kind {
        "classic" | "classicThin" => {
            vec![tip, at(size, size * 0.45), at(size * 0.7, 0.0), at(size, -size * 0.45)]
        }
        "block" | "blockThin" => vec![tip, at(size, size * 0.45), at(size, -size * 0.45)],
        "open" | "openThin" | "openAsync" => {
            let (a, b) = (at(size, size * 0.45), at(size, -size * 0.45));
            let path =
                format!("M{} {}L{} {}L{} {}", num(a.0), num(a.1), num(tip.0), num(tip.1), num(b.0), num(b.1));
            return Some((path, false));
        }
        "diamond" | "diamondThin" => {
            vec![tip, at(size * 0.5, size * 0.35), at(size, 0.0), at(size * 0.5, -size * 0.35)]
        }
        "oval" | "circle" | "circlePlus" | "dot" => {
            let (r, centre) = (size * 0.35, at(size * 0.35, 0.0));
            (0..12)
                .map(|i| {
                    let a = f64::from(i) * std::f64::consts::PI / 6.0;
                    (centre.0 + r * a.cos(), centre.1 + r * a.sin())
                })
                .collect()
        }
        _ => return None,
    };
    let mut path = String::new();
    for (i, (x, y)) in points.iter().enumerate() {
        path.push_str(&format!("{}{} {}", if i == 0 { 'M' } else { 'L' }, num(*x), num(*y)));
    }
    path.push('Z');
    Some((path, filled))
}
