//! A diagram as draw.io keeps it: an `mxfile` of pages, each an
//! `mxGraphModel` of cells. Pages are read plain or compressed and written
//! back plain, keeping what Paperloom does not use.

use std::io::Read;

use base64::Engine;

use super::style::Style;
use crate::xml::{Content, Element, parse_xml, write_element};

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Diagram {
    pub pages: Vec<Page>,
    /// The `mxfile`'s attributes.
    pub attrs: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Page {
    pub id: String,
    pub name: String,
    /// The `mxGraphModel`'s attributes: grid, page size, background…
    pub model: Vec<(String, String)>,
    pub cells: Vec<Cell>,
}

/// Where a cell is: a shape's box, or an edge's bends and loose ends.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Geometry {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// An edge's geometry, or a label placed along its edge.
    pub relative: bool,
    /// An edge's bends, in order.
    pub points: Vec<(f64, f64)>,
    /// Where an edge with no source or target shape starts or ends.
    pub source_point: Option<(f64, f64)>,
    pub target_point: Option<(f64, f64)>,
    /// How far a label is moved from where it would be.
    pub offset: Option<(f64, f64)>,
    /// Other attributes and children, kept.
    pub other_attrs: Vec<(String, String)>,
    pub(crate) other: Vec<Element>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Cell {
    pub id: String,
    /// Empty for the root cell.
    pub parent: String,
    /// Its label (HTML when its style says `html=1`).
    pub value: String,
    pub style: Style,
    pub vertex: bool,
    pub edge: bool,
    pub source: String,
    pub target: String,
    pub geometry: Option<Geometry>,
    /// The `object` or `UserObject` it sits in and that element's own
    /// attributes (a link, a tooltip, properties).
    pub wrapper: Option<(String, Vec<(String, String)>)>,
    /// Its other attributes (`visible`, `collapsed`, `connectable`…), kept.
    pub other: Vec<(String, String)>,
}

impl Cell {
    pub fn attr(&self, key: &str) -> Option<&str> {
        self.other.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }
}

/// A number as draw.io writes one: `120`, `10.5`.
pub(crate) fn num(value: f64) -> String {
    format!("{}", (value * 100.0).round() / 100.0)
}

fn number(value: Option<&str>) -> f64 {
    value.and_then(|v| v.trim().parse().ok()).filter(|v: &f64| v.is_finite()).unwrap_or(0.0)
}

fn point(element: &Element) -> (f64, f64) {
    (number(element.attr("x")), number(element.attr("y")))
}

fn geometry(element: &Element) -> Geometry {
    let mut geometry = Geometry {
        x: number(element.attr("x")),
        y: number(element.attr("y")),
        width: number(element.attr("width")),
        height: number(element.attr("height")),
        relative: element.attr("relative") == Some("1"),
        other_attrs: element
            .attrs
            .iter()
            .filter(|(k, _)| !matches!(k.as_str(), "x" | "y" | "width" | "height" | "relative" | "as"))
            .cloned()
            .collect(),
        ..Geometry::default()
    };
    for child in element.elements() {
        match (child.local(), child.attr("as")) {
            ("mxPoint", Some("sourcePoint")) => geometry.source_point = Some(point(child)),
            ("mxPoint", Some("targetPoint")) => geometry.target_point = Some(point(child)),
            ("mxPoint", Some("offset")) => geometry.offset = Some(point(child)),
            ("Array", Some("points")) => {
                geometry.points = child.elements().filter(|p| p.local() == "mxPoint").map(point).collect();
            }
            _ => geometry.other.push(child.clone()),
        }
    }
    geometry
}

const CELL_KEYS: [&str; 8] = ["id", "parent", "value", "style", "vertex", "edge", "source", "target"];

/// A cell, from an `mxCell` or the `object` / `UserObject` around one.
fn cell(element: &Element) -> Option<Cell> {
    let (inner, wrapper) = match element.local() {
        "mxCell" => (element, None),
        "object" | "UserObject" => (element.child("mxCell")?, Some(element)),
        _ => return None,
    };
    let text = |key: &str| inner.attr(key).unwrap_or_default().to_string();
    Some(Cell {
        id: wrapper.and_then(|w| w.attr("id")).or_else(|| inner.attr("id")).unwrap_or_default().to_string(),
        parent: text("parent"),
        value: match wrapper {
            Some(w) => w.attr("label").unwrap_or_default().to_string(),
            None => text("value"),
        },
        style: Style::parse(inner.attr("style").unwrap_or_default()),
        vertex: inner.attr("vertex") == Some("1"),
        edge: inner.attr("edge") == Some("1"),
        source: text("source"),
        target: text("target"),
        geometry: inner.child("mxGeometry").map(geometry),
        wrapper: wrapper.map(|w| {
            let attrs = w.attrs.iter().filter(|(k, _)| k != "id" && k != "label").cloned().collect();
            (w.name.clone(), attrs)
        }),
        other: inner.attrs.iter().filter(|(k, _)| !CELL_KEYS.contains(&k.as_str())).cloned().collect(),
    })
}

fn page(id: &str, name: &str, model: &Element) -> Page {
    let cells =
        model.child("root").map(|root| root.elements().filter_map(cell).collect()).unwrap_or_default();
    Page { id: id.to_string(), name: name.to_string(), model: model.attrs.clone(), cells }
}

/// `%E4%B8%AD` → `中`, as `decodeURIComponent` reads it.
fn uri_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(byte) = text.get(i + 1..i + 3).and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A page draw.io compressed: Base64 of the raw-deflated, URI-encoded XML.
fn inflate(text: &str) -> Option<String> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(text.trim()).ok()?;
    let mut encoded = String::new();
    flate2::read::DeflateDecoder::new(&bytes[..]).read_to_string(&mut encoded).ok()?;
    Some(uri_decode(&encoded))
}

impl Page {
    /// A page with nothing on it but its root and its one layer.
    pub fn blank(id: &str, name: &str) -> Page {
        Page {
            id: id.to_string(),
            name: name.to_string(),
            model: Vec::new(),
            cells: vec![
                Cell { id: "0".into(), ..Cell::default() },
                Cell { id: "1".into(), parent: "0".into(), ..Cell::default() },
            ],
        }
    }

    pub fn cell(&self, id: &str) -> Option<&Cell> {
        self.cells.iter().find(|c| c.id == id)
    }

    pub fn cell_mut(&mut self, id: &str) -> Option<&mut Cell> {
        self.cells.iter_mut().find(|c| c.id == id)
    }

    /// The root cell's id.
    pub fn root(&self) -> &str {
        self.cells.iter().find(|c| c.parent.is_empty()).map_or("0", |c| c.id.as_str())
    }

    /// Its layers: the cells right under the root.
    pub fn layers(&self) -> Vec<&Cell> {
        let root = self.root();
        self.cells.iter().filter(|c| c.parent == root && !c.vertex && !c.edge).collect()
    }
}

impl Diagram {
    /// A diagram from draw.io's XML: an `mxfile` (pages plain or
    /// compressed) or a bare `mxGraphModel`.
    pub fn parse(xml: &str) -> Option<Diagram> {
        let root = parse_xml(xml)?;
        let mut diagram = match root.local() {
            "mxGraphModel" => Diagram { pages: vec![page("page-1", "Page-1", &root)], attrs: Vec::new() },
            "mxfile" => {
                let pages = root
                    .elements()
                    .filter(|e| e.local() == "diagram")
                    .enumerate()
                    .filter_map(|(i, d)| {
                        let id = d.attr("id").map_or_else(|| format!("page-{}", i + 1), str::to_string);
                        let name = d.attr("name").map_or_else(|| format!("Page-{}", i + 1), str::to_string);
                        match d.child("mxGraphModel") {
                            Some(model) => Some(page(&id, &name, model)),
                            None => {
                                let model = parse_xml(&inflate(&d.text())?)?;
                                (model.local() == "mxGraphModel").then(|| page(&id, &name, &model))
                            }
                        }
                    })
                    .collect();
                let attrs = root.attrs.iter().filter(|(k, _)| k != "compressed").cloned().collect();
                Diagram { pages, attrs }
            }
            _ => return None,
        };
        if diagram.pages.is_empty() {
            diagram.pages.push(Page::blank("page-1", "Page-1"));
        }
        Some(diagram)
    }

    /// The diagram as an `mxfile`, its pages plain.
    pub fn to_xml(&self) -> String {
        let mut file = Element { name: "mxfile".into(), attrs: self.attrs.clone(), children: Vec::new() };
        for page in &self.pages {
            let root = Element {
                name: "root".into(),
                attrs: Vec::new(),
                children: page.cells.iter().map(|c| Content::Element(cell_element(c))).collect(),
            };
            let model = Element {
                name: "mxGraphModel".into(),
                attrs: page.model.clone(),
                children: vec![Content::Element(root)],
            };
            let attrs = vec![("id".to_string(), page.id.clone()), ("name".to_string(), page.name.clone())];
            file.children.push(Content::Element(Element {
                name: "diagram".into(),
                attrs,
                children: vec![Content::Element(model)],
            }));
        }
        let mut out = String::new();
        write_element(&file, &mut out);
        out
    }
}

fn point_element(at: (f64, f64), role: Option<&str>) -> Element {
    let mut attrs = vec![("x".to_string(), num(at.0)), ("y".to_string(), num(at.1))];
    if let Some(role) = role {
        attrs.push(("as".to_string(), role.to_string()));
    }
    Element { name: "mxPoint".into(), attrs, children: Vec::new() }
}

fn geometry_element(geometry: &Geometry) -> Element {
    let mut attrs = Vec::new();
    for (key, value) in
        [("x", geometry.x), ("y", geometry.y), ("width", geometry.width), ("height", geometry.height)]
    {
        if value.abs() > 1e-9 {
            attrs.push((key.to_string(), num(value)));
        }
    }
    if geometry.relative {
        attrs.push(("relative".to_string(), "1".to_string()));
    }
    attrs.extend(geometry.other_attrs.iter().cloned());
    attrs.push(("as".to_string(), "geometry".to_string()));
    let mut children = Vec::new();
    if let Some(at) = geometry.source_point {
        children.push(point_element(at, Some("sourcePoint")));
    }
    if let Some(at) = geometry.target_point {
        children.push(point_element(at, Some("targetPoint")));
    }
    if !geometry.points.is_empty() {
        let points = geometry.points.iter().map(|&p| Content::Element(point_element(p, None))).collect();
        children.push(Element {
            name: "Array".into(),
            attrs: vec![("as".to_string(), "points".to_string())],
            children: points,
        });
    }
    if let Some(at) = geometry.offset {
        children.push(point_element(at, Some("offset")));
    }
    children.extend(geometry.other.iter().cloned());
    Element {
        name: "mxGeometry".into(),
        attrs,
        children: children.into_iter().map(Content::Element).collect(),
    }
}

fn cell_element(cell: &Cell) -> Element {
    let mut attrs = Vec::new();
    let mut push = |key: &str, value: &str| attrs.push((key.to_string(), value.to_string()));
    if cell.wrapper.is_none() {
        push("id", &cell.id);
        if !cell.value.is_empty() {
            push("value", &cell.value);
        }
    }
    if !cell.style.is_empty() {
        push("style", &cell.style.to_string());
    }
    if cell.vertex {
        push("vertex", "1");
    }
    if cell.edge {
        push("edge", "1");
    }
    if !cell.parent.is_empty() {
        push("parent", &cell.parent);
    }
    if !cell.source.is_empty() {
        push("source", &cell.source);
    }
    if !cell.target.is_empty() {
        push("target", &cell.target);
    }
    attrs.extend(cell.other.iter().cloned());
    let children = cell.geometry.iter().map(|g| Content::Element(geometry_element(g))).collect();
    let inner = Element { name: "mxCell".into(), attrs, children };
    match &cell.wrapper {
        None => inner,
        Some((tag, extra)) => {
            let mut attrs = vec![("label".to_string(), cell.value.clone())];
            attrs.extend(extra.iter().cloned());
            attrs.push(("id".to_string(), cell.id.clone()));
            Element { name: tag.clone(), attrs, children: vec![Content::Element(inner)] }
        }
    }
}
