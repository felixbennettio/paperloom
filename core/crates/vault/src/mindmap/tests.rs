use super::*;

const NOTE: &str = "---\nmindmap: logic\n---\n# Thermometry\nWhy it matters.\n\n- Methods\n  - NV centres\n  - Fluorescence\n    Note under fluorescence.\n- Results\n## Open questions\n- Calibration\n";

fn topic(text: &str, children: Vec<Topic>) -> Topic {
    Topic { text: text.into(), children, ..Topic::default() }
}

fn texts(topic: &Topic) -> Vec<&str> {
    topic.children.iter().map(|c| c.text.as_str()).collect()
}

#[test]
fn reads_headings_and_lists_as_branches() {
    let map = read_map(NOTE, "File name");
    assert_eq!((map.text.as_str(), map.note.as_str()), ("Thermometry", "Why it matters."));
    assert_eq!(texts(&map), ["Methods", "Results", "Open questions"]);
    assert_eq!(texts(&map.children[0]), ["NV centres", "Fluorescence"]);
    assert_eq!(map.children[0].children[1].note, "Note under fluorescence.");
    assert_eq!(texts(&map.children[2]), ["Calibration"]);

    let plain = read_map("1. one\n2. [ ] two\n   * deeper\n", "Title");
    assert_eq!(plain.text, "Title");
    assert_eq!(texts(&plain), ["one", "two"]);
    assert_eq!(texts(&plain.children[1]), ["deeper"]);
    assert_eq!(read_map("", "Empty"), topic("Empty", vec![]));
}

#[test]
fn writes_the_outline_back_and_keeps_the_rest() {
    let map = read_map(NOTE, "File name");
    let written = write_map(NOTE, &map);
    assert_eq!(
        written,
        "---\nmindmap: logic\n---\n# Thermometry\n\nWhy it matters.\n\n- Methods\n  - NV centres\n  - Fluorescence\n    Note under fluorescence.\n- Results\n- Open questions\n  - Calibration\n"
    );
    assert_eq!(read_map(&written, "File name"), map);
    assert_eq!(
        write_map(&written, &read_map(&written, "File name")),
        written,
        "a second round changes nothing"
    );
    let code = "# Code\n- Topic\n  ```rust\n      let x = 1;\n  ```\n";
    let once = write_map(code, &read_map(code, "Code"));
    assert_eq!(write_map(&once, &read_map(&once, "Code")), once, "code under a topic keeps its indent");
    assert_eq!(new_map(" Ideas ", MapStyle::Org), "---\nmindmap: org\n---\n# Ideas\n");
}

#[test]
fn edits_by_path() {
    let mut map = topic(
        "Centre",
        vec![
            topic("A", vec![topic("a1", vec![]), topic("a2", vec![])]),
            topic("B", vec![]),
            topic("C", vec![]),
        ],
    );
    assert_eq!(
        edit_map(&mut map, &MapEdit::AddChild { id: "0.0".into(), text: " a3 ".into() }).unwrap(),
        "0.0.2"
    );
    assert_eq!(texts(&map.children[0]), ["a1", "a2", "a3"]);
    assert_eq!(
        edit_map(&mut map, &MapEdit::AddSibling { id: "0.1".into(), text: "B2".into() }).unwrap(),
        "0.2"
    );
    assert_eq!(texts(&map), ["A", "B", "B2", "C"]);
    assert_eq!(
        edit_map(&mut map, &MapEdit::Rename { id: "0.2".into(), text: "Bee\ntwo".into() }).unwrap(),
        "0.2"
    );
    assert_eq!(map.children[2].text, "Bee two");
    assert_eq!(edit_map(&mut map, &MapEdit::Delete { id: "0.2".into() }).unwrap(), "0.2");
    assert_eq!(texts(&map), ["A", "B", "C"]);

    // Across parents, and the new parent's path shifting as the old place empties.
    assert_eq!(
        edit_map(&mut map, &MapEdit::Move { id: "0.0.0".into(), parent: "0.2".into(), index: 0 }).unwrap(),
        "0.2.0"
    );
    assert_eq!(texts(&map.children[2]), ["a1"]);
    assert_eq!(
        edit_map(&mut map, &MapEdit::Move { id: "0.0".into(), parent: "0.2".into(), index: 9 }).unwrap(),
        "0.1.1"
    );
    assert_eq!(texts(&map), ["B", "C"]);
    assert_eq!(texts(&map.children[1]), ["a1", "A"]);
    // Within one parent.
    assert_eq!(
        edit_map(&mut map, &MapEdit::Move { id: "0.0".into(), parent: "0".into(), index: 2 }).unwrap(),
        "0.1"
    );
    assert_eq!(texts(&map), ["C", "B"]);

    for wrong in [
        MapEdit::Delete { id: "0".into() },
        MapEdit::AddSibling { id: "0".into(), text: "x".into() },
        MapEdit::Move { id: "0.0".into(), parent: "0.0.1".into(), index: 0 },
        MapEdit::Rename { id: "0.7".into(), text: "x".into() },
        MapEdit::Rename { id: "1.0".into(), text: "x".into() },
    ] {
        assert!(edit_map(&mut map, &wrong).is_err(), "{wrong:?}");
    }
}

#[test]
fn links_topics_and_keeps_links_through_new_text() {
    let mut map = topic("Centre", vec![topic("A", vec![]), topic("B", vec![])]);
    edit_map(&mut map, &MapEdit::SetLink { id: "0.0".into(), link: "https://example.com".into() }).unwrap();
    assert_eq!(map.children[0].text, "[A](https://example.com)");
    edit_map(&mut map, &MapEdit::Rename { id: "0.0".into(), text: "Ay".into() }).unwrap();
    assert_eq!(map.children[0].text, "[Ay](https://example.com)", "new text keeps the link");
    edit_map(&mut map, &MapEdit::Rename { id: "0.0".into(), text: "See [[Methods]]".into() }).unwrap();
    assert_eq!(map.children[0].text, "See [[Methods]]", "unless it brings its own");
    edit_map(&mut map, &MapEdit::ToggleMarker { id: "0.1".into(), marker: "star-yellow".into() }).unwrap();
    edit_map(&mut map, &MapEdit::SetLink { id: "0.1".into(), link: "Methods".into() }).unwrap();
    assert_eq!(map.children[1].text, "⭐ [[Methods|B]]");
    edit_map(&mut map, &MapEdit::SetLink { id: "0.1".into(), link: String::new() }).unwrap();
    assert_eq!(map.children[1].text, "⭐ B");

    // The note keeps the link as Markdown, and reads it back.
    edit_map(&mut map, &MapEdit::SetLink { id: "0.1".into(), link: "Methods".into() }).unwrap();
    let text = write_map("---\nmindmap: logic\n---\n", &map);
    assert!(text.contains("- ⭐ [[Methods|B]]\n"), "{text}");
    assert_eq!(read_map(&text, "x"), map);

    let layout = layout_map(&map, &[], MapStyle::Logic, &[]);
    let b = layout.topics.iter().find(|t| t.id == "0.1").unwrap();
    assert_eq!((b.text.as_str(), b.link.as_str()), ("⭐ B", "Methods"), "shown as its text");
    let plain = layout_map(&topic("Centre", vec![topic("⭐ B", vec![])]), &[], MapStyle::Logic, &[]);
    assert!(b.width > plain.topics[1].width, "room for the link's mark");
}

#[test]
fn labels_topics_with_tags() {
    let mut map = topic("Centre", vec![topic("[[Methods|A]] #draft", vec![])]);
    edit_map(&mut map, &MapEdit::Rename { id: "0.0".into(), text: "Ay #urgent".into() }).unwrap();
    assert_eq!(map.children[0].text, "[[Methods|Ay]] #draft #urgent", "new text keeps the link and labels");
    edit_map(&mut map, &MapEdit::SetLabels { id: "0.0".into(), labels: vec!["To read".into()] }).unwrap();
    assert_eq!(map.children[0].text, "[[Methods|Ay]] #To-read");
    let layout = layout_map(&map, &[], MapStyle::Logic, &[]);
    let a = &layout.topics[1];
    assert_eq!((a.text.as_str(), a.labels.clone()), ("Ay", vec!["To-read".to_string()]));
    let plain =
        layout_map(&topic("Centre", vec![topic("[[Methods|Ay]]", vec![])]), &[], MapStyle::Logic, &[]);
    assert!(a.height > plain.topics[1].height, "a row for the labels");
    edit_map(&mut map, &MapEdit::SetLabels { id: "0.0".into(), labels: vec![] }).unwrap();
    assert_eq!(map.children[0].text, "[[Methods|Ay]]");
}

#[test]
fn lays_maps_out() {
    let map = topic(
        "Centre",
        vec![
            topic("North", vec![topic("n1", vec![]), topic("n2", vec![])]),
            topic("East", vec![]),
            topic("South", vec![]),
            topic("West", vec![]),
        ],
    );
    let layout = layout_map(&map, &[], MapStyle::Logic, &[]);
    assert_eq!(layout.topics.len(), 7);
    let find = |id: &str| layout.topics.iter().find(|t| t.id == id).unwrap();
    let centre = find("0");
    assert_eq!((centre.side, centre.font_size, centre.children), (0, 20.0, 4));
    let right = layout.topics.iter().filter(|t| t.side == 1).count();
    let left = layout.topics.iter().filter(|t| t.side == -1).count();
    assert!(right >= 2 && left >= 2, "both sides used: {right} right, {left} left");
    for t in &layout.topics {
        assert!(t.x >= 40.0 - 1e-9 && t.y >= 40.0 - 1e-9, "{t:?}");
        assert!(t.x + t.width <= layout.width - 40.0 + 1e-9 && t.y + t.height <= layout.height - 40.0 + 1e-9);
        match t.side {
            1 => assert!(t.x >= centre.x + centre.width, "{t:?}"),
            -1 => assert!(t.x + t.width <= centre.x, "{t:?}"),
            _ => {}
        }
    }
    let n1 = find("0.0.0");
    assert_eq!((n1.parent.as_deref(), n1.depth), (Some("0.0"), 2));
    assert!(n1.width > 0.0 && n1.height > 0.0);

    let right = layout_map(&map, &[], MapStyle::Right, &[]);
    assert!(right.topics.iter().skip(1).all(|t| t.side == 1));
    let org = layout_map(&map, &[], MapStyle::Org, &[]);
    let top = org.topics.iter().find(|t| t.id == "0").unwrap();
    assert!(org.topics.iter().skip(1).all(|t| t.y >= top.y + top.height));

    let folded = layout_map(&map, &[], MapStyle::Logic, &["0.0".to_string(), "0.1".to_string()]);
    assert_eq!(folded.topics.len(), 5, "what is under North is folded away");
    let north = folded.topics.iter().find(|t| t.id == "0.0").unwrap();
    assert!(
        north.folded && !folded.topics.iter().find(|t| t.id == "0.1").unwrap().folded,
        "East has nothing to fold"
    );

    let wide = measure("中文主题", 1);
    let narrow = measure("abcd", 1);
    assert!(wide.0 > narrow.0, "wide characters take more room");
    assert!(measure(&"word ".repeat(40), 2).1 > measure("word", 2).1, "long text wraps");
    assert_eq!(MapStyle::parse("ORG"), MapStyle::Org);
    assert_eq!(MapStyle::parse("anything").as_str(), "logic");
}

#[test]
fn lays_out_trees_and_timelines_with_their_lines() {
    let map = topic(
        "Centre",
        vec![
            topic("A", vec![topic("a1", vec![]), topic("a2", vec![])]),
            topic("B", vec![topic("b1", vec![])]),
            topic("C", vec![]),
        ],
    );
    let find = |layout: &Layout, id: &str| layout.topics.iter().find(|t| t.id == id).unwrap().clone();

    let tree = layout_map(&map, &[], MapStyle::Tree, &[]);
    let (centre, a, a1, a2, b) = (
        find(&tree, "0"),
        find(&tree, "0.0"),
        find(&tree, "0.0.0"),
        find(&tree, "0.0.1"),
        find(&tree, "0.1"),
    );
    // Each topic below the one before it, set in under its parent.
    assert!(a.y > centre.y + centre.height && a1.y > a.y + a.height && a2.y > a1.y && b.y > a2.y);
    assert!(
        (a.x - centre.x - 28.0).abs() < 1e-9 && (a1.x - a.x - 28.0).abs() < 1e-9 && (b.x - a.x).abs() < 1e-9
    );
    assert!(centre.edge.is_empty());
    assert!(a1.edge.starts_with('M') && a1.edge.contains('L') && !a1.edge.contains('C'), "{}", a1.edge);

    let line = layout_map(&map, &[], MapStyle::Timeline, &[]);
    let (centre, a, a1, b, b1, c) = (
        find(&line, "0"),
        find(&line, "0.0"),
        find(&line, "0.0.0"),
        find(&line, "0.1"),
        find(&line, "0.1.0"),
        find(&line, "0.2"),
    );
    let axis = |t: &Placed| t.y + t.height / 2.0;
    // The main topics along the line, left to right.
    assert!((axis(&a) - axis(&centre)).abs() < 1e-9 && (axis(&c) - axis(&centre)).abs() < 1e-9);
    assert!(a.x > centre.x + centre.width && b.x > a.x + a.width && c.x > b.x + b.width);
    // What is under them above the line, then below it.
    assert!(a1.y + a1.height < a.y && b1.y > b.y + b.height);
    assert!(b.edge.starts_with(&format!("M{:.1}", a.x + a.width)), "{}", b.edge);
    for t in &line.topics {
        assert!(
            t.x >= 40.0 - 1e-9 && t.y >= 40.0 - 1e-9 && t.y + t.height <= line.height - 40.0 + 1e-9,
            "{t:?}"
        );
    }

    let logic = layout_map(&map, &[], MapStyle::Logic, &[]);
    assert!(logic.topics.iter().skip(1).all(|t| t.edge.contains('C')));
    assert_eq!(MapStyle::parse("Tree"), MapStyle::Tree);
    assert_eq!(MapStyle::parse("timeline").as_str(), "timeline");
}

#[test]
fn keeps_relations_summaries_and_boundaries() {
    let note = "---\nmindmap: logic\n---\n# Centre\n\n- A ^a\n  - a1\n- B\n- C\n";
    let mut map = read_map(note, "x");
    assert_eq!((map.children[0].text.as_str(), map.children[0].anchor.as_str()), ("A", "a"));
    let mut extras = read_extras(note);
    assert!(extras.is_empty());
    let mut edit = |edit: ExtraEdit| edit_extras(&mut map, &mut extras, &edit);
    edit(ExtraEdit::Relate { from: "0.0".into(), to: "0.2".into(), text: " leads   to ".into() }).unwrap();
    edit(ExtraEdit::Summarize { first: "0.2".into(), last: "0.1".into(), text: "Later".into() }).unwrap();
    edit(ExtraEdit::Enclose { first: "0.0.0".into(), last: "0.0.0".into(), text: "Detail".into() }).unwrap();
    assert!(edit(ExtraEdit::Enclose { first: "0".into(), last: "0".into(), text: String::new() }).is_err());
    assert!(
        edit(ExtraEdit::Summarize { first: "0.0.0".into(), last: "0.1".into(), text: String::new() })
            .is_err()
    );
    assert!(edit(ExtraEdit::Relate { from: "0.1".into(), to: "0.1".into(), text: String::new() }).is_err());
    assert!(edit(ExtraEdit::Remove { index: 7 }).is_err());

    let written = write_map_with(note, &map, &extras);
    assert_eq!(
        written,
        "---\nmindmap: logic\n---\n# Centre\n\n- A ^a\n  - a1 ^t3\n- B ^t2\n- C ^t1\n\n%% mindmap\nrelation a t1 leads to\nsummary t2 t1 Later\nboundary t3 t3 Detail\n%%\n"
    );
    assert_eq!(read_map(&written, "x"), map, "the comment is no topic's note");
    assert_eq!(read_extras(&written), extras);
    assert_eq!(write_map(&written, &read_map(&written, "x")), written, "a second round changes nothing");

    // Extras whose topics are gone go with them.
    let mut fewer = map.clone();
    edit_map(&mut fewer, &MapEdit::Delete { id: "0.2".into() }).unwrap();
    assert!(write_map(&written, &fewer).ends_with("- B ^t2\n\n%% mindmap\nboundary t3 t3 Detail\n%%\n"));

    let mut changed = extras.clone();
    edit_extras(&mut map, &mut changed, &ExtraEdit::Retext { index: 0, text: "causes".into() }).unwrap();
    edit_extras(&mut map, &mut changed, &ExtraEdit::Remove { index: 1 }).unwrap();
    assert_eq!(changed.iter().map(|e| e.text.as_str()).collect::<Vec<_>>(), ["causes", "Detail"]);
    let centred = "# C\n- x ^k\n\n%% mindmap\nrelation centre k\nnonsense\nsummary k\n";
    assert_eq!(
        read_extras(centred),
        [Extra { kind: ExtraKind::Relation, first: CENTRE.into(), last: "k".into(), text: String::new() }],
        "an unclosed comment runs to the end, and broken lines are skipped"
    );
}

#[test]
fn lays_out_relations_summaries_and_boundaries() {
    let note = "# Centre\n- A ^a\n  - a1 ^t3\n- B ^t2\n- C ^t1\n\n%% mindmap\nrelation a t1 leads to\nsummary t2 t1 Later\nboundary t3 t3 Detail\nrelation a gone\n%%\n";
    let map = read_map(note, "x");
    let extras = read_extras(note);
    for style in [
        MapStyle::Logic,
        MapStyle::Right,
        MapStyle::Org,
        MapStyle::Tree,
        MapStyle::Timeline,
        MapStyle::Fishbone,
    ] {
        let layout = layout_map(&map, &extras, style, &[]);
        let kinds: Vec<(usize, ExtraKind)> = layout.extras.iter().map(|e| (e.index, e.kind)).collect();
        assert_eq!(
            kinds,
            [(0, ExtraKind::Relation), (1, ExtraKind::Summary), (2, ExtraKind::Boundary)],
            "{style:?}: one with a topic gone is left out"
        );
        let relation = &layout.extras[0];
        assert_eq!((relation.first.as_str(), relation.last.as_str()), ("0.0", "0.2"));
        assert!(relation.path.starts_with('M') && relation.path.contains('Q'), "{}", relation.path);
        assert!(layout.extras[2].path.ends_with('Z'));
        assert_eq!((layout.extras[1].first.as_str(), layout.extras[1].last.as_str()), ("0.1", "0.2"));
        for extra in &layout.extras {
            assert!(extra.width > 0.0, "{style:?}: {extra:?}");
            assert!(extra.x >= 0.0 && extra.y >= 0.0, "{style:?}: {extra:?}");
            assert!(extra.x + extra.width <= layout.width + 0.1, "{style:?}: {extra:?}");
            assert!(extra.y + extra.height <= layout.height + 0.1, "{style:?}: {extra:?}");
        }
    }
    let folded = layout_map(&map, &extras, MapStyle::Logic, &["0.0".to_string()]);
    assert_eq!(folded.extras.len(), 2, "a boundary folded away is not drawn");
}

#[test]
fn lays_out_a_fishbone() {
    let map = topic(
        "Late deliveries",
        vec![
            topic(
                "People",
                vec![topic("Too few drivers", vec![topic("Pay", vec![])]), topic("Training", vec![])],
            ),
            topic("Machines", vec![topic("Old vans", vec![]), topic("No tracking", vec![])]),
            topic("Methods with a rather long name", vec![]),
            topic("Materials", vec![topic("Late parts", vec![])]),
            topic("Weather", vec![]),
        ],
    );
    let layout = layout_map(&map, &[], MapStyle::Fishbone, &[]);
    assert_eq!(MapStyle::parse("Fishbone"), MapStyle::Fishbone);
    let at = |id: &str| layout.topics.iter().find(|t| t.id == id).unwrap();
    let head = at("0");
    let axis = head.y + head.height / 2.0;
    // The bones go above and below the spine in turn, further left each pair.
    for (id, up) in [("0.0", true), ("0.1", false), ("0.2", true), ("0.3", false), ("0.4", true)] {
        let t = at(id);
        assert_eq!(t.y + t.height < axis, up, "{id}");
        assert!(t.x + t.width < head.x, "{id} is left of the head");
    }
    assert!(at("0.2").x + at("0.2").width <= at("0.0").x.max(at("0.1").x));
    assert!(at("0.4").x < at("0.2").x);
    // Nothing overlaps.
    for (i, a) in layout.topics.iter().enumerate() {
        for b in &layout.topics[i + 1..] {
            let apart = a.x + a.width <= b.x
                || b.x + b.width <= a.x
                || a.y + a.height <= b.y
                || b.y + b.height <= a.y;
            assert!(apart, "{} and {} overlap", a.id, b.id);
        }
    }
    // The first upper bone brings the spine from the head; a lower one does not.
    assert!(at("0.0").edge.starts_with(&format!("M{:.1} {axis:.1}L", head.x)), "{}", at("0.0").edge);
    assert_eq!(at("0.1").edge.matches('M').count(), 1);
    assert_eq!(at("0.2").edge.matches('M').count(), 2);
    let sub = at("0.0.0");
    assert!(
        sub.edge.ends_with(&format!("L{:.1} {:.1}", sub.x + sub.width, sub.y + sub.height / 2.0)),
        "{}",
        sub.edge
    );
    assert!(layout.topics.iter().skip(1).all(|t| t.side == -1));
}
