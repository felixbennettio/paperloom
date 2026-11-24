//! The shapes a diagram draws, as SVG path data made of `M`, `L`, `Q`, `C`
//! and `Z` (which every app draws as it is), and where a line meets each
//! shape's outline.

use super::model::num;
use super::style::Style;

/// A box on the page.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub fn centre(&self) -> (f64, f64) {
        (self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    pub fn right(&self) -> f64 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }

    pub fn contains(&self, (x, y): (f64, f64)) -> bool {
        x >= self.x && x <= self.right() && y >= self.y && y <= self.bottom()
    }
}

/// A shape's outline, and the lines drawn inside it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Outline {
    pub path: String,
    pub inner: String,
    /// Details filled with the stroke colour (state dots and markers).
    pub solid: String,
    /// Whether the outline is filled; a line or a text has none to fill.
    pub filled: bool,
}

/// SVG path data, written as it is built.
#[derive(Default)]
struct Pen(String);

impl Pen {
    fn m(&mut self, x: f64, y: f64) -> &mut Self {
        self.0.push_str(&format!("M{} {}", num(x), num(y)));
        self
    }

    fn l(&mut self, x: f64, y: f64) -> &mut Self {
        self.0.push_str(&format!("L{} {}", num(x), num(y)));
        self
    }

    fn q(&mut self, cx: f64, cy: f64, x: f64, y: f64) -> &mut Self {
        self.0.push_str(&format!("Q{} {} {} {}", num(cx), num(cy), num(x), num(y)));
        self
    }

    fn c(&mut self, points: [f64; 6]) -> &mut Self {
        let [a, b, c, d, e, f] = points.map(num);
        self.0.push_str(&format!("C{a} {b} {c} {d} {e} {f}"));
        self
    }

    fn z(&mut self) -> &mut Self {
        self.0.push('Z');
        self
    }

    /// A closed polygon.
    fn polygon(&mut self, points: &[(f64, f64)]) -> &mut Self {
        for (i, &(x, y)) in points.iter().enumerate() {
            if i == 0 {
                self.m(x, y);
            } else {
                self.l(x, y);
            }
        }
        self.z()
    }

    /// An ellipse in four curves.
    fn ellipse(&mut self, cx: f64, cy: f64, rx: f64, ry: f64) -> &mut Self {
        const K: f64 = 0.552_284_75;
        let (kx, ky) = (rx * K, ry * K);
        self.m(cx + rx, cy)
            .c([cx + rx, cy + ky, cx + kx, cy + ry, cx, cy + ry])
            .c([cx - kx, cy + ry, cx - rx, cy + ky, cx - rx, cy])
            .c([cx - rx, cy - ky, cx - kx, cy - ry, cx, cy - ry])
            .c([cx + kx, cy - ry, cx + rx, cy - ky, cx + rx, cy])
            .z()
    }

    fn rounded(&mut self, r: Rect, radius: f64) -> &mut Self {
        let radius = radius.min(r.width / 2.0).min(r.height / 2.0).max(0.0);
        let (x, y, right, bottom) = (r.x, r.y, r.right(), r.bottom());
        self.m(x + radius, y)
            .l(right - radius, y)
            .q(right, y, right, y + radius)
            .l(right, bottom - radius)
            .q(right, bottom, right - radius, bottom)
            .l(x + radius, bottom)
            .q(x, bottom, x, bottom - radius)
            .l(x, y + radius)
            .q(x, y, x + radius, y)
            .z()
    }

    fn done(&mut self) -> String {
        std::mem::take(&mut self.0)
    }
}

/// What a shape is drawn as: draw.io's names, and the common flowchart
/// shapes mapped onto them; anything else is a rectangle.
pub fn kind(style: &Style) -> &str {
    match style.shape() {
        "mxgraph.basic.star" => "star",
        "mxgraph.basic.pentagon" => "pentagon",
        "mxgraph.basic.octagon2" => "octagon",
        "mxgraph.bpmn.task" | "mxgraph.bpmn.task2" => "bpmnTask",
        "mxgraph.bpmn.data" | "mxgraph.bpmn.data2" => "note",
        "mxgraph.bpmn.swimlane" => "swimlane",
        "mxgraph.bpmn.gateway2" => match style.get("gwType") {
            Some("exclusive") => "exclusiveGateway",
            Some("parallel") => "parallelGateway",
            _ if style.get("outline") == Some("end") => "inclusiveGateway",
            _ => "rhombus",
        },
        "mxgraph.bpmn.shape" if style.get("background") == Some("gateway") => match style.get("symbol") {
            Some("exclusiveGw") => "exclusiveGateway",
            Some("parallelGw") => "parallelGateway",
            _ if style.get("outline") == Some("end") => "inclusiveGateway",
            _ => "rhombus",
        },
        "mxgraph.bpmn.shape" | "mxgraph.bpmn.event" => match style.get("outline") {
            Some("catching" | "throwing" | "boundInt" | "boundNonint") => "doubleEllipse",
            Some("end") => "bpmnEnd",
            _ => "ellipse",
        },
        "mxgraph.flowchart.terminator" => "terminator",
        "mxgraph.flowchart.decision" => "rhombus",
        "mxgraph.flowchart.process" => "rectangle",
        "mxgraph.flowchart.document" => "document",
        "mxgraph.flowchart.data" => "parallelogram",
        "mxgraph.flowchart.database" | "mxgraph.flowchart.stored_data" | "cylinder3" | "datastore" => {
            "cylinder"
        }
        "mxgraph.flowchart.predefined_process" => "process",
        "mxgraph.flowchart.preparation" => "hexagon",
        "mxgraph.flowchart.manual_input" => "manualInput",
        "mxgraph.flowchart.delay" => "delay",
        "mxgraph.flowchart.start_1" | "mxgraph.flowchart.start_2" | "mxgraph.flowchart.on-page_reference" => {
            "ellipse"
        }
        "mxgraph.flowchart.off-page_reference" | "offPageConnector" => "offPage",
        "mxgraph.flowchart.merge_or_storage" => "triangleDown",
        "mxgraph.flowchart.extract_or_measurement" => "triangleUp",
        "mxgraph.flowchart.annotation_1" | "mxgraph.flowchart.annotation_2" => "annotation",
        other => match other {
            "ellipse"
            | "doubleEllipse"
            | "rhombus"
            | "triangle"
            | "hexagon"
            | "parallelogram"
            | "trapezoid"
            | "cylinder"
            | "document"
            | "process"
            | "cloud"
            | "umlActor"
            | "note"
            | "card"
            | "step"
            | "swimlane"
            | "text"
            | "line"
            | "image"
            | "label"
            | "manualInput"
            | "delay"
            | "folder"
            | "component"
            | "singleArrow"
            | "doubleArrow"
            | "callout"
            | "cross"
            | "tape"
            | "cube"
            | "curlyBracket"
            | "dataStorage"
            | "internalStorage"
            | "or"
            | "xor"
            | "sumEllipse"
            | "orEllipse"
            | "plus"
            | "partialRectangle"
            | "parallelMarker"
            | "waypoint"
            | "umlLifeline"
            | "umlFrame"
            | "umlBoundary"
            | "umlEntity"
            | "umlControl"
            | "providedRequiredInterface"
            | "requiredInterface"
            | "lollipop"
            | "umlState"
            | "endState"
            | "umlDestroy" => other,
            _ if style.has("group") => "group",
            _ => "rectangle",
        },
    }
}

/// A shape's outline and inner lines in a box.
pub fn outline(style: &Style, r: Rect) -> Outline {
    let (x, y, w, h) = (r.x, r.y, r.width, r.height);
    let (right, bottom, cx, cy) = (r.right(), r.bottom(), x + w / 2.0, y + h / 2.0);
    let mut pen = Pen::default();
    let mut inner = Pen::default();
    let mut solid = Pen::default();
    let mut filled = true;
    // A size given as a share of the width, as draw.io's `size`.
    // With `fixedSize`, an absolute size instead.
    let share = |default: f64| {
        if style.flag("fixedSize", false) {
            style.number("size", 20.0).clamp(0.0, w / 2.0)
        } else {
            style.number("size", default).clamp(0.0, 0.5) * w
        }
    };
    match kind(style) {
        "ellipse" | "bpmnEnd" => {
            pen.ellipse(cx, cy, w / 2.0, h / 2.0);
        }
        "endState" => {
            filled = false;
            pen.ellipse(cx, cy, w / 2.0, h / 2.0);
            let inset = 4.0_f64.min(w / 5.0).min(h / 5.0);
            solid.ellipse(cx, cy, w / 2.0 - inset, h / 2.0 - inset);
        }
        "sumEllipse" | "orEllipse" => {
            pen.ellipse(cx, cy, w / 2.0, h / 2.0);
            if kind(style) == "sumEllipse" {
                inner
                    .m(x + w * 0.145, y + h * 0.145)
                    .l(x + w * 0.855, y + h * 0.855)
                    .m(x + w * 0.855, y + h * 0.145)
                    .l(x + w * 0.145, y + h * 0.855);
            } else {
                inner.m(x, cy).l(right, cy).m(cx, y).l(cx, bottom);
            }
        }
        "doubleEllipse" => {
            pen.ellipse(cx, cy, w / 2.0, h / 2.0);
            inner.ellipse(cx, cy, (w / 2.0 - 4.0).max(0.0), (h / 2.0 - 4.0).max(0.0));
        }
        "rhombus" => {
            pen.polygon(&[(cx, y), (right, cy), (cx, bottom), (x, cy)]);
        }
        "exclusiveGateway" | "parallelGateway" | "inclusiveGateway" => {
            pen.polygon(&[(cx, y), (right, cy), (cx, bottom), (x, cy)]);
            match kind(style) {
                "exclusiveGateway" => {
                    let points = [
                        (0.105, 0.0),
                        (0.5, 0.38),
                        (0.895, 0.0),
                        (1.0, 0.11),
                        (0.6172, 0.5),
                        (1.0, 0.89),
                        (0.895, 1.0),
                        (0.5, 0.62),
                        (0.105, 1.0),
                        (0.0, 0.89),
                        (0.3808, 0.5),
                        (0.0, 0.11),
                    ]
                    .map(|(px, py)| (x + w * (0.31 + px * 0.38), y + h * (0.25 + py * 0.5)));
                    solid.polygon(&points);
                }
                "parallelGateway" => {
                    let points = [
                        (0.38, 0.0),
                        (0.62, 0.0),
                        (0.62, 0.38),
                        (1.0, 0.38),
                        (1.0, 0.62),
                        (0.62, 0.62),
                        (0.62, 1.0),
                        (0.38, 1.0),
                        (0.38, 0.62),
                        (0.0, 0.62),
                        (0.0, 0.38),
                        (0.38, 0.38),
                    ]
                    .map(|(px, py)| (x + w * (0.25 + px * 0.5), y + h * (0.25 + py * 0.5)));
                    solid.polygon(&points);
                }
                _ => {
                    let share = if style.shape() == "mxgraph.bpmn.gateway2" { 0.3 } else { 0.25 };
                    let (rx, ry) = (w * share, h * share);
                    let thickness = style.number("strokeWidth", 1.0).min(rx).min(ry);
                    solid.ellipse(cx, cy, rx, ry).ellipse(cx, cy, rx - thickness, -(ry - thickness));
                }
            }
        }
        "pentagon" => {
            pen.polygon(&[
                (cx, y),
                (right, y + h * 0.38),
                (x + w * 0.81, bottom),
                (x + w * 0.19, bottom),
                (x, y + h * 0.38),
            ]);
        }
        "star" => {
            pen.polygon(&[
                (cx, y),
                (x + w * 0.61, y + h * 0.35),
                (right, y + h * 0.35),
                (x + w * 0.68, y + h * 0.59),
                (x + w * 0.8, bottom),
                (cx, y + h * 0.75),
                (x + w * 0.2, bottom),
                (x + w * 0.32, y + h * 0.59),
                (x, y + h * 0.35),
                (x + w * 0.39, y + h * 0.35),
            ]);
        }
        "octagon" => {
            let s = (style.number("dx", 0.5).max(0.0) * 2.0).min(w / 2.0).min(h / 2.0);
            pen.polygon(&[
                (x + s, y),
                (right - s, y),
                (right, y + s),
                (right, bottom - s),
                (right - s, bottom),
                (x + s, bottom),
                (x, bottom - s),
                (x, y + s),
            ]);
        }
        "tape" => {
            let dy = h * style.number("size", 0.4).clamp(0.0, 1.0);
            pen.m(x, y + dy / 2.0)
                .q(x + w / 4.0, y + dy * 1.4, cx, y + dy / 2.0)
                .q(x + w * 0.75, y - dy * 0.4, right, y + dy / 2.0)
                .l(right, bottom - dy / 2.0)
                .q(x + w * 0.75, bottom - dy * 1.4, cx, bottom - dy / 2.0)
                .q(x + w / 4.0, bottom + dy * 0.4, x, bottom - dy / 2.0)
                .z();
        }
        "cube" => {
            let s = style.number("size", 20.0).clamp(0.0, w.min(h));
            pen.polygon(&[
                (x, y),
                (right - s, y),
                (right, y + s),
                (right, bottom),
                (x + s, bottom),
                (x, bottom - s),
            ]);
            inner.m(x + s, bottom).l(x + s, y + s).l(x, y).m(x + s, y + s).l(right, y + s);
        }
        "curlyBracket" => {
            filled = false;
            let s = w * style.number("size", 0.5).clamp(0.0, 1.0);
            if style.flag("rounded", false) {
                let radius = (style.number("arcSize", 20.0) / 2.0).max(0.0).min(s).min(w - s).min(h / 4.0);
                pen.m(right, y)
                    .l(x + s + radius, y)
                    .q(x + s, y, x + s, y + radius)
                    .l(x + s, cy - radius)
                    .q(x + s, cy, x + s - radius, cy)
                    .l(x, cy)
                    .l(x + s - radius, cy)
                    .q(x + s, cy, x + s, cy + radius)
                    .l(x + s, bottom - radius)
                    .q(x + s, bottom, x + s + radius, bottom)
                    .l(right, bottom);
            } else {
                pen.m(right, y)
                    .l(x + s, y)
                    .l(x + s, cy)
                    .l(x, cy)
                    .l(x + s, cy)
                    .l(x + s, bottom)
                    .l(right, bottom);
            }
        }
        "dataStorage" => {
            let s = if style.flag("fixedSize", false) {
                style.number("size", 20.0).clamp(0.0, w)
            } else {
                w * style.number("size", 0.1).clamp(0.0, 1.0)
            };
            pen.m(x + s, y)
                .l(right, y)
                .q(right - s * 2.0, cy, right, bottom)
                .l(x + s, bottom)
                .q(x - s, cy, x + s, y)
                .z();
        }
        "or" | "xor" | "requiredInterface" => {
            pen.m(x, y).q(right, y, right, cy).q(right, bottom, x, bottom);
            if kind(style) == "requiredInterface" {
                filled = false;
            } else {
                if kind(style) == "xor" {
                    pen.q(cx, cy, x, y);
                }
                pen.z();
            }
        }
        "internalStorage" | "plus" => {
            if style.flag("rounded", false) {
                let arc = style.number("arcSize", 15.0);
                let radius =
                    if style.flag("absoluteArcSize", false) { arc / 2.0 } else { w.min(h) * arc / 100.0 };
                pen.rounded(r, radius);
            } else {
                pen.polygon(&[(x, y), (right, y), (right, bottom), (x, bottom)]);
            }
            if kind(style) == "internalStorage" {
                let dx = style.number("dx", 20.0).clamp(0.0, w);
                let dy = style.number("dy", 20.0).clamp(0.0, h);
                inner.m(x + dx, y).l(x + dx, bottom).m(x, y + dy).l(right, y + dy);
            } else {
                let border = (w.min(h) / 5.0 + 1.0).min(w.min(h) / 2.0);
                inner.m(cx, y + border).l(cx, bottom - border).m(x + border, cy).l(right - border, cy);
            }
        }
        "partialRectangle" => {
            filled = false;
            for (side, a, b) in [
                ("top", (x, y), (right, y)),
                ("right", (right, y), (right, bottom)),
                ("bottom", (right, bottom), (x, bottom)),
                ("left", (x, bottom), (x, y)),
            ] {
                if style.flag(side, true) {
                    pen.m(a.0, a.1).l(b.0, b.1);
                }
            }
        }
        "parallelMarker" => {
            filled = false;
            for start in [x, x + w * 0.4, x + w * 0.8] {
                solid.polygon(&[
                    (start, y),
                    (start + w / 5.0, y),
                    (start + w / 5.0, bottom),
                    (start, bottom),
                ]);
            }
        }
        "waypoint" => {
            filled = false;
            let diameter = ((style.number("size", 6.0) - 2.0).max(0.0)
                + 2.0 * style.number("strokeWidth", 1.0))
            .min(w)
            .min(h);
            solid.ellipse(cx, cy, diameter / 2.0, diameter / 2.0);
        }
        "umlFrame" => {
            let tw = style.number("width", 60.0).max(10.0).min(w);
            let th = style.number("height", 30.0).max(15.0).min(h);
            pen.polygon(&[
                (x, y),
                (x + tw, y),
                (x + tw, y + (th - 15.0).max(0.0)),
                (x + (tw - 10.0).max(0.0), y + th),
                (x, y + th),
            ]);
            inner.m(x + tw, y).l(right, y).l(right, bottom).l(x, bottom).l(x, y + th);
        }
        "umlBoundary" => {
            pen.ellipse(x + w * 7.0 / 12.0, cy, w * 5.0 / 12.0, h / 2.0);
            inner.m(x, y + h / 4.0).l(x, y + h * 0.75).m(x, cy).l(x + w / 6.0, cy);
        }
        "umlEntity" => {
            pen.ellipse(cx, cy, w / 2.0, h / 2.0);
            inner.m(x + w / 8.0, bottom).l(x + w * 7.0 / 8.0, bottom);
        }
        "umlControl" => {
            pen.ellipse(cx, y + h * 9.0 / 16.0, w / 2.0, h * 7.0 / 16.0);
            inner.m(x + w * 0.625, y).l(x + w * 0.375, y + h * 0.1375).l(x + w * 0.625, y + h * 0.25);
        }
        "providedRequiredInterface" => {
            let inset =
                (style.number("inset", 2.0) + style.number("strokeWidth", 1.0)).clamp(0.0, w.min(h) / 2.0);
            pen.ellipse(cx - inset, cy, w / 2.0 - inset, h / 2.0 - inset);
            inner.m(cx, y).q(right, y, right, cy).q(right, bottom, cx, bottom);
        }
        "lollipop" => {
            let size = style.number("size", 10.0).clamp(0.0, w.min(h));
            pen.ellipse(cx, y + size / 2.0, size / 2.0, size / 2.0);
            inner.m(cx, y + size).l(cx, bottom);
        }
        "umlDestroy" => {
            filled = false;
            pen.m(right, y).l(x, bottom).m(x, y).l(right, bottom);
        }
        "umlState" | "bpmnTask" => {
            let radius = if kind(style) == "umlState" {
                let arc = style.number("arcSize", 0.1).max(0.0);
                if !style.flag("rounded", false) {
                    0.0
                } else if style.flag("absoluteArcSize", false) {
                    arc
                } else {
                    w.min(h) * arc
                }
            } else {
                style.number("size", 10.0).max(0.0)
            };
            pen.rounded(r, radius);
            if kind(style) == "bpmnTask" && style.flag("isLoopSub", false) {
                let s = 14.0_f64.min(w).min(h);
                let left = cx - s / 2.0;
                let top = bottom - s - 5.0_f64.min((h - s).max(0.0));
                inner.polygon(&[(left, top), (left + s, top), (left + s, top + s), (left, top + s)]);
                inner
                    .m(cx, top + s * 2.0 / 7.0)
                    .l(cx, top + s * 5.0 / 7.0)
                    .m(left + s * 2.0 / 7.0, top + s / 2.0)
                    .l(left + s * 5.0 / 7.0, top + s / 2.0);
            }
        }
        "umlLifeline" => {
            let head = style.number("size", 40.0).clamp(0.0, h);
            let mut participant = style.clone();
            participant.set(
                "shape",
                style
                    .get("participant")
                    .filter(|p| matches!(*p, "umlActor" | "umlBoundary" | "umlEntity" | "umlControl"))
                    .unwrap_or("rectangle"),
            );
            let head_outline = outline(&participant, Rect { height: head, ..r });
            pen.0.push_str(&head_outline.path);
            inner.0.push_str(&head_outline.inner);
            let mut top = y + head;
            if style.flag("lifelineDashed", true) {
                while top < bottom {
                    inner.m(cx, top).l(cx, (top + 6.0).min(bottom));
                    top += 10.0;
                }
            } else {
                inner.m(cx, top).l(cx, bottom);
            }
        }
        "triangle" => {
            let points = match style.get("direction").unwrap_or("east") {
                "north" => [(x, bottom), (cx, y), (right, bottom)],
                "south" => [(x, y), (right, y), (cx, bottom)],
                "west" => [(right, y), (x, cy), (right, bottom)],
                _ => [(x, y), (right, cy), (x, bottom)],
            };
            pen.polygon(&points);
        }
        "triangleDown" => {
            pen.polygon(&[(x, y), (right, y), (cx, bottom)]);
        }
        "triangleUp" => {
            pen.polygon(&[(x, bottom), (cx, y), (right, bottom)]);
        }
        "hexagon" => {
            let s = share(0.25);
            pen.polygon(&[
                (x + s, y),
                (right - s, y),
                (right, cy),
                (right - s, bottom),
                (x + s, bottom),
                (x, cy),
            ]);
        }
        "parallelogram" => {
            let s = share(0.2);
            pen.polygon(&[(x + s, y), (right, y), (right - s, bottom), (x, bottom)]);
        }
        "trapezoid" => {
            let s = share(0.2);
            pen.polygon(&[(x, bottom), (x + s, y), (right - s, y), (right, bottom)]);
        }
        "manualInput" => {
            pen.polygon(&[(x, y + h * 0.25), (right, y), (right, bottom), (x, bottom)]);
        }
        "offPage" => {
            pen.polygon(&[(x, y), (right, y), (right, y + h * 0.6), (cx, bottom), (x, y + h * 0.6)]);
        }
        "step" => {
            let s = share(0.2);
            pen.polygon(&[
                (x, y),
                (right - s, y),
                (right, cy),
                (right - s, bottom),
                (x, bottom),
                (x + s, cy),
            ]);
        }
        "delay" => {
            let rx = (w / 2.0).min(h / 2.0);
            pen.m(x, y).l(right - rx, y).c([
                right - rx + rx * 1.33,
                y,
                right - rx + rx * 1.33,
                bottom,
                right - rx,
                bottom,
            ]);
            pen.l(x, bottom).z();
        }
        "terminator" => {
            pen.rounded(r, h / 2.0);
        }
        "cylinder" => {
            let ry = style.number("size", 15.0).min(h / 3.0).max(0.0);
            const K: f64 = 0.552_284_75;
            let (kx, ky) = (w / 2.0 * K, ry * K);
            pen.m(x, y + ry)
                .c([x, y + ry - ky, cx - kx, y, cx, y])
                .c([cx + kx, y, right, y + ry - ky, right, y + ry])
                .l(right, bottom - ry)
                .c([right, bottom - ry + ky, cx + kx, bottom, cx, bottom])
                .c([cx - kx, bottom, x, bottom - ry + ky, x, bottom - ry])
                .z();
            // The near rim of the top.
            inner.m(x, y + ry).c([x, y + ry + ky, cx - kx, y + 2.0 * ry, cx, y + 2.0 * ry]).c([
                cx + kx,
                y + 2.0 * ry,
                right,
                y + ry + ky,
                right,
                y + ry,
            ]);
        }
        "document" => {
            let dy = h * style.number("size", 0.3).clamp(0.0, 1.0) / 2.0;
            pen.m(x, y)
                .l(right, y)
                .l(right, bottom - dy)
                .q(x + w * 0.75, bottom - 2.0 * dy, cx, bottom - dy)
                .q(x + w * 0.25, bottom, x, bottom - dy)
                .z();
        }
        "process" => {
            let s = share(0.1);
            pen.polygon(&[(x, y), (right, y), (right, bottom), (x, bottom)]);
            inner.m(x + s, y).l(x + s, bottom).m(right - s, y).l(right - s, bottom);
        }
        "cloud" => {
            let p = |fx: f64, fy: f64| (x + fx * w, y + fy * h);
            let curve = |pen: &mut Pen, a: (f64, f64), b: (f64, f64), c: (f64, f64)| {
                pen.c([a.0, a.1, b.0, b.1, c.0, c.1]);
            };
            let start = p(0.25, 0.25);
            pen.m(start.0, start.1);
            curve(&mut pen, p(0.05, 0.25), p(0.0, 0.5), p(0.16, 0.55));
            curve(&mut pen, p(0.0, 0.66), p(0.18, 0.9), p(0.31, 0.8));
            curve(&mut pen, p(0.4, 1.0), p(0.7, 1.0), p(0.8, 0.8));
            curve(&mut pen, p(1.0, 0.8), p(1.0, 0.6), p(0.875, 0.5));
            curve(&mut pen, p(1.0, 0.3), p(0.8, 0.1), p(0.625, 0.2));
            curve(&mut pen, p(0.5, 0.05), p(0.3, 0.05), start);
            pen.z();
        }
        "umlActor" => {
            pen.ellipse(cx, y + h / 8.0, w / 4.0, h / 8.0);
            inner
                .m(cx, y + h / 4.0)
                .l(cx, y + h * 2.0 / 3.0)
                .m(x, y + h / 3.0)
                .l(right, y + h / 3.0)
                .m(cx, y + h * 2.0 / 3.0)
                .l(x, bottom)
                .m(cx, y + h * 2.0 / 3.0)
                .l(right, bottom);
        }
        "note" => {
            let s = style.number("size", 30.0).min(w).min(h).max(0.0);
            pen.polygon(&[(x, y), (right - s, y), (right, y + s), (right, bottom), (x, bottom)]);
            inner.m(right - s, y).l(right - s, y + s).l(right, y + s);
        }
        "folder" => {
            let tab = style.number("tabWidth", 50.0).min(w);
            let lip = style.number("tabHeight", 14.0).min(h);
            pen.polygon(&[
                (x, y),
                (x + tab, y),
                (x + tab, y + lip),
                (right, y + lip),
                (right, bottom),
                (x, bottom),
            ]);
            inner.m(x, y + lip).l(x + tab, y + lip);
        }
        "component" => {
            pen.polygon(&[(x + 10.0, y), (right, y), (right, bottom), (x + 10.0, bottom)]);
            for top in [y + h * 0.25 - 5.0, y + h * 0.75 - 5.0] {
                inner.m(x, top).l(x + 20.0, top).l(x + 20.0, top + 10.0).l(x, top + 10.0).z();
            }
        }
        "card" => {
            let s = style.number("size", 30.0).min(w).min(h).max(0.0);
            pen.polygon(&[(x + s, y), (right, y), (right, bottom), (x, bottom), (x, y + s)]);
        }
        "swimlane" => {
            let head = style.number("startSize", 23.0).max(0.0);
            pen.polygon(&[(x, y), (right, y), (right, bottom), (x, bottom)]);
            if style.flag("horizontal", true) {
                inner.m(x, y + head.min(h)).l(right, y + head.min(h));
            } else {
                inner.m(x + head.min(w), y).l(x + head.min(w), bottom);
            }
        }
        "singleArrow" | "doubleArrow" => {
            // draw.io's shares: the shaft's of the height, the head's of the width.
            let shaft = h * style.number("arrowWidth", 0.3).clamp(0.05, 1.0) / 2.0;
            let head = (w * style.number("arrowSize", 0.2).clamp(0.0, 0.5)).min(w);
            let (top, low) = (cy - shaft, cy + shaft);
            if kind(style) == "singleArrow" {
                pen.polygon(&[
                    (x, top),
                    (right - head, top),
                    (right - head, y),
                    (right, cy),
                    (right - head, bottom),
                    (right - head, low),
                    (x, low),
                ]);
            } else {
                pen.polygon(&[
                    (x, cy),
                    (x + head, y),
                    (x + head, top),
                    (right - head, top),
                    (right - head, y),
                    (right, cy),
                    (right - head, bottom),
                    (right - head, low),
                    (x + head, low),
                    (x + head, bottom),
                ]);
            }
        }
        "callout" => {
            // A speech bubble: its tail `size` high, at `position` along the bottom.
            let tail = style.number("size", 30.0).clamp(0.0, h);
            let at = x + w * style.number("position", 0.5).clamp(0.0, 1.0);
            let tip = x + w * style.number("position2", 0.5).clamp(0.0, 1.0);
            let base = style.number("base", 20.0).clamp(0.0, w);
            let body = bottom - tail;
            pen.polygon(&[
                (x, y),
                (right, y),
                (right, body),
                ((at + base).min(right), body),
                (tip, bottom),
                (at, body),
                (x, body),
            ]);
        }
        "cross" => {
            // A plus sign: each arm `size` of the box's side from its edge.
            let s = style.number("size", 0.2).clamp(0.0, 0.5);
            let (dx, dy) = (w * s, h * s);
            let (inner_left, inner_right) = (cx - (w / 2.0 - dx) / 2.0, cx + (w / 2.0 - dx) / 2.0);
            let (inner_top, inner_bottom) = (cy - (h / 2.0 - dy) / 2.0, cy + (h / 2.0 - dy) / 2.0);
            pen.polygon(&[
                (inner_left, y),
                (inner_right, y),
                (inner_right, inner_top),
                (right, inner_top),
                (right, inner_bottom),
                (inner_right, inner_bottom),
                (inner_right, bottom),
                (inner_left, bottom),
                (inner_left, inner_bottom),
                (x, inner_bottom),
                (x, inner_top),
                (inner_left, inner_top),
            ]);
        }
        "annotation" => {
            filled = false;
            pen.m(x + w * 0.5, y).l(x, y).l(x, bottom).l(x + w * 0.5, bottom);
        }
        "line" => {
            filled = false;
            pen.m(x, cy).l(right, cy);
        }
        "text" | "group" => {
            filled = false;
        }
        _ => {
            if style.flag("rounded", false) {
                let size = style.number("arcSize", 15.0);
                let radius =
                    if style.flag("absoluteArcSize", false) { size / 2.0 } else { w.min(h) * size / 100.0 };
                pen.rounded(r, radius);
            } else {
                pen.polygon(&[(x, y), (right, y), (right, bottom), (x, bottom)]);
            }
        }
    }
    Outline { path: pen.done(), inner: inner.done(), solid: solid.done(), filled }
}

/// Where the line from a shape's centre towards `toward` leaves its
/// outline: rectangles, ellipses and diamonds are exact, other shapes are
/// taken as their box.
pub fn perimeter(style: &Style, r: Rect, toward: (f64, f64)) -> (f64, f64) {
    let (cx, cy) = r.centre();
    let (dx, dy) = (toward.0 - cx, toward.1 - cy);
    if dx.abs() < 1e-9 && dy.abs() < 1e-9 {
        return (cx, cy);
    }
    let (hw, hh) = (r.width / 2.0, r.height / 2.0);
    let scale = match kind(style) {
        "waypoint" => return (cx, cy),
        "ellipse" | "doubleEllipse" | "sumEllipse" | "orEllipse" | "endState" | "bpmnEnd" => {
            if hw <= 0.0 || hh <= 0.0 {
                return (cx, cy);
            }
            1.0 / ((dx / hw).powi(2) + (dy / hh).powi(2)).sqrt()
        }
        "rhombus" | "exclusiveGateway" | "parallelGateway" | "inclusiveGateway" => {
            if hw <= 0.0 || hh <= 0.0 {
                return (cx, cy);
            }
            1.0 / (dx.abs() / hw + dy.abs() / hh)
        }
        _ => {
            let sx = if dx.abs() > 1e-9 { hw / dx.abs() } else { f64::INFINITY };
            let sy = if dy.abs() > 1e-9 { hh / dy.abs() } else { f64::INFINITY };
            sx.min(sy)
        }
    };
    (cx + dx * scale, cy + dy * scale)
}
