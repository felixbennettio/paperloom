use super::*;
use crate::annotations::AnnotationRect;
use crate::{NewAnnotation, NewItem};

/// Three pages under a two-level page tree: the first two take their size
/// and resources from the node above them.
fn three_pages(path: &Path) {
    let mut doc = Document::with_version("1.7");
    let root = doc.new_object_id();
    let inner = doc.new_object_id();
    let mut page = |parent: ObjectId, label: &str, own_size: bool| {
        let content = doc.add_object(Stream::new(Dictionary::new(), format!("% {label}").into_bytes()));
        let mut dict = Dictionary::new();
        dict.set("Type", "Page");
        dict.set("Parent", Object::Reference(parent));
        dict.set("Contents", Object::Reference(content));
        if own_size {
            dict.set("MediaBox", [0, 0, 300, 400].map(Object::Integer).to_vec());
            dict.set("Resources", Dictionary::new());
        }
        doc.add_object(dict)
    };
    let first = page(inner, "one", false);
    let second = page(inner, "two", false);
    let third = page(root, "three", true);
    let mut node = Dictionary::new();
    node.set("Type", "Pages");
    node.set("Parent", Object::Reference(root));
    node.set("Kids", vec![Object::Reference(first), Object::Reference(second)]);
    node.set("Count", 2_i64);
    node.set("MediaBox", [0, 0, 600, 800].map(Object::Integer).to_vec());
    node.set("Resources", Dictionary::new());
    doc.objects.insert(inner, Object::Dictionary(node));
    let mut tree = Dictionary::new();
    tree.set("Type", "Pages");
    tree.set("Kids", vec![Object::Reference(inner), Object::Reference(third)]);
    tree.set("Count", 3_i64);
    doc.objects.insert(root, Object::Dictionary(tree));
    let mut catalog = Dictionary::new();
    catalog.set("Type", "Catalog");
    catalog.set("Pages", Object::Reference(root));
    let catalog = doc.add_object(catalog);
    doc.trailer.set("Root", Object::Reference(catalog));
    doc.save(path).unwrap();
}

/// The comment in each page's content, in page order.
fn labels(path: &str) -> Vec<String> {
    let doc = Document::load(path).unwrap();
    doc.get_pages()
        .values()
        .map(|&page| {
            // Each stream ends with a line break.
            let content = doc.get_page_content(page);
            String::from_utf8_lossy(&content).trim_end().trim_start_matches("% ").to_string()
        })
        .collect()
}

fn mark(attachment: &str, page: u32, comment: &str) -> NewAnnotation {
    NewAnnotation {
        attachment_id: attachment.into(),
        kind: "highlight".into(),
        color: "#ffd400".into(),
        page_index: page,
        rects: vec![AnnotationRect { x: 50.0, y: 100.0, width: 120.0, height: 12.0 }],
        text: Some(format!("text on {page}")),
        comment: Some(comment.into()),
        strokes: Vec::new(),
        font_size: None,
    }
}

#[test]
fn edits_pages_and_keeps_marks_on_them() {
    let dir = tempfile::tempdir().unwrap();
    let library = PaperLibrary::new(dir.path().join("library.sqlite").display().to_string()).unwrap();
    let item = library.add_item(NewItem { title: "Pages".into(), ..NewItem::default() }).unwrap();
    let source = dir.path().join("pages.pdf");
    three_pages(&source);
    let file = library.attach_file(item.id.clone(), source.display().to_string()).unwrap();
    for page in 0..3 {
        library.add_annotation(mark(&file.id, page, &format!("note {page}"))).unwrap();
    }
    library.set_attachment_text(file.id.clone(), vec!["one".into(), "two".into(), "three".into()]).unwrap();
    assert_eq!(labels(&file.file_path), ["one", "two", "three"]);

    let moved = library.edit_pdf_pages(file.id.clone(), PageEdit::Move { from: 2, to: 0 }).unwrap();
    assert_eq!(labels(&file.file_path), ["three", "one", "two"]);
    assert_ne!(moved.size, 0);
    let doc = Document::load(&file.file_path).unwrap();
    let second = doc.get_pages()[&2];
    let size = doc.get_dictionary(second).unwrap().get(b"MediaBox").unwrap().as_array().unwrap().clone();
    assert_eq!(size[2], Object::Integer(600), "a moved page keeps the size it took from above");
    let pages = |library: &PaperLibrary| -> Vec<(u32, String)> {
        let mut marks: Vec<(u32, String)> = library
            .annotations(file.id.clone())
            .unwrap()
            .into_iter()
            .map(|a| (a.page_index, a.comment.unwrap_or_default()))
            .collect();
        marks.sort();
        marks
    };
    assert_eq!(pages(&library), [(0, "note 2".into()), (1, "note 0".into()), (2, "note 1".into())]);
    let hits = library.search_everything("three".into(), vec!["pdf".into()], 5).unwrap();
    assert_eq!(hits[0].page_index, Some(0), "the page text moved too");

    library.edit_pdf_pages(file.id.clone(), PageEdit::Delete { pages: vec![1] }).unwrap();
    assert_eq!(labels(&file.file_path), ["three", "two"]);
    assert_eq!(pages(&library), [(0, "note 2".into()), (1, "note 1".into())]);

    library.edit_pdf_pages(file.id.clone(), PageEdit::Rotate { pages: vec![0], degrees: -90 }).unwrap();
    let doc = Document::load(&file.file_path).unwrap();
    let first = doc.get_pages()[&1];
    assert_eq!(doc.get_dictionary(first).unwrap().get(b"Rotate").unwrap().as_i64().unwrap(), 270);

    library.edit_pdf_pages(file.id.clone(), PageEdit::InsertBlank { at: 1 }).unwrap();
    assert_eq!(labels(&file.file_path), ["three", "", "two"]);
    assert_eq!(pages(&library), [(0, "note 2".into()), (2, "note 1".into())]);

    for wrong in [
        PageEdit::Delete { pages: vec![0, 1, 2] },
        PageEdit::Rotate { pages: vec![0], degrees: 45 },
        PageEdit::Move { from: 9, to: 0 },
    ] {
        assert!(matches!(
            library.edit_pdf_pages(file.id.clone(), wrong),
            Err(CoreError::InvalidInput { .. })
        ));
    }

    let copy = dir.path().join("annotated.pdf");
    assert_eq!(library.export_annotated_pdf(file.id.clone(), copy.display().to_string()).unwrap(), 2);
    let doc = Document::load(&copy).unwrap();
    let first = doc.get_pages()[&1];
    let annots = doc.get_dictionary(first).unwrap().get(b"Annots").unwrap().as_array().unwrap().clone();
    let highlight = doc.get_dictionary(annots[0].as_reference().unwrap()).unwrap();
    assert_eq!(highlight.get(b"Subtype").unwrap().as_name().unwrap(), b"Highlight");
    assert_eq!(highlight.get(b"QuadPoints").unwrap().as_array().unwrap().len(), 8);
    assert_eq!(labels(&file.file_path).len(), 3, "the library's file stays as it was");
}

#[test]
fn writes_text_strings_for_any_language() {
    assert_eq!(text_string("plain"), Object::string_literal("plain"));
    let Object::String(bytes, _) = text_string("中") else { panic!("a string") };
    assert_eq!(bytes, [0xfe, 0xff, 0x4e, 0x2d]);
    assert_eq!(components("#ff0000")[0], Object::Real(1.0));
}

#[test]
fn writes_text_boxes_shapes_and_arrows() {
    use paperloom_model::{AnnotationId, AttachmentId, Point, Rect};
    let mark = |kind, strokes: Vec<Vec<Point>>| Annotation {
        id: AnnotationId::new(),
        attachment_id: AttachmentId::new(),
        kind,
        color: "#ff0000".into(),
        page_index: 0,
        rects: vec![Rect { x: 10.0, y: 20.0, width: 100.0, height: 30.0 }],
        text: None,
        comment: Some("See §3".into()),
        tags: Vec::new(),
        strokes,
        font_size: Some(12.0),
        date_added: 0,
        date_modified: 0,
    };
    let text = annotation_dict(&mark(AnnotationKind::Text, vec![]), (1, 0)).unwrap();
    assert_eq!(text.get(b"Subtype").unwrap().as_name().unwrap(), b"FreeText");
    assert_eq!(text.get(b"DA").unwrap().as_str().unwrap(), b"/Helv 12.0 Tf 1.000 0.000 0.000 rg");
    assert!(text.get(b"C").is_err(), "a coloured C would fill the box");

    let square = annotation_dict(&mark(AnnotationKind::Square, vec![]), (1, 0)).unwrap();
    assert_eq!(square.get(b"Subtype").unwrap().as_name().unwrap(), b"Square");
    assert!(square.get(b"QuadPoints").is_err());

    let line = vec![vec![Point { x: 10.0, y: 20.0 }, Point { x: 110.0, y: 50.0 }]];
    let arrow = annotation_dict(&mark(AnnotationKind::Arrow, line), (1, 0)).unwrap();
    assert_eq!(arrow.get(b"Subtype").unwrap().as_name().unwrap(), b"Line");
    assert_eq!(arrow.get(b"L").unwrap().as_array().unwrap().len(), 4);
    let ends = arrow.get(b"LE").unwrap().as_array().unwrap();
    assert_eq!(ends[1].as_name().unwrap(), b"OpenArrow");
    assert!(
        annotation_dict(&mark(AnnotationKind::Arrow, vec![]), (1, 0)).is_none(),
        "an arrow needs its line"
    );
}

#[test]
fn writes_stamps_with_their_look() {
    use paperloom_model::{AnnotationId, AttachmentId, Rect};
    let stamp = |word: &str| Annotation {
        id: AnnotationId::new(),
        attachment_id: AttachmentId::new(),
        kind: AnnotationKind::Stamp,
        color: "#e03131".into(),
        page_index: 0,
        rects: vec![Rect { x: 100.0, y: 600.0, width: 140.0, height: 36.0 }],
        text: None,
        comment: Some(word.into()),
        tags: Vec::new(),
        strokes: Vec::new(),
        font_size: None,
        date_added: 0,
        date_modified: 0,
    };
    let approved = annotation_dict(&stamp("Not approved"), (1, 0)).unwrap();
    assert_eq!(approved.get(b"Subtype").unwrap().as_name().unwrap(), b"Stamp");
    assert_eq!(approved.get(b"Name").unwrap().as_name().unwrap(), b"NotApproved");
    let look = stamp_appearance(&stamp("Reviewed (ok)")).unwrap();
    let content = String::from_utf8(look.content.clone()).unwrap();
    assert!(content.contains("(REVIEWED \\(OK\\)) Tj"), "{content}");
    assert_eq!(look.dict.get(b"Subtype").unwrap().as_name().unwrap(), b"Form");
    assert!(stamp_appearance(&stamp("已批准")).is_none(), "beyond the standard fonts");
    assert_eq!(stamp_name("已批准"), "已批准");
}
