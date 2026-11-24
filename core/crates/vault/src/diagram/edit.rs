//! Changes to a page, as the diagram editors make them: shapes from the
//! palette, edges between them, moving, resizing, labels, styles, layers of
//! stacking, groups, aligning and spreading, duplicating and deleting.

use super::model::{Cell, Geometry, Page};
use super::scene::origin;
use super::style::Style;

/// A row stacked in a table-like shape (a UML class, an entity).
pub struct PaletteRow {
    pub text: &'static str,
    pub height: f64,
}

/// A shape the editors offer.
pub struct PaletteShape {
    pub id: &'static str,
    pub name: &'static str,
    /// `General`, `Arrows`, `Flowchart`, `UML`, `Entity Relation` or `BPMN`.
    pub group: &'static str,
    pub style: &'static str,
    pub width: f64,
    pub height: f64,
    /// What it starts with written in it.
    pub text: &'static str,
    /// Rows stacked under its header.
    pub rows: &'static [PaletteRow],
}

const fn shape(
    id: &'static str,
    name: &'static str,
    group: &'static str,
    style: &'static str,
    width: f64,
    height: f64,
) -> PaletteShape {
    PaletteShape { id, name, group, style, width, height, text: "", rows: &[] }
}

const ROW: &str = "text;strokeColor=none;fillColor=none;align=left;verticalAlign=middle;spacingLeft=4;spacingRight=4;\
overflow=hidden;rotatable=0;whiteSpace=wrap;html=1;";
const TABLE: &str = "swimlane;fontStyle=1;align=center;verticalAlign=top;childLayout=stackLayout;horizontal=1;\
startSize=26;horizontalStack=0;resizeParent=1;resizeParentMax=0;resizeLast=0;collapsible=1;marginBottom=0;\
whiteSpace=wrap;html=1;";

/// The shapes the editors offer, by group.
pub const PALETTE: &[PaletteShape] = &[
    shape("rectangle", "Rectangle", "General", "rounded=0;whiteSpace=wrap;html=1;", 120.0, 60.0),
    shape("rounded", "Rounded Rectangle", "General", "rounded=1;whiteSpace=wrap;html=1;", 120.0, 60.0),
    shape("ellipse", "Ellipse", "General", "ellipse;whiteSpace=wrap;html=1;", 120.0, 80.0),
    shape("circle", "Circle", "General", "ellipse;whiteSpace=wrap;html=1;aspect=fixed;", 80.0, 80.0),
    shape("diamond", "Diamond", "General", "rhombus;whiteSpace=wrap;html=1;", 80.0, 80.0),
    shape("triangle", "Triangle", "General", "triangle;whiteSpace=wrap;html=1;", 60.0, 80.0),
    shape(
        "hexagon",
        "Hexagon",
        "General",
        "shape=hexagon;perimeter=hexagonPerimeter2;whiteSpace=wrap;html=1;fixedSize=1;",
        120.0,
        80.0,
    ),
    shape(
        "parallelogram",
        "Parallelogram",
        "General",
        "shape=parallelogram;perimeter=parallelogramPerimeter;whiteSpace=wrap;html=1;fixedSize=1;",
        120.0,
        60.0,
    ),
    shape(
        "trapezoid",
        "Trapezoid",
        "General",
        "shape=trapezoid;perimeter=trapezoidPerimeter;whiteSpace=wrap;html=1;fixedSize=1;",
        120.0,
        60.0,
    ),
    shape(
        "cylinder",
        "Cylinder",
        "General",
        "shape=cylinder3;whiteSpace=wrap;html=1;boundedLbl=1;backgroundOutline=1;size=15;",
        60.0,
        80.0,
    ),
    shape("cloud", "Cloud", "General", "ellipse;shape=cloud;whiteSpace=wrap;html=1;", 120.0, 80.0),
    shape(
        "document",
        "Document",
        "General",
        "shape=document;whiteSpace=wrap;html=1;boundedLbl=1;",
        120.0,
        80.0,
    ),
    shape(
        "note",
        "Sticky Note",
        "General",
        "shape=note;whiteSpace=wrap;html=1;backgroundOutline=1;size=15;",
        80.0,
        100.0,
    ),
    shape("card", "Card", "General", "shape=card;whiteSpace=wrap;html=1;", 80.0, 100.0),
    shape(
        "step",
        "Step",
        "General",
        "shape=step;perimeter=stepPerimeter;whiteSpace=wrap;html=1;fixedSize=1;",
        120.0,
        80.0,
    ),
    shape(
        "callout",
        "Callout",
        "General",
        "shape=callout;whiteSpace=wrap;html=1;perimeter=calloutPerimeter;size=20;position=0.3;position2=0.2;base=20;",
        120.0,
        80.0,
    ),
    shape("cross", "Cross", "General", "shape=cross;whiteSpace=wrap;html=1;size=0.25;", 80.0, 80.0),
    shape("tape", "Tape", "General", "shape=tape;size=0.4;whiteSpace=wrap;html=1;", 120.0, 60.0),
    shape("cube", "Cube", "General", "shape=cube;size=20;whiteSpace=wrap;html=1;", 100.0, 80.0),
    shape(
        "curly-bracket",
        "Curly Bracket",
        "General",
        "shape=curlyBracket;rounded=1;fillColor=none;whiteSpace=wrap;html=1;",
        20.0,
        120.0,
    ),
    shape(
        "data-storage",
        "Data Storage",
        "General",
        "shape=dataStorage;size=0.1;whiteSpace=wrap;html=1;",
        120.0,
        60.0,
    ),
    shape(
        "internal-storage",
        "Internal Storage",
        "General",
        "shape=internalStorage;dx=20;dy=20;whiteSpace=wrap;html=1;",
        100.0,
        80.0,
    ),
    shape("or", "OR", "General", "shape=or;whiteSpace=wrap;html=1;", 80.0, 60.0),
    shape("xor", "XOR", "General", "shape=xor;whiteSpace=wrap;html=1;", 80.0, 60.0),
    shape("sum-ellipse", "Sum", "General", "shape=sumEllipse;whiteSpace=wrap;html=1;", 60.0, 60.0),
    shape("or-ellipse", "OR Circle", "General", "shape=orEllipse;whiteSpace=wrap;html=1;", 60.0, 60.0),
    shape("star", "Star", "General", "shape=mxgraph.basic.star;whiteSpace=wrap;html=1;", 80.0, 80.0),
    shape("plus", "Plus", "General", "shape=plus;whiteSpace=wrap;html=1;", 40.0, 40.0),
    shape(
        "partial-rectangle",
        "Partial Rectangle",
        "General",
        "shape=partialRectangle;top=0;bottom=0;left=1;right=1;fillColor=none;whiteSpace=wrap;html=1;",
        100.0,
        60.0,
    ),
    shape(
        "parallel-marker",
        "Parallel Marker",
        "General",
        "shape=parallelMarker;whiteSpace=wrap;html=1;",
        30.0,
        30.0,
    ),
    shape(
        "waypoint",
        "Waypoint",
        "General",
        "shape=waypoint;size=6;perimeter=centerPerimeter;whiteSpace=wrap;html=1;",
        20.0,
        20.0,
    ),
    shape(
        "octagon",
        "Octagon",
        "General",
        "shape=mxgraph.basic.octagon2;dx=15;whiteSpace=wrap;html=1;",
        80.0,
        80.0,
    ),
    shape(
        "pentagon",
        "Pentagon",
        "General",
        "shape=mxgraph.basic.pentagon;whiteSpace=wrap;html=1;",
        100.0,
        90.0,
    ),
    shape(
        "singleArrow",
        "Arrow",
        "Arrows",
        "shape=singleArrow;whiteSpace=wrap;html=1;arrowWidth=0.4;arrowSize=0.3;",
        120.0,
        60.0,
    ),
    shape(
        "doubleArrow",
        "Double Arrow",
        "Arrows",
        "shape=doubleArrow;whiteSpace=wrap;html=1;arrowWidth=0.4;arrowSize=0.25;",
        140.0,
        60.0,
    ),
    PaletteShape {
        text: "Text",
        ..shape(
            "text",
            "Text",
            "General",
            "text;html=1;align=center;verticalAlign=middle;whiteSpace=wrap;",
            60.0,
            30.0,
        )
    },
    shape(
        "actor",
        "Actor",
        "General",
        "shape=umlActor;verticalLabelPosition=bottom;verticalAlign=top;html=1;outlineConnect=0;",
        30.0,
        60.0,
    ),
    PaletteShape {
        text: "Container",
        ..shape("container", "Container", "General", "swimlane;whiteSpace=wrap;html=1;", 200.0, 200.0)
    },
    shape(
        "terminator",
        "Start / End",
        "Flowchart",
        "shape=mxgraph.flowchart.terminator;whiteSpace=wrap;html=1;",
        120.0,
        40.0,
    ),
    shape("process", "Process", "Flowchart", "rounded=0;whiteSpace=wrap;html=1;", 120.0, 60.0),
    shape("decision", "Decision", "Flowchart", "rhombus;whiteSpace=wrap;html=1;", 100.0, 80.0),
    shape(
        "data",
        "Data",
        "Flowchart",
        "shape=parallelogram;perimeter=parallelogramPerimeter;whiteSpace=wrap;html=1;fixedSize=1;",
        120.0,
        60.0,
    ),
    shape(
        "predefined",
        "Predefined Process",
        "Flowchart",
        "shape=process;whiteSpace=wrap;html=1;backgroundOutline=1;",
        120.0,
        60.0,
    ),
    shape(
        "database",
        "Database",
        "Flowchart",
        "shape=cylinder3;whiteSpace=wrap;html=1;boundedLbl=1;backgroundOutline=1;size=15;",
        60.0,
        80.0,
    ),
    shape(
        "manual-input",
        "Manual Input",
        "Flowchart",
        "shape=manualInput;whiteSpace=wrap;html=1;",
        120.0,
        60.0,
    ),
    shape("delay", "Delay", "Flowchart", "shape=delay;whiteSpace=wrap;html=1;", 100.0, 60.0),
    shape(
        "preparation",
        "Preparation",
        "Flowchart",
        "shape=hexagon;perimeter=hexagonPerimeter2;whiteSpace=wrap;html=1;fixedSize=1;",
        120.0,
        60.0,
    ),
    shape(
        "off-page",
        "Off-page Connector",
        "Flowchart",
        "shape=offPageConnector;whiteSpace=wrap;html=1;",
        60.0,
        60.0,
    ),
    shape("connector", "Connector", "Flowchart", "ellipse;whiteSpace=wrap;html=1;aspect=fixed;", 40.0, 40.0),
    PaletteShape {
        text: "Class",
        rows: &[
            PaletteRow { text: "+ field: type", height: 26.0 },
            PaletteRow { text: "+ method(): type", height: 26.0 },
        ],
        ..shape("class", "Class", "UML", TABLE, 160.0, 78.0)
    },
    PaletteShape {
        text: "Interface",
        rows: &[PaletteRow { text: "+ method(): type", height: 26.0 }],
        ..shape("interface", "Interface", "UML", TABLE, 160.0, 52.0)
    },
    shape(
        "uml-actor",
        "Actor",
        "UML",
        "shape=umlActor;verticalLabelPosition=bottom;verticalAlign=top;html=1;outlineConnect=0;",
        30.0,
        60.0,
    ),
    shape("use-case", "Use Case", "UML", "ellipse;whiteSpace=wrap;html=1;", 140.0, 70.0),
    shape(
        "package",
        "Package",
        "UML",
        "shape=folder;whiteSpace=wrap;html=1;tabWidth=50;tabHeight=14;",
        120.0,
        80.0,
    ),
    shape(
        "component",
        "Component",
        "UML",
        "shape=component;align=left;spacingLeft=36;whiteSpace=wrap;html=1;",
        120.0,
        60.0,
    ),
    PaletteShape {
        text: "Object",
        ..shape(
            "lifeline",
            "Lifeline",
            "UML",
            "shape=umlLifeline;perimeter=lifelinePerimeter;size=40;verticalAlign=top;whiteSpace=wrap;html=1;",
            100.0,
            240.0,
        )
    },
    shape(
        "actor-lifeline",
        "Actor Lifeline",
        "UML",
        "shape=umlLifeline;perimeter=lifelinePerimeter;size=40;verticalAlign=top;participant=umlActor;whiteSpace=wrap;html=1;",
        50.0,
        240.0,
    ),
    shape(
        "boundary-lifeline",
        "Boundary Lifeline",
        "UML",
        "shape=umlLifeline;perimeter=lifelinePerimeter;size=40;verticalAlign=top;participant=umlBoundary;whiteSpace=wrap;html=1;",
        50.0,
        240.0,
    ),
    shape(
        "entity-lifeline",
        "Entity Lifeline",
        "UML",
        "shape=umlLifeline;perimeter=lifelinePerimeter;size=40;verticalAlign=top;participant=umlEntity;whiteSpace=wrap;html=1;",
        50.0,
        240.0,
    ),
    shape(
        "control-lifeline",
        "Control Lifeline",
        "UML",
        "shape=umlLifeline;perimeter=lifelinePerimeter;size=40;verticalAlign=top;participant=umlControl;whiteSpace=wrap;html=1;",
        50.0,
        240.0,
    ),
    PaletteShape {
        text: "sd",
        ..shape(
            "uml-frame",
            "Frame",
            "UML",
            "shape=umlFrame;width=80;height=30;align=left;verticalAlign=top;whiteSpace=wrap;html=1;",
            300.0,
            200.0,
        )
    },
    shape(
        "uml-boundary",
        "Boundary",
        "UML",
        "shape=umlBoundary;verticalLabelPosition=bottom;verticalAlign=top;whiteSpace=wrap;html=1;",
        60.0,
        50.0,
    ),
    shape(
        "uml-entity",
        "Entity",
        "UML",
        "shape=umlEntity;verticalLabelPosition=bottom;verticalAlign=top;whiteSpace=wrap;html=1;",
        50.0,
        50.0,
    ),
    shape(
        "uml-control",
        "Control",
        "UML",
        "shape=umlControl;verticalLabelPosition=bottom;verticalAlign=top;whiteSpace=wrap;html=1;",
        50.0,
        50.0,
    ),
    shape(
        "provided-required-interface",
        "Provided / Required Interface",
        "UML",
        "shape=providedRequiredInterface;whiteSpace=wrap;html=1;",
        30.0,
        30.0,
    ),
    shape(
        "required-interface",
        "Required Interface",
        "UML",
        "shape=requiredInterface;fillColor=none;whiteSpace=wrap;html=1;",
        15.0,
        30.0,
    ),
    shape(
        "provided-interface",
        "Provided Interface",
        "UML",
        "shape=lollipop;size=20;fillColor=none;whiteSpace=wrap;html=1;",
        30.0,
        70.0,
    ),
    PaletteShape {
        text: "State",
        ..shape(
            "uml-state",
            "State",
            "UML",
            "shape=umlState;rounded=1;arcSize=0.1;whiteSpace=wrap;html=1;",
            120.0,
            60.0,
        )
    },
    shape(
        "start-state",
        "Start State",
        "UML",
        "ellipse;fillColor=#000000;strokeColor=#000000;whiteSpace=wrap;html=1;",
        30.0,
        30.0,
    ),
    shape(
        "end-state",
        "End State",
        "UML",
        "ellipse;shape=endState;fillColor=#000000;strokeColor=#000000;whiteSpace=wrap;html=1;",
        30.0,
        30.0,
    ),
    shape(
        "fork-join",
        "Fork / Join",
        "UML",
        "shape=line;strokeWidth=8;strokeColor=#000000;whiteSpace=wrap;html=1;",
        120.0,
        10.0,
    ),
    shape(
        "uml-destroy",
        "Destroy",
        "UML",
        "shape=umlDestroy;strokeWidth=3;fillColor=none;whiteSpace=wrap;html=1;",
        30.0,
        30.0,
    ),
    PaletteShape {
        text: "Entity",
        rows: &[
            PaletteRow { text: "id: integer", height: 26.0 },
            PaletteRow { text: "name: text", height: 26.0 },
        ],
        ..shape("entity", "Entity", "Entity Relation", TABLE, 160.0, 78.0)
    },
    shape("relationship", "Relationship", "Entity Relation", "rhombus;whiteSpace=wrap;html=1;", 120.0, 60.0),
    shape("attribute", "Attribute", "Entity Relation", "ellipse;whiteSpace=wrap;html=1;", 100.0, 50.0),
    PaletteShape {
        text: "Task",
        ..shape(
            "bpmn-task",
            "Task",
            "BPMN",
            "shape=mxgraph.bpmn.task2;rectStyle=rounded;size=10;taskMarker=abstract;whiteSpace=wrap;html=1;",
            120.0,
            80.0,
        )
    },
    PaletteShape {
        text: "Sub-process",
        ..shape(
            "bpmn-subprocess",
            "Sub-process",
            "BPMN",
            "shape=mxgraph.bpmn.task2;rectStyle=rounded;size=10;taskMarker=abstract;isLoopSub=1;whiteSpace=wrap;html=1;",
            140.0,
            100.0,
        )
    },
    shape(
        "bpmn-start",
        "Start Event",
        "BPMN",
        "shape=mxgraph.bpmn.event;outline=standard;symbol=general;whiteSpace=wrap;html=1;",
        40.0,
        40.0,
    ),
    shape(
        "bpmn-intermediate",
        "Intermediate Event",
        "BPMN",
        "shape=mxgraph.bpmn.event;outline=catching;symbol=general;whiteSpace=wrap;html=1;",
        40.0,
        40.0,
    ),
    shape(
        "bpmn-end",
        "End Event",
        "BPMN",
        "shape=mxgraph.bpmn.event;outline=end;symbol=general;whiteSpace=wrap;html=1;",
        40.0,
        40.0,
    ),
    shape(
        "bpmn-exclusive",
        "Exclusive Gateway",
        "BPMN",
        "shape=mxgraph.bpmn.gateway2;perimeter=rhombusPerimeter;outline=none;symbol=none;gwType=exclusive;whiteSpace=wrap;html=1;",
        60.0,
        60.0,
    ),
    shape(
        "bpmn-parallel",
        "Parallel Gateway",
        "BPMN",
        "shape=mxgraph.bpmn.gateway2;perimeter=rhombusPerimeter;outline=none;symbol=none;gwType=parallel;whiteSpace=wrap;html=1;",
        60.0,
        60.0,
    ),
    shape(
        "bpmn-inclusive",
        "Inclusive Gateway",
        "BPMN",
        "shape=mxgraph.bpmn.gateway2;perimeter=rhombusPerimeter;outline=end;symbol=general;whiteSpace=wrap;html=1;",
        60.0,
        60.0,
    ),
    shape("bpmn-data", "Data Object", "BPMN", "shape=note;size=15;whiteSpace=wrap;html=1;", 50.0, 70.0),
    PaletteShape {
        text: "Pool",
        ..shape(
            "bpmn-pool",
            "Pool",
            "BPMN",
            "swimlane;horizontal=0;startSize=30;container=1;whiteSpace=wrap;html=1;",
            500.0,
            200.0,
        )
    },
    PaletteShape {
        text: "Lane",
        ..shape(
            "bpmn-lane",
            "Lane",
            "BPMN",
            "swimlane;horizontal=0;startSize=30;container=1;whiteSpace=wrap;html=1;",
            450.0,
            120.0,
        )
    },
];

/// The style a new edge gets.
pub const EDGE_STYLE: &str =
    "edgeStyle=orthogonalEdgeStyle;rounded=0;orthogonalLoop=1;jettySize=auto;html=1;";

/// A change to a page; shapes and edges by id, places in page coordinates.
#[derive(Debug, Clone, PartialEq)]
pub enum DiagramEdit {
    /// A shape from the palette with its top left at `x`, `y`.
    AddShape {
        shape: String,
        x: f64,
        y: f64,
    },
    /// An edge from a shape (or a point) to a shape (or a point).
    Connect {
        source: String,
        target: String,
        from: (f64, f64),
        to: (f64, f64),
    },
    /// Moves shapes and edges.
    Move {
        ids: Vec<String>,
        dx: f64,
        dy: f64,
    },
    /// A shape's new box.
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
    /// Takes cells away, with what they hold and the edges that end on
    /// them.
    Delete {
        ids: Vec<String>,
    },
    /// Joins an edge's start (`source`) or end to a shape, or (with an
    /// empty `cell`) leaves it loose at `at`.
    Reconnect {
        id: String,
        source: bool,
        cell: String,
        at: (f64, f64),
    },
    /// An edge's bends.
    SetBends {
        id: String,
        points: Vec<(f64, f64)>,
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
    /// Spaces shapes evenly, side to side or top to bottom.
    Distribute {
        ids: Vec<String>,
        horizontal: bool,
    },
    /// Copies cells (and what they hold), moved by `dx`, `dy`.
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
}

/// A picture's address as draw.io keeps it in a style: data without
/// `;base64`, and no `;` (which would end the style).
fn stored_image(image: &str) -> String {
    let image = image.trim();
    match image.strip_prefix("data:").and_then(|rest| rest.split_once(',')) {
        Some((kind, data)) => format!("data:{},{data}", kind.trim_end_matches(";base64")),
        None => image.replace(';', "%3B"),
    }
}

/// A label as a cell keeps it: HTML (escaped, lines as `<br>`) when its
/// style says `html=1`.
fn stored_text(style: &Style, text: &str) -> String {
    if !style.flag("html", false) {
        return text.to_string();
    }
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('\n', "<br>")
}

/// An id no cell on the page has.
fn fresh_id(page: &Page, taken: &[String]) -> String {
    (1..)
        .map(|n| format!("pl-{n}"))
        .find(|id| page.cell(id).is_none() && !taken.contains(id))
        .unwrap_or_default()
}

/// Where new shapes go: the page's first layer.
fn layer(page: &Page) -> String {
    page.layers().first().map_or_else(|| "1".to_string(), |l| l.id.clone())
}

/// A shape's box on the page.
fn page_box(page: &Page, cell: &Cell) -> Option<(f64, f64, f64, f64)> {
    let g = cell.geometry.as_ref()?;
    let (ox, oy) = origin(page, &cell.parent);
    Some((ox + g.x, oy + g.y, g.width, g.height))
}

/// A cell's id and the ids of every cell inside it.
fn with_descendants(page: &Page, ids: &[String]) -> Vec<String> {
    let mut all: Vec<String> = ids.to_vec();
    let mut i = 0;
    while i < all.len() {
        let id = all[i].clone();
        for cell in page.cells.iter().filter(|c| c.parent == id) {
            if !all.contains(&cell.id) {
                all.push(cell.id.clone());
            }
        }
        i += 1;
    }
    all
}

fn vertex<'a>(page: &'a mut Page, id: &str) -> Result<&'a mut Cell, String> {
    page.cell_mut(id).filter(|c| c.vertex).ok_or_else(|| format!("“{id}” is not a shape on this page."))
}

/// Makes a change; the cells to select afterwards.
pub fn edit_page(page: &mut Page, edit: &DiagramEdit) -> Result<Vec<String>, String> {
    match edit {
        DiagramEdit::AddShape { shape, x, y } => {
            let from = PALETTE
                .iter()
                .find(|p| p.id == shape)
                .ok_or_else(|| format!("No shape is called “{shape}”."))?;
            let style = Style::parse(from.style);
            let (id, parent) = (fresh_id(page, &[]), layer(page));
            page.cells.push(Cell {
                id: id.clone(),
                parent,
                value: stored_text(&style, from.text),
                style,
                vertex: true,
                geometry: Some(Geometry {
                    x: *x,
                    y: *y,
                    width: from.width,
                    height: from.height,
                    ..Geometry::default()
                }),
                ..Cell::default()
            });
            let mut top = Style::parse(from.style).number("startSize", 26.0);
            let mut taken = vec![id.clone()];
            for row in from.rows {
                let style = Style::parse(ROW);
                let row_id = fresh_id(page, &taken);
                taken.push(row_id.clone());
                page.cells.push(Cell {
                    id: row_id,
                    parent: id.clone(),
                    value: stored_text(&style, row.text),
                    style,
                    vertex: true,
                    geometry: Some(Geometry {
                        y: top,
                        width: from.width,
                        height: row.height,
                        ..Geometry::default()
                    }),
                    ..Cell::default()
                });
                top += row.height;
            }
            Ok(vec![id])
        }
        DiagramEdit::Connect { source, target, from, to } => {
            for end in [source, target].into_iter().filter(|e| !e.is_empty()) {
                vertex(page, end)?;
            }
            let (id, parent) = (fresh_id(page, &[]), layer(page));
            page.cells.push(Cell {
                id: id.clone(),
                parent,
                style: Style::parse(EDGE_STYLE),
                edge: true,
                source: source.clone(),
                target: target.clone(),
                geometry: Some(Geometry {
                    relative: true,
                    source_point: source.is_empty().then_some(*from),
                    target_point: target.is_empty().then_some(*to),
                    ..Geometry::default()
                }),
                ..Cell::default()
            });
            Ok(vec![id])
        }
        DiagramEdit::Move { ids, dx, dy } => {
            let moved: Vec<String> = ids.iter().filter(|id| page.cell(id).is_some()).cloned().collect();
            // Edges between moved shapes keep their bends with them.
            let carried: Vec<String> = page
                .cells
                .iter()
                .filter(|c| c.edge && !moved.contains(&c.id))
                .filter(|c| moved.contains(&c.source) && moved.contains(&c.target))
                .map(|c| c.id.clone())
                .collect();
            for cell in page.cells.iter_mut().filter(|c| moved.contains(&c.id) || carried.contains(&c.id)) {
                let Some(g) = cell.geometry.as_mut() else { continue };
                if cell.edge {
                    for p in
                        g.points.iter_mut().chain(g.source_point.iter_mut()).chain(g.target_point.iter_mut())
                    {
                        (p.0, p.1) = (p.0 + dx, p.1 + dy);
                    }
                } else if !g.relative {
                    (g.x, g.y) = (g.x + dx, g.y + dy);
                }
            }
            Ok(moved)
        }
        DiagramEdit::Resize { id, x, y, width, height } => {
            let parent = vertex(page, id)?.parent.clone();
            let (ox, oy) = origin(page, &parent);
            let g = vertex(page, id)?.geometry.get_or_insert_with(Geometry::default);
            (g.x, g.y, g.width, g.height) = (x - ox, y - oy, width.max(1.0), height.max(1.0));
            Ok(vec![id.clone()])
        }
        DiagramEdit::SetText { id, text } => {
            let cell = page.cell_mut(id).ok_or_else(|| format!("“{id}” is not on this page."))?;
            cell.value = stored_text(&cell.style, text);
            Ok(vec![id.clone()])
        }
        DiagramEdit::SetStyle { ids, key, value } => {
            for cell in page.cells.iter_mut().filter(|c| ids.contains(&c.id)) {
                cell.style.set(key, value);
            }
            Ok(ids.clone())
        }
        DiagramEdit::Delete { ids } => {
            let mut gone = with_descendants(page, ids);
            let edges: Vec<String> = page
                .cells
                .iter()
                .filter(|c| c.edge && (gone.contains(&c.source) || gone.contains(&c.target)))
                .map(|c| c.id.clone())
                .collect();
            gone.extend(edges);
            let gone = with_descendants(page, &gone);
            // The root and the layers stay.
            let root = page.root().to_string();
            page.cells.retain(|c| !gone.contains(&c.id) || c.id == root || c.parent == root);
            Ok(Vec::new())
        }
        DiagramEdit::Reconnect { id, source, cell, at } => {
            if !cell.is_empty() {
                vertex(page, cell)?;
            }
            let edge =
                page.cell_mut(id).filter(|c| c.edge).ok_or_else(|| format!("“{id}” is not an edge."))?;
            let parent = edge.parent.clone();
            let (ox, oy) = origin(page, &parent);
            let edge = page.cell_mut(id).ok_or_else(|| format!("“{id}” is not an edge."))?;
            let g = edge.geometry.get_or_insert_with(|| Geometry { relative: true, ..Geometry::default() });
            let loose = cell.is_empty().then_some((at.0 - ox, at.1 - oy));
            if *source {
                edge.source = cell.clone();
                g.source_point = loose;
            } else {
                edge.target = cell.clone();
                g.target_point = loose;
            }
            Ok(vec![id.clone()])
        }
        DiagramEdit::SetBends { id, points } => {
            let parent = page
                .cell(id)
                .filter(|c| c.edge)
                .ok_or_else(|| format!("“{id}” is not an edge."))?
                .parent
                .clone();
            let (ox, oy) = origin(page, &parent);
            let edge = page.cell_mut(id).ok_or_else(|| format!("“{id}” is not an edge."))?;
            let g = edge.geometry.get_or_insert_with(|| Geometry { relative: true, ..Geometry::default() });
            g.points = points.iter().map(|&(x, y)| (x - ox, y - oy)).collect();
            Ok(vec![id.clone()])
        }
        DiagramEdit::ToFront { ids } => {
            // With what they hold, which stays over them.
            let all = with_descendants(page, ids);
            let (mut front, rest): (Vec<Cell>, Vec<Cell>) =
                std::mem::take(&mut page.cells).into_iter().partition(|c| all.contains(&c.id));
            page.cells = rest;
            page.cells.append(&mut front);
            Ok(ids.clone())
        }
        DiagramEdit::ToBack { ids } => {
            for id in ids.iter().rev() {
                let Some(at) = page.cells.iter().position(|c| &c.id == id) else { continue };
                let cell = page.cells.remove(at);
                // Right after what holds it, under everything else there.
                let after = page.cells.iter().position(|c| c.id == cell.parent).map_or(0, |p| p + 1);
                page.cells.insert(after, cell);
            }
            Ok(ids.clone())
        }
        DiagramEdit::Group { ids } => group(page, ids),
        DiagramEdit::Ungroup { id } => {
            let (gx, gy) = vertex(page, id)?.geometry.as_ref().map_or((0.0, 0.0), |g| (g.x, g.y));
            let parent = vertex(page, id)?.parent.clone();
            let mut freed = Vec::new();
            for cell in page.cells.iter_mut().filter(|c| &c.parent == id) {
                cell.parent = parent.clone();
                if let Some(g) = cell.geometry.as_mut().filter(|g| !g.relative) {
                    (g.x, g.y) = (g.x + gx, g.y + gy);
                }
                freed.push(cell.id.clone());
            }
            page.cells.retain(|c| &c.id != id);
            Ok(freed)
        }
        DiagramEdit::Align { ids, how } => {
            let boxes: Vec<(String, (f64, f64, f64, f64))> = ids
                .iter()
                .filter_map(|id| {
                    page.cell(id).filter(|c| c.vertex).and_then(|c| Some((id.clone(), page_box(page, c)?)))
                })
                .collect();
            let left = boxes.iter().map(|(_, b)| b.0).fold(f64::INFINITY, f64::min);
            let top = boxes.iter().map(|(_, b)| b.1).fold(f64::INFINITY, f64::min);
            let right = boxes.iter().map(|(_, b)| b.0 + b.2).fold(f64::NEG_INFINITY, f64::max);
            let bottom = boxes.iter().map(|(_, b)| b.1 + b.3).fold(f64::NEG_INFINITY, f64::max);
            for (id, (x, y, w, h)) in boxes {
                let (nx, ny) = match how.as_str() {
                    "left" => (left, y),
                    "center" => ((left + right - w) / 2.0, y),
                    "right" => (right - w, y),
                    "top" => (x, top),
                    "middle" => (x, (top + bottom - h) / 2.0),
                    "bottom" => (x, bottom - h),
                    other => return Err(format!("Shapes can't be aligned “{other}”.")),
                };
                if let Some(g) = vertex(page, &id)?.geometry.as_mut() {
                    (g.x, g.y) = (g.x + nx - x, g.y + ny - y);
                }
            }
            Ok(ids.clone())
        }
        DiagramEdit::Distribute { ids, horizontal } => {
            let mut boxes: Vec<(String, (f64, f64, f64, f64))> = ids
                .iter()
                .filter_map(|id| {
                    page.cell(id).filter(|c| c.vertex).and_then(|c| Some((id.clone(), page_box(page, c)?)))
                })
                .collect();
            if boxes.len() < 3 {
                return Ok(ids.clone());
            }
            let middle =
                |b: &(f64, f64, f64, f64)| if *horizontal { b.0 + b.2 / 2.0 } else { b.1 + b.3 / 2.0 };
            boxes.sort_by(|a, b| middle(&a.1).total_cmp(&middle(&b.1)));
            let (first, last) = (middle(&boxes[0].1), middle(&boxes[boxes.len() - 1].1));
            let step = (last - first) / (boxes.len() - 1) as f64;
            for (i, (id, b)) in boxes.iter().enumerate() {
                let shift = first + step * i as f64 - middle(b);
                if let Some(g) = vertex(page, id)?.geometry.as_mut() {
                    if *horizontal {
                        g.x += shift;
                    } else {
                        g.y += shift;
                    }
                }
            }
            Ok(ids.clone())
        }
        DiagramEdit::Duplicate { ids, dx, dy } => duplicate(page, ids, *dx, *dy),
        DiagramEdit::AddImage { image, x, y, width, height } => {
            if image.trim().is_empty() || *width <= 0.0 || *height <= 0.0 {
                return Err("That picture cannot be placed.".into());
            }
            let style = Style::parse(&format!(
                "shape=image;verticalLabelPosition=bottom;labelBackgroundColor=default;verticalAlign=top;\
                 aspect=fixed;imageAspect=0;image={};",
                stored_image(image)
            ));
            let (id, parent) = (fresh_id(page, &[]), layer(page));
            page.cells.push(Cell {
                id: id.clone(),
                parent,
                style,
                vertex: true,
                geometry: Some(Geometry {
                    x: *x,
                    y: *y,
                    width: *width,
                    height: *height,
                    ..Geometry::default()
                }),
                ..Cell::default()
            });
            Ok(vec![id])
        }
    }
}

/// Puts shapes in a new group around them.
fn group(page: &mut Page, ids: &[String]) -> Result<Vec<String>, String> {
    let members: Vec<(String, (f64, f64, f64, f64))> = ids
        .iter()
        .filter_map(|id| {
            page.cell(id).filter(|c| c.vertex).and_then(|c| Some((id.clone(), page_box(page, c)?)))
        })
        .collect();
    if members.len() < 2 {
        return Err("Pick two shapes or more to group.".into());
    }
    let parent = page.cell(&members[0].0).map(|c| c.parent.clone()).unwrap_or_default();
    let (ox, oy) = origin(page, &parent);
    let left = members.iter().map(|(_, b)| b.0).fold(f64::INFINITY, f64::min);
    let top = members.iter().map(|(_, b)| b.1).fold(f64::INFINITY, f64::min);
    let right = members.iter().map(|(_, b)| b.0 + b.2).fold(f64::NEG_INFINITY, f64::max);
    let bottom = members.iter().map(|(_, b)| b.1 + b.3).fold(f64::NEG_INFINITY, f64::max);
    let id = fresh_id(page, &[]);
    let at = page.cells.iter().position(|c| c.id == members[0].0).unwrap_or(page.cells.len());
    page.cells.insert(
        at,
        Cell {
            id: id.clone(),
            parent: parent.clone(),
            style: Style::parse("group;"),
            vertex: true,
            other: vec![("connectable".to_string(), "0".to_string())],
            geometry: Some(Geometry {
                x: left - ox,
                y: top - oy,
                width: right - left,
                height: bottom - top,
                ..Geometry::default()
            }),
            ..Cell::default()
        },
    );
    for (member, (x, y, _, _)) in &members {
        if let Some(cell) = page.cell_mut(member) {
            cell.parent = id.clone();
            if let Some(g) = cell.geometry.as_mut() {
                (g.x, g.y) = (x - left, y - top);
            }
        }
    }
    Ok(vec![id])
}

/// Copies cells with what they hold under new ids, edges among them joined
/// to the copies.
fn duplicate(page: &mut Page, ids: &[String], dx: f64, dy: f64) -> Result<Vec<String>, String> {
    let all = with_descendants(page, ids);
    let mut renamed: Vec<(String, String)> = Vec::new();
    for id in &all {
        let taken: Vec<String> = renamed.iter().map(|(_, new)| new.clone()).collect();
        renamed.push((id.clone(), fresh_id(page, &taken)));
    }
    let new_id = |old: &str| renamed.iter().find(|(o, _)| o == old).map(|(_, n)| n.clone());
    let copies: Vec<Cell> = page
        .cells
        .iter()
        .filter(|c| all.contains(&c.id) && (c.vertex || c.edge))
        .map(|c| {
            let mut copy = c.clone();
            copy.id = new_id(&c.id).unwrap_or_default();
            let top = ids.contains(&c.id);
            if let Some(parent) = new_id(&c.parent) {
                copy.parent = parent;
            }
            if c.edge {
                copy.source = new_id(&c.source).unwrap_or_else(|| c.source.clone());
                copy.target = new_id(&c.target).unwrap_or_else(|| c.target.clone());
            }
            if let Some(g) = copy.geometry.as_mut().filter(|_| top) {
                if c.edge {
                    for p in
                        g.points.iter_mut().chain(g.source_point.iter_mut()).chain(g.target_point.iter_mut())
                    {
                        (p.0, p.1) = (p.0 + dx, p.1 + dy);
                    }
                } else if !g.relative {
                    (g.x, g.y) = (g.x + dx, g.y + dy);
                }
            }
            copy
        })
        .collect();
    let made: Vec<String> = ids.iter().filter_map(|id| new_id(id)).collect();
    page.cells.extend(copies);
    Ok(made)
}
