//! Mind maps in other apps' files, and as pictures: OPML (outliners,
//! MindNode, XMind), FreeMind and Freeplane `.mm`, XMind's `content.json`
//! (XMind 2020 and later) and `content.xml` (XMind 8), and SVG. An XMind
//! file is a ZIP of these; the core packs and unpacks it.

use std::fmt::Write as _;

use serde_json::{Value, json};

use crate::mindmap::{
    CENTRE, Extra, ExtraKind, LABEL_ROW, LABEL_SIZE, Layout, MapStyle, MapTheme, PlacedExtra, Topic,
    is_web_link, labels_width, layout_map, marker_named, split_labels, split_link, split_markers, text_width,
    with_labels, with_link, with_markers,
};
use crate::xml::{Content, Element, escape, parse_xml};

/// A topic's text on one line, as an outline keeps it.
fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A note's lines with the indentation of pretty printed files removed.
fn tidy_note(text: &str) -> String {
    let lines: Vec<&str> = text.lines().map(str::trim).collect();
    let start = lines.iter().position(|l| !l.is_empty()).unwrap_or(lines.len());
    let end = lines.iter().rposition(|l| !l.is_empty()).map_or(start, |i| i + 1);
    lines[start..end].join("\n")
}

/// A topic's text with a file's link on it, when it is one a note can
/// follow: a web address, not an XMind topic (`xmind:#…`) or attachment.
fn linked(text: String, href: Option<&str>) -> String {
    match href.map(str::trim) {
        Some(href) if is_web_link(href) && !href.starts_with("xmind:") && !href.starts_with("xap:") => {
            with_link(&text, href)
        }
        _ => text,
    }
}

/// A topic's text with a file's labels after it, beside any tags it
/// already ends with.
fn labelled(text: String, labels: &[String]) -> String {
    if labels.is_empty() {
        return text;
    }
    let mut all = split_labels(&text).1;
    all.extend(labels.iter().cloned());
    with_labels(&text, &all)
}

/// A topic's text as other apps show it, and its link when they can follow
/// it (a web address; a note's name means nothing to them).
fn shown_and_href(text: &str) -> (String, Option<String>) {
    let (shown, link) = split_link(text);
    (shown, is_web_link(&link).then_some(link))
}

/// The centre of a file's outlines: the only one, or a topic named `title`
/// holding them all.
fn centre(mut topics: Vec<Topic>, title: &str) -> Topic {
    if topics.len() == 1 {
        topics.remove(0)
    } else {
        Topic { text: title.to_string(), children: topics, ..Topic::default() }
    }
}

// OPML.

fn opml_topic(outline: &Element) -> Topic {
    let text = one_line(outline.attr("text").or_else(|| outline.attr("title")).unwrap_or_default());
    Topic {
        text: linked(text, outline.attr("url").or_else(|| outline.attr("htmlUrl"))),
        note: tidy_note(outline.attr("_note").unwrap_or_default()),
        children: outline.elements().filter(|e| e.local() == "outline").map(opml_topic).collect(),
        anchor: String::new(),
    }
}

/// The map in an OPML file; `title` names the centre when the file has
/// several top outlines and no title of its own.
pub fn map_from_opml(xml: &str, title: &str) -> Option<Topic> {
    let root = parse_xml(xml).filter(|r| r.local() == "opml")?;
    let body = root.child("body")?;
    let head_title = root.child("head").and_then(|h| h.child("title")).map(|t| one_line(&t.text()));
    let title = head_title.filter(|t| !t.is_empty()).unwrap_or_else(|| title.to_string());
    Some(centre(body.elements().filter(|e| e.local() == "outline").map(opml_topic).collect(), &title))
}

fn opml_outline(topic: &Topic, level: usize, out: &mut String) {
    let indent = "  ".repeat(level);
    let (text, href) = shown_and_href(&topic.text);
    let _ = write!(out, "{indent}<outline text=\"{}\"", escape(&text));
    if let Some(href) = href {
        let _ = write!(out, " type=\"link\" url=\"{}\"", escape(&href));
    }
    if !topic.note.is_empty() {
        let _ = write!(out, " _note=\"{}\"", escape(&topic.note));
    }
    if topic.children.is_empty() {
        out.push_str("/>\n");
        return;
    }
    out.push_str(">\n");
    for child in &topic.children {
        opml_outline(child, level + 1, out);
    }
    let _ = writeln!(out, "{indent}</outline>");
}

/// The map as OPML, the centre as the one top outline.
pub fn map_to_opml(root: &Topic) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<opml version=\"2.0\">\n");
    let _ = writeln!(
        out,
        "  <head>\n    <title>{}</title>\n  </head>\n  <body>",
        escape(&split_link(&root.text).0)
    );
    opml_outline(root, 2, &mut out);
    out.push_str("  </body>\n</opml>\n");
    out
}

// FreeMind and Freeplane.

/// The text of an HTML fragment, a line for each paragraph.
fn html_text(element: &Element) -> String {
    fn walk(element: &Element, out: &mut String) {
        for content in &element.children {
            match content {
                Content::Text(text) => {
                    let mut last_space = out.ends_with([' ', '\n']) || out.is_empty();
                    for c in text.chars() {
                        if c.is_whitespace() {
                            if !last_space {
                                out.push(' ');
                            }
                            last_space = true;
                        } else {
                            out.push(c);
                            last_space = false;
                        }
                    }
                }
                Content::Element(child) if child.local().eq_ignore_ascii_case("head") => {}
                Content::Element(child) => {
                    walk(child, out);
                    let name = child.local().to_ascii_lowercase();
                    if matches!(
                        name.as_str(),
                        "p" | "div" | "br" | "li" | "tr" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
                    ) {
                        out.push('\n');
                    }
                }
            }
        }
    }
    let mut out = String::new();
    walk(element, &mut out);
    tidy_note(&out)
}

fn freemind_topic(node: &Element) -> Topic {
    let rich = |kind: &str| {
        node.elements()
            .find(|e| {
                e.local() == "richcontent" && e.attr("TYPE").is_some_and(|t| t.eq_ignore_ascii_case(kind))
            })
            .map(html_text)
    };
    let text = node.attr("TEXT").map(str::to_string).or_else(|| rich("NODE")).unwrap_or_default();
    Topic {
        text: linked(one_line(&text), node.attr("LINK")),
        note: rich("NOTE").unwrap_or_default(),
        children: node.elements().filter(|e| e.local() == "node").map(freemind_topic).collect(),
        anchor: String::new(),
    }
}

/// The map in a FreeMind or Freeplane `.mm` file.
pub fn map_from_freemind(xml: &str) -> Option<Topic> {
    let root = parse_xml(xml).filter(|r| r.local() == "map")?;
    root.child("node").map(freemind_topic)
}

/// A node's opening tag's text and link.
fn freemind_text(topic: &Topic, out: &mut String) {
    let (text, href) = shown_and_href(&topic.text);
    let _ = write!(out, "<node TEXT=\"{}\"", escape(&text));
    if let Some(href) = href {
        let _ = write!(out, " LINK=\"{}\"", escape(&href));
    }
}

fn freemind_node(topic: &Topic, position: Option<&str>, out: &mut String) {
    freemind_text(topic, out);
    if let Some(position) = position {
        let _ = write!(out, " POSITION=\"{position}\"");
    }
    out.push_str(">\n");
    if !topic.note.is_empty() {
        out.push_str("<richcontent TYPE=\"NOTE\"><html><head></head><body>");
        for line in topic.note.lines() {
            let _ = write!(out, "<p>{}</p>", escape(line));
        }
        out.push_str("</body></html></richcontent>\n");
    }
    for child in &topic.children {
        freemind_node(child, None, out);
    }
    out.push_str("</node>\n");
}

/// The map as a FreeMind file (Freeplane opens it too), the first half of
/// the branches on the right.
pub fn map_to_freemind(root: &Topic) -> String {
    let mut out = String::from("<map version=\"1.0.1\">\n");
    freemind_text(root, &mut out);
    out.push_str(">\n");
    if !root.note.is_empty() {
        out.push_str("<richcontent TYPE=\"NOTE\"><html><head></head><body>");
        for line in root.note.lines() {
            let _ = write!(out, "<p>{}</p>", escape(line));
        }
        out.push_str("</body></html></richcontent>\n");
    }
    let right = root.children.len().div_ceil(2);
    for (i, child) in root.children.iter().enumerate() {
        freemind_node(child, Some(if i < right { "right" } else { "left" }), &mut out);
    }
    out.push_str("</node>\n</map>\n");
    out
}

// XMind.

/// XMind's name for a layout, and back.
fn xmind_structure(style: MapStyle) -> &'static str {
    match style {
        MapStyle::Logic => "org.xmind.ui.map.unbalanced",
        MapStyle::Right => "org.xmind.ui.logic.right",
        MapStyle::Org => "org.xmind.ui.org-chart.down",
        MapStyle::Tree => "org.xmind.ui.tree.right",
        MapStyle::Timeline => "org.xmind.ui.timeline.horizontal",
        MapStyle::Fishbone => "org.xmind.ui.fishbone.rightHeaded",
    }
}

fn style_from_xmind(structure: &str) -> MapStyle {
    if structure.contains("timeline") {
        MapStyle::Timeline
    } else if structure.contains("fishbone") {
        MapStyle::Fishbone
    } else if structure.contains("org-chart") {
        MapStyle::Org
    } else if structure.contains("tree") {
        MapStyle::Tree
    } else if structure.contains(".right") {
        MapStyle::Right
    } else {
        MapStyle::Logic
    }
}

/// A topic ID from a file, kept as the topic's anchor while the file's
/// relations, summaries and boundaries are read.
fn file_anchor(id: &str) -> String {
    id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').collect()
}

/// XMind's range of children: `(0,2)`, or `master` for all of them.
fn xmind_range(range: &str, count: usize) -> Option<(usize, usize)> {
    if count == 0 {
        return None;
    }
    if range.trim() == "master" {
        return Some((0, count - 1));
    }
    let (a, b) = range.trim().strip_prefix('(')?.strip_suffix(')')?.split_once(',')?;
    let (a, b): (usize, usize) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
    (a <= b && b < count).then_some((a, b))
}

/// A summary or boundary over a run of children, which carry the file's
/// IDs as anchors.
fn run_extra(kind: ExtraKind, children: &[Topic], range: &str, text: &str) -> Option<Extra> {
    let (a, b) = xmind_range(range, children.len())?;
    Some(Extra {
        kind,
        first: children[a].anchor.clone(),
        last: children[b].anchor.clone(),
        text: one_line(text),
    })
}

/// Short block IDs (`t1`, `t2`…) for the topics the extras name, none for
/// the rest, and `centre` for the centre; extras naming no topic go.
fn settle_anchors(root: &mut Topic, extras: &mut Vec<Extra>) {
    fn walk(topic: &mut Topic, used: &[String], names: &mut Vec<(String, String)>) {
        let file = std::mem::take(&mut topic.anchor);
        if !file.is_empty() && used.contains(&file) {
            topic.anchor = format!("t{}", names.iter().filter(|(_, n)| n != CENTRE).count() + 1);
            names.push((file, topic.anchor.clone()));
        }
        for child in &mut topic.children {
            walk(child, used, names);
        }
    }
    let used: Vec<String> = extras.iter().flat_map(|e| [e.first.clone(), e.last.clone()]).collect();
    let mut names = Vec::new();
    let centre = std::mem::take(&mut root.anchor);
    if !centre.is_empty() {
        names.push((centre, CENTRE.to_string()));
    }
    for child in &mut root.children {
        walk(child, &used, &mut names);
    }
    let name = |file: &str| names.iter().find(|(f, _)| f == file).map(|(_, n)| n.clone());
    extras.retain_mut(|extra| match (name(&extra.first), name(&extra.last)) {
        (Some(first), Some(last)) => {
            (extra.first, extra.last) = (first, last);
            true
        }
        _ => false,
    });
}

fn xmind_json_topic(topic: &Value, extras: &mut Vec<Extra>) -> Topic {
    let children: Vec<Topic> = topic["children"]["attached"]
        .as_array()
        .map(|topics| topics.iter().map(|t| xmind_json_topic(t, extras)).collect())
        .unwrap_or_default();
    let summary_topics = topic["children"]["summary"].as_array();
    for summary in topic["summaries"].as_array().into_iter().flatten() {
        let text = summary_topics
            .into_iter()
            .flatten()
            .find(|t| t["id"] == summary["topicId"])
            .and_then(|t| t["title"].as_str())
            .unwrap_or_default();
        let range = summary["range"].as_str().unwrap_or_default();
        extras.extend(run_extra(ExtraKind::Summary, &children, range, text));
    }
    for boundary in topic["boundaries"].as_array().into_iter().flatten() {
        let (range, title) =
            (boundary["range"].as_str().unwrap_or_default(), boundary["title"].as_str().unwrap_or_default());
        extras.extend(run_extra(ExtraKind::Boundary, &children, range, title));
    }
    // Markers Paperloom knows come as the emoji a note keeps.
    let markers: Vec<_> = topic["markers"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| marker_named(m["markerId"].as_str().unwrap_or_default()))
        .collect();
    Topic {
        text: labelled(
            linked(
                with_markers(&markers, &one_line(topic["title"].as_str().unwrap_or_default())),
                topic["href"].as_str(),
            ),
            &topic["labels"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|l| l.as_str().map(str::to_string))
                .collect::<Vec<_>>(),
        ),
        note: tidy_note(topic["notes"]["plain"]["content"].as_str().unwrap_or_default()),
        children,
        anchor: file_anchor(topic["id"].as_str().unwrap_or_default()),
    }
}

/// The first sheet's map in an XMind `content.json`, its layout, and its
/// relations, summaries and boundaries.
pub fn map_from_xmind_json(json: &str) -> Option<(Topic, MapStyle, Vec<Extra>)> {
    let sheets: Value = serde_json::from_str(json).ok()?;
    let sheet = sheets.as_array()?.first()?;
    let root = sheet.get("rootTopic")?;
    let style = style_from_xmind(root["structureClass"].as_str().unwrap_or_default());
    let mut extras = Vec::new();
    let mut map = xmind_json_topic(root, &mut extras);
    for relation in sheet["relationships"].as_array().into_iter().flatten() {
        let end = |key: &str| file_anchor(relation[key].as_str().unwrap_or_default());
        let (first, last) = (end("end1Id"), end("end2Id"));
        if !first.is_empty() && !last.is_empty() {
            let text = one_line(relation["title"].as_str().unwrap_or_default());
            extras.push(Extra { kind: ExtraKind::Relation, first, last, text });
        }
    }
    settle_anchors(&mut map, &mut extras);
    Some((map, style, extras))
}

fn xmind_xml_topic(topic: &Element, extras: &mut Vec<Extra>) -> Topic {
    let groups =
        || topic.child("children").into_iter().flat_map(Element::elements).filter(|t| t.local() == "topics");
    let children: Vec<Topic> = groups()
        .filter(|t| t.attr("type").is_none_or(|kind| kind == "attached"))
        .flat_map(Element::elements)
        .filter(|t| t.local() == "topic")
        .map(|t| xmind_xml_topic(t, extras))
        .collect();
    let title = |element: &Element| element.child("title").map(Element::text).unwrap_or_default();
    let listed = |name: &str, item: &'static str| {
        topic.child(name).into_iter().flat_map(Element::elements).filter(move |e| e.local() == item)
    };
    for summary in listed("summaries", "summary") {
        let text = groups()
            .filter(|t| t.attr("type") == Some("summary"))
            .flat_map(Element::elements)
            .find(|t| t.attr("id").is_some() && t.attr("id") == summary.attr("topic-id"))
            .map(title)
            .unwrap_or_default();
        extras.extend(run_extra(
            ExtraKind::Summary,
            &children,
            summary.attr("range").unwrap_or_default(),
            &text,
        ));
    }
    for boundary in listed("boundaries", "boundary") {
        let range = boundary.attr("range").unwrap_or_default();
        extras.extend(run_extra(ExtraKind::Boundary, &children, range, &title(boundary)));
    }
    let markers: Vec<_> = topic
        .child("marker-refs")
        .into_iter()
        .flat_map(Element::elements)
        .filter_map(|m| marker_named(m.attr("marker-id").unwrap_or_default()))
        .collect();
    Topic {
        text: labelled(
            linked(
                with_markers(&markers, &one_line(&title(topic))),
                topic.attr("xlink:href").or_else(|| topic.attr("href")),
            ),
            &listed("labels", "label").map(Element::text).collect::<Vec<_>>(),
        ),
        note: topic
            .child("notes")
            .and_then(|n| n.child("plain"))
            .map(|p| tidy_note(&p.text()))
            .unwrap_or_default(),
        children,
        anchor: file_anchor(topic.attr("id").unwrap_or_default()),
    }
}

/// The first sheet's map in an XMind 8 `content.xml`, its layout, and its
/// relations, summaries and boundaries.
pub fn map_from_xmind_xml(xml: &str) -> Option<(Topic, MapStyle, Vec<Extra>)> {
    let root = parse_xml(xml).filter(|r| r.local() == "xmap-content")?;
    let sheet = root.child("sheet")?;
    let topic = sheet.child("topic")?;
    let mut extras = Vec::new();
    let mut map = xmind_xml_topic(topic, &mut extras);
    let relations = sheet.child("relationships").into_iter().flat_map(Element::elements);
    for relation in relations.filter(|r| r.local() == "relationship") {
        let end = |key: &str| file_anchor(relation.attr(key).unwrap_or_default());
        let (first, last) = (end("end1"), end("end2"));
        if !first.is_empty() && !last.is_empty() {
            let text = one_line(&relation.child("title").map(Element::text).unwrap_or_default());
            extras.push(Extra { kind: ExtraKind::Relation, first, last, text });
        }
    }
    settle_anchors(&mut map, &mut extras);
    Some((map, style_from_xmind(topic.attr("structure-class").unwrap_or_default()), extras))
}

/// A topic in XMind's JSON, with the summaries and boundaries over its
/// children; `ids` gathers each anchor's XMind ID for the relations.
fn xmind_topic_json(topic: &Topic, extras: &[Extra], id: &mut u32, ids: &mut Vec<(String, String)>) -> Value {
    *id += 1;
    let own = format!("paperloom-{id}");
    let anchor = if *id == 1 { CENTRE } else { topic.anchor.as_str() };
    if !anchor.is_empty() {
        ids.push((anchor.to_string(), own.clone()));
    }
    let (shown, href) = shown_and_href(&topic.text);
    let (markers, title) = split_markers(&shown);
    let (title, labels) = split_labels(title);
    let mut value = json!({ "id": own, "class": "topic", "title": title });
    if let Some(href) = href {
        value["href"] = json!(href);
    }
    if !labels.is_empty() {
        value["labels"] = json!(labels);
    }
    if !markers.is_empty() {
        value["markers"] = json!(markers.iter().map(|m| json!({ "markerId": m.id })).collect::<Vec<_>>());
    }
    if !topic.note.is_empty() {
        value["notes"] = json!({ "plain": { "content": topic.note } });
    }
    let attached: Vec<Value> = topic.children.iter().map(|c| xmind_topic_json(c, extras, id, ids)).collect();
    let index_of =
        |anchor: &str| topic.children.iter().position(|c| !c.anchor.is_empty() && c.anchor == anchor);
    let (mut summaries, mut summary_topics, mut boundaries) = (Vec::new(), Vec::new(), Vec::new());
    for extra in extras.iter().filter(|e| e.kind != ExtraKind::Relation) {
        let (Some(a), Some(b)) = (index_of(&extra.first), index_of(&extra.last)) else { continue };
        let range = format!("({},{})", a.min(b), a.max(b));
        *id += 1;
        let extra_id = format!("paperloom-{id}");
        if extra.kind == ExtraKind::Summary {
            *id += 1;
            let topic_id = format!("paperloom-{id}");
            summary_topics.push(json!({ "id": topic_id, "class": "topic", "title": extra.text }));
            summaries.push(json!({ "id": extra_id, "range": range, "topicId": topic_id }));
        } else {
            let mut boundary = json!({ "id": extra_id, "range": range });
            if !extra.text.is_empty() {
                boundary["title"] = json!(extra.text);
            }
            boundaries.push(boundary);
        }
    }
    if !attached.is_empty() || !summary_topics.is_empty() {
        let mut children = json!({});
        if !attached.is_empty() {
            children["attached"] = json!(attached);
        }
        if !summary_topics.is_empty() {
            children["summary"] = json!(summary_topics);
        }
        value["children"] = children;
    }
    if !summaries.is_empty() {
        value["summaries"] = json!(summaries);
    }
    if !boundaries.is_empty() {
        value["boundaries"] = json!(boundaries);
    }
    value
}

/// The map as XMind's `content.json`: one sheet, with its relations,
/// summaries and boundaries.
pub fn map_to_xmind_json(root: &Topic, extras: &[Extra], style: MapStyle) -> String {
    let (mut id, mut ids) = (0, Vec::new());
    let mut topic = xmind_topic_json(root, extras, &mut id, &mut ids);
    topic["structureClass"] = json!(xmind_structure(style));
    let xmind_id = |anchor: &str| ids.iter().find(|(a, _)| a == anchor).map(|(_, id)| id.clone());
    let relations: Vec<Value> = extras
        .iter()
        .filter(|e| e.kind == ExtraKind::Relation)
        .filter_map(|e| Some((xmind_id(&e.first)?, xmind_id(&e.last)?, e)))
        .enumerate()
        .map(|(n, (end1, end2, e))| {
            let mut relation =
                json!({ "id": format!("paperloom-relation-{}", n + 1), "end1Id": end1, "end2Id": end2 });
            if !e.text.is_empty() {
                relation["title"] = json!(e.text);
            }
            relation
        })
        .collect();
    let title = split_link(&root.text).0;
    let mut sheet = json!({ "id": "paperloom-sheet", "class": "sheet", "title": title, "rootTopic": topic });
    if !relations.is_empty() {
        sheet["relationships"] = json!(relations);
    }
    json!([sheet]).to_string()
}

// SVG.

/// A topic's text in lines no wider than the layout allows.
fn wrap(text: &str, size: f64, room: f64) -> Vec<String> {
    let mut lines = vec![String::new()];
    let mut width = 0.0;
    for c in text.chars() {
        let em = if (c as u32) >= 0x1100 && !c.is_ascii() { 1.0 } else { 0.58 };
        let w = em * size;
        if width + w > room + 0.5 && !lines.last().is_some_and(String::is_empty) {
            lines.push(String::new());
            width = 0.0;
        }
        if let Some(line) = lines.last_mut() {
            line.push(c);
        }
        width += w;
    }
    lines
}

/// The map as an SVG picture, as it lies in `layout`, in a theme's colours.
fn layout_svg(layout: &Layout, org: bool, theme: &MapTheme) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w:.0}\" height=\"{h:.0}\" viewBox=\"0 0 {w:.0} {h:.0}\" \
         font-family=\"-apple-system, BlinkMacSystemFont, 'Segoe UI', 'PingFang SC', 'Microsoft YaHei', sans-serif\">",
        w = layout.width,
        h = layout.height
    );
    let _ = writeln!(out, "<rect width=\"100%\" height=\"100%\" fill=\"{}\"/>", theme.background);
    let of_kind = |kind: ExtraKind| layout.extras.iter().filter(move |e| e.kind == kind);
    if of_kind(ExtraKind::Relation).next().is_some() {
        let _ = writeln!(
            out,
            "<defs><marker id=\"arrow\" viewBox=\"0 0 10 10\" refX=\"9\" refY=\"5\" markerWidth=\"7\" \
             markerHeight=\"7\" orient=\"auto-start-reverse\"><path d=\"M0 0L10 5L0 10Z\" fill=\"{}\"/>\
             </marker></defs>",
            theme.line
        );
    }
    for frame in of_kind(ExtraKind::Boundary) {
        let _ = writeln!(
            out,
            "<path d=\"{}\" fill=\"{c}\" fill-opacity=\"0.06\" stroke=\"{c}\" stroke-opacity=\"0.5\" \
             stroke-width=\"1.5\" stroke-dasharray=\"6 4\"/>",
            frame.path,
            c = theme.centre
        );
    }
    for topic in layout.topics.iter().filter(|t| !t.edge.is_empty()) {
        let width = if topic.depth == 1 { 3 } else { 2 };
        let _ = writeln!(
            out,
            "<path d=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{width}\" stroke-linecap=\"round\"/>",
            topic.edge,
            theme.branch(&topic.id)
        );
    }
    for brace in of_kind(ExtraKind::Summary) {
        let _ = writeln!(
            out,
            "<path d=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"1.5\" stroke-linejoin=\"round\"/>",
            brace.path, theme.line
        );
    }
    for relation in of_kind(ExtraKind::Relation) {
        let _ = writeln!(
            out,
            "<path d=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"1.5\" stroke-dasharray=\"5 4\" \
             marker-end=\"url(#arrow)\"/>",
            relation.path, theme.line
        );
    }
    for topic in &layout.topics {
        let color = theme.branch(&topic.id);
        let (x, y, w, h) = (topic.x, topic.y, topic.width, topic.height);
        let text_color = match topic.depth {
            0 => {
                let _ = writeln!(
                    out,
                    "<rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{w:.1}\" height=\"{h:.1}\" rx=\"14\" fill=\"{}\"/>",
                    theme.centre
                );
                theme.centre_text
            }
            1 if theme.filled => {
                let _ = writeln!(
                    out,
                    "<rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{w:.1}\" height=\"{h:.1}\" rx=\"10\" fill=\"{color}\"/>"
                );
                theme.centre_text
            }
            1 => {
                let _ = writeln!(
                    out,
                    "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"10\" fill=\"{}\" \
                     stroke=\"{color}\" stroke-width=\"2\"/>",
                    x + 1.0,
                    y + 1.0,
                    w - 2.0,
                    h - 2.0,
                    theme.topic
                );
                theme.text
            }
            _ => {
                let _ = writeln!(
                    out,
                    "<line x1=\"{x:.1}\" y1=\"{b:.1}\" x2=\"{r:.1}\" y2=\"{b:.1}\" stroke=\"{color}\" stroke-width=\"2\"/>",
                    b = y + h - 1.0,
                    r = x + w
                );
                theme.text
            }
        };
        let pad = if topic.depth == 0 { 24.0 } else { 14.0 };
        let size = topic.font_size;
        // A linked topic has a mark after its text; a web link goes along.
        let mark = if topic.link.is_empty() { 0.0 } else { size * 1.2 };
        let web = is_web_link(&topic.link);
        if web {
            let _ = write!(out, "<a href=\"{}\">", escape(&topic.link));
        }
        let lines = wrap(&topic.text, size, w - 2.0 * pad - mark);
        let line_height = size * 1.4;
        let label_row = if topic.labels.is_empty() { 0.0 } else { LABEL_ROW };
        let top = y + (h - line_height * lines.len() as f64 - label_row) / 2.0;
        let weight = if topic.depth == 0 { " font-weight=\"600\"" } else { "" };
        let middle = x + (w - mark) / 2.0;
        let _ = write!(
            out,
            "<text x=\"{middle:.1}\" font-size=\"{size}\" fill=\"{text_color}\" text-anchor=\"middle\" \
             dominant-baseline=\"central\"{weight}>"
        );
        for (i, line) in lines.iter().enumerate() {
            let _ = write!(
                out,
                "<tspan x=\"{middle:.1}\" y=\"{:.1}\">{}</tspan>",
                top + line_height * (i as f64 + 0.5),
                escape(line)
            );
        }
        out.push_str("</text>");
        if mark > 0.0 {
            let _ = write!(
                out,
                "<text x=\"{:.1}\" y=\"{:.1}\" font-size=\"{size}\" fill=\"{color}\" text-anchor=\"middle\" \
                 dominant-baseline=\"central\">\u{2197}</text>",
                x + w - pad - mark / 2.0,
                y + h / 2.0
            );
        }
        if web {
            out.push_str("</a>");
        }
        // The labels in a row under the text.
        let mut left = x + (w - labels_width(&topic.labels)) / 2.0;
        let label_top = top + line_height * lines.len() as f64 + LABEL_ROW - LABEL_SIZE * 1.4;
        for label in &topic.labels {
            let width = text_width(label, LABEL_SIZE) + 10.0;
            let _ = write!(
                out,
                "<rect x=\"{left:.1}\" y=\"{label_top:.1}\" width=\"{width:.1}\" height=\"{:.1}\" rx=\"4\" \
                 fill=\"{color}\" fill-opacity=\"0.15\"/><text x=\"{:.1}\" y=\"{:.1}\" font-size=\"{LABEL_SIZE}\" \
                 fill=\"{text_color}\" text-anchor=\"middle\" dominant-baseline=\"central\">{}</text>",
                LABEL_SIZE * 1.4,
                left + width / 2.0,
                label_top + LABEL_SIZE * 0.7,
                escape(label)
            );
            left += width + 4.0;
        }
        out.push('\n');
        if topic.folded {
            let (cx, cy) = if org {
                (x + w / 2.0, y + h + 10.0)
            } else if topic.side < 0 {
                (x - 10.0, y + h / 2.0)
            } else {
                (x + w + 10.0, y + h / 2.0)
            };
            let _ = writeln!(
                out,
                "<circle cx=\"{cx:.1}\" cy=\"{cy:.1}\" r=\"8\" fill=\"{bg}\" stroke=\"{color}\"/>\
                 <text x=\"{cx:.1}\" y=\"{cy:.1}\" font-size=\"10\" fill=\"{color}\" text-anchor=\"middle\" \
                 dominant-baseline=\"central\">{}</text>",
                topic.children,
                bg = theme.background
            );
        }
    }
    for extra in layout.extras.iter().filter(|e| e.width > 0.0) {
        extra_label(&mut out, extra, theme);
    }
    out.push_str("</svg>\n");
    out
}

/// An extra's text: a summary like a topic, a relation's and a boundary's
/// in a small box.
fn extra_label(out: &mut String, extra: &PlacedExtra, theme: &MapTheme) {
    let (x, y, w, h) = (extra.x, extra.y, extra.width, extra.height);
    let (size, fill, stroke, radius, pad) = match extra.kind {
        ExtraKind::Summary => (14.0, theme.topic, theme.line, 8.0, 14.0),
        ExtraKind::Relation => (12.0, theme.background, "none", 4.0, 8.0),
        ExtraKind::Boundary => (12.0, theme.centre, "none", 4.0, 8.0),
    };
    let color = if extra.kind == ExtraKind::Boundary { theme.centre_text } else { theme.text };
    let _ = writeln!(
        out,
        "<rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{w:.1}\" height=\"{h:.1}\" rx=\"{radius}\" fill=\"{fill}\" stroke=\"{stroke}\"/>"
    );
    let lines = wrap(&extra.text, size, w - 2.0 * pad);
    let line_height = size * 1.4;
    let top = y + (h - line_height * lines.len() as f64) / 2.0;
    let _ = write!(
        out,
        "<text font-size=\"{size}\" fill=\"{color}\" text-anchor=\"middle\" dominant-baseline=\"central\">"
    );
    for (i, line) in lines.iter().enumerate() {
        let _ = write!(
            out,
            "<tspan x=\"{:.1}\" y=\"{:.1}\">{}</tspan>",
            x + w / 2.0,
            top + line_height * (i as f64 + 0.5),
            escape(line)
        );
    }
    out.push_str("</text>\n");
}

/// The map as an SVG picture, with its relations, summaries and
/// boundaries, in a theme's colours.
pub fn map_svg(
    root: &Topic,
    extras: &[Extra],
    style: MapStyle,
    folded: &[String],
    theme: &MapTheme,
) -> String {
    layout_svg(&layout_map(root, extras, style, folded), style == MapStyle::Org, theme)
}

#[cfg(test)]
mod tests;
