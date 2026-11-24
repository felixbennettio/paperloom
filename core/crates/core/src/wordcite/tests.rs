use std::sync::Arc;

use super::*;
use crate::NewItem;
use crate::unzip::tests::deflated_zip;

struct Library {
    library: Arc<PaperLibrary>,
    first: String,
    second: String,
}

fn library() -> Library {
    let library = PaperLibrary::in_memory().unwrap();
    let first = library
        .add_item(NewItem {
            title: "Nanometre-scale thermometry in a living cell".into(),
            authors: vec!["Georg Kucsko".into()],
            container_title: Some("Nature".into()),
            year: Some(2013),
            doi: Some("10.1038/nature12373".into()),
            ..NewItem::default()
        })
        .unwrap()
        .id;
    let second = library
        .add_item(NewItem {
            title: "Radioactivity".into(),
            authors: vec!["Marie Curie".into()],
            container_title: Some("Comptes rendus".into()),
            year: Some(1903),
            ..NewItem::default()
        })
        .unwrap()
        .id;
    Library { library, first, second }
}

fn document(body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}</w:body></w:document>"#
    )
}

fn paragraph(text: &str) -> String {
    format!("<w:p><w:r><w:t>{text}</w:t></w:r></w:p>")
}

fn part(bytes: &[u8], name: &str) -> String {
    String::from_utf8(unzip::file(bytes, name).unwrap()).unwrap()
}

fn run(library: &PaperLibrary, files: &[(&str, &[u8])], style: &str) -> (WordCitations, Vec<u8>) {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.docx");
    let destination = dir.path().join("cited.docx");
    let bytes = deflated_zip(files);
    fs::write(&source, &bytes).unwrap();
    let report = library
        .cite_word_document(source.display().to_string(), destination.display().to_string(), style.into())
        .unwrap();
    assert_eq!(fs::read(&source).unwrap(), bytes, "the original is untouched");
    assert!(!destination.with_extension("docx.partial").exists());
    (report, fs::read(destination).unwrap())
}

fn texts(xml: &str) -> Vec<String> {
    elements(xml, "w:p").into_iter().map(|e| Paragraph::read(xml, e).text).collect()
}

#[test]
fn replaces_three_runs_and_keeps_the_first_runs_formatting() {
    let Library { library, .. } = library();
    let body = r#"<w:p w:rsidR="1234"><w:r><w:rPr><w:b/><w:color w:val="123456"/></w:rPr><w:t>Before [@kuc</w:t></w:r><w:r><w:rPr><w:i/></w:rPr><w:t>sko2013</w:t></w:r><w:r><w:rPr><w:u w:val="single"/></w:rPr><w:t>nanometre]</w:t></w:r></w:p><w:sectPr/>"#;
    let xml = document(body);
    let (report, bytes) = run(&library, &[(PARTS[0], xml.as_bytes())], "ieee");
    assert_eq!(report, WordCitations { citations: 1, references: 1, missing: Vec::new() });
    let output = part(&bytes, PARTS[0]);
    let expected =
        body.replace("Before [@kuc", "Before [1]").replace("sko2013", "").replace("nanometre]", "");
    assert!(output.contains(expected.strip_suffix("<w:sectPr/>").unwrap()), "{output}");
    assert_eq!(texts(&output)[0], "Before [1]");
    assert!(output.contains("<w:rPr><w:b/></w:rPr><w:t xml:space=\"preserve\">References</w:t>"));
    assert!(output.contains(r#"<w:ind w:left="480" w:hanging="480"/>"#));
}

#[test]
fn numbers_body_footnotes_and_endnotes_in_reading_order_and_resolves_all_key_forms() {
    let Library { library, first, second } = library();
    let xml = document(&paragraph("[@KUCSKO2013NANOMETRE]"));
    let footnotes = format!(
        "<w:footnotes><w:footnote w:id=\"1\">{}{}</w:footnote></w:footnotes>",
        paragraph(&format!("[@{second}]")),
        paragraph("[@10.1038/NATURE12373]")
    );
    let endnotes = format!(
        "<w:endnotes><w:endnote w:id=\"1\">{}</w:endnote></w:endnotes>",
        paragraph(&format!("[@{first}]"))
    );
    for style in ["ieee", "gb-7714-2015-numeric"] {
        // ZIP order differs deliberately from reading order.
        let (report, bytes) = run(
            &library,
            &[(PARTS[2], endnotes.as_bytes()), (PARTS[1], footnotes.as_bytes()), (PARTS[0], xml.as_bytes())],
            style,
        );
        assert_eq!(report.citations, 4);
        assert_eq!(report.references, 2);
        assert!(report.missing.is_empty());
        assert_eq!(texts(&part(&bytes, PARTS[0]))[0], "[1]");
        let footnote_text = texts(&part(&bytes, PARTS[1]));
        assert_eq!(footnote_text[0], "[2]");
        assert_eq!(footnote_text[1], "[1]");
        assert_eq!(texts(&part(&bytes, PARTS[2])), ["[1]"]);
        let body = texts(&part(&bytes, PARTS[0]));
        assert_eq!(body[1], if style.starts_with("gb-") { "参考文献" } else { "References" });
        assert!(body[2].contains("Nanometre") && body[3].contains("Radioactivity"), "{body:?}");
    }
}

#[test]
fn doi_locators_and_groups_format_like_notes() {
    let Library { library, second, .. } = library();
    let raw = format!("[@10.1038/nature12373, p. 3] [see @doi:10.1038/NATURE12373; @{second}]");
    let expected = library.cite_note(&raw, "apa", Format::Text);
    let xml = document(&paragraph(&raw));
    let (report, bytes) = run(&library, &[(PARTS[0], xml.as_bytes())], "apa");
    assert_eq!((report.citations, report.references), (2, 2));
    let citations = paperloom_vault::plain_citations(&raw);
    let text = texts(&part(&bytes, PARTS[0]));
    assert_eq!(
        text[0],
        format!("{} {}", expected.citations[&citations[0].start], expected.citations[&citations[1].start])
    );
    assert!(text[0].contains("Kucsko") && text[0].contains('3'));
    assert_eq!(&text[2..], expected.entries.as_slice());
}

#[test]
fn plain_scanning_ignores_markdown_rules_and_reports_byte_offsets() {
    let text = "---\n    [@a]\n`[@b]` %%[@c]%%\n---\n[see @a; @b, p. 3]";
    let citations = paperloom_vault::plain_citations(text);
    assert_eq!(citations.len(), 4);
    assert_eq!(citations.iter().map(|c| c.line).collect::<Vec<_>>(), [1, 2, 2, 4]);
    assert_eq!(
        citations.iter().map(|c| &text[c.start..c.end]).collect::<Vec<_>>(),
        ["[@a]", "[@b]", "[@c]", "[see @a; @b, p. 3]"]
    );
    assert_eq!(citations[3].works[1].page.as_deref(), Some("3"));
}

#[test]
fn missing_works_leave_the_entire_citation_as_written_once_in_order() {
    let Library { library, .. } = library();
    let raw = "[@Unknown] [see @kucsko2013nanometre; @Unknown] [@Other] [@Unknown] [@curie1903radioactivity]";
    let xml = document(&paragraph(raw));
    let (report, bytes) = run(&library, &[(PARTS[0], xml.as_bytes())], "ieee");
    assert_eq!(report.citations, 1);
    assert_eq!(report.references, 1, "the known half of an unresolved group is not in the list");
    assert_eq!(report.missing, ["Unknown", "Other"]);
    assert_eq!(texts(&part(&bytes, PARTS[0]))[0], raw.replace("[@curie1903radioactivity]", "[1]"));
}

#[test]
fn placeholders_inherit_paragraph_and_first_run_properties() {
    let Library { library, .. } = library();
    for placeholder in [" {BiBlIoGrApHy} ", "{REFERENCES}", "{参考文献}"] {
        let placeholder = format!(
            r#"<w:p w:rsidR="1234"><w:pPr><w:keepNext/><w:ind w:left="720"/></w:pPr><w:r><w:rPr><w:i/><w:sz w:val="20"/></w:rPr><w:t>{placeholder}</w:t></w:r><w:r><w:rPr><w:b/></w:rPr><w:t></w:t></w:r></w:p>"#
        );
        let xml = document(&format!(
            "{}{placeholder}",
            paragraph("[see @kucsko2013nanometre; @curie1903radioactivity]")
        ));
        let (report, bytes) = run(&library, &[(PARTS[0], xml.as_bytes())], "ieee");
        let output = part(&bytes, PARTS[0]);
        assert_eq!((report.citations, report.references), (1, 2));
        assert_eq!(texts(&output).len(), 3);
        assert_eq!(output.matches("<w:p><w:pPr><w:keepNext/><w:ind w:left=\"720\"/></w:pPr><w:r><w:rPr><w:i/><w:sz w:val=\"20\"/></w:rPr>").count(), 2);
        assert!(
            !output.contains("{BiBlIoGrApHy}")
                && !output.contains("{REFERENCES}")
                && !output.contains("{参考文献}")
        );
        assert!(!output.contains(">References<"));
    }
}

#[test]
fn appends_before_the_body_section_but_after_paragraph_section_properties() {
    let Library { library, .. } = library();
    for final_section in ["<w:sectPr><w:pgSz w:w=\"11906\"/></w:sectPr>", ""] {
        let nested = "<w:p><w:pPr><w:sectPr><w:type w:val=\"continuous\"/></w:sectPr></w:pPr><w:r><w:t>Last paragraph</w:t></w:r></w:p>";
        let xml = document(&format!("{}{nested}{final_section}", paragraph("[@kucsko2013nanometre]")));
        let (_, bytes) = run(&library, &[(PARTS[0], xml.as_bytes())], "ieee");
        let output = part(&bytes, PARTS[0]);
        assert!(output.find(nested).unwrap() < output.find(">References<").unwrap());
        let reference_end = output.rfind("</w:p>").unwrap() + "</w:p>".len();
        assert_eq!(&output[reference_end..], format!("{final_section}</w:body></w:document>"));
    }
}

#[test]
fn decodes_entities_cites_indented_plain_text_and_preserves_whitespace() {
    let Library { library, .. } = library();
    let xml = document(concat!(
        "<w:p><w:r><w:t xml:space='default'>    &amp; &lt; &#8212; &#x1F600; `[@kucsko2013nanometre]` </w:t></w:r>",
        "<w:r><w:t> &amp; unchanged &#8212;</w:t></w:r></w:p>",
        "<w:p><w:r><w:t>%%[@kucsko2013nanometre]%%</w:t></w:r></w:p>",
        "<w:p><w:r><w:t>[@kucsko2013nanometre]</w:t></w:r><w:r><w:t> </w:t></w:r></w:p>"
    ));
    let (report, bytes) = run(&library, &[(PARTS[0], xml.as_bytes())], "ieee");
    assert_eq!(report.citations, 3);
    let output = part(&bytes, PARTS[0]);
    assert_eq!(texts(&output)[0], "    & < — 😀 `[1]`  & unchanged —");
    assert!(output.contains("<w:t xml:space='preserve'>    &amp; &lt; — 😀 `[1]` </w:t>"), "{output}");
    assert!(output.contains("<w:t> &amp; unchanged &#8212;</w:t>"), "unchanged nodes keep their bytes");
    // A citation ends in the following node; its remaining suffix needs preservation.
    let split =
        document("<w:p><w:r><w:t>[@kucsko2013</w:t></w:r><w:r><w:t>nanometre] after </w:t></w:r></w:p>");
    let (_, bytes) = run(&library, &[(PARTS[0], split.as_bytes())], "ieee");
    assert!(part(&bytes, PARTS[0]).contains("<w:t xml:space=\"preserve\"> after </w:t>"));
}

#[test]
fn only_visible_text_in_innermost_paragraphs_is_cited() {
    let Library { library, .. } = library();
    let xml = document(concat!(
        "<w:p><w:r><w:instrText>[@UnknownField]</w:instrText><w:delText>[@UnknownDeletion]</w:delText>",
        "<w:t>One</w:t><w:tab/><w:t>Two</w:t><w:br/><w:cr/><w:t>[@kucsko2013nanometre]</w:t></w:r></w:p>",
        "<w:p><w:r><w:drawing><w:txbxContent><w:p><w:r><w:t>[@kucsko2013nanometre]</w:t></w:r></w:p></w:txbxContent></w:drawing></w:r></w:p>",
        "<w:p><w:r><w:t>[@kucsko2013</w:t></w:r></w:p><w:p><w:r><w:t>nanometre]</w:t></w:r></w:p>"
    ));
    let (report, bytes) = run(&library, &[(PARTS[0], xml.as_bytes())], "ieee");
    assert_eq!(report.citations, 2, "a nested paragraph is not cited twice");
    assert!(report.missing.is_empty());
    let output = part(&bytes, PARTS[0]);
    assert_eq!(texts(&output)[0], "One\tTwo\n\n[1]");
    assert!(
        output
            .contains("<w:instrText>[@UnknownField]</w:instrText><w:delText>[@UnknownDeletion]</w:delText>")
    );
    assert!(output.contains("<w:t>[@kucsko2013</w:t>") && output.contains("<w:t>nanometre]</w:t>"));
}

#[test]
fn no_resolved_citations_leave_the_placeholder_and_all_parts_unchanged() {
    let Library { library, .. } = library();
    let xml = document(&format!("{}{}", paragraph("[@Unknown]"), paragraph("{references}")));
    let (report, bytes) = run(&library, &[(PARTS[0], xml.as_bytes())], "ieee");
    assert_eq!(report, WordCitations { citations: 0, references: 0, missing: vec!["Unknown".into()] });
    assert_eq!(part(&bytes, PARTS[0]), xml);
}

#[test]
fn allows_destination_equal_to_source_and_copies_other_entries_in_order() {
    let Library { library, first, .. } = library();
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("paper.docx");
    let xml = document(&paragraph(&format!("[@{first}]")));
    let files: [(&str, &[u8]); 4] = [
        ("[Content_Types].xml", b"<Types/>"),
        ("word/media/image1.png", b"\x89PNG\r\n\x1a\n\x00\xff"),
        (PARTS[0], xml.as_bytes()),
        ("word/styles.xml", b"<styles><!-- keep exactly --></styles>"),
    ];
    fs::write(&source, deflated_zip(&files)).unwrap();
    let path = source.display().to_string();
    let report = library.cite_word_document(path.clone(), path, "ieee".into()).unwrap();
    assert_eq!((report.citations, report.references), (1, 1));
    assert!(!source.with_extension("docx.partial").exists());
    let bytes = fs::read(&source).unwrap();
    let entries = unzip::entries(&bytes).unwrap();
    assert_eq!(
        entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        files.iter().map(|(n, _)| *n).collect::<Vec<_>>()
    );
    for (entry, (name, content)) in entries.iter().zip(files) {
        if name != PARTS[0] {
            assert_eq!(entry.contents(XML_LIMIT).unwrap(), content);
        }
    }
    assert_eq!(texts(&part(&bytes, PARTS[0]))[0], "[1]");
}

#[test]
fn rejects_non_docx_and_keeps_the_destination_on_failure() {
    let Library { library, .. } = library();
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.docx");
    let destination = dir.path().join("existing.docx");
    fs::write(&destination, b"existing").unwrap();
    for bytes in [b"not a ZIP".to_vec(), deflated_zip(&[("other.xml", b"<x/>")])] {
        fs::write(&source, bytes).unwrap();
        let error = library
            .cite_word_document(
                source.display().to_string(),
                destination.display().to_string(),
                "ieee".into(),
            )
            .unwrap_err();
        assert!(
            matches!(error, CoreError::InvalidInput { message } if message == "This is not a Word document (.docx).")
        );
        assert_eq!(fs::read(&destination).unwrap(), b"existing");
        assert!(!destination.with_extension("docx.partial").exists());
    }
}

#[test]
fn removes_partial_on_rename_and_unpack_failures() {
    let Library { library, .. } = library();
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.docx");
    let destination = dir.path().join("directory.docx");
    fs::create_dir(&destination).unwrap();
    let xml = document(&paragraph("[@kucsko2013nanometre]"));
    fs::write(&source, deflated_zip(&[(PARTS[0], xml.as_bytes())])).unwrap();
    let result = library.cite_word_document(
        source.display().to_string(),
        destination.display().to_string(),
        "ieee".into(),
    );
    assert!(matches!(result, Err(CoreError::Storage { .. })));
    assert!(destination.is_dir());
    assert!(!destination.with_extension("docx.partial").exists());

    let mut bytes = deflated_zip(&[(PARTS[0], xml.as_bytes()), ("word/media/bad.bin", b"broken")]);
    // An unsupported compression method in the second central directory entry.
    let central =
        bytes.windows(4).enumerate().filter(|(_, w)| *w == [0x50, 0x4b, 0x01, 0x02]).nth(1).unwrap().0;
    bytes[central + 10..central + 12].copy_from_slice(&99u16.to_le_bytes());
    fs::write(&source, bytes).unwrap();
    let destination = dir.path().join("unreadable.docx");
    assert!(matches!(
        library.cite_word_document(
            source.display().to_string(),
            destination.display().to_string(),
            "ieee".into()
        ),
        Err(CoreError::InvalidInput { .. })
    ));
    assert!(!destination.exists());
    assert!(!destination.with_extension("docx.partial").exists());
}

fn odt_document(body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0"><office:body><office:text>{body}</office:text></office:body></office:document-content>"#
    )
}

fn run_odt(library: &PaperLibrary, files: &[(&str, &[u8])], style: &str) -> (WordCitations, Vec<u8>) {
    let dir = tempfile::tempdir().unwrap();
    // The source extension deliberately does not identify the format.
    let source = dir.path().join("source.bin");
    let destination = dir.path().join("cited.odt");
    let bytes = deflated_zip(files);
    fs::write(&source, &bytes).unwrap();
    let report = library
        .cite_word_document(source.display().to_string(), destination.display().to_string(), style.into())
        .unwrap();
    assert_eq!(fs::read(source).unwrap(), bytes, "the original is untouched");
    assert!(!destination.with_extension("odt.partial").exists());
    (report, fs::read(destination).unwrap())
}

#[test]
fn odt_replaces_split_spans_and_keeps_the_first_spans_formatting() {
    let Library { library, .. } = library();
    let body = concat!(
        r#"<text:p text:style-name="Body">Before &amp; <text:span text:style-name="Bold">[@kucsko2013</text:span>"#,
        r#"<text:span text:style-name="Italic">nanometre] after &#8212;</text:span></text:p>"#,
        "<text:p>{references}</text:p>"
    );
    let xml = odt_document(body);
    let expected = library.cite_note("[@kucsko2013nanometre]", "ieee", Format::Text);
    let (report, bytes) =
        run_odt(&library, &[("mimetype", odt::MIMETYPE), ("content.xml", xml.as_bytes())], "ieee");
    assert_eq!(report, WordCitations { citations: 1, references: 1, missing: Vec::new() });
    let references: String =
        expected.entries.iter().map(|e| format!("<text:p>{}</text:p>", escape(e))).collect();
    assert_eq!(
        part(&bytes, "content.xml"),
        odt_document(
            &body
                .replace("[@kucsko2013", "[1]")
                .replace("nanometre] after &#8212;", " after —")
                .replace("<text:p>{references}</text:p>", &references)
        )
    );
}

#[test]
fn odt_models_spaces_tabs_breaks_entities_and_inline_markup() {
    let Library { library, .. } = library();
    let prefix = concat!(
        "&amp; &#x1F600;<text:s text:c='3'/><text:a>Link</text:a><text:tab/>Tab<text:line-break/>",
        "<text:bookmark-start text:name='mark'/><text:soft-page-break/>"
    );
    let body = format!(
        "<text:h text:outline-level='2'>{prefix}[@kucsko2013&#110;anometre]</text:h>\
         <text:p>[@kucsko2013nanometre,<text:s/>p.<text:tab/>3<text:line-break/>]</text:p>"
    );
    let xml = odt_document(&body);
    let paragraphs = odt::paragraphs(&xml).unwrap();
    assert_eq!(paragraphs[0].text, "& 😀   Link\tTab\n[@kucsko2013nanometre]");
    assert_eq!(paragraphs[1].text, "[@kucsko2013nanometre, p.\t3\n]");
    let expected = library.cite_note("[@kucsko2013nanometre, p. 3]", "ieee", Format::Text);
    let (report, bytes) =
        run_odt(&library, &[("mimetype", odt::MIMETYPE), ("content.xml", xml.as_bytes())], "ieee");
    assert_eq!((report.citations, report.references), (2, 1));
    assert!(report.missing.is_empty());
    let output = part(&bytes, "content.xml");
    assert!(output.contains(&format!("<text:h text:outline-level='2'>{prefix}[1]</text:h>")), "{output}");
    assert!(output.contains(&format!("<text:p>{}</text:p>", escape(&expected.citations[&0]))), "{output}");
}

#[test]
fn odt_numbers_notes_at_their_anchors_and_skips_the_note_label() {
    let Library { library, first, second } = library();
    let third = library
        .add_item(NewItem {
            title: "Third work".into(),
            authors: vec!["Ada Lovelace".into()],
            year: Some(1843),
            ..NewItem::default()
        })
        .unwrap()
        .id;
    for style in ["ieee", "gb-7714-2015-numeric"] {
        for class in ["footnote", "endnote"] {
            let body = format!(
                "<text:p>Before [@{first}]<text:note text:note-class='{class}' text:id='note1'>\
                 <text:note-citation>[@UnknownNoteLabel]</text:note-citation><text:note-body>\
                 <text:p>Note [@{second}]</text:p><text:p>Again [@{first}]</text:p>\
                 </text:note-body></text:note>After [@{third}]</text:p><text:p>{{references}}</text:p>"
            );
            let xml = odt_document(&body);
            let paragraphs = odt::paragraphs(&xml).unwrap();
            assert_eq!(paragraphs[0].text, format!("Before [@{first}]After [@{third}]"));
            assert_eq!(paragraphs[1].text, format!("Note [@{second}]"));
            assert_eq!(paragraphs[2].text, format!("Again [@{first}]"));
            let expected = library.cite_note(
                &format!("[@{first}] [@{second}] [@{first}] [@{third}]"),
                style,
                Format::Text,
            );
            let (report, bytes) =
                run_odt(&library, &[("mimetype", odt::MIMETYPE), ("content.xml", xml.as_bytes())], style);
            assert_eq!(report, WordCitations { citations: 4, references: 3, missing: Vec::new() });
            let references: String =
                expected.entries.iter().map(|e| format!("<text:p>{}</text:p>", escape(e))).collect();
            assert_eq!(
                part(&bytes, "content.xml"),
                odt_document(
                    &body
                        .replace(&format!("[@{first}]"), "[1]")
                        .replace(&format!("[@{second}]"), "[2]")
                        .replace(&format!("[@{third}]"), "[3]")
                        .replace("<text:p>{references}</text:p>", &references)
                )
            );
        }
    }
}

#[test]
fn odt_placeholders_keep_their_element_name_and_attributes() {
    let Library { library, .. } = library();
    let raw = "[see @kucsko2013nanometre; @curie1903radioactivity]";
    let expected = library.cite_note(raw, "ieee", Format::Text);
    for placeholder in [" {BiBlIoGrApHy} ", "{REFERENCES}", "{参考文献}"] {
        for name in ["text:p", "text:h"] {
            let opening = format!("<{name} text:style-name='Bibliography' xml:id='refs'>");
            let closing = format!("</{name}>");
            let body = format!("<text:p>{raw}</text:p>{opening}{placeholder}{closing}");
            let xml = odt_document(&body);
            let (report, bytes) =
                run_odt(&library, &[("mimetype", odt::MIMETYPE), ("content.xml", xml.as_bytes())], "ieee");
            assert_eq!((report.citations, report.references), (1, 2));
            let references: String =
                expected.entries.iter().map(|e| format!("{opening}{}{closing}", escape(e))).collect();
            assert_eq!(
                part(&bytes, "content.xml"),
                odt_document(&format!("<text:p>{}</text:p>{references}", escape(&expected.citations[&0])))
            );
        }
    }
}

#[test]
fn odt_appends_the_heading_and_escaped_entries_inside_office_text() {
    let Library { library, .. } = library();
    let first = library
        .add_item(NewItem {
            title: "Cells & <light>".into(),
            authors: vec!["Ada Lovelace".into()],
            year: Some(1843),
            ..NewItem::default()
        })
        .unwrap()
        .id;
    let raw = format!("[@{first}]");
    for style in ["ieee", "gb-7714-2015-numeric"] {
        let xml = odt_document(&format!("<text:p>{raw}</text:p><text:p>Last</text:p>"));
        let expected = library.cite_note(&raw, style, Format::Text);
        let (report, bytes) =
            run_odt(&library, &[("mimetype", odt::MIMETYPE), ("content.xml", xml.as_bytes())], style);
        assert_eq!((report.citations, report.references), (1, 1));
        let heading = if style.starts_with("gb-") { "参考文献" } else { "References" };
        let references: String =
            expected.entries.iter().map(|e| format!("<text:p>{}</text:p>", escape(e))).collect();
        assert_eq!(
            part(&bytes, "content.xml"),
            odt_document(&format!(
                r#"<text:p>[1]</text:p><text:p>Last</text:p><text:h text:outline-level="1">{heading}</text:h>{references}"#
            ))
        );
        assert!(references.contains("&amp;") && references.contains("&lt;") && references.contains("&gt;"));
    }
}

#[test]
fn odt_preserves_package_payloads_and_writes_mimetype_first_and_stored() {
    let Library { library, .. } = library();
    let xml = odt_document("<text:p>[@kucsko2013nanometre]</text:p>");
    let files: [(&str, &[u8]); 5] = [
        ("mimetype", odt::MIMETYPE),
        ("styles.xml", b"<styles><text:p>[@UnknownHeader]</text:p></styles>"),
        ("Pictures/image.png", b"\x89PNG\r\n\x1a\n\x00\xff"),
        ("content.xml", xml.as_bytes()),
        ("META-INF/manifest.xml", b"<manifest><!-- keep exactly --></manifest>"),
    ];
    let (report, bytes) = run_odt(&library, &files, "ieee");
    assert!(report.missing.is_empty());
    let entries = unzip::entries(&bytes).unwrap();
    assert_eq!(
        entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        files.iter().map(|(name, _)| *name).collect::<Vec<_>>()
    );
    assert_eq!(&bytes[..4], b"PK\x03\x04");
    assert_eq!(&bytes[8..10], &[0, 0], "mimetype uses stored compression");
    assert_eq!(&bytes[26..30], &[8, 0, 0, 0], "mimetype has no extra fields");
    assert_eq!(&bytes[30..38], b"mimetype");
    assert_eq!(&bytes[38..38 + odt::MIMETYPE.len()], odt::MIMETYPE);
    for (entry, (name, content)) in entries.iter().zip(files) {
        if name != "content.xml" {
            assert_eq!(entry.contents(XML_LIMIT).unwrap(), content);
        }
    }
}

#[test]
fn odt_leaves_annotations_deletions_and_fields_as_written() {
    let Library { library, .. } = library();
    let ignored = concat!(
        "<office:annotation><text:p>[@UnknownAnnotation] [@kucsko2013nanometre]</text:p></office:annotation>",
        "<text:tracked-changes><text:changed-region><text:deletion><text:p>[@UnknownTracked] [@curie1903radioactivity]</text:p>",
        "</text:deletion></text:changed-region></text:tracked-changes>",
        "<text:deletion><text:p>[@UnknownDeletion]</text:p></text:deletion>",
        "<text:sequence>[@UnknownSequence]</text:sequence><text:page-number>[@UnknownField]</text:page-number>"
    );
    let body = format!("<text:p>{ignored}[@kucsko2013nanometre]</text:p>");
    let xml = odt_document(&body);
    let paragraphs = odt::paragraphs(&xml).unwrap();
    assert_eq!(paragraphs.len(), 1);
    assert_eq!(paragraphs[0].text, "[@kucsko2013nanometre]");
    let (report, bytes) =
        run_odt(&library, &[("mimetype", odt::MIMETYPE), ("content.xml", xml.as_bytes())], "ieee");
    assert_eq!(report, WordCitations { citations: 1, references: 1, missing: Vec::new() });
    assert!(part(&bytes, "content.xml").contains(&format!("<text:p>{ignored}[1]</text:p>")));
}

#[test]
fn odt_missing_keys_and_unresolved_placeholders_follow_word_behavior() {
    let Library { library, .. } = library();
    for raw in [
        "[@Unknown] [see @kucsko2013nanometre; @Unknown] [@Other] [@Unknown]",
        "[@Unknown] [see @kucsko2013nanometre; @Unknown] [@Other] [@curie1903radioactivity]",
    ] {
        let xml = odt_document(&format!("<text:p>{raw}</text:p><text:p>{{references}}</text:p>"));
        let (report, bytes) =
            run_odt(&library, &[("mimetype", odt::MIMETYPE), ("content.xml", xml.as_bytes())], "ieee");
        assert_eq!(report.missing, ["Unknown", "Other"]);
        let output = part(&bytes, "content.xml");
        if raw.contains("curie") {
            assert_eq!((report.citations, report.references), (1, 1));
            assert!(
                output.contains(&format!(
                    "<text:p>{}</text:p>",
                    raw.replace("[@curie1903radioactivity]", "[1]")
                ))
            );
            assert!(!output.contains("Nanometre"), "an unresolved group's known work is excluded");
        } else {
            assert_eq!((report.citations, report.references), (0, 0));
            assert_eq!(output, xml);
        }
    }
}

#[test]
fn odt_detection_falls_back_to_office_text_and_prefers_odt_over_word() {
    let Library { library, .. } = library();
    let xml = odt_document("<text:p>[@kucsko2013nanometre]</text:p>");
    let word = document(&paragraph("[@UnknownWord]"));
    for files in [
        vec![("content.xml", xml.as_bytes())],
        vec![("mimetype", b"wrong".as_slice()), ("content.xml", xml.as_bytes())],
        vec![("mimetype", odt::MIMETYPE), (PARTS[0], word.as_bytes()), ("content.xml", xml.as_bytes())],
    ] {
        let (report, bytes) = run_odt(&library, &files, "ieee");
        assert_eq!(report, WordCitations { citations: 1, references: 1, missing: Vec::new() });
        assert!(part(&bytes, "content.xml").contains("<text:p>[1]</text:p>"));
        if files.iter().any(|(name, _)| *name == PARTS[0]) {
            assert_eq!(part(&bytes, PARTS[0]), word);
        }
    }
}

#[test]
fn odt_failures_preserve_the_destination_and_remove_the_partial() {
    let Library { library, .. } = library();
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.odt");
    let destination = dir.path().join("existing.odt");
    fs::write(&destination, b"existing").unwrap();
    for files in [vec![("other.xml", b"<x/>".as_slice())], vec![("mimetype", odt::MIMETYPE)]] {
        fs::write(&source, deflated_zip(&files)).unwrap();
        assert!(matches!(
            library.cite_word_document(
                source.display().to_string(),
                destination.display().to_string(),
                "ieee".into()
            ),
            Err(CoreError::InvalidInput { .. })
        ));
        assert_eq!(fs::read(&destination).unwrap(), b"existing");
        assert!(!destination.with_extension("odt.partial").exists());
    }
    let xml = odt_document("<text:p>[@kucsko2013nanometre]</text:p>");
    fs::write(&source, deflated_zip(&[("mimetype", odt::MIMETYPE), ("content.xml", xml.as_bytes())]))
        .unwrap();
    let destination = dir.path().join("directory.odt");
    fs::create_dir(&destination).unwrap();
    assert!(matches!(
        library.cite_word_document(
            source.display().to_string(),
            destination.display().to_string(),
            "ieee".into()
        ),
        Err(CoreError::Storage { .. })
    ));
    assert!(destination.is_dir());
    assert!(!destination.with_extension("odt.partial").exists());
}

/// A Word and an OpenDocument file as a person might write them, and their
/// copies cited in GB/T 7714, written where `WORDCITE_SAMPLES` says: CI opens
/// the copies with LibreOffice to see that office software reads them.
#[test]
fn writes_office_samples_when_asked() {
    let Ok(dir) = std::env::var("WORDCITE_SAMPLES") else { return };
    let dir = Path::new(&dir);
    fs::create_dir_all(dir).unwrap();
    let Library { library, .. } = library();
    let body = [
        paragraph("Temperature can be measured inside living cells [@kucsko2013nanometre]."),
        paragraph("The first measurements of radioactivity [@curie1903radioactivity, p. 3]."),
        paragraph("{bibliography}"),
    ]
    .concat();
    let document = document(&body);
    let types = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#;
    let rels = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?><Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#;
    let docx = deflated_zip(&[
        ("[Content_Types].xml", types.as_bytes()),
        ("_rels/.rels", rels.as_bytes()),
        ("word/document.xml", document.as_bytes()),
    ]);
    let content = concat!(
        r#"<?xml version="1.0" encoding="UTF-8"?><office:document-content xmlns:office="urn:oasis:names:tc:opendocument:xmlns:office:1.0" xmlns:text="urn:oasis:names:tc:opendocument:xmlns:text:1.0" office:version="1.3"><office:body><office:text>"#,
        "<text:p>Temperature can be measured inside living cells [@kucsko2013nanometre].</text:p>",
        "<text:p>The first measurements of radioactivity<text:note text:note-class=\"footnote\" text:id=\"n1\"><text:note-citation>1</text:note-citation><text:note-body><text:p>See [@curie1903radioactivity, p. 3].</text:p></text:note-body></text:note>.</text:p>",
        "<text:p>{参考文献}</text:p></office:text></office:body></office:document-content>"
    );
    let manifest = r#"<?xml version="1.0" encoding="UTF-8"?><manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0" manifest:version="1.3"><manifest:file-entry manifest:full-path="/" manifest:media-type="application/vnd.oasis.opendocument.text"/><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/></manifest:manifest>"#;
    let odt = deflated_zip(&[
        ("mimetype", odt::MIMETYPE),
        ("META-INF/manifest.xml", manifest.as_bytes()),
        ("content.xml", content.as_bytes()),
    ]);
    for (name, bytes) in [("paper.docx", docx), ("paper.odt", odt)] {
        let source = dir.join(name);
        fs::write(&source, bytes).unwrap();
        let cited = dir.join(format!("cited-{name}"));
        let report = library
            .cite_word_document(
                source.display().to_string(),
                cited.display().to_string(),
                "gb-7714-2015-numeric".into(),
            )
            .unwrap();
        assert_eq!((report.citations, report.references), (2, 2), "{name}");
    }
}
