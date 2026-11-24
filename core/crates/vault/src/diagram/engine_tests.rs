use base64::Engine;

use super::route::{End, arrowhead, marker_inset, route, trim_line};
use super::shapes::{kind, outline, perimeter};
use super::*;

/// Whether two points are the same to well within a hundredth.
fn close(a: (f64, f64), b: (f64, f64)) -> bool {
    (a.0 - b.0).abs() < 1e-6 && (a.1 - b.1).abs() < 1e-6
}

const PLAIN: &str = r#"<mxfile host="app" compressed="false"><diagram id="d1" name="Flow"><mxGraphModel dx="800" grid="1" gridSize="10"><root><mxCell id="0"/><mxCell id="1" parent="0"/><mxCell id="a" value="Start &amp;amp; go" style="rounded=1;whiteSpace=wrap;html=1;" vertex="1" parent="1"><mxGeometry x="40" y="40" width="120" height="60" as="geometry"/></mxCell><object label="Check" link="https://example.org" id="b"><mxCell style="rhombus;whiteSpace=wrap;html=1;" vertex="1" parent="1"><mxGeometry x="240" y="30" width="80" height="80" as="geometry"/></mxCell></object><mxCell id="e" value="yes" style="edgeStyle=orthogonalEdgeStyle;rounded=0;html=1;" edge="1" parent="1" source="a" target="b"><mxGeometry relative="1" as="geometry"><Array as="points"><mxPoint x="200" y="70"/></Array></mxGeometry></mxCell></root></mxGraphModel></diagram></mxfile>"#;

#[test]
fn reads_and_sets_styles() {
    let mut style = Style::parse("ellipse;whiteSpace=wrap;html=1;fillColor=#dae8fc;");
    assert_eq!(style.shape(), "ellipse");
    assert_eq!(style.get("fillColor"), Some("#dae8fc"));
    assert!(style.flag("html", false) && style.has("ellipse"));
    style.set("fillColor", "#ffffff");
    style.set("html", "");
    style.set("dashed", "1");
    assert_eq!(style.to_string(), "ellipse;whiteSpace=wrap;fillColor=#ffffff;dashed=1;");
    assert_eq!(Style::parse("shape=cylinder3;size=15;").shape(), "cylinder3");
    assert_eq!(Style::parse("rounded=1;").shape(), "rectangle");
}

#[test]
fn reads_diagrams_and_writes_them_back() {
    let diagram = Diagram::parse(PLAIN).unwrap();
    assert_eq!(diagram.attrs, [("host".to_string(), "app".to_string())]);
    let page = &diagram.pages[0];
    assert_eq!((page.id.as_str(), page.name.as_str()), ("d1", "Flow"));
    assert_eq!(page.cells.len(), 5);
    let check = page.cell("b").unwrap();
    assert_eq!(check.value, "Check");
    assert_eq!(
        check.wrapper,
        Some(("object".to_string(), vec![("link".to_string(), "https://example.org".to_string())]))
    );
    let edge = page.cell("e").unwrap();
    assert!(edge.edge && edge.source == "a" && edge.target == "b");
    assert_eq!(edge.geometry.as_ref().unwrap().points, [(200.0, 70.0)]);
    assert_eq!(page.layers().len(), 1);
    assert_eq!(Diagram::parse(&diagram.to_xml()), Some(diagram));
    assert!(Diagram::parse("<svg/>").is_none());
    assert_eq!(
        Diagram::parse("<mxfile/>").unwrap().pages[0].cells.len(),
        2,
        "an empty file gets a blank page"
    );
}

#[test]
fn reads_compressed_pages() {
    let model = r#"<mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/><mxCell id="x" value="中文" vertex="1" parent="1"><mxGeometry x="1" y="2" width="3" height="4" as="geometry"/></mxCell></root></mxGraphModel>"#;
    let encoded: String = model
        .bytes()
        .map(|b| if b.is_ascii_alphanumeric() { char::from(b).to_string() } else { format!("%{b:02X}") })
        .collect();
    let mut deflater = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut deflater, encoded.as_bytes()).unwrap();
    let packed = base64::engine::general_purpose::STANDARD.encode(deflater.finish().unwrap());
    let file = format!("<mxfile><diagram id=\"p\" name=\"Zip\">{packed}</diagram></mxfile>");
    let diagram = Diagram::parse(&file).unwrap();
    assert_eq!(diagram.pages[0].cell("x").unwrap().value, "中文");
    assert_eq!(diagram.pages[0].name, "Zip");
}

#[test]
fn draws_shapes_and_meets_their_outlines() {
    let r = Rect { x: 0.0, y: 0.0, width: 100.0, height: 50.0 };
    assert_eq!(outline(&Style::parse(""), r).path, "M0 0L100 0L100 50L0 50Z");
    assert!(outline(&Style::parse("rounded=1;"), r).path.starts_with("M7.5 0L92.5 0Q100 0 100 7.5"));
    let cylinder = outline(&Style::parse("shape=cylinder3;"), r);
    assert!(cylinder.filled && !cylinder.inner.is_empty());
    assert!(!outline(&Style::parse("text;"), r).filled);
    // Arrows, callouts and crosses: draw.io's shapes, as polygons.
    let corners = |style: &str| outline(&Style::parse(style), r).path.matches('L').count() + 1;
    assert_eq!(corners("shape=singleArrow;"), 7);
    assert_eq!(corners("shape=doubleArrow;"), 10);
    assert_eq!(corners("shape=callout;"), 7);
    assert_eq!(corners("shape=cross;"), 12);
    assert!(
        outline(&Style::parse("shape=singleArrow;arrowSize=0.2;"), r).path.contains("L100 25"),
        "its tip"
    );
    assert!(close(perimeter(&Style::parse(""), r, (200.0, 25.0)), (100.0, 25.0)));
    assert!(close(perimeter(&Style::parse("ellipse;"), r, (50.0, 100.0)), (50.0, 50.0)));
    assert!(close(perimeter(&Style::parse("rhombus;"), r, (200.0, 25.0)), (100.0, 25.0)));
}

// Style, native kind, counts of L/Q/C, and whether the outline is filled.
const EXTRA_SHAPES: &[(&str, &str, [usize; 3], bool)] = &[
    ("shape=tape;", "tape", [1, 4, 0], true),
    ("shape=cube;size=20;", "cube", [5, 0, 0], true),
    ("shape=curlyBracket;", "curlyBracket", [6, 0, 0], false),
    ("shape=dataStorage;", "dataStorage", [2, 2, 0], true),
    ("shape=internalStorage;", "internalStorage", [3, 0, 0], true),
    ("shape=or;", "or", [0, 2, 0], true),
    ("shape=xor;", "xor", [0, 3, 0], true),
    ("shape=sumEllipse;", "sumEllipse", [0, 0, 4], true),
    ("shape=orEllipse;", "orEllipse", [0, 0, 4], true),
    ("shape=mxgraph.basic.star;", "star", [9, 0, 0], true),
    ("shape=plus;", "plus", [3, 0, 0], true),
    ("shape=partialRectangle;", "partialRectangle", [4, 0, 0], false),
    ("shape=parallelMarker;", "parallelMarker", [0, 0, 0], false),
    ("shape=waypoint;", "waypoint", [0, 0, 0], false),
    ("shape=mxgraph.basic.octagon2;dx=10;", "octagon", [7, 0, 0], true),
    ("shape=mxgraph.basic.pentagon;", "pentagon", [4, 0, 0], true),
    ("shape=umlLifeline;size=20;", "umlLifeline", [3, 0, 0], true),
    ("shape=umlFrame;", "umlFrame", [4, 0, 0], true),
    ("shape=umlBoundary;", "umlBoundary", [0, 0, 4], true),
    ("shape=umlEntity;", "umlEntity", [0, 0, 4], true),
    ("shape=umlControl;", "umlControl", [0, 0, 4], true),
    ("shape=providedRequiredInterface;", "providedRequiredInterface", [0, 0, 4], true),
    ("shape=requiredInterface;", "requiredInterface", [0, 2, 0], false),
    ("shape=lollipop;", "lollipop", [0, 0, 4], true),
    ("shape=umlState;rounded=1;", "umlState", [4, 4, 0], true),
    ("shape=endState;", "endState", [0, 0, 4], false),
    ("ellipse;shape=endState;", "endState", [0, 0, 4], false),
    ("shape=umlDestroy;", "umlDestroy", [2, 0, 0], false),
    ("shape=mxgraph.bpmn.task2;isLoopSub=1;", "bpmnTask", [4, 4, 0], true),
    ("shape=mxgraph.bpmn.event;outline=end;", "bpmnEnd", [0, 0, 4], true),
    ("shape=mxgraph.bpmn.gateway2;gwType=exclusive;", "exclusiveGateway", [3, 0, 0], true),
    ("shape=mxgraph.bpmn.gateway2;gwType=parallel;", "parallelGateway", [3, 0, 0], true),
    ("shape=mxgraph.bpmn.gateway2;outline=end;symbol=general;", "inclusiveGateway", [3, 0, 0], true),
];

#[test]
fn draws_additional_native_shapes() {
    let r = Rect { x: 10.0, y: 20.0, width: 100.0, height: 80.0 };
    for &(text, expected, commands, filled) in EXTRA_SHAPES {
        let style = Style::parse(text);
        assert_eq!(kind(&style), expected, "{text}");
        let drawn = outline(&style, r);
        assert_eq!(drawn.filled, filled, "{text}");
        assert_eq!(['L', 'Q', 'C'].map(|c| drawn.path.matches(c).count()), commands, "{text}");
        if filled || expected == "endState" {
            assert!(drawn.path.ends_with('Z'), "{text}: {}", drawn.path);
        } else if !matches!(expected, "parallelMarker" | "waypoint") {
            assert!(!drawn.path.contains('Z'), "{text}");
        }
        let connection = perimeter(&style, r, (200.0, 60.0));
        assert!(close(connection, if expected == "waypoint" { r.centre() } else { (110.0, 60.0) }), "{text}");
    }
    let draw = |text: &str| outline(&Style::parse(text), r);
    assert_eq!(draw("shape=cube;size=20;darkOpacity=0.5;").inner, "M30 100L30 40L10 20M30 40L110 40");
    assert_eq!(draw("shape=internalStorage;dx=12;dy=17;").inner, "M22 20L22 100M10 37L110 37");
    assert_eq!(draw("shape=internalStorage;dx=500;dy=-4;").inner, "M110 20L110 100M10 20L110 20");
    assert_eq!(draw("shape=orEllipse;").inner, "M10 60L110 60M60 20L60 100");
    assert_eq!(draw("shape=sumEllipse;").inner, "M24.5 31.6L95.5 88.4M95.5 31.6L24.5 88.4");
    assert_eq!(draw("shape=plus;").inner, "M60 37L60 83M27 60L93 60");
    assert_eq!(draw("shape=partialRectangle;top=0;right=0;").path, "M110 100L10 100M10 100L10 20");
    for side in ["top", "right", "bottom", "left"] {
        let mut style = Style::parse("shape=partialRectangle;top=0;right=0;bottom=0;left=0;");
        assert!(outline(&style, r).path.is_empty());
        style.set(side, "1");
        assert_eq!(outline(&style, r).path.matches('L').count(), 1, "{side}");
    }
    assert_eq!(draw("shape=umlFrame;width=70;height=35;").path, "M10 20L80 20L80 40L70 55L10 55Z");
    assert_eq!(draw("shape=umlFrame;width=70;height=35;").inner, "M80 20L110 20L110 100L10 100L10 55");
    assert_eq!(draw("shape=umlEntity;").inner, "M22.5 100L97.5 100");
    assert_eq!(draw("shape=lollipop;size=20;").inner, "M60 40L60 100");
    assert_eq!(draw("shape=umlDestroy;").path, "M110 20L10 100M10 20L110 100");
    assert_eq!(draw("shape=parallelMarker;").solid.matches('Z').count(), 3);
    let waypoint = draw("shape=waypoint;size=6;");
    assert!(waypoint.path.is_empty() && waypoint.solid.starts_with("M63 60C"));
    assert_eq!(waypoint.solid.matches('C').count(), 4);
    let state = draw("shape=endState;");
    assert!(state.solid.starts_with("M106 60C") && state.solid.ends_with('Z'));
    let sub = draw("shape=mxgraph.bpmn.task2;isLoopSub=1;");
    assert_eq!(sub.inner, "M53 81L67 81L67 95L53 95ZM60 85L60 91M57 88L63 88");
    assert!(draw("shape=mxgraph.bpmn.task2;").inner.is_empty());
    assert_eq!(draw("shape=mxgraph.bpmn.gateway2;gwType=exclusive;").solid.matches('L').count(), 11);
    assert_eq!(draw("shape=mxgraph.bpmn.gateway2;gwType=parallel;").solid.matches('L').count(), 11);
    assert_eq!(draw("shape=mxgraph.bpmn.gateway2;outline=end;").solid.matches('C').count(), 8);
    let bracket = draw("shape=curlyBracket;rounded=1;");
    assert_eq!(bracket.path.matches('Q').count(), 4);
    assert!(!bracket.filled && !bracket.path.contains('Z'));
}

#[test]
fn draws_lifeline_heads_and_separate_dashes() {
    let r = Rect { x: 10.0, y: 20.0, width: 100.0, height: 80.0 };
    let dashes = "M60 40L60 46M60 50L60 56M60 60L60 66M60 70L60 76M60 80L60 86M60 90L60 96";
    let plain = outline(&Style::parse("shape=umlLifeline;size=20;"), r);
    assert_eq!(plain.path, "M10 20L110 20L110 40L10 40Z");
    assert_eq!(plain.inner, dashes);
    for participant in ["umlActor", "umlBoundary", "umlEntity", "umlControl"] {
        let style = Style::parse(&format!("shape=umlLifeline;size=20;participant={participant};"));
        assert_eq!(kind(&style), "umlLifeline");
        let drawn = outline(&style, r);
        let head = outline(&Style::parse(&format!("shape={participant};")), Rect { height: 20.0, ..r });
        assert_eq!(drawn.path, head.path, "{participant}");
        assert_eq!(drawn.inner, format!("{}{dashes}", head.inner), "{participant}");
    }
    let solid = outline(&Style::parse("shape=umlLifeline;size=20;lifelineDashed=0;"), r);
    assert_eq!(solid.inner, "M60 40L60 100");
    let full_head = outline(&Style::parse("shape=umlLifeline;size=200;"), r);
    assert!(full_head.inner.is_empty());
}

#[test]
fn recognises_drawio_bpmn_variants() {
    for (text, expected) in [
        ("shape=mxgraph.bpmn.task;", "bpmnTask"),
        ("shape=mxgraph.bpmn.shape;outline=standard;", "ellipse"),
        ("shape=mxgraph.bpmn.event;outline=catching;", "doubleEllipse"),
        ("shape=mxgraph.bpmn.shape;outline=throwing;", "doubleEllipse"),
        ("shape=mxgraph.bpmn.shape;outline=end;", "bpmnEnd"),
        ("shape=mxgraph.bpmn.shape;background=gateway;outline=none;symbol=exclusiveGw;", "exclusiveGateway"),
        ("shape=mxgraph.bpmn.shape;background=gateway;outline=none;symbol=parallelGw;", "parallelGateway"),
        ("shape=mxgraph.bpmn.shape;background=gateway;outline=end;symbol=general;", "inclusiveGateway"),
        ("shape=mxgraph.bpmn.gateway2;gwType=complex;", "rhombus"),
        ("shape=mxgraph.bpmn.data;", "note"),
        ("shape=mxgraph.bpmn.data2;", "note"),
        ("shape=mxgraph.bpmn.swimlane;horizontal=0;", "swimlane"),
    ] {
        assert_eq!(kind(&Style::parse(text)), expected, "{text}");
    }
}

const EXTRA_PALETTE: &[(&str, &str)] = &[
    ("tape", "tape"),
    ("cube", "cube"),
    ("curly-bracket", "curlyBracket"),
    ("data-storage", "dataStorage"),
    ("internal-storage", "internalStorage"),
    ("or", "or"),
    ("xor", "xor"),
    ("sum-ellipse", "sumEllipse"),
    ("or-ellipse", "orEllipse"),
    ("star", "star"),
    ("plus", "plus"),
    ("partial-rectangle", "partialRectangle"),
    ("parallel-marker", "parallelMarker"),
    ("waypoint", "waypoint"),
    ("octagon", "octagon"),
    ("pentagon", "pentagon"),
    ("lifeline", "umlLifeline"),
    ("actor-lifeline", "umlLifeline"),
    ("boundary-lifeline", "umlLifeline"),
    ("entity-lifeline", "umlLifeline"),
    ("control-lifeline", "umlLifeline"),
    ("uml-frame", "umlFrame"),
    ("uml-boundary", "umlBoundary"),
    ("uml-entity", "umlEntity"),
    ("uml-control", "umlControl"),
    ("provided-required-interface", "providedRequiredInterface"),
    ("required-interface", "requiredInterface"),
    ("provided-interface", "lollipop"),
    ("uml-state", "umlState"),
    ("start-state", "ellipse"),
    ("end-state", "endState"),
    ("fork-join", "line"),
    ("uml-destroy", "umlDestroy"),
    ("bpmn-task", "bpmnTask"),
    ("bpmn-subprocess", "bpmnTask"),
    ("bpmn-start", "ellipse"),
    ("bpmn-intermediate", "doubleEllipse"),
    ("bpmn-end", "bpmnEnd"),
    ("bpmn-exclusive", "exclusiveGateway"),
    ("bpmn-parallel", "parallelGateway"),
    ("bpmn-inclusive", "inclusiveGateway"),
    ("bpmn-data", "note"),
    ("bpmn-pool", "swimlane"),
    ("bpmn-lane", "swimlane"),
];

#[test]
fn offers_and_exports_additional_shapes() {
    let mut page = Page::blank("p", "Shapes");
    for (index, &(id, expected)) in EXTRA_PALETTE.iter().enumerate() {
        let entry = PALETTE.iter().find(|p| p.id == id).unwrap();
        assert_eq!(kind(&Style::parse(entry.style)), expected, "{id}");
        assert!(entry.width > 0.0 && entry.height > 0.0, "{id}");
        if id.starts_with("bpmn-") {
            assert_eq!(entry.group, "BPMN");
        }
        edit_page(&mut page, &DiagramEdit::AddShape { shape: id.into(), x: index as f64 * 600.0, y: 20.0 })
            .unwrap();
    }
    // Files keep draw.io's names and parameters, not the native kind names.
    let diagram = Diagram { pages: vec![page.clone()], attrs: vec![] };
    assert_eq!(Diagram::parse(&diagram.to_xml()), Some(diagram));
    let drawn = scene(&page);
    assert_eq!(drawn.items.len(), EXTRA_PALETTE.len());
    let svg = scene_svg(&drawn, None);
    for item in &drawn.items {
        assert!(!item.path.is_empty() || !item.arrows.is_empty(), "{}", item.id);
        for path in [&item.path, &item.inner, &item.arrows].into_iter().filter(|p| !p.is_empty()) {
            assert!(svg.contains(&format!("<path d=\"{path}\"")), "{}: {path}", item.id);
        }
    }
    let find = |id: &str| &drawn.items[EXTRA_PALETTE.iter().position(|(name, _)| *name == id).unwrap()];
    assert_eq!(find("bpmn-start").stroke_width, 1.0);
    assert_eq!(find("bpmn-end").stroke_width, 3.0);
    assert_eq!(find("bpmn-intermediate").inner.matches('C').count(), 4);
    assert_eq!(find("fork-join").stroke_width, 8.0);
    assert_eq!(find("start-state").fill, "#000000");
    assert!(find("end-state").fill.is_empty() && !find("end-state").arrows.is_empty());
    assert_eq!(find("lifeline").text_height, 36.0);
    assert_eq!((find("uml-frame").text_width, find("uml-frame").text_height), (76.0, 26.0));
    assert!(find("bpmn-pool").container && find("bpmn-lane").container);
}

/// Every palette shape on one page, named, as SVG; written where
/// `DIAGRAM_GALLERY` says (CI keeps it for a person to look at).
#[test]
fn draws_a_gallery_of_the_palette() {
    let mut page = Page::blank("p", "Gallery");
    for (index, entry) in PALETTE.iter().enumerate() {
        let (column, row) = ((index % 8) as f64, (index / 8) as f64);
        let before = page.cells.len();
        edit_page(
            &mut page,
            &DiagramEdit::AddShape { shape: entry.id.into(), x: column * 220.0, y: row * 200.0 },
        )
        .unwrap();
        if let Some(cell) = page.cells.get_mut(before)
            && cell.value.is_empty()
        {
            cell.value = entry.name.into();
        }
    }
    let svg = scene_svg(&scene(&page), None);
    assert!(svg.contains("<svg"));
    if let Ok(path) = std::env::var("DIAGRAM_GALLERY") {
        let path = std::path::Path::new(&path);
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder).unwrap();
        }
        std::fs::write(path, svg).unwrap();
    }
}

#[test]
fn exports_each_additional_kind_as_native_paths() {
    let mut page = Page::blank("p", "Kinds");
    for (i, &(text, _, _, _)) in EXTRA_SHAPES.iter().enumerate() {
        page.cells.push(Cell {
            id: format!("shape-{i}"),
            parent: "1".into(),
            vertex: true,
            style: Style::parse(text),
            geometry: Some(Geometry {
                x: i as f64 * 150.0,
                y: 0.0,
                width: 100.0,
                height: 80.0,
                ..Geometry::default()
            }),
            ..Cell::default()
        });
    }
    let drawn = scene(&page);
    let svg = scene_svg(&drawn, None);
    assert_eq!(drawn.items.len(), EXTRA_SHAPES.len());
    for item in &drawn.items {
        let path = if item.path.is_empty() { &item.arrows } else { &item.path };
        assert!(!path.is_empty());
        assert!(svg.contains(&format!("<path d=\"{path}\"")), "{}", item.id);
    }
    assert!(!svg.contains("<image"));
}

#[test]
fn draws_crows_foot_markers_at_edge_ends() {
    for (name, prefix, lines, circle) in [
        ("ERone", "M94 56L94 44", 1, false),
        ("ERmandOne", "M94 56L94 44M88 56L88 44", 2, false),
        ("ERmany", "M100 44L88 50M88 50L100 56", 2, false),
        ("ERoneToMany", "M88 56L88 44M100 44L88 50M88 50L100 56", 3, false),
        ("ERzeroToMany", "M100 44L88 50M88 50L100 56M88 50L100 50", 3, true),
        ("ERzeroToOne", "M94 56L94 44M87.5 50L100 50", 2, true),
    ] {
        let (path, filled) = arrowhead(name, true, 10.0, (100.0, 50.0), (0.0, 50.0), 1.0).unwrap();
        assert!(!filled, "{name}");
        assert!(path.starts_with(prefix), "{name}: {path}");
        assert_eq!(path.matches('L').count(), lines, "{name}");
        assert_eq!(path.matches('C').count(), if circle { 4 } else { 0 }, "{name}");
        if circle {
            assert!(path.contains("M87 50C"), "{name}: {path}");
        }
        for (tip, from) in
            [((0.0, 50.0), (100.0, 50.0)), ((50.0, 100.0), (50.0, 0.0)), ((100.0, 100.0), (0.0, 0.0))]
        {
            let (path, filled) = arrowhead(name, false, 10.0, tip, from, 1.0).unwrap();
            assert!(!filled && !path.contains("NaN"), "{name}: {path}");
        }
        let mut page = Page::blank("p", "ER");
        page.cells.push(Cell {
            id: "edge".into(),
            parent: "1".into(),
            edge: true,
            style: Style::parse(&format!("endArrow={name};startArrow={name};startSize=10;endSize=10;")),
            geometry: Some(Geometry {
                source_point: Some((0.0, 50.0)),
                target_point: Some((100.0, 50.0)),
                ..Geometry::default()
            }),
            ..Cell::default()
        });
        let drawn = scene(&page);
        let edge = &drawn.items[0];
        assert_eq!(edge.points, [(0.0, 50.0), (100.0, 50.0)], "{name}");
        assert!(edge.arrows.is_empty() && !edge.open_arrows.is_empty(), "{name}");
        assert_eq!(edge.path, if circle { "M23.5 50L76.5 50" } else { "M0 50L100 50" }, "{name}");
        assert_eq!(marker_inset(name, 10.0, 1.0), if circle { 23.5 } else { 0.0 });
        let svg = scene_svg(&drawn, None);
        assert!(svg.contains(&format!("d=\"{}\" fill=\"none\"", edge.open_arrows)), "{name}");
        assert!(arrowhead(name, false, 10.0, (0.0, 0.0), (0.0, 0.0), 1.0).is_none());
    }
    assert_eq!(trim_line(&[(0.0, 0.0), (10.0, 0.0), (10.0, 30.0)], 15.0, 5.0), [(10.0, 5.0), (10.0, 25.0)]);
    assert_eq!(trim_line(&[(0.0, 0.0), (10.0, 0.0)], 20.0, 20.0), [(5.0, 0.0)]);
}

#[test]
fn routes_edges() {
    let plain = Style::parse("");
    let a = Rect { x: 0.0, y: 0.0, width: 100.0, height: 50.0 };
    let b = Rect { x: 200.0, y: 100.0, width: 100.0, height: 50.0 };
    let (from, to) = (End::Shape(a, &plain), End::Shape(b, &plain));
    assert_eq!(route(&plain, &from, &to, &[]), [(100.0, 50.0), (200.0, 100.0)]);
    let orthogonal = Style::parse("edgeStyle=orthogonalEdgeStyle;");
    assert_eq!(
        route(&orthogonal, &from, &to, &[]),
        [(100.0, 25.0), (150.0, 25.0), (150.0, 125.0), (200.0, 125.0)]
    );
    // Fixed ends with the target behind: out, round, and in.
    let fixed = Style::parse("edgeStyle=orthogonalEdgeStyle;exitX=1;exitY=0.5;entryX=0;entryY=0.5;");
    let behind = Rect { x: -200.0, y: 100.0, width: 100.0, height: 50.0 };
    assert_eq!(
        route(&fixed, &from, &End::Shape(behind, &plain), &[]),
        [(100.0, 25.0), (120.0, 25.0), (120.0, 75.0), (-220.0, 75.0), (-220.0, 125.0), (-200.0, 125.0)]
    );
    assert_eq!(
        route(&orthogonal, &from, &to, &[(50.0, 125.0)]),
        [(50.0, 50.0), (50.0, 125.0), (200.0, 125.0)]
    );
    assert_eq!(
        route(&plain, &End::Point(0.0, 0.0), &End::Point(10.0, 0.0), &[(5.0, 0.0)]),
        [(0.0, 0.0), (10.0, 0.0)]
    );
}

#[test]
fn lays_out_a_page() {
    let diagram = Diagram::parse(PLAIN).unwrap();
    let scene = scene(&diagram.pages[0]);
    let ids: Vec<&str> = scene.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["a", "b", "e"]);
    let start = &scene.items[0];
    assert!(start.path.starts_with("M49 40"), "{}", start.path);
    assert_eq!(
        (start.fill.as_str(), start.stroke.as_str(), start.text.as_str()),
        ("#ffffff", "#000000", "Start & go")
    );
    assert_eq!(scene.items[1].path, "M280 30L320 70L280 110L240 70Z");
    let edge = &scene.items[2];
    assert!(edge.edge);
    assert_eq!(edge.points, [(160.0, 70.0), (240.0, 70.0)]);
    assert_eq!(edge.path, "M160 70L240 70");
    assert!(edge.arrows.starts_with("M240 70"), "{}", edge.arrows);
    assert_eq!(edge.text, "yes");
    assert!(close(
        (edge.text_x + edge.text_width / 2.0, edge.text_y + edge.text_height / 2.0),
        (200.0, 70.0)
    ));
    assert_eq!(scene.grid, 10.0);
    assert!(scene.x <= 40.0 && scene.x + scene.width >= 320.0);
}

#[test]
fn places_what_groups_hold_and_hides_what_is_hidden() {
    let xml = r#"<mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/><mxCell id="L" parent="0" visible="0"/>
<mxCell id="g" style="group;" vertex="1" parent="1"><mxGeometry x="100" y="100" width="200" height="100" as="geometry"/></mxCell>
<mxCell id="c" value="in" vertex="1" parent="g"><mxGeometry x="10" y="20" width="50" height="30" as="geometry"/></mxCell>
<mxCell id="h" value="hidden" vertex="1" parent="L"><mxGeometry width="10" height="10" as="geometry"/></mxCell>
<mxCell id="e2" style="endArrow=none;" edge="1" parent="1" source="c"><mxGeometry relative="1" as="geometry"><mxPoint x="400" y="400" as="targetPoint"/></mxGeometry></mxCell>
<mxCell id="lbl" value="mid" style="edgeLabel;" vertex="1" connectable="0" parent="e2"><mxGeometry relative="1" as="geometry"><mxPoint y="-10" as="offset"/></mxGeometry></mxCell>
</root></mxGraphModel>"#;
    let diagram = Diagram::parse(xml).unwrap();
    let scene = scene(&diagram.pages[0]);
    let ids: Vec<&str> = scene.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["g", "c", "e2", "lbl"]);
    let (group, inside) = (&scene.items[0], &scene.items[1]);
    assert!(group.container && group.fill.is_empty() && group.stroke.is_empty());
    assert_eq!((inside.x, inside.y, inside.parent.as_str()), (110.0, 120.0, "g"));
    let edge = &scene.items[2];
    assert_eq!(edge.points, [(150.0, 150.0), (400.0, 400.0)]);
    assert!(edge.arrows.is_empty() && edge.open_arrows.is_empty());
    let label = &scene.items[3];
    assert!(close((label.x + label.width / 2.0, label.y + label.height / 2.0), (275.0, 265.0)));
    assert_eq!(label.text, "mid");
}

#[test]
fn reads_labels_as_text() {
    assert_eq!(label_text("<b>Bold</b> and<br>next<div>line</div>", true), "Bold and\nnext\nline");
    assert_eq!(label_text("x&nbsp;&lt;y", true), "x <y");
    assert_eq!(label_text("a &amp; b", false), "a &amp; b");
}

#[test]
fn draws_pictures_that_hold_their_diagram() {
    let picture = diagram_picture(PLAIN);
    assert!(picture.contains("<path d=\"M280 30L320 70L280 110L240 70Z\""), "{picture}");
    assert!(picture.contains(">Check</tspan>"));
    assert_eq!(diagram_xml(&picture).as_deref(), Some(PLAIN));
    assert_eq!(
        diagram_xml(&diagram_picture("<mxfile><diagram>junk</diagram></mxfile>")).unwrap(),
        "<mxfile><diagram>junk</diagram></mxfile>"
    );
}

#[test]
fn edits_pages() {
    let mut page = Page::blank("p", "P");
    let add = |page: &mut Page, shape: &str, x: f64, y: f64| {
        edit_page(page, &DiagramEdit::AddShape { shape: shape.into(), x, y }).unwrap().remove(0)
    };
    let a = add(&mut page, "rectangle", 10.0, 20.0);
    assert_eq!(a, "pl-1");
    let g = page.cell(&a).unwrap().geometry.clone().unwrap();
    assert_eq!((g.x, g.y, g.width, g.height), (10.0, 20.0, 120.0, 60.0));
    assert_eq!(page.cell(&a).unwrap().parent, "1");
    let class = add(&mut page, "class", 300.0, 20.0);
    let rows: Vec<(f64, &str)> = page
        .cells
        .iter()
        .filter(|c| c.parent == class)
        .map(|c| (c.geometry.as_ref().unwrap().y, c.value.as_str()))
        .collect();
    assert_eq!(rows, [(26.0, "+ field: type"), (52.0, "+ method(): type")]);
    assert!(
        edit_page(&mut page, &DiagramEdit::AddShape { shape: "nothing".into(), x: 0.0, y: 0.0 }).is_err()
    );

    let connect =
        DiagramEdit::Connect { source: a.clone(), target: class.clone(), from: (0.0, 0.0), to: (0.0, 0.0) };
    let edge = edit_page(&mut page, &connect).unwrap().remove(0);
    assert_eq!(
        (page.cell(&edge).unwrap().source.as_str(), page.cell(&edge).unwrap().target.as_str()),
        (a.as_str(), class.as_str())
    );
    let loose =
        DiagramEdit::Connect { source: String::new(), target: a.clone(), from: (0.0, 300.0), to: (0.0, 0.0) };
    let dangling = edit_page(&mut page, &loose).unwrap().remove(0);
    assert_eq!(page.cell(&dangling).unwrap().geometry.as_ref().unwrap().source_point, Some((0.0, 300.0)));

    edit_page(&mut page, &DiagramEdit::Move { ids: vec![a.clone()], dx: 5.0, dy: 5.0 }).unwrap();
    edit_page(&mut page, &DiagramEdit::SetText { id: a.clone(), text: "a<b\nc".into() }).unwrap();
    assert_eq!(page.cell(&a).unwrap().value, "a&lt;b<br>c");
    let resize = DiagramEdit::Resize { id: a.clone(), x: 15.0, y: 25.0, width: 200.0, height: 0.0 };
    edit_page(&mut page, &resize).unwrap();
    let g = page.cell(&a).unwrap().geometry.clone().unwrap();
    assert_eq!((g.x, g.y, g.width, g.height), (15.0, 25.0, 200.0, 1.0));
    edit_page(
        &mut page,
        &DiagramEdit::SetStyle { ids: vec![a.clone()], key: "fillColor".into(), value: "#ff0000".into() },
    )
    .unwrap();
    assert_eq!(page.cell(&a).unwrap().style.get("fillColor"), Some("#ff0000"));

    // Grouped and ungrouped, shapes stay where they were.
    let b = add(&mut page, "ellipse", 400.0, 300.0);
    let group =
        edit_page(&mut page, &DiagramEdit::Group { ids: vec![a.clone(), b.clone()] }).unwrap().remove(0);
    let inside = page.cell(&b).unwrap().geometry.clone().unwrap();
    assert_eq!((page.cell(&b).unwrap().parent.as_str(), inside.x, inside.y), (group.as_str(), 385.0, 275.0));
    let scene = scene(&page);
    let drawn = scene.items.iter().find(|i| i.id == b).unwrap();
    assert_eq!((drawn.x, drawn.y), (400.0, 300.0));
    let freed = edit_page(&mut page, &DiagramEdit::Ungroup { id: group.clone() }).unwrap();
    assert_eq!(freed, [a.clone(), b.clone()]);
    assert!(page.cell(&group).is_none());
    let g = page.cell(&b).unwrap().geometry.clone().unwrap();
    assert_eq!((g.x, g.y), (400.0, 300.0));

    edit_page(&mut page, &DiagramEdit::Align { ids: vec![a.clone(), b.clone()], how: "left".into() })
        .unwrap();
    assert_eq!(page.cell(&b).unwrap().geometry.as_ref().unwrap().x, 15.0);
    assert!(
        edit_page(&mut page, &DiagramEdit::Align { ids: vec![a.clone()], how: "sideways".into() }).is_err()
    );
    let c = add(&mut page, "circle", 0.0, 100.0);
    edit_page(
        &mut page,
        &DiagramEdit::Distribute { ids: vec![a.clone(), b.clone(), c.clone()], horizontal: false },
    )
    .unwrap();
    let middles: Vec<f64> = [&a, &c, &b]
        .iter()
        .map(|id| {
            let g = page.cell(id).unwrap().geometry.clone().unwrap();
            g.y + g.height / 2.0
        })
        .collect();
    assert!((middles[1] - (middles[0] + middles[2]) / 2.0).abs() < 1e-9, "{middles:?}");

    let copies = edit_page(
        &mut page,
        &DiagramEdit::Duplicate { ids: vec![class.clone(), edge.clone()], dx: 10.0, dy: 10.0 },
    )
    .unwrap();
    assert_eq!(copies.len(), 2);
    let copied_edge = page.cell(&copies[1]).unwrap();
    assert_eq!((copied_edge.source.as_str(), copied_edge.target.as_str()), (a.as_str(), copies[0].as_str()));
    assert_eq!(page.cells.iter().filter(|c| c.parent == copies[0]).count(), 2, "the rows come too");

    edit_page(&mut page, &DiagramEdit::ToBack { ids: vec![b.clone()] }).unwrap();
    assert_eq!(page.cells[2].id, b);
    edit_page(&mut page, &DiagramEdit::ToFront { ids: vec![b.clone()] }).unwrap();
    assert_eq!(page.cells.last().unwrap().id, b);

    edit_page(&mut page, &DiagramEdit::Delete { ids: vec![class.clone()] }).unwrap();
    assert!(page.cell(&class).is_none() && page.cell(&edge).is_none(), "its edges go with it");
    assert_eq!(page.cells.iter().filter(|c| c.parent == class).count(), 0, "and its rows");
    assert!(page.cell("0").is_some() && page.cell("1").is_some());
}

#[test]
fn places_pictures_as_drawio_does() {
    let mut page = Page::blank("p", "P");
    let picture = DiagramEdit::AddImage {
        image: "data:image/png;base64,iVBORw0KGgo=".into(),
        x: 10.0,
        y: 20.0,
        width: 40.0,
        height: 30.0,
    };
    let id = edit_page(&mut page, &picture).unwrap().remove(0);
    let style = page.cell(&id).unwrap().style.clone();
    assert_eq!(style.get("image"), Some("data:image/png,iVBORw0KGgo="), "kept as draw.io keeps it");
    assert_eq!(style.shape(), "image");
    let drawn = scene(&page);
    let item = drawn.items.iter().find(|i| i.id == id).unwrap();
    assert_eq!(item.image, "data:image/png;base64,iVBORw0KGgo=");
    assert!(!item.image_aspect && item.fill.is_empty() && item.stroke.is_empty(), "{item:?}");
    let svg = scene_svg(&drawn, None);
    assert!(
        svg.contains(
            "<image href=\"data:image/png;base64,iVBORw0KGgo=\" x=\"10\" y=\"20\" width=\"40\" height=\"30\" \
             preserveAspectRatio=\"none\"/>"
        ),
        "{svg}"
    );

    let web = DiagramEdit::AddImage {
        image: "https://x.org/a;b.png".into(),
        x: 0.0,
        y: 0.0,
        width: 8.0,
        height: 8.0,
    };
    let id = edit_page(&mut page, &web).unwrap().remove(0);
    assert_eq!(page.cell(&id).unwrap().style.get("image"), Some("https://x.org/a%3Bb.png"));
    let empty = DiagramEdit::AddImage { image: String::new(), x: 0.0, y: 0.0, width: 8.0, height: 8.0 };
    assert!(edit_page(&mut page, &empty).is_err());
}
