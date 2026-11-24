use paperloom_model::{Creator, Doi, ItemType};

use super::*;

fn nature() -> Item {
    let mut item = Item::new(ItemType::JournalArticle, "Nanometre-scale thermometry in a living cell");
    item.creators = vec![
        Creator::person("Kucsko", "G."),
        Creator::person("Maurer", "P. C."),
        Creator::person("Yao", "N. Y."),
        Creator::person("Lukin", "M. D."),
    ];
    item.container_title = Some("Nature".into());
    item.year = Some(2013);
    item.date = Some("2013-07-31".into());
    item.volume = Some("500".into());
    item.issue = Some("7460".into());
    item.pages = Some("54-58".into());
    item.doi = Some(Doi::parse("10.1038/nature12373").unwrap());
    item.language = Some("en".into());
    item
}

fn chinese() -> Item {
    let mut item = Item::new(ItemType::JournalArticle, "基于深度学习的文献分类方法");
    item.creators = vec![
        Creator::literal("张三"),
        Creator::literal("李四"),
        Creator::literal("王五"),
        Creator::literal("赵六"),
    ];
    item.container_title = Some("计算机学报".into());
    item.year = Some(2021);
    item.volume = Some("44".into());
    item.issue = Some("3".into());
    item.pages = Some("512-525".into());
    item.language = Some("zh-CN".into());
    item
}

#[test]
fn every_offered_style_exists() {
    for info in STYLES {
        let out = format(&[nature()], info.id, Format::Text).unwrap_or_else(|e| panic!("{}: {e}", info.id));
        // Styles change the title's case; note styles may have no reference list.
        let text = format!("{} {}", out.entries.join(" "), out.citation).to_lowercase();
        assert!(text.contains("thermometry") || text.contains("kucsko"), "{}: {text}", info.id);
        assert!(out.entries.len() <= 1, "{}", info.id);
    }
}

#[test]
fn formats_apa() {
    let out = format(&[nature()], "apa", Format::Text).unwrap();
    let entry = &out.entries[0];
    assert!(entry.starts_with("Kucsko, G."), "{entry}");
    assert!(entry.contains("(2013)"), "{entry}");
    assert!(entry.contains("https://doi.org/10.1038/nature12373"), "{entry}");
    assert!(out.citation.contains("Kucsko") && out.citation.contains("2013"), "{}", out.citation);
}

#[test]
fn formats_gb_t_7714_numeric() {
    let out = format(&[chinese(), nature()], "gb-7714-2015-numeric", Format::Text).unwrap();
    assert_eq!(out.entries.len(), 2);
    assert!(out.entries[0].starts_with("[1]"), "{}", out.entries[0]);
    assert!(out.entries[0].contains("张三"), "{}", out.entries[0]);
    assert!(out.entries[0].contains("[J]"), "{}", out.entries[0]);
    assert!(out.entries[1].contains("Nature"), "{}", out.entries[1]);
    assert!(out.citation.contains('1') && out.citation.contains('2'), "{}", out.citation);
    // Four authors: the first three, then 等 for the Chinese work and
    // et al. for the English one.
    assert!(out.entries[0].contains("王五, 等") && !out.entries[0].contains("赵六"), "{}", out.entries[0]);
    assert!(out.entries[1].contains(", et al. ") && !out.entries[1].contains('等'), "{}", out.entries[1]);
    assert!(!out.entries[1].contains("et al.."), "{}", out.entries[1]);
}

#[test]
fn numbers_a_documents_citations_together() {
    let groups = vec![
        vec![Cite { index: 1, page: Some("56".into()) }],
        vec![Cite { index: 0, page: None }, Cite { index: 1, page: None }],
    ];
    let doc = format_document(&[chinese(), nature()], &groups, "gb-7714-2015-numeric", Format::Text).unwrap();
    assert_eq!(doc.citations.len(), 2);
    // This numeric style leaves pages out of citations; APA (below) shows them.
    assert_eq!(doc.citations[0], "[1]");
    assert!(doc.citations[1].contains('2'), "the Chinese work, cited second, is [2]: {:?}", doc.citations);
    assert_eq!(doc.entries.len(), 2);
    assert!(doc.entries[0].starts_with("[1]") && doc.entries[0].contains("Nature"), "{:?}", doc.entries);
    let apa = format_document(
        &[nature()],
        &[vec![Cite { index: 0, page: Some("54".into()) }]],
        "apa",
        Format::Text,
    )
    .unwrap();
    assert!(apa.citations[0].contains("Kucsko") && apa.citations[0].contains("54"), "{:?}", apa.citations);
}

#[test]
fn renders_html() {
    let out = format(&[nature()], "apa", Format::Html).unwrap();
    let entry = &out.entries[0];
    assert!(entry.contains("font-style: italic"), "{entry}");
    assert!(entry.contains(r#"<a href="https://doi.org/10.1038/nature12373">"#), "{entry}");
}

#[test]
fn reports_unknown_styles_and_handles_no_items() {
    assert!(matches!(format(&[nature()], "no-such-style", Format::Text), Err(Error::UnknownStyle(_))));
    assert_eq!(format(&[], "apa", Format::Text).unwrap(), Formatted::default());
}

const BARE: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<style xmlns="http://purl.org/net/xbiblio/csl" class="in-text" version="1.0">
  <info>
    <title>Bare Titles</title>
    <id>http://www.zotero.org/styles/bare-titles</id>
    <updated>2024-01-01T00:00:00+00:00</updated>
  </info>
  <citation>
    <layout prefix="(" suffix=")" delimiter="; ">
      <text variable="title"/>
    </layout>
  </citation>
  <bibliography>
    <layout>
      <text variable="title" prefix="Title: "/>
    </layout>
  </bibliography>
</style>"#;

/// A journal that cites like Nature, which is built in.
const NATURE_JOURNAL: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<style xmlns="http://purl.org/net/xbiblio/csl" version="1.0" default-locale="en-GB">
  <info>
    <title>Nature Communications</title>
    <id>http://www.zotero.org/styles/nature-communications</id>
    <link href="http://www.zotero.org/styles/nature-communications" rel="self"/>
    <link href="http://www.zotero.org/styles/nature" rel="independent-parent"/>
    <updated>2014-06-17T02:29:16+00:00</updated>
  </info>
</style>"#;

#[test]
fn adds_independent_and_dependent_styles() {
    assert_eq!(
        read_style(BARE).unwrap(),
        StyleFile { id: "bare-titles".into(), name: "Bare Titles".into(), parent: None }
    );
    assert_eq!(read_style(NATURE_JOURNAL).unwrap().parent.as_deref(), Some("nature"));
    assert!(matches!(read_style("<html/>"), Err(Error::InvalidStyle(_))));
    assert_eq!(
        (short_name("http://www.zotero.org/styles/apa/"), short_name("My Style (2).csl")),
        ("apa".into(), "my-style-2".into())
    );

    assert!(!has_style("bare-titles"));
    assert_eq!(add_style("bare-titles", BARE).unwrap(), "Bare Titles");
    let out = format(&[nature()], "bare-titles", Format::Text).unwrap();
    assert_eq!(out.citation, "(Nanometre-scale thermometry in a living cell)");
    assert_eq!(out.entries, ["Title: Nanometre-scale thermometry in a living cell"]);

    assert_eq!(add_style("nature-communications", NATURE_JOURNAL).unwrap(), "Nature Communications");
    let out = format(&[nature()], "nature-communications", Format::Text).unwrap();
    assert!(out.entries[0].contains("Kucsko"), "cites like Nature: {:?}", out.entries);
    assert!(added_styles().contains(&("bare-titles".into(), "Bare Titles".into())));
    let orphan = NATURE_JOURNAL.replace("styles/nature\"", "styles/no-such-parent\"");
    assert!(matches!(add_style("orphan", &orphan), Err(Error::MissingParent(p)) if p == "no-such-parent"));

    remove_style("bare-titles");
    remove_style("nature-communications");
    assert!(!has_style("bare-titles") && has_style("nature"));
}
