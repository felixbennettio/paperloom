use super::*;

/// A scene as Excalidraw writes it, with a picture and a deleted stroke.
const EXCALIDRAW: &str = r##"{
  "type": "excalidraw",
  "version": 2,
  "source": "https://excalidraw.com",
  "elements": [
    {"id": "img", "type": "image", "x": 0, "y": 0, "width": 100, "height": 80, "fileId": "f1", "isDeleted": false},
    {"id": "s1", "type": "freedraw", "x": 10, "y": 20, "width": 30, "height": 10, "angle": 0,
     "strokeColor": "#e03131", "backgroundColor": "transparent", "strokeWidth": 1, "opacity": 100,
     "points": [[0, 0], [10, 5], [30, 10]], "pressures": [0.2, 0.6, 0.9], "simulatePressure": false,
     "seed": 42, "version": 7, "versionNonce": 99, "groupIds": ["g1"], "customData": {"mine": 1}},
    {"id": "s2", "type": "freedraw", "x": 0, "y": 0, "points": [[0, 0], [5, 5]], "pressures": [],
     "simulatePressure": true, "strokeWidth": 2, "opacity": 40, "customData": {"paperloom": "highlighter"}},
    {"id": "gone", "type": "freedraw", "isDeleted": true, "points": [[0, 0]]},
    {"id": "r1", "type": "rectangle", "x": 50, "y": 60, "width": 120, "height": 40, "backgroundColor": "#ffec99"},
    {"id": "a1", "type": "arrow", "x": 0, "y": 100, "points": [[0, 0], [80, 0]], "endArrowhead": "arrow"},
    {"id": "t1", "type": "text", "x": 5, "y": 150, "width": 90, "height": 25, "text": "Hello\nworld",
     "fontSize": 16, "fontFamily": 5}
  ],
  "appState": {"viewBackgroundColor": "#ffffff", "gridSize": 20, "paperloomPaper": "grid"},
  "files": {"f1": {"mimeType": "image/png", "id": "f1", "dataURL": "data:image/png;base64,AAAA"}}
}"##;

fn parse_json(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

fn element<'a>(scene: &'a Value, id: &str) -> &'a Value {
    scene["elements"].as_array().unwrap().iter().find(|e| e["id"] == id).unwrap()
}

#[test]
fn reads_what_it_draws() {
    let drawing = Drawing::parse(EXCALIDRAW).unwrap();
    assert_eq!(drawing.paper, "grid");
    let kinds: Vec<&str> = drawing.elements.iter().map(|e| e.kind.as_str()).collect();
    assert_eq!(kinds, ["image", "freedraw", "freedraw", "rectangle", "arrow", "text"]);
    assert_eq!(drawing.elements[0].file_id, "f1");
    assert_eq!(drawing.files["f1"], "data:image/png;base64,AAAA");
    let stroke = &drawing.elements[1];
    assert_eq!((stroke.x, stroke.y, stroke.stroke_color.as_str()), (10.0, 20.0, "#e03131"));
    assert_eq!(stroke.points, [(0.0, 0.0), (10.0, 5.0), (30.0, 10.0)]);
    assert_eq!(stroke.pressures, [0.2, 0.6, 0.9]);
    assert!(!stroke.highlighter);
    let highlighter = &drawing.elements[2];
    assert!(highlighter.highlighter && highlighter.pressures.is_empty());
    assert_eq!(highlighter.opacity, 40.0);
    assert!(drawing.elements[4].arrowhead);
    assert_eq!(drawing.elements[5].text, "Hello\nworld");
    assert_eq!(drawing.elements[5].font_size, 16.0);
    assert!(Drawing::parse("{\"nodes\": []}").is_err());
}

#[test]
fn keeps_what_it_does_not_draw() {
    let mut drawing = Drawing::parse(EXCALIDRAW).unwrap();
    // The first stroke moves; the rest stay as they were.
    drawing.elements[1].x = 15.0;
    drawing.elements.remove(3);
    drawing.paper = "lines".into();
    let scene = parse_json(&drawing.to_json(Some(EXCALIDRAW)));
    assert_eq!(scene["source"], "https://excalidraw.com");
    assert_eq!(scene["files"]["f1"]["dataURL"], "data:image/png;base64,AAAA");
    assert_eq!(scene["appState"]["gridSize"], 20);
    assert_eq!(scene["appState"]["paperloomPaper"], "lines");
    let ids: Vec<&str> =
        scene["elements"].as_array().unwrap().iter().map(|e| e["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["img", "s1", "s2", "a1", "t1"]);
    let moved = element(&scene, "s1");
    assert_eq!((moved["x"].as_f64(), moved["version"].as_i64()), (Some(15.0), Some(8)));
    assert_eq!((moved["seed"].as_i64(), moved["groupIds"][0].as_str()), (Some(42), Some("g1")));
    assert_eq!(moved["customData"]["mine"], 1);
    assert_ne!(moved["versionNonce"], 99);
    // Unchanged, even where Excalidraw wrote whole numbers.
    let text = element(&scene, "t1");
    assert_eq!((text["version"].as_i64(), text["fontFamily"].as_i64()), (Some(1), Some(5)));
    assert_eq!(element(&scene, "s2")["customData"]["paperloom"], "highlighter");
    assert!(scene["elements"].as_array().unwrap().iter().all(|e| e["id"] != "gone" && e["id"] != "r1"));
}

#[test]
fn writes_new_drawings_excalidraw_can_open() {
    let drawing = Drawing {
        elements: vec![
            DrawElement {
                id: "p1".into(),
                kind: "freedraw".into(),
                x: 1.0,
                y: 2.0,
                stroke_color: "#1e1e1e".into(),
                background_color: "transparent".into(),
                stroke_width: 0.5,
                opacity: 100.0,
                points: vec![(0.0, 0.0), (4.0, 3.0)],
                pressures: vec![0.5, 0.7],
                font_size: 20.0,
                ..DrawElement::default()
            },
            DrawElement {
                id: "h1".into(),
                kind: "freedraw".into(),
                stroke_color: "#ffd43b".into(),
                background_color: "transparent".into(),
                stroke_width: 4.0,
                opacity: 40.0,
                points: vec![(0.0, 0.0), (40.0, 0.0)],
                highlighter: true,
                font_size: 20.0,
                ..DrawElement::default()
            },
            DrawElement {
                id: "x1".into(),
                kind: "text".into(),
                stroke_color: "#1e1e1e".into(),
                background_color: "transparent".into(),
                text: "Note & more".into(),
                font_size: 20.0,
                width: 120.0,
                height: 25.0,
                opacity: 100.0,
                ..DrawElement::default()
            },
        ],
        paper: String::new(),
        files: HashMap::new(),
    };
    let json = drawing.to_json(None);
    let scene = parse_json(&json);
    assert_eq!((scene["type"].as_str(), scene["version"].as_i64()), (Some("excalidraw"), Some(2)));
    let stroke = element(&scene, "p1");
    assert_eq!(stroke["simulatePressure"], false);
    assert_eq!((stroke["roughness"].as_i64(), stroke["version"].as_i64()), (Some(0), Some(1)));
    assert!(stroke["seed"].as_i64().is_some_and(|s| (0..0x7fff_ffff).contains(&s)));
    assert_eq!(element(&scene, "h1")["simulatePressure"], true);
    assert_eq!(element(&scene, "x1")["originalText"], "Note & more");
    let back = Drawing::parse(&json).unwrap();
    assert_eq!(back.paper, "blank");
    assert_eq!(back.elements, drawing.elements);
    // Saving the same drawing again changes nothing.
    assert_eq!(parse_json(&back.to_json(Some(&json)))["elements"], scene["elements"]);
}

#[test]
fn keeps_drawings_in_the_vault() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = Vault::open(dir.path()).unwrap();
    let path = vault.create_drawing("Sketches", "Setup", "dots").unwrap();
    assert_eq!(path, "Sketches/Setup.excalidraw");
    assert_eq!(
        vault.create_drawing("Sketches", "Setup.excalidraw", "").unwrap(),
        "Sketches/Setup 2.excalidraw"
    );
    assert_eq!(
        vault.drawings().collect::<Vec<_>>(),
        ["Sketches/Setup 2.excalidraw", "Sketches/Setup.excalidraw"]
    );
    let mut drawing = vault.read_drawing("Sketches/Setup").unwrap();
    assert_eq!(drawing.paper, "dots");
    drawing.elements.push(DrawElement {
        id: "l1".into(),
        kind: "line".into(),
        points: vec![(0.0, 0.0), (10.0, 10.0)],
        stroke_width: 2.0,
        opacity: 100.0,
        ..DrawElement::default()
    });
    vault.write_drawing(&path, &drawing).unwrap();
    assert_eq!(vault.read_drawing(&path).unwrap().elements.len(), 1);
    assert!(matches!(vault.read_drawing("Missing"), Err(Error::NotFound(_))));
    assert!(vault.read_drawing("../outside").is_err());
}

#[test]
fn draws_an_svg() {
    let drawing = Drawing::parse(EXCALIDRAW).unwrap();
    let svg = drawing_svg(&drawing);
    assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""), "{svg}");
    // The highlighter goes first, under the pen stroke.
    let highlighter = svg.find("stroke=\"#1e1e1e\"").unwrap();
    assert!(highlighter < svg.find("stroke=\"#e03131\"").unwrap(), "{svg}");
    assert!(svg.contains("<rect x=\"50.0\" y=\"60.0\" width=\"120.0\" height=\"40.0\" fill=\"#ffec99\""));
    assert!(svg.contains(">Hello</tspan>") && svg.contains(">world</tspan>"));
    assert_eq!(svg.matches("<g ").count(), 6);
    assert!(
        svg.contains("<image href=\"data:image/png;base64,AAAA\" x=\"0.0\" y=\"0.0\" width=\"100.0\""),
        "{svg}"
    );
    assert!(drawing_svg(&Drawing::default()).contains("viewBox=\"-16.0 -16.0 352.0 192.0\""));
}

#[test]
fn widens_strokes_with_pressure() {
    assert_eq!(stroke_diameter(1.0, 0.5), 4.25);
    assert!(stroke_diameter(1.0, 0.0) < stroke_diameter(1.0, 1.0));
    assert_eq!(stroke_diameter(2.0, 7.0), stroke_diameter(2.0, 1.0));
}

#[test]
fn shows_in_notes_as_pictures() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = Vault::open(dir.path()).unwrap();
    let path = vault.create_drawing("Sketches", "Setup", "").unwrap();
    let mut drawing = vault.read_drawing(&path).unwrap();
    drawing.elements.push(DrawElement {
        id: "r".into(),
        kind: "rectangle".into(),
        width: 40.0,
        height: 20.0,
        stroke_color: "#e03131".into(),
        background_color: "transparent".into(),
        stroke_width: 2.0,
        opacity: 100.0,
        ..DrawElement::default()
    });
    vault.write_drawing(&path, &drawing).unwrap();
    let text = "Before\n\n![[Setup.excalidraw]]\n\nAfter";
    vault.write("Index.md", text).unwrap();
    let html = vault.render("Index.md", text);
    assert!(
        html.contains(concat!(
            r#"<a class="embed-drawing" href="paperloom:drawing/Sketches/Setup.excalidraw" "#,
            r#"data-drawing="Sketches/Setup.excalidraw"><svg"#
        )),
        "{html}"
    );
    assert!(html.contains("stroke=\"#e03131\"") && html.contains("<p>After</p>"), "{html}");
    // Without `!` it stays a link.
    assert!(vault.render("Index.md", "[[Setup.excalidraw]]").contains("class=\"wikilink\""));
}

#[test]
fn keeps_new_pictures_in_its_files() {
    let url = "data:image/jpeg;base64,/9j/AAAA";
    let id = picture_id(url);
    assert_eq!(id, picture_id(url), "the same picture, the same ID");
    let drawing = Drawing {
        elements: vec![DrawElement {
            id: "i1".into(),
            kind: "image".into(),
            width: 200.0,
            height: 100.0,
            opacity: 100.0,
            file_id: id.clone(),
            ..DrawElement::default()
        }],
        paper: String::new(),
        files: HashMap::from([(id.clone(), url.to_string())]),
    };
    let scene = parse_json(&drawing.to_json(None));
    assert_eq!(scene["files"][&id]["mimeType"], "image/jpeg");
    assert_eq!(element(&scene, "i1")["fileId"], id.as_str());
    assert_eq!(Drawing::parse(&drawing.to_json(None)).unwrap().files[&id], url);
}
