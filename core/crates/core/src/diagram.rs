//! Diagrams for the apps' own editors: a page laid out as they draw it,
//! changes written back as the diagram's XML (draw.io's format, which
//! diagrams.net and its plugins read too), the shapes on offer, and
//! pictures of a page.

use paperloom_vault::{
    DIAGRAM_PALETTE, Diagram, DiagramEdit, DiagramItem, DiagramPage, diagram_scene, diagram_svg,
    edit_diagram_page,
};
use serde::{Deserialize, Serialize};

use crate::{CoreError, PaperVault};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, uniffi::Record)]
pub struct DiagramPoint {
    pub x: f64,
    pub y: f64,
}

/// A shape or an edge as the editors draw it, in page coordinates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct DiagramDrawn {
    pub id: String,
    pub edge: bool,
    /// The group or container it sits in, if any.
    pub parent: String,
    /// Its box; an edge's is around its line.
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// Degrees clockwise about its box's middle.
    pub rotation: f64,
    /// The outline (or the edge's line), and the lines inside a shape, as
    /// SVG path data of `M`, `L`, `Q`, `C` and `Z`.
    pub path: String,
    pub inner: String,
    /// Arrowheads filled with the line's colour, and those only outlined.
    pub arrows: String,
    pub open_arrows: String,
    /// `#rrggbb`, or empty for none.
    pub fill: String,
    pub stroke: String,
    pub stroke_width: f64,
    pub dashed: bool,
    pub opacity: f64,
    /// Its label as plain text.
    pub text: String,
    pub text_x: f64,
    pub text_y: f64,
    pub text_width: f64,
    pub text_height: f64,
    pub font_size: f64,
    pub font_color: String,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub align: String,
    pub vertical_align: String,
    pub label_background: String,
    pub wrap: bool,
    /// An edge's ends, by id; empty for a loose end.
    pub source: String,
    pub target: String,
    /// An edge's line, point by point.
    pub points: Vec<DiagramPoint>,
    /// Its bends as stored (an edge's own, without its ends).
    pub bends: Vec<DiagramPoint>,
    /// Whether it holds other shapes.
    pub container: bool,
    /// Its style as written: `key=value;` pairs.
    pub style: String,
    /// A picture's address (a `data:` URI or a web address); empty for
    /// other shapes.
    #[uniffi(default = "")]
    pub image: String,
    /// Whether the picture keeps its proportions in its box.
    #[uniffi(default = true)]
    pub image_aspect: bool,
}

/// A page of a diagram, ready to draw.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct DiagramView {
    /// The diagram's pages, by name.
    pub pages: Vec<String>,
    /// The page shown.
    pub page: u32,
    /// In the order they stack, the bottom first.
    pub items: Vec<DiagramDrawn>,
    /// How far the page's drawing reaches.
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub background: String,
    /// The grid's spacing, 0 when it is off.
    pub grid: f64,
}

/// A shape the editors offer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
pub struct DiagramShape {
    pub id: String,
    pub name: String,
    /// `General`, `Arrows`, `Flowchart`, `UML`, `Entity Relation` or `BPMN`.
    pub group: String,
    pub width: f64,
    pub height: f64,
    /// A small drawing of it (outline, inner lines and details), as SVG
    /// path data in its own box.
    pub path: String,
}

/// A change to a diagram; shapes and edges by id, places in page
/// coordinates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Enum)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DiagramChange {
    AddShape {
        shape: String,
        x: f64,
        y: f64,
    },
    /// An edge from a shape (or, with no `source`, from `from`) to a shape
    /// (or `to`).
    Connect {
        source: String,
        target: String,
        from: DiagramPoint,
        to: DiagramPoint,
    },
    Move {
        ids: Vec<String>,
        dx: f64,
        dy: f64,
    },
    Resize {
        id: String,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
    SetText {
        id: String,
        text: String,
    },
    /// A style key on each; an empty value takes it away.
    SetStyle {
        ids: Vec<String>,
        key: String,
        value: String,
    },
    Delete {
        ids: Vec<String>,
    },
    /// Joins an edge's start (`source`) or end to a shape, or (with an
    /// empty `cell`) leaves it loose at `at`.
    Reconnect {
        id: String,
        source: bool,
        cell: String,
        at: DiagramPoint,
    },
    SetBends {
        id: String,
        points: Vec<DiagramPoint>,
    },
    ToFront {
        ids: Vec<String>,
    },
    ToBack {
        ids: Vec<String>,
    },
    Group {
        ids: Vec<String>,
    },
    Ungroup {
        id: String,
    },
    /// `left`, `center`, `right`, `top`, `middle` or `bottom`.
    Align {
        ids: Vec<String>,
        how: String,
    },
    Distribute {
        ids: Vec<String>,
        horizontal: bool,
    },
    Duplicate {
        ids: Vec<String>,
        dx: f64,
        dy: f64,
    },
    /// A picture (a `data:` URI or a web address) in a box, as draw.io
    /// places one.
    AddImage {
        image: String,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
    AddPage {
        name: String,
    },
    RenamePage {
        name: String,
    },
    RemovePage,
}

/// A diagram after a change: its XML, the page to show, and what to
/// select.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
pub struct DiagramChanged {
    pub xml: String,
    pub page: u32,
    pub selection: Vec<String>,
}

fn parse(xml: &str) -> Result<Diagram, CoreError> {
    Diagram::parse(xml).ok_or_else(|| CoreError::InvalidInput {
        message: "This is not a diagram Paperloom can read.".into(),
    })
}

fn point(p: (f64, f64)) -> DiagramPoint {
    DiagramPoint { x: p.0, y: p.1 }
}

fn drawn(item: DiagramItem, page: &DiagramPage) -> DiagramDrawn {
    let cell = page.cell(&item.id);
    DiagramDrawn {
        bends: cell
            .and_then(|c| c.geometry.as_ref())
            .filter(|_| item.edge)
            .map(|g| g.points.iter().map(|&p| point(p)).collect())
            .unwrap_or_default(),
        style: cell.map(|c| c.style.to_string()).unwrap_or_default(),
        id: item.id,
        edge: item.edge,
        parent: item.parent,
        x: item.x,
        y: item.y,
        width: item.width,
        height: item.height,
        rotation: item.rotation,
        path: item.path,
        inner: item.inner,
        arrows: item.arrows,
        open_arrows: item.open_arrows,
        fill: item.fill,
        stroke: item.stroke,
        stroke_width: item.stroke_width,
        dashed: item.dashed,
        opacity: item.opacity,
        text: item.text,
        text_x: item.text_x,
        text_y: item.text_y,
        text_width: item.text_width,
        text_height: item.text_height,
        font_size: item.font_size,
        font_color: item.font_color,
        bold: item.bold,
        italic: item.italic,
        underline: item.underline,
        align: item.align,
        vertical_align: item.vertical_align,
        label_background: item.label_background,
        wrap: item.wrap,
        source: item.source,
        target: item.target,
        points: item.points.into_iter().map(point).collect(),
        container: item.container,
        image: item.image,
        image_aspect: item.image_aspect,
    }
}

/// A page of a diagram laid out for the editors; a page past the last
/// shows the last.
#[uniffi::export]
pub fn diagram_view(xml: String, page: u32) -> Result<DiagramView, CoreError> {
    let diagram = parse(&xml)?;
    let index = (page as usize).min(diagram.pages.len() - 1);
    let shown = &diagram.pages[index];
    let scene = diagram_scene(shown);
    Ok(DiagramView {
        pages: diagram.pages.iter().map(|p| p.name.clone()).collect(),
        page: index as u32,
        items: scene.items.into_iter().map(|item| drawn(item, shown)).collect(),
        x: scene.x,
        y: scene.y,
        width: scene.width,
        height: scene.height,
        background: scene.background,
        grid: scene.grid,
    })
}

/// Changes a diagram.
#[uniffi::export]
pub fn diagram_change(xml: String, page: u32, change: DiagramChange) -> Result<DiagramChanged, CoreError> {
    let mut diagram = parse(&xml)?;
    let mut index = (page as usize).min(diagram.pages.len() - 1);
    let edit = match change {
        DiagramChange::AddPage { name } => {
            let n = diagram.pages.len() + 1;
            let id = (n..)
                .map(|k| format!("page-{k}"))
                .find(|id| diagram.pages.iter().all(|p| &p.id != id))
                .unwrap_or_default();
            let name = if name.trim().is_empty() { format!("Page-{n}") } else { name.trim().to_string() };
            diagram.pages.push(DiagramPage::blank(&id, &name));
            index = diagram.pages.len() - 1;
            None
        }
        DiagramChange::RenamePage { name } => {
            if !name.trim().is_empty() {
                diagram.pages[index].name = name.trim().to_string();
            }
            None
        }
        DiagramChange::RemovePage => {
            if diagram.pages.len() == 1 {
                return Err(CoreError::InvalidInput { message: "A diagram keeps at least one page.".into() });
            }
            diagram.pages.remove(index);
            index = index.min(diagram.pages.len() - 1);
            None
        }
        DiagramChange::AddShape { shape, x, y } => Some(DiagramEdit::AddShape { shape, x, y }),
        DiagramChange::Connect { source, target, from, to } => {
            Some(DiagramEdit::Connect { source, target, from: (from.x, from.y), to: (to.x, to.y) })
        }
        DiagramChange::Move { ids, dx, dy } => Some(DiagramEdit::Move { ids, dx, dy }),
        DiagramChange::Resize { id, x, y, width, height } => {
            Some(DiagramEdit::Resize { id, x, y, width, height })
        }
        DiagramChange::SetText { id, text } => Some(DiagramEdit::SetText { id, text }),
        DiagramChange::SetStyle { ids, key, value } => Some(DiagramEdit::SetStyle { ids, key, value }),
        DiagramChange::Delete { ids } => Some(DiagramEdit::Delete { ids }),
        DiagramChange::Reconnect { id, source, cell, at } => {
            Some(DiagramEdit::Reconnect { id, source, cell, at: (at.x, at.y) })
        }
        DiagramChange::SetBends { id, points } => {
            Some(DiagramEdit::SetBends { id, points: points.iter().map(|p| (p.x, p.y)).collect() })
        }
        DiagramChange::ToFront { ids } => Some(DiagramEdit::ToFront { ids }),
        DiagramChange::ToBack { ids } => Some(DiagramEdit::ToBack { ids }),
        DiagramChange::Group { ids } => Some(DiagramEdit::Group { ids }),
        DiagramChange::Ungroup { id } => Some(DiagramEdit::Ungroup { id }),
        DiagramChange::Align { ids, how } => Some(DiagramEdit::Align { ids, how }),
        DiagramChange::Distribute { ids, horizontal } => Some(DiagramEdit::Distribute { ids, horizontal }),
        DiagramChange::Duplicate { ids, dx, dy } => Some(DiagramEdit::Duplicate { ids, dx, dy }),
        DiagramChange::AddImage { image, x, y, width, height } => {
            Some(DiagramEdit::AddImage { image, x, y, width, height })
        }
    };
    let selection = match edit {
        Some(edit) => edit_diagram_page(&mut diagram.pages[index], &edit)
            .map_err(|message| CoreError::InvalidInput { message })?,
        None => Vec::new(),
    };
    Ok(DiagramChanged { xml: diagram.to_xml(), page: index as u32, selection })
}

/// The shapes the editors offer, each with a small drawing of it.
#[uniffi::export]
pub fn diagram_shapes() -> Vec<DiagramShape> {
    DIAGRAM_PALETTE
        .iter()
        .map(|shape| {
            let mut page = DiagramPage::blank("p", "P");
            let added = edit_diagram_page(
                &mut page,
                &DiagramEdit::AddShape { shape: shape.id.into(), x: 0.0, y: 0.0 },
            );
            let path = added
                .ok()
                .and_then(|_| diagram_scene(&page).items.into_iter().next())
                // Inner lines and details too: a gateway's cross, a waypoint's dot.
                .map(|item| [item.path, item.inner, item.arrows].concat())
                .unwrap_or_default();
            DiagramShape {
                id: shape.id.to_string(),
                name: shape.name.to_string(),
                group: shape.group.to_string(),
                width: shape.width,
                height: shape.height,
                path,
            }
        })
        .collect()
}

/// A page of a diagram as an SVG picture.
#[uniffi::export]
pub fn diagram_page_svg(xml: String, page: u32) -> Result<String, CoreError> {
    let diagram = parse(&xml)?;
    let index = (page as usize).min(diagram.pages.len() - 1);
    Ok(diagram_svg(&diagram_scene(&diagram.pages[index]), None))
}

#[uniffi::export]
impl PaperVault {
    /// The vault's diagrams (`.drawio.svg` and `.drawio` files), by path.
    pub fn diagrams(&self) -> Result<Vec<String>, CoreError> {
        Ok(self.lock()?.diagrams().cloned().collect())
    }

    /// A diagram's XML, in draw.io's format.
    pub fn read_diagram(&self, path: String) -> Result<String, CoreError> {
        Ok(self.lock()?.read_diagram(&path)?)
    }

    /// Saves a diagram: its XML, and for a `.drawio.svg` a picture holding
    /// it (`svg`, or, when none is given, the first page as Paperloom
    /// draws it).
    pub fn write_diagram(&self, path: String, xml: String, svg: Option<String>) -> Result<(), CoreError> {
        Ok(self.lock()?.write_diagram(&path, &xml, svg.as_deref())?)
    }

    /// Makes an empty `.drawio.svg` in `folder`; its path.
    pub fn create_diagram(&self, folder: String, name: String) -> Result<String, CoreError> {
        Ok(self.lock()?.create_diagram(&folder, &name)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_a_diagram_through_its_xml() {
        let xml = paperloom_vault::EMPTY_DIAGRAM.to_string();
        let added =
            diagram_change(xml, 0, DiagramChange::AddShape { shape: "decision".into(), x: 10.0, y: 10.0 })
                .unwrap();
        assert_eq!(added.selection, ["pl-1"]);
        let view = diagram_view(added.xml.clone(), 0).unwrap();
        assert_eq!(view.pages, ["Page-1"]);
        assert_eq!(view.items.len(), 1);
        assert!(view.items[0].style.starts_with("rhombus;"));
        let paged =
            diagram_change(added.xml.clone(), 0, DiagramChange::AddPage { name: " Second ".into() }).unwrap();
        assert_eq!(paged.page, 1);
        let view = diagram_view(paged.xml.clone(), 9).unwrap();
        assert_eq!((view.pages.len(), view.page, view.items.len()), (2, 1, 0));
        let removed = diagram_change(paged.xml, 1, DiagramChange::RemovePage).unwrap();
        assert_eq!(diagram_view(removed.xml.clone(), 0).unwrap().items.len(), 1);
        assert!(diagram_change(removed.xml.clone(), 0, DiagramChange::RemovePage).is_err());
        assert!(diagram_page_svg(removed.xml, 0).unwrap().contains("<path "));
        assert!(diagram_view("<html/>".into(), 0).is_err());
        let shapes = diagram_shapes();
        assert!(shapes.len() > 30);
        assert!(shapes.iter().all(|s| !s.path.is_empty() || s.id == "text"), "every shape has a drawing");
    }
}
