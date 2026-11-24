use super::*;
use crate::{NewItem, annotations::AnnotationRect};

fn line(text: &str) -> RecognizedLine {
    RecognizedLine { text: text.into(), x: 0.1, y: 0.2, width: 0.5, height: 0.1 }
}

fn recognized(page_index: u32, texts: &[&str]) -> RecognizedPage {
    RecognizedPage { page_index, lines: texts.iter().map(|text| line(text)).collect() }
}

/// Shared indirect resources and inherited size/rotation, an indirect
/// content array, a native text page, and an offset crop on an empty page.
fn fixture() -> Document {
    let mut doc = Document::with_version("1.7");
    let root = doc.new_object_id();
    let mut font = Dictionary::new();
    font.set("Type", "Font");
    font.set("Subtype", "Type1");
    font.set("BaseFont", "Helvetica");
    font.set("Encoding", "WinAnsiEncoding");
    let font = doc.add_object(font);
    let mut fonts = Dictionary::new();
    fonts.set("Existing", font);
    fonts.set("PaperloomText0", font);
    let fonts = doc.add_object(fonts);
    let mut image = Dictionary::new();
    image.set("Type", "XObject");
    image.set("Subtype", "Image");
    image.set("Width", 1_i64);
    image.set("Height", 1_i64);
    image.set("ColorSpace", "DeviceRGB");
    image.set("BitsPerComponent", 8_i64);
    let image = doc.add_object(Stream::new(image, vec![255; 3]));
    let mut xobjects = Dictionary::new();
    xobjects.set("Im1", image);
    let mut resources = Dictionary::new();
    resources.set("Font", fonts);
    resources.set("XObject", xobjects);
    let resources = doc.add_object(resources);
    let mut ids = Vec::new();
    for index in 0..4 {
        let mut page = Dictionary::new();
        page.set("Type", "Page");
        page.set("Parent", root);
        if index != 2 {
            page.set("Rotate", 0_i64);
        }
        if index == 3 {
            let crop = doc.add_object(vec![40.into(), 60.into(), 440.into(), 660.into()]);
            page.set("CropBox", crop);
        } else {
            let content = match index {
                0 => "q 600 0 0 800 0 0 cm /Im1 Do Q\n",
                1 => {
                    "BT /Existing 12 Tf 1 0 0 1 20 700 Tm (This page already has enough alphabetic text.) Tj ET\n"
                }
                _ => "",
            };
            let stream = doc.add_object(Stream::new(Dictionary::new(), content.as_bytes().to_vec()));
            if index == 0 {
                // A second stream to keep (not a comment: lopdf's content
                // parser stops at one, and its text extraction with it).
                let tail = doc.add_object(Stream::new(Dictionary::new(), b"q Q\n".to_vec()));
                let array = doc.add_object(vec![Object::Reference(stream), Object::Reference(tail)]);
                page.set("Contents", array);
            } else {
                page.set("Contents", stream);
            }
        }
        ids.push(doc.add_object(page));
    }
    let size = doc.add_object(vec![0.into(), 0.into(), 600.into(), 800.into()]);
    let mut tree = Dictionary::new();
    tree.set("Type", "Pages");
    tree.set("Kids", ids.into_iter().map(Object::Reference).collect::<Vec<_>>());
    tree.set("Count", 4_i64);
    tree.set("MediaBox", size);
    tree.set("Rotate", 90_i64);
    tree.set("Resources", resources);
    doc.objects.insert(root, Object::Dictionary(tree));
    let mut catalog = Dictionary::new();
    catalog.set("Type", "Catalog");
    catalog.set("Pages", root);
    let catalog = doc.add_object(catalog);
    doc.trailer.set("Root", catalog);
    doc
}

fn last_content(doc: &Document, page: ObjectId) -> Content<Vec<Operation>> {
    let streams = doc.get_page_contents(page);
    let stream = doc.get_object(*streams.last().unwrap()).unwrap().as_stream().unwrap();
    Content::decode(&stream.get_plain_content().unwrap()).unwrap()
}

fn values(content: &Content<Vec<Operation>>, operator: &str) -> Vec<f64> {
    content
        .operations
        .iter()
        .find(|op| op.operator == operator)
        .unwrap()
        .operands
        .iter()
        .map(|n| number(n).unwrap())
        .collect()
}

fn font_id(doc: &Document, page: ObjectId) -> ObjectId {
    let resources = doc.get_dictionary(page).unwrap().get(b"Resources").unwrap().as_dict().unwrap();
    let fonts = resources.get(b"Font").unwrap().as_dict().unwrap();
    assert!(fonts.has(b"Existing"));
    assert!(fonts.has(b"PaperloomText0"));
    fonts.get(b"PaperloomText1").unwrap().as_reference().unwrap()
}

#[test]
fn appends_invisible_unicode_and_preserves_existing_content() {
    let mut doc = fixture();
    let ids = doc.get_pages();
    let native = doc.get_object(ids[&2]).unwrap().clone();
    let native_content = doc.get_page_content(ids[&2]);
    let original: Vec<_> = doc
        .get_page_contents(ids[&1])
        .into_iter()
        .map(|id| (id, doc.get_object(id).unwrap().clone()))
        .collect();
    let inherited_resources = inherited(&doc, ids[&1], b"Resources").unwrap().clone();
    let latin = "Selectable words from a scanned picture";
    let chinese = "中文识别文字也应当可以选择复制和搜索😀";
    assert!(
        apply(
            &mut doc,
            vec![
                recognized(0, &["superseded"]),
                recognized(0, &[latin, "", "   ", chinese]),
                recognized(1, &["must not be written"]),
                recognized(2, &["AB中文😀"]),
                recognized(3, &["crop"]),
                recognized(500, &["missing"]),
                recognized(u32::MAX, &["overflow"]),
            ],
        )
        .unwrap()
    );
    assert_eq!(doc.get_object(ids[&2]).unwrap(), &native);
    assert_eq!(doc.get_page_content(ids[&2]), native_content);
    assert_eq!(doc.get_page_contents(ids[&1]).len(), original.len() + 1);
    for (id, object) in original {
        assert_eq!(doc.get_object(id).unwrap(), &object);
    }
    assert_eq!(inherited(&doc, ids[&2], b"Resources").unwrap(), &inherited_resources);
    let content = last_content(&doc, ids[&1]);
    assert_eq!(content.operations.first().unwrap().operator, "q");
    assert_eq!(content.operations.last().unwrap().operator, "Q");
    for operator in ["BT", "ET", "Tr", "Tf", "Tm", "Tj"] {
        assert_eq!(content.operations.iter().filter(|op| op.operator == operator).count(), 2);
    }
    assert_eq!(values(&content, "Tr"), [3.0]);
    let stream = doc.get_page_contents(ids[&1]).last().copied().unwrap();
    let bytes = doc.get_object(stream).unwrap().as_stream().unwrap().get_plain_content().unwrap();
    assert!(String::from_utf8(bytes).unwrap().contains("3 Tr"));
    let tf = content.operations.iter().find(|op| op.operator == "Tf").unwrap();
    // Written as `80`, the size reads back as an integer.
    assert_eq!(tf.operands[0], Object::Name(b"PaperloomText1".to_vec()));
    assert_eq!(tf.operands[1].as_float().unwrap(), 80.0);
    assert_eq!(values(&content, "Tm"), [1.0, 0.0, 0.0, 1.0, 60.0, 576.0]);
    let extracted = doc.extract_text(&[1]).unwrap();
    assert_eq!(extracted, format!("{latin}\n{chinese}\n"));
    assert_eq!(doc.extract_text(&[3]).unwrap(), "AB中文😀\n");
    assert_eq!(font_id(&doc, ids[&1]), font_id(&doc, ids[&3]));
    assert_eq!(font_id(&doc, ids[&1]), font_id(&doc, ids[&4]));
    assert_eq!(
        doc.objects
            .values()
            .filter(|o| o.as_dict().is_ok_and(|d| {
                d.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Type0".as_slice())
            }))
            .count(),
        1
    );
    check_font(&doc, font_id(&doc, ids[&1]));
}

#[test]
fn places_text_in_rotated_and_cropped_user_space() {
    // A 600 x 800 page shown as 800 x 600: size 60, baseline 432,
    // left 80; undoing the 90-degree turn gives (168, 80).
    let mut doc = fixture();
    assert!(apply(&mut doc, vec![recognized(2, &["AB中文😀"]), recognized(3, &["crop"])]).unwrap());
    let ids = doc.get_pages();
    let rotated = last_content(&doc, ids[&3]);
    let tf = rotated.operations.iter().find(|op| op.operator == "Tf").unwrap();
    assert_eq!(number(&tf.operands[1]), Some(60.0));
    assert_eq!(values(&rotated, "Tm"), [0.0, 1.0, -1.0, 0.0, 168.0, 80.0]);
    assert!((values(&rotated, "Tz")[0] - 222.222_222).abs() < 0.001);
    // Crop [40 60 440 660]: size 60, baseline 432, left 40;
    // adding the crop origin gives (80, 492).
    let cropped = last_content(&doc, ids[&4]);
    let tf = cropped.operations.iter().find(|op| op.operator == "Tf").unwrap();
    assert_eq!(number(&tf.operands[1]), Some(60.0));
    assert_eq!(values(&cropped, "Tm"), [1.0, 0.0, 0.0, 1.0, 80.0, 492.0]);
    let cases = [
        (0, [1.0, 0.0, 0.0, 1.0, 80.0, 492.0], 60.0),
        (90, [0.0, 1.0, -1.0, 0.0, 152.0, 120.0], 40.0),
        (180, [-1.0, 0.0, 0.0, -1.0, 400.0, 228.0], 60.0),
        (270, [0.0, -1.0, 1.0, 0.0, 328.0, 600.0], 40.0),
        (-90, [0.0, -1.0, 1.0, 0.0, 328.0, 600.0], 40.0),
    ];
    for (rotation, matrix, size) in cases {
        let mut doc = fixture();
        let page = doc.get_pages()[&4];
        doc.get_dictionary_mut(page).unwrap().set("Rotate", rotation);
        assert!(apply(&mut doc, vec![recognized(3, &["crop"])]).unwrap());
        let content = last_content(&doc, page);
        assert_eq!(values(&content, "Tm"), matrix, "rotation {rotation}");
        let tf = content.operations.iter().find(|op| op.operator == "Tf").unwrap();
        assert_eq!(number(&tf.operands[1]), Some(size));
    }
}

#[test]
fn takes_offset_boxes_and_rotation_from_the_page_tree() {
    let cases = [
        (b"MediaBox".as_slice(), 0, [1.0, 0.0, 0.0, 1.0, 80.0, 492.0]),
        (b"CropBox".as_slice(), 90, [0.0, 1.0, -1.0, 0.0, 152.0, 120.0]),
    ];
    for (key, rotation, expected) in cases {
        let mut doc = fixture();
        let page = doc.get_pages()[&3];
        let root = doc.get_dictionary(page).unwrap().get(b"Parent").unwrap().as_reference().unwrap();
        let bounds = doc.add_object(vec![40.into(), 60.into(), 440.into(), 660.into()]);
        let tree = doc.get_dictionary_mut(root).unwrap();
        tree.set(key, bounds);
        tree.set("Rotate", rotation);
        assert!(apply(&mut doc, vec![recognized(2, &["offset"])]).unwrap());
        assert_eq!(values(&last_content(&doc, page), "Tm"), expected);
    }
}

fn u16_at(bytes: &[u8], offset: usize) -> u16 {
    u16::from_be_bytes(bytes[offset..offset + 2].try_into().unwrap())
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn check_font(doc: &Document, id: ObjectId) {
    let font = doc.get_dictionary(id).unwrap();
    assert_eq!(font.get(b"Subtype").unwrap().as_name().unwrap(), b"Type0");
    assert_eq!(font.get(b"Encoding").unwrap().as_name().unwrap(), b"Identity-H");
    let unicode = font.get_deref(b"ToUnicode", doc).unwrap().as_stream().unwrap();
    assert_eq!(unicode.get_plain_content().unwrap(), TO_UNICODE);
    let descendant = font.get(b"DescendantFonts").unwrap().as_array().unwrap()[0].as_reference().unwrap();
    let descendant = doc.get_dictionary(descendant).unwrap();
    assert_eq!(descendant.get(b"Subtype").unwrap().as_name().unwrap(), b"CIDFontType2");
    assert_eq!(descendant.get(b"DW").unwrap().as_i64().unwrap(), 500);
    let map = descendant.get_deref(b"CIDToGIDMap", doc).unwrap().as_stream().unwrap();
    let map = map.get_plain_content().unwrap();
    assert_eq!(map.len(), 131072);
    assert!(map.iter().all(|&byte| byte == 0));
    let descriptor = descendant.get_deref(b"FontDescriptor", doc).unwrap().as_dict().unwrap();
    let file = descriptor.get_deref(b"FontFile2", doc).unwrap().as_stream().unwrap();
    let bytes = file.get_plain_content().unwrap();
    assert_eq!(file.dict.get(b"Length1").unwrap().as_i64().unwrap() as usize, bytes.len());
    assert_eq!(u32_at(&bytes, 0), 0x0001_0000);
    assert_eq!(u16_at(&bytes, 4), 10);
    assert_eq!((u16_at(&bytes, 6), u16_at(&bytes, 8), u16_at(&bytes, 10)), (128, 3, 32));
    let mut tables = BTreeMap::new();
    let mut end = 12 + 10 * 16;
    let mut previous = [0; 4];
    for index in 0..10 {
        let at = 12 + index * 16;
        let tag: [u8; 4] = bytes[at..at + 4].try_into().unwrap();
        assert!(tag > previous, "sorted directory");
        previous = tag;
        let offset = u32_at(&bytes, at + 8) as usize;
        let length = u32_at(&bytes, at + 12) as usize;
        assert_eq!(offset % 4, 0);
        assert!(offset >= end && offset + length <= bytes.len());
        end = (offset + length).next_multiple_of(4);
        let mut table = bytes[offset..offset + length].to_vec();
        if tag == *b"head" {
            table[8..12].fill(0);
        }
        assert_eq!(checksum(&table), u32_at(&bytes, at + 4));
        tables.insert(tag, table);
    }
    assert_eq!(checksum(&bytes), 0xb1b0_afba);
    assert_eq!(u32_at(&tables[b"head"], 12), 0x5f0f_3cf5);
    assert_eq!(u16_at(&tables[b"head"], 18), 1000);
    assert_eq!(u16_at(&tables[b"head"], 50), 0, "short loca offsets");
    assert_eq!(u16_at(&tables[b"maxp"], 4), 1, "one glyph");
    assert_eq!(u16_at(&tables[b"hhea"], 34), 1, "one metric");
    assert_eq!(tables[b"hmtx"], [1, 244, 0, 0]);
    assert_eq!(tables[b"loca"], [0, 0, 0, 6]);
    assert_eq!(tables[b"glyf"], [0; 12], "an empty glyph with no instructions");
    assert_eq!(u16_at(&tables[b"cmap"], 12), 4);
    assert_eq!(u16_at(&tables[b"cmap"], 14) as usize + 12, tables[b"cmap"].len());
    assert_eq!(u16_at(&tables[b"name"], 2), 6);
    assert_eq!(u32_at(&tables[b"post"], 0), 0x0003_0000);
    assert_eq!(tables[b"OS/2"].len(), 78);
}

#[test]
fn skips_empty_missing_and_invalid_lines_and_last_duplicate_wins() {
    let mut doc = fixture();
    let before = doc.objects.clone();
    let mut invalid = line("invalid");
    invalid.x = f64::NAN;
    let mut zero = line("zero");
    zero.height = 0.0;
    let mut outside = line("outside");
    outside.width = 1.0;
    assert!(
        !apply(
            &mut doc,
            vec![
                recognized(0, &["discarded"]),
                recognized(0, &[]),
                recognized(3, &["", " \t"]),
                RecognizedPage { page_index: 2, lines: vec![invalid, zero, outside] },
                recognized(20, &["missing"]),
            ],
        )
        .unwrap()
    );
    assert_eq!(doc.objects, before);
}

#[test]
fn uses_the_twenty_letter_threshold_and_reuses_its_font() {
    for (native, changes) in [("一".repeat(19), true), ("一".repeat(20), false)] {
        let mut doc = fixture();
        let page = doc.get_pages()[&4];
        assert!(apply(&mut doc, vec![recognized(3, &[&native])]).unwrap());
        let id = font_id(&doc, page);
        let objects = doc.objects.len();
        assert_eq!(apply(&mut doc, vec![recognized(3, &["another"])]).unwrap(), changes);
        assert_eq!(font_id(&doc, page), id);
        assert_eq!(doc.objects.len(), objects + usize::from(changes));
    }
}

#[test]
fn updates_the_attachment_but_keeps_marks_and_search_text() {
    let dir = tempfile::tempdir().unwrap();
    let library = PaperLibrary::new(dir.path().join("library.sqlite").display().to_string()).unwrap();
    let item = library.add_item(NewItem { title: "Scan".into(), ..NewItem::default() }).unwrap();
    let source = dir.path().join("scan.pdf");
    fixture().save(&source).unwrap();
    let file = library.attach_file(item.id, source.display().to_string()).unwrap();
    let mark = library
        .add_annotation(crate::NewAnnotation {
            attachment_id: file.id.clone(),
            kind: "highlight".into(),
            color: "#ffd400".into(),
            page_index: 0,
            rects: vec![AnnotationRect { x: 60.0, y: 576.0, width: 300.0, height: 80.0 }],
            text: Some("recognized words".into()),
            comment: Some("keep this mark".into()),
            strokes: vec![],
            font_size: None,
        })
        .unwrap();
    let latin = "Searchable recognition from the scanned page";
    let stored = vec![latin.into(), "native".into(), "rotated".into(), "cropped".into()];
    library.set_attachment_text(file.id.clone(), stored.clone()).unwrap();
    let original = std::fs::read(&file.file_path).unwrap();
    let updated = library.add_text_layer(file.id.clone(), vec![recognized(0, &[latin, "中文😀"])]).unwrap();
    assert_eq!(updated.id, file.id);
    assert_eq!(updated.file_path, file.file_path);
    assert!(updated.is_pdf);
    assert_ne!(std::fs::read(&file.file_path).unwrap(), original);
    let (hash, size) = crate::files::sha256_and_size(Path::new(&file.file_path)).unwrap();
    assert_eq!(updated.size, size);
    let saved = library.lock().unwrap().attachment(&AttachmentId::from_string(&file.id)).unwrap().unwrap();
    assert_eq!(saved.sha256, hash);
    assert_eq!(saved.size, size);
    let kept = library.annotations(file.id.clone()).unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0], mark);
    let kept = library.lock().unwrap().attachment_pages(&AttachmentId::from_string(&file.id)).unwrap();
    assert_eq!(kept, stored.into_iter().enumerate().map(|(i, s)| (i as u32, s)).collect::<Vec<_>>());
    let hits = library.search_everything("recognition".into(), vec!["pdf".into()], 5).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].page_index, Some(0));
    let doc = Document::load(&file.file_path).unwrap();
    let page = doc.get_pages()[&1];
    let fonts: Vec<String> =
        doc.get_page_fonts(page).unwrap().keys().map(|k| String::from_utf8_lossy(k).into_owned()).collect();
    let text_ops: Vec<String> = Content::decode(&doc.get_page_content(page))
        .unwrap()
        .operations
        .iter()
        .filter(|op| ["Tf", "Tj"].contains(&op.operator.as_str()))
        .map(|op| format!("{} {:?}", op.operator, op.operands))
        .collect();
    assert_eq!(
        doc.extract_text(&[1]).unwrap(),
        format!("{latin}\n中文😀\n"),
        "fonts {fonts:?}, text operators {text_ops:?}"
    );
    let before = std::fs::read(&file.file_path).unwrap();
    let unchanged = library
        .add_text_layer(file.id.clone(), vec![recognized(0, &["skip"]), recognized(50, &["absent"])])
        .unwrap();
    assert_eq!(unchanged, updated);
    assert_eq!(std::fs::read(&file.file_path).unwrap(), before);
    assert!(!Path::new(&file.file_path).with_extension("pdf.paperloom-tmp").exists());
    assert_eq!(library.add_text_layer(file.id.clone(), vec![]).unwrap(), updated);
    assert!(matches!(library.add_text_layer("unknown".into(), vec![]), Err(CoreError::NotFound { .. })));
    let other = dir.path().join("words.txt");
    std::fs::write(&other, "not a PDF").unwrap();
    let other = library.attach_file(updated.item_id, other.display().to_string()).unwrap();
    assert!(matches!(
        library.add_text_layer(other.id, vec![]),
        Err(CoreError::InvalidInput { message }) if message == "A text layer can only be added to a PDF attachment."
    ));
}
