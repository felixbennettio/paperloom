use std::fs::{self, File};

use super::*;
use crate::backup::{extract, read_entries};
use crate::{NewItem, PaperLibrary};

#[test]
fn writes_a_note_as_a_word_document() {
    let dir = tempfile::tempdir().unwrap();
    let library = PaperLibrary::new(dir.path().join("library.sqlite").display().to_string()).unwrap();
    library
        .add_item(NewItem {
            item_type: String::new(),
            title: "Nanometre-scale thermometry in a living cell".into(),
            authors: vec!["Georg Kucsko".into()],
            container_title: Some("Nature".into()),
            year: Some(2013),
            doi: Some("10.1038/nature12373".into()),
        })
        .unwrap();
    let vault = PaperVault::open(dir.path().join("Vault").display().to_string()).unwrap();
    vault.connect_library(library);
    let note = "---\nstatus: draft\n---\n# Findings\n\nThe **key** idea [@10.1038/nature12373, p. 55] and \
                x < y & [[Other note|that one]].\n\n1. first\n2. second\n\n- loose\n\n  item\n\n\
                See [the paper](https://doi.org/10.1038/nature12373).\n\n```\nlet x = 1;\nlet y = 2;\n```\n";
    vault.write_note("Draft.md".into(), note.into()).unwrap();

    let out = dir.path().join("Draft.docx");
    vault.export_docx("Draft.md".into(), out.display().to_string()).unwrap();
    assert!(!dir.path().join("Draft.docx.partial").exists());

    let mut file = File::open(&out).unwrap();
    let entries = read_entries(&mut file).unwrap();
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names[0], "[Content_Types].xml");
    assert!(names.contains(&"word/document.xml") && names.contains(&"word/styles.xml"));
    let read = |name: &str| {
        let entry = entries.iter().find(|e| e.name == name).unwrap();
        let target = dir.path().join("part.xml");
        extract(&mut File::open(&out).unwrap(), entry, &target).unwrap();
        fs::read_to_string(&target).unwrap()
    };
    let document = read("word/document.xml");
    assert!(
        document.contains(
            r#"<w:pStyle w:val="Title"/></w:pPr><w:r><w:rPr></w:rPr><w:t xml:space="preserve">Draft</w:t>"#
        ),
        "{document}"
    );
    assert!(document.contains(r#"<w:pStyle w:val="Heading1"/>"#) && document.contains(">Findings<"));
    assert!(!document.contains("status: draft"), "no front matter");
    assert!(document.contains("<w:b/></w:rPr><w:t xml:space=\"preserve\">key</w:t>"), "{document}");
    assert!(document.contains("Kucsko") && document.contains("55"), "the citation is formatted: {document}");
    assert!(document.contains("x &lt; y &amp; that one"), "text arriving in pieces is one run: {document}");
    assert!(
        document.contains(">1.\t<") && document.contains(">2.\t<") && document.contains(">•\t<"),
        "{document}"
    );
    assert!(document.contains(r#"<w:hyperlink r:id="rIdLink1">"#), "{document}");
    assert!(document.contains(r#"<w:pStyle w:val="Code"/>"#) && document.contains("<w:br/>"), "{document}");
    assert!(
        document.contains(">References<") && document.contains(r#"<w:pStyle w:val="Bibliography"/>"#),
        "{document}"
    );
    let relations = read("word/_rels/document.xml.rels");
    assert!(
        relations.contains(r#"Target="https://doi.org/10.1038/nature12373" TargetMode="External""#),
        "{relations}"
    );
    assert!(read("docProps/core.xml").contains("<dc:title>Draft</dc:title>"));
}
