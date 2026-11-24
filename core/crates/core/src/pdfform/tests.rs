use std::path::Path;

use super::*;
use crate::NewItem;

/// A check box's or radio button's looks: on (named `on`) and off.
fn on_off(doc: &mut Document, on: &str) -> Object {
    let on_look = doc.add_object(Stream::new(Dictionary::new(), b"0 g 2 2 6 6 re f".to_vec()));
    let off_look = doc.add_object(Stream::new(Dictionary::new(), Vec::new()));
    let mut normal = Dictionary::new();
    normal.set(on, Object::Reference(on_look));
    normal.set("Off", Object::Reference(off_look));
    let mut appearance = Dictionary::new();
    appearance.set("N", normal);
    appearance.into()
}

fn rect(r: [i64; 4]) -> Vec<Object> {
    r.map(Object::Integer).to_vec()
}

/// A page with a form: a text field that is its own box, one under a
/// parent, a check box, two radio buttons and a combo box.
fn form_pdf(path: &Path) {
    let mut doc = Document::with_version("1.7");
    let pages = doc.new_object_id();
    let page = doc.new_object_id();
    let (mut annots, mut fields) = (Vec::new(), Vec::new());

    let mut name = Dictionary::new();
    name.set("FT", "Tx");
    name.set("T", Object::string_literal("Name"));
    name.set("Subtype", "Widget");
    name.set("Rect", rect([50, 700, 250, 720]));
    name.set("P", Object::Reference(page));
    name.set("DA", Object::string_literal("/Helv 0 Tf 0 0 1 rg"));
    name.set("MaxLen", 20);
    let name = doc.add_object(name);
    annots.push(name);
    fields.push(name);

    let address = doc.new_object_id();
    let city = doc.new_object_id();
    let mut city_box = Dictionary::new();
    city_box.set("Subtype", "Widget");
    city_box.set("Parent", Object::Reference(city));
    city_box.set("Rect", rect([50, 600, 250, 680]));
    let city_box = doc.add_object(city_box);
    let mut city_field = Dictionary::new();
    city_field.set("T", Object::string_literal("City"));
    city_field.set("Parent", Object::Reference(address));
    city_field.set("V", text_string("Zürich"));
    city_field.set("Ff", MULTILINE);
    city_field.set("Kids", vec![Object::Reference(city_box)]);
    doc.objects.insert(city, Object::Dictionary(city_field));
    let mut parent = Dictionary::new();
    parent.set("T", Object::string_literal("Address"));
    parent.set("FT", "Tx");
    parent.set("Kids", vec![Object::Reference(city)]);
    doc.objects.insert(address, Object::Dictionary(parent));
    annots.push(city_box);
    fields.push(address);

    let mut agree = Dictionary::new();
    agree.set("FT", "Btn");
    agree.set("T", Object::string_literal("Agree"));
    agree.set("Subtype", "Widget");
    agree.set("Rect", rect([50, 560, 62, 572]));
    agree.set("V", "Off");
    agree.set("AS", "Off");
    agree.set("AP", on_off(&mut doc, "Yes"));
    let agree = doc.add_object(agree);
    annots.push(agree);
    fields.push(agree);

    let size = doc.new_object_id();
    let mut kids = Vec::new();
    for (on, x) in [("S", 50), ("L", 80)] {
        let mut button = Dictionary::new();
        button.set("Subtype", "Widget");
        button.set("Parent", Object::Reference(size));
        button.set("Rect", rect([x, 520, x + 12, 532]));
        button.set("AS", "Off");
        button.set("AP", on_off(&mut doc, on));
        let button = doc.add_object(button);
        annots.push(button);
        kids.push(Object::Reference(button));
    }
    let mut group = Dictionary::new();
    group.set("FT", "Btn");
    group.set("T", Object::string_literal("Size"));
    group.set("Ff", RADIO);
    group.set("Kids", kids);
    doc.objects.insert(size, Object::Dictionary(group));
    fields.push(size);

    let mut colour = Dictionary::new();
    colour.set("FT", "Ch");
    colour.set("T", Object::string_literal("Colour"));
    colour.set("Subtype", "Widget");
    colour.set("Ff", COMBO);
    colour.set("Rect", rect([50, 480, 150, 495]));
    let blue = vec![Object::string_literal("b"), Object::string_literal("Blue")];
    colour.set("Opt", vec![Object::string_literal("Red"), Object::Array(blue)]);
    colour.set("V", Object::string_literal("Red"));
    let colour = doc.add_object(colour);
    annots.push(colour);
    fields.push(colour);

    let mut page_dict = Dictionary::new();
    page_dict.set("Type", "Page");
    page_dict.set("Parent", Object::Reference(pages));
    page_dict.set("MediaBox", rect([0, 0, 612, 792]));
    page_dict.set("Annots", annots.iter().map(|&a| Object::Reference(a)).collect::<Vec<_>>());
    doc.objects.insert(page, Object::Dictionary(page_dict));
    let mut tree = Dictionary::new();
    tree.set("Type", "Pages");
    tree.set("Kids", vec![Object::Reference(page)]);
    tree.set("Count", 1);
    doc.objects.insert(pages, Object::Dictionary(tree));
    let mut form = Dictionary::new();
    form.set("Fields", fields.iter().map(|&f| Object::Reference(f)).collect::<Vec<_>>());
    form.set("DA", Object::string_literal("/Helv 0 Tf 0 g"));
    form.set("XFA", Object::string_literal("<xdp/>"));
    let form = doc.add_object(form);
    let mut catalog = Dictionary::new();
    catalog.set("Type", "Catalog");
    catalog.set("Pages", Object::Reference(pages));
    catalog.set("AcroForm", Object::Reference(form));
    let catalog = doc.add_object(catalog);
    doc.trailer.set("Root", Object::Reference(catalog));
    doc.save(path).unwrap();
}

#[test]
fn reads_and_fills_in_a_form() {
    let dir = tempfile::tempdir().unwrap();
    let library = PaperLibrary::new(dir.path().join("library.sqlite").display().to_string()).unwrap();
    let item = library.add_item(NewItem { title: "Form".into(), ..NewItem::default() }).unwrap();
    let source = dir.path().join("form.pdf");
    form_pdf(&source);
    let file = library.attach_file(item.id.clone(), source.display().to_string()).unwrap();

    let fields = library.pdf_form_fields(file.id.clone()).unwrap();
    let read: Vec<(&str, &str, &str)> =
        fields.iter().map(|f| (f.name.as_str(), f.kind.as_str(), f.value.as_str())).collect();
    assert_eq!(
        read,
        [
            ("Name", "text", ""),
            ("Address.City", "text", "Zürich"),
            ("Agree", "checkbox", "Off"),
            ("Size", "radio", "Off"),
            ("Colour", "choice", "Red"),
        ]
    );
    let name = &fields[0];
    assert_eq!((name.max_length, name.font_size, name.widgets.len()), (20, 0.0, 1));
    assert_eq!((name.widgets[0].page_index, name.widgets[0].x, name.widgets[0].width), (0, 50.0, 200.0));
    assert!(fields[1].multiline && !fields[0].multiline);
    assert_eq!(fields[1].widgets[0].page_index, 0, "found in the page's annotations");
    assert_eq!(fields[2].widgets[0].on_value, "Yes");
    assert_eq!(fields[3].widgets.iter().map(|w| w.on_value.as_str()).collect::<Vec<_>>(), ["S", "L"]);
    assert_eq!(fields[4].options, ["Red", "b"]);
    assert_eq!(fields[4].option_labels, ["Red", "Blue"]);
    assert!(fields[4].combo);

    let value = |name: &str, value: &str| PdfFieldValue { name: name.into(), value: value.into() };
    let filled = library
        .fill_pdf_form(
            file.id.clone(),
            vec![
                value("Name", "Ada (Lovelace)"),
                value("Address.City", "北京"),
                value("Agree", "Yes"),
                value("Size", "L"),
                value("Colour", "b"),
                value("Nobody", "x"),
            ],
        )
        .unwrap();
    assert_ne!(filled.size, 0);
    let fields = library.pdf_form_fields(file.id.clone()).unwrap();
    assert_eq!(
        fields.iter().map(|f| f.value.as_str()).collect::<Vec<_>>(),
        ["Ada (Lovelace)", "北京", "Yes", "L", "b"]
    );

    let doc = Document::load(&file.file_path).unwrap();
    let catalog = doc.catalog().unwrap();
    let form = doc.get_dictionary(catalog.get(b"AcroForm").unwrap().as_reference().unwrap()).unwrap();
    assert!(!form.has(b"XFA"), "an XFA form would stand in for the values");
    assert!(form.get(b"NeedAppearances").unwrap().as_bool().unwrap(), "Chinese is left for readers to draw");
    let read = read_fields(&doc);
    // The text field draws its text in Helvetica, in its colour.
    let (name_box, _) = &read[0].boxes[0];
    let appearance = doc.get_dictionary(*name_box).unwrap().get(b"AP").unwrap().as_dict().unwrap();
    let look = appearance.get(b"N").unwrap().as_reference().unwrap();
    let content =
        String::from_utf8_lossy(&doc.get_object(look).unwrap().as_stream().unwrap().content).into_owned();
    assert!(content.contains("(Ada \\(Lovelace\\)) Tj") && content.contains("0 0 1 rg"), "{content}");
    // Only the radio button chosen is on.
    let states: Vec<Vec<u8>> = read[3]
        .boxes
        .iter()
        .map(|(id, _)| doc.get_dictionary(*id).unwrap().get(b"AS").unwrap().as_name().unwrap().to_vec())
        .collect();
    assert_eq!(states, [b"Off".to_vec(), b"L".to_vec()]);

    library.fill_pdf_form(file.id.clone(), vec![value("Agree", "Off")]).unwrap();
    assert_eq!(library.pdf_form_fields(file.id.clone()).unwrap()[2].value, "Off", "unticked");
}

#[test]
fn draws_text_and_reads_appearances() {
    assert_eq!(appearance_size("/Helv 9.5 Tf 0 g"), 9.5);
    assert_eq!(appearance_size("0 g"), 0.0);
    assert_eq!(appearance_color("/Helv 0 Tf 1 0 0 rg"), "1 0 0 rg");
    assert_eq!(appearance_color(""), "0 g");
    assert_eq!(win_ansi("Café"), Some(b"Caf\xe9".to_vec()));
    assert_eq!(win_ansi("中"), None);
    assert_eq!(wrap_lines("one two three four", 10.0, 55.0), ["one two", "three four"]);
}
