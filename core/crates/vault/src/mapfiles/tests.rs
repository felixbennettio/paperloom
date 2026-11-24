use super::*;

fn topic(text: &str, note: &str, children: Vec<Topic>) -> Topic {
    Topic { text: text.into(), note: note.into(), children, anchor: String::new() }
}

fn sample() -> Topic {
    topic(
        "Thermometry <basics> & \"more\"",
        "Why it matters\n\nSecond paragraph",
        vec![
            topic("Methods", "", vec![topic("Resistance", "", vec![]), topic("热电偶", "冷端补偿", vec![])]),
            topic("Errors", "", vec![]),
            topic("Calibration", "Fixed points", vec![]),
        ],
    )
}

#[test]
fn reads_back_what_it_writes() {
    let map = sample();
    assert_eq!(map_from_opml(&map_to_opml(&map), "Ignored"), Some(map.clone()));
    assert_eq!(map_from_freemind(&map_to_freemind(&map)), Some(map.clone()));
    for style in [
        MapStyle::Logic,
        MapStyle::Right,
        MapStyle::Org,
        MapStyle::Tree,
        MapStyle::Timeline,
        MapStyle::Fishbone,
    ] {
        assert_eq!(
            map_from_xmind_json(&map_to_xmind_json(&map, &[], style)),
            Some((map.clone(), style, vec![]))
        );
    }
}

#[test]
fn reads_opml_with_several_top_outlines() {
    let opml = r#"<?xml version="1.0"?>
<!-- exported -->
<opml version="1.0">
  <head><title>Reading list</title></head>
  <body>
    <outline text="Papers">
      <outline title="Smith 2020" _note="Read &#10;twice"/>
    </outline>
    <outline text="Books"/>
  </body>
</opml>"#;
    let map = map_from_opml(opml, "File name").unwrap();
    assert_eq!(map.text, "Reading list");
    assert_eq!(map.children.len(), 2);
    assert_eq!(map.children[0].children[0], topic("Smith 2020", "Read\ntwice", vec![]));
    assert!(map_from_opml("<map/>", "x").is_none());
}

#[test]
fn reads_freemind_rich_text() {
    let mm = r#"<map version="freeplane 1.9.13">
<!--To view this file, download free mind mapping software Freeplane-->
<node TEXT="Centre" ID="ID_1">
<node POSITION="right" ID="ID_2"><richcontent TYPE="NODE"><html>
  <head>
  </head>
  <body>
    <p>
      Rich <b>topic</b>
    </p>
  </body>
</html></richcontent>
<richcontent TYPE="NOTE"><html><head><title>x</title></head><body><p>One</p><p>Two &amp; three</p></body></html></richcontent>
<node TEXT="Leaf &#x4E2D;"/>
</node>
</node>
</map>"#;
    let map = map_from_freemind(mm).unwrap();
    assert_eq!(map.text, "Centre");
    assert_eq!(
        map.children,
        vec![topic("Rich topic", "One\nTwo & three", vec![topic("Leaf 中", "", vec![])])]
    );
}

#[test]
fn reads_xmind_8_content() {
    let xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="no"?>
<xmap-content xmlns="urn:xmind:xmap:xmlns:content:2.0" version="2.0">
<sheet id="s1"><topic id="t0" structure-class="org.xmind.ui.logic.right"><title>Central &amp; main</title>
<notes><plain>Root note</plain><html><xhtml:p>Root note</xhtml:p></html></notes>
<children><topics type="attached"><topic id="t1"><title>A</title></topic><topic id="t2"><title>B</title>
<children><topics type="attached"><topic id="t3"><title><![CDATA[B < 1]]></title></topic></topics></children></topic></topics>
<topics type="detached"><topic id="t9"><title>Floating</title></topic></topics></children></topic>
<title>Sheet 1</title></sheet></xmap-content>"#;
    let (map, style, _) = map_from_xmind_xml(xml).unwrap();
    assert_eq!(style, MapStyle::Right);
    assert_eq!(
        map,
        topic(
            "Central & main",
            "Root note",
            vec![topic("A", "", vec![]), topic("B", "", vec![topic("B < 1", "", vec![])])]
        )
    );
}

#[test]
fn reads_xmind_json_from_xmind() {
    let json = r#"[{"id":"a","class":"sheet","title":"Sheet 1","rootTopic":{"id":"b","class":"topic",
        "title":"Central\nTopic","structureClass":"org.xmind.ui.org-chart.down",
        "children":{"attached":[{"id":"c","title":"Main Topic 1","notes":{"plain":{"content":"Note"}}}],
        "detached":[{"id":"d","title":"Floating"}]}}}]"#;
    let (map, style, _) = map_from_xmind_json(json).unwrap();
    assert_eq!(style, MapStyle::Org);
    assert_eq!(map, topic("Central Topic", "", vec![topic("Main Topic 1", "Note", vec![])]));
    assert!(map_from_xmind_json("{}").is_none());
}

#[test]
fn draws_an_svg() {
    let map = sample();
    let svg = map_svg(&map, &[], MapStyle::Logic, &[], MapTheme::named(""));
    assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""), "{svg}");
    assert_eq!(svg.matches("<path ").count(), 5);
    // The centre is wider than a topic may be, so it takes two lines.
    assert!(svg.contains(">Thermometry &lt;basics&gt; &amp;</tspan>"), "{svg}");
    assert!(svg.contains(">热电偶</tspan>"));
    let folded = map_svg(&map, &[], MapStyle::Org, &["0.0".to_string()], MapTheme::named("dark"));
    assert_eq!(folded.matches("<path ").count(), 3);
    assert!(folded.contains("<circle "));
    assert!(folded.contains("fill=\"#0d1117\"") && !svg.contains("#0d1117"), "the dark theme's paper");
    assert!(svg.trim_end().ends_with("</svg>"));
    let note =
        "# Centre\n- A ^a\n- B ^b\n\n%% mindmap\nrelation a b leads to\nsummary a b Both\nboundary a a\n%%\n";
    let extras = crate::mindmap::read_extras(note);
    let drawn =
        map_svg(&crate::mindmap::read_map(note, "x"), &extras, MapStyle::Logic, &[], MapTheme::named(""));
    assert!(drawn.contains("marker-end=\"url(#arrow)\"") && drawn.contains("<marker "), "{drawn}");
    assert!(drawn.contains(">leads to</tspan>") && drawn.contains(">Both</tspan>"), "{drawn}");
    assert_eq!(drawn.matches("stroke-dasharray").count(), 2, "the relation and the boundary");
}

#[test]
fn wraps_long_topics() {
    let lines = wrap(&"word ".repeat(40), 14.0, 260.0);
    assert!(lines.len() > 1);
    assert!(lines.iter().all(|l| l.chars().count() as f64 * 0.58 * 14.0 <= 261.0));
    assert_eq!(wrap("", 14.0, 260.0), vec![String::new()]);
}

#[test]
fn carries_relations_summaries_and_boundaries_through_xmind() {
    let anchored =
        |text: &str, anchor: &str, children| Topic { anchor: anchor.into(), ..topic(text, "", children) };
    let map = topic(
        "Centre",
        "",
        vec![
            anchored("A", "t1", vec![anchored("a1", "t2", vec![])]),
            anchored("B", "t3", vec![]),
            topic("C", "", vec![]),
        ],
    );
    let extra = |kind, first: &str, last: &str, text: &str| Extra {
        kind,
        first: first.into(),
        last: last.into(),
        text: text.into(),
    };
    // In the order they are read back: from the deepest topics up, then the relations.
    let extras = vec![
        extra(ExtraKind::Boundary, "t2", "t2", ""),
        extra(ExtraKind::Summary, "t1", "t3", "Both"),
        extra(ExtraKind::Relation, CENTRE, "t3", "leads to"),
    ];
    let json = map_to_xmind_json(&map, &extras, MapStyle::Logic);
    assert_eq!(map_from_xmind_json(&json), Some((map.clone(), MapStyle::Logic, extras.clone())));

    let xml = r#"<?xml version="1.0"?>
<xmap-content xmlns="urn:xmind:xmap:xmlns:content:2.0" version="2.0">
<sheet id="s"><topic id="root"><title>Centre</title><children>
<topics type="attached"><topic id="x"><title>X</title></topic><topic id="y"><title>Y</title></topic></topics>
<topics type="summary"><topic id="sum"><title>Pair</title></topic></topics></children>
<summaries><summary id="m" range="(0,1)" topic-id="sum"/></summaries>
<boundaries><boundary id="b" range="master"><title>All</title></boundary></boundaries></topic>
<relationships><relationship id="r" end1="y" end2="root"><title>back</title></relationship>
<relationship id="q" end1="y" end2="nowhere"/></relationships></sheet></xmap-content>"#;
    let (map, _, extras) = map_from_xmind_xml(xml).unwrap();
    assert_eq!(map, topic("Centre", "", vec![anchored("X", "t1", vec![]), anchored("Y", "t2", vec![])]));
    assert_eq!(
        extras,
        [
            extra(ExtraKind::Summary, "t1", "t2", "Pair"),
            extra(ExtraKind::Boundary, "t1", "t2", "All"),
            extra(ExtraKind::Relation, "t2", CENTRE, "back"),
        ],
        "a relation to a topic not on the map is left out"
    );
}

#[test]
fn carries_markers_through_xmind() {
    let json = r#"[{"id":"a","class":"sheet","rootTopic":{"id":"b","title":"Plan",
        "children":{"attached":[{"id":"c","title":"Write","markers":[{"markerId":"task-done"},{"markerId":"priority-1"},{"markerId":"people-red"}]}]}}}]"#;
    let (map, style, extras) = map_from_xmind_json(json).unwrap();
    assert_eq!(map.children[0].text, "1️⃣ ✅ Write", "known markers, in their order");
    let written: serde_json::Value = serde_json::from_str(&map_to_xmind_json(&map, &extras, style)).unwrap();
    let topic = &written[0]["rootTopic"]["children"]["attached"][0];
    assert_eq!(topic["title"], "Write");
    assert_eq!(topic["markers"][0]["markerId"], "priority-1");
    assert_eq!(topic["markers"][1]["markerId"], "task-done");

    let xml = r#"<xmap-content><sheet><topic id="t0"><title>Plan</title><children><topics type="attached">
        <topic id="t1"><title>Ask</title><marker-refs><marker-ref marker-id="symbol-question"/></marker-refs></topic>
        </topics></children></topic></sheet></xmap-content>"#;
    assert_eq!(map_from_xmind_xml(xml).unwrap().0.children[0].text, "❓ Ask");
}

#[test]
fn carries_links_through_files() {
    let map = topic(
        "Plan",
        "",
        vec![
            topic("⭐ [Site](https://example.com/a?b=1&c=2)", "", vec![]),
            topic("[[Methods|How]]", "", vec![]),
        ],
    );
    // XMind keeps web links as hyperlinks; a note's name means nothing there.
    let json: serde_json::Value =
        serde_json::from_str(&map_to_xmind_json(&map, &[], MapStyle::Logic)).unwrap();
    let attached = &json[0]["rootTopic"]["children"]["attached"];
    assert_eq!(attached[0]["title"], "Site");
    assert_eq!(attached[0]["href"], "https://example.com/a?b=1&c=2");
    assert_eq!(attached[1]["title"], "How");
    assert!(attached[1].get("href").is_none());
    let (back, _, _) = map_from_xmind_json(&json.to_string()).unwrap();
    assert_eq!(back.children[0].text, map.children[0].text);
    assert_eq!(back.children[1].text, "How");

    let opml = map_to_opml(&map);
    assert!(
        opml.contains(r#"<outline text="⭐ Site" type="link" url="https://example.com/a?b=1&amp;c=2"/>"#),
        "{opml}"
    );
    assert_eq!(map_from_opml(&opml, "x").unwrap().children[0].text, map.children[0].text);
    let mm = map_to_freemind(&map);
    assert!(mm.contains(r#"<node TEXT="⭐ Site" LINK="https://example.com/a?b=1&amp;c=2""#), "{mm}");
    assert_eq!(map_from_freemind(&mm).unwrap().children[0].text, map.children[0].text);

    // XMind 8's topic links (to other topics) are not web links.
    let xml = r#"<xmap-content xmlns:xlink="http://www.w3.org/1999/xlink"><sheet><topic id="t0"><title>Plan</title>
        <children><topics type="attached">
        <topic id="t1" xlink:href="https://xmind.app"><title>Site</title></topic>
        <topic id="t2" xlink:href="xmind:#t1"><title>Back</title></topic>
        </topics></children></topic></sheet></xmap-content>"#;
    let map8 = map_from_xmind_xml(xml).unwrap().0;
    assert_eq!(map8.children[0].text, "[Site](https://xmind.app)");
    assert_eq!(map8.children[1].text, "Back");

    let svg = map_svg(&map, &[], MapStyle::Logic, &[], MapTheme::named(""));
    assert!(
        svg.contains("<a href=\"https://example.com/a?b=1&amp;c=2\">") && svg.contains(">⭐ Site<"),
        "{svg}"
    );
    assert!(svg.contains(">How<") && svg.matches("\u{2197}").count() == 2, "{svg}");
}

#[test]
fn carries_labels_through_xmind() {
    let json = r#"[{"id":"a","class":"sheet","rootTopic":{"id":"b","title":"Plan",
        "children":{"attached":[{"id":"c","title":"Write","labels":["first draft","v2"]}]}}}]"#;
    let (map, style, extras) = map_from_xmind_json(json).unwrap();
    assert_eq!(map.children[0].text, "Write #first-draft #v2");
    let written: serde_json::Value = serde_json::from_str(&map_to_xmind_json(&map, &extras, style)).unwrap();
    let topic = &written[0]["rootTopic"]["children"]["attached"][0];
    assert_eq!(topic["title"], "Write");
    assert_eq!(topic["labels"], serde_json::json!(["first-draft", "v2"]));

    let xml = r#"<xmap-content><sheet><topic id="t0"><title>Plan</title><children><topics type="attached">
        <topic id="t1"><title>Ask</title><labels><label>later</label></labels></topic>
        </topics></children></topic></sheet></xmap-content>"#;
    assert_eq!(map_from_xmind_xml(xml).unwrap().0.children[0].text, "Ask #later");
    let tagged = r#"[{"id":"a","class":"sheet","rootTopic":{"id":"b","title":"Use #hashtag"}}]"#;
    assert_eq!(map_from_xmind_json(tagged).unwrap().0.text, "Use #hashtag", "a title's own tags stay");

    let svg = map_svg(&map, &[], MapStyle::Logic, &[], MapTheme::named(""));
    assert!(
        svg.contains(">Write</tspan>") && svg.contains(">first-draft</text>") && svg.contains(">v2</text>"),
        "{svg}"
    );
}
