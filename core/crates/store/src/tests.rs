use paperloom_model::{Creator, ItemType};

use super::*;

fn article(title: &str, doi: Option<&str>) -> Item {
    let mut item = Item::new(ItemType::JournalArticle, title);
    item.doi = doi.map(|d| Doi::parse(d).unwrap());
    item.creators.push(Creator::person("Curie", "Marie"));
    item
}

#[test]
fn adds_and_reads_items() {
    let mut lib = Library::open_in_memory().unwrap();
    let a = article("First", Some("10.1000/one"));
    let b = article("Second", None);
    lib.add_item(&a).unwrap();
    lib.add_item(&b).unwrap();

    assert_eq!(lib.item(&a.id).unwrap(), Some(a.clone()));
    assert_eq!(lib.item(&ItemId::from_string("missing")).unwrap(), None);
    let titles: Vec<String> = lib.items().unwrap().into_iter().map(|i| i.title).collect();
    assert_eq!(titles.len(), 2);
    assert!(titles.contains(&"First".to_string()) && titles.contains(&"Second".to_string()));
    assert_eq!(lib.find_by_doi(&Doi::parse("https://doi.org/10.1000/ONE").unwrap()).unwrap(), Some(a));
}

#[test]
fn rejects_duplicate_dois() {
    let mut lib = Library::open_in_memory().unwrap();
    let a = article("First", Some("10.1000/one"));
    lib.add_item(&a).unwrap();
    let err = lib.add_item(&article("Copy", Some("doi:10.1000/ONE"))).unwrap_err();
    match err {
        Error::DuplicateDoi { existing, .. } => assert_eq!(existing, a.id),
        other => panic!("unexpected error {other}"),
    }
}

#[test]
fn updates_items() {
    let mut lib = Library::open_in_memory().unwrap();
    let mut a = article("Draft title", None);
    lib.add_item(&a).unwrap();
    a.title = "Final title".into();
    a.year = Some(2026);
    lib.update_item(&a).unwrap();
    assert_eq!(lib.item(&a.id).unwrap().unwrap().title, "Final title");

    let b = article("Other", Some("10.1000/two"));
    lib.add_item(&b).unwrap();
    a.doi = b.doi.clone();
    assert!(matches!(lib.update_item(&a), Err(Error::DuplicateDoi { .. })));
    assert!(matches!(lib.update_item(&article("Never added", None)), Err(Error::NotFound(_))));
}

#[test]
fn trash_frees_the_doi_and_restore_checks_it() {
    let mut lib = Library::open_in_memory().unwrap();
    let a = article("First", Some("10.1000/one"));
    lib.add_item(&a).unwrap();
    lib.trash_item(&a.id).unwrap();
    assert!(lib.items().unwrap().is_empty());
    assert_eq!(lib.trashed_items().unwrap().len(), 1);

    let again = article("Added again", Some("10.1000/one"));
    lib.add_item(&again).unwrap();
    assert!(matches!(lib.restore_item(&a.id), Err(Error::DuplicateDoi { .. })));

    lib.trash_item(&again.id).unwrap();
    lib.restore_item(&a.id).unwrap();
    assert_eq!(lib.items().unwrap(), vec![a]);
}

#[test]
fn nests_collections_and_keeps_order() {
    let mut lib = Library::open_in_memory().unwrap();
    let thesis = lib.create_collection("Thesis", None).unwrap();
    let reading = lib.create_collection("  Reading  ", None).unwrap();
    let ch1 = lib.create_collection("Chapter 1", Some(&thesis.id)).unwrap();
    let ch2 = lib.create_collection("Chapter 2", Some(&thesis.id)).unwrap();

    assert_eq!(reading.name, "Reading");
    assert_eq!((thesis.position, reading.position), (0, 1));
    assert_eq!((ch1.position, ch2.position), (0, 1));
    let all = lib.collections().unwrap();
    assert_eq!(all.len(), 4);
    assert_eq!(&all[..2], &[thesis.clone(), reading]);

    let missing = CollectionId::from_string("missing");
    assert!(matches!(lib.create_collection("Orphan", Some(&missing)), Err(Error::NotFound(_))));

    lib.rename_collection(&ch1.id, "Introduction").unwrap();
    assert_eq!(lib.collection(&ch1.id).unwrap().unwrap().name, "Introduction");
}

#[test]
fn collection_membership() {
    let mut lib = Library::open_in_memory().unwrap();
    let thesis = lib.create_collection("Thesis", None).unwrap();
    let ch1 = lib.create_collection("Chapter 1", Some(&thesis.id)).unwrap();
    let a = article("First", None);
    let b = article("Second", None);
    lib.add_item(&a).unwrap();
    lib.add_item(&b).unwrap();

    lib.add_to_collection(&ch1.id, &a.id).unwrap();
    lib.add_to_collection(&ch1.id, &a.id).unwrap(); // idempotent
    lib.add_to_collection(&thesis.id, &b.id).unwrap();
    assert_eq!(lib.items_in_collection(&ch1.id).unwrap(), vec![a.clone()]);
    assert_eq!(lib.items_in_collection(&thesis.id).unwrap(), vec![b.clone()]);

    lib.remove_from_collection(&thesis.id, &b.id).unwrap();
    assert!(lib.items_in_collection(&thesis.id).unwrap().is_empty());

    // Deleting a collection removes its sub-collections, not the items.
    lib.delete_collection(&thesis.id).unwrap();
    assert!(lib.collections().unwrap().is_empty());
    assert_eq!(lib.items().unwrap().len(), 2);
}

#[test]
fn persists_to_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.sqlite");
    let a = article("Kept", Some("10.1000/kept"));
    {
        let mut lib = Library::open(&path).unwrap();
        lib.add_item(&a).unwrap();
        lib.create_collection("Saved", None).unwrap();
    }
    let lib = Library::open(&path).unwrap();
    assert_eq!(lib.items().unwrap(), vec![a]);
    assert_eq!(lib.collections().unwrap()[0].name, "Saved");
}

#[test]
fn refuses_a_newer_schema() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("future.sqlite");
    {
        let conn = Connection::open(&path).unwrap();
        conn.pragma_update(None, "user_version", 999).unwrap();
    }
    assert!(matches!(Library::open(&path), Err(Error::NewerSchema(999))));
}

fn attachment(item: &Item, sha256: &str) -> paperloom_model::Attachment {
    let id = paperloom_model::AttachmentId::new();
    paperloom_model::Attachment {
        path: format!("{id}/paper.pdf"),
        id,
        item_id: item.id.clone(),
        title: "paper.pdf".into(),
        content_type: "application/pdf".into(),
        size: 1234,
        sha256: sha256.into(),
        date_added: now_ms(),
    }
}

#[test]
fn keeps_attachments_with_their_item() {
    let mut lib = Library::open_in_memory().unwrap();
    let a = article("With PDF", None);
    lib.add_item(&a).unwrap();
    let pdf = attachment(&a, "abc");
    lib.add_attachment(&pdf).unwrap();
    assert_eq!(lib.attachments(&a.id).unwrap(), vec![pdf.clone()]);
    assert_eq!(lib.attachment(&pdf.id).unwrap(), Some(pdf.clone()));
    assert_eq!(lib.find_attachment(&a.id, "abc").unwrap(), Some(pdf.clone()));
    assert_eq!(lib.find_attachment(&a.id, "other").unwrap(), None);
    lib.remove_attachment(&pdf.id).unwrap();
    assert!(lib.attachments(&a.id).unwrap().is_empty());
    assert!(matches!(lib.remove_attachment(&pdf.id), Err(Error::NotFound(_))));
}

#[test]
fn attachments_need_an_item() {
    let mut lib = Library::open_in_memory().unwrap();
    let never_added = article("Ghost", None);
    assert!(lib.add_attachment(&attachment(&never_added, "abc")).is_err(), "foreign key");
}

#[test]
fn keeps_annotations_with_their_attachment() {
    use paperloom_model::{Annotation, AnnotationId, AnnotationKind, Rect};
    let mut lib = Library::open_in_memory().unwrap();
    let a = article("Marked up", None);
    lib.add_item(&a).unwrap();
    let pdf = attachment(&a, "abc");
    lib.add_attachment(&pdf).unwrap();
    let mut mark = Annotation {
        id: AnnotationId::new(),
        attachment_id: pdf.id.clone(),
        kind: AnnotationKind::Highlight,
        color: "#ffd400".into(),
        page_index: 2,
        rects: vec![Rect { x: 72.0, y: 700.0, width: 300.0, height: 12.0 }],
        text: Some("important".into()),
        comment: None,
        tags: Vec::new(),
        strokes: Vec::new(),
        font_size: None,
        date_added: now_ms(),
        date_modified: now_ms(),
    };
    lib.add_annotation(&mark).unwrap();
    assert_eq!(lib.annotations(&pdf.id).unwrap(), vec![mark.clone()]);
    let stored: String = lib
        .conn
        .query_row("SELECT data FROM annotations WHERE id = ?1", [mark.id.as_str()], |row| row.get(0))
        .unwrap();
    assert!(!stored.contains("\"tags\""), "old JSON representation remains valid without tags");
    mark.comment = Some("Check this".into());
    mark.tags = vec!["#auto".into(), "翻译".into()];
    lib.update_annotation(&mark).unwrap();
    assert_eq!(lib.annotation(&mark.id).unwrap().unwrap(), mark);
    let stored: String = lib
        .conn
        .query_row("SELECT data FROM annotations WHERE id = ?1", [mark.id.as_str()], |row| row.get(0))
        .unwrap();
    assert_eq!(serde_json::from_str::<Annotation>(&stored).unwrap().tags, mark.tags);
    lib.remove_attachment(&pdf.id).unwrap();
    assert!(lib.annotations(&pdf.id).unwrap().is_empty(), "annotations go with their attachment");
    assert!(matches!(lib.remove_annotation(&mark.id), Err(Error::NotFound(_))));
}

#[test]
fn lists_entries_by_filter() {
    use paperloom_model::ReadStatus;
    let mut lib = Library::open_in_memory().unwrap();
    let a = article("Starred and read", Some("10.1000/a"));
    let b = article("Tagged", None);
    let c = article("Trashed", None);
    for item in [&a, &b, &c] {
        lib.add_item(item).unwrap();
    }
    lib.set_starred(&a.id, true).unwrap();
    lib.set_read_status(&a.id, ReadStatus::Read).unwrap();
    lib.set_item_tags(&b.id, &["method".into(), " Important ".into(), "".into()]).unwrap();
    lib.add_attachment(&attachment(&b, "pdf")).unwrap();
    lib.trash_item(&c.id).unwrap();
    let inbox = lib.create_collection("Inbox", None).unwrap();
    lib.add_to_collection(&inbox.id, &b.id).unwrap();

    let titles = |filter: Filter| -> Vec<String> {
        lib.entries(&filter).unwrap().into_iter().map(|e| e.item.title).collect()
    };
    assert_eq!(titles(Filter::default()).len(), 2);
    assert_eq!(titles(Filter { starred_only: true, ..Filter::default() }), ["Starred and read"]);
    assert_eq!(
        titles(Filter { read_status: Some(ReadStatus::Read), ..Filter::default() }),
        ["Starred and read"]
    );
    assert_eq!(titles(Filter { tag: Some("Important".into()), ..Filter::default() }), ["Tagged"]);
    assert_eq!(titles(Filter { collection: Some(inbox.id.clone()), ..Filter::default() }), ["Tagged"]);
    assert_eq!(titles(Filter { trashed: true, ..Filter::default() }), ["Trashed"]);

    let tagged = lib.entry(&b.id).unwrap().unwrap();
    assert_eq!(tagged.tags, ["Important", "method"]);
    assert_eq!((tagged.pdf_count, tagged.starred, tagged.read_status), (1, false, ReadStatus::Unread));
}

#[test]
fn manages_tags() {
    let mut lib = Library::open_in_memory().unwrap();
    let a = article("A", None);
    let b = article("B", None);
    lib.add_item(&a).unwrap();
    lib.add_item(&b).unwrap();
    lib.set_item_tags(&a.id, &["ml".into(), "todo".into()]).unwrap();
    lib.set_item_tags(&b.id, &["ML".into()]).unwrap();
    lib.set_tag_color("todo", Some("#ff6666")).unwrap();
    let names = |lib: &Library| -> Vec<(String, Option<String>, u32)> {
        lib.tags().unwrap().into_iter().map(|(t, n)| (t.name, t.color, n)).collect()
    };
    assert_eq!(
        names(&lib),
        [("ML".into(), None, 1), ("ml".into(), None, 1), ("todo".into(), Some("#ff6666".into()), 1)]
    );
    lib.rename_tag("ML", "ml").unwrap();
    assert_eq!(lib.item_tags(&b.id).unwrap(), ["ml"], "merged into the existing tag");
    lib.rename_tag("todo", "later").unwrap();
    assert_eq!(lib.item_tags(&a.id).unwrap(), ["later", "ml"]);
    lib.delete_tag("ml").unwrap();
    assert_eq!(lib.item_tags(&a.id).unwrap(), ["later"]);
    assert_eq!(names(&lib), [("later".into(), Some("#ff6666".into()), 1)]);
}

#[test]
fn deletes_items_for_good() {
    let mut lib = Library::open_in_memory().unwrap();
    let a = article("Gone", Some("10.1000/gone"));
    lib.add_item(&a).unwrap();
    let pdf = attachment(&a, "x");
    lib.add_attachment(&pdf).unwrap();
    lib.set_item_tags(&a.id, &["t".into()]).unwrap();
    assert_eq!(lib.delete_item(&a.id).unwrap(), vec![pdf.clone()]);
    assert_eq!(lib.item(&a.id).unwrap(), None);
    assert_eq!(lib.attachment(&pdf.id).unwrap(), None);
    assert!(matches!(lib.delete_item(&a.id), Err(Error::NotFound(_))));
    assert!(matches!(lib.set_starred(&a.id, true), Err(Error::NotFound(_))));
}

#[test]
fn keeps_page_text() {
    let mut lib = Library::open_in_memory().unwrap();
    let a = article("With text", None);
    lib.add_item(&a).unwrap();
    let pdf = attachment(&a, "t");
    lib.add_attachment(&pdf).unwrap();
    lib.set_attachment_pages(&pdf.id, &["first page".into(), "  ".into(), "third page".into()]).unwrap();
    assert_eq!(
        lib.attachment_pages(&pdf.id).unwrap(),
        [(0, "first page".to_string()), (2, "third page".to_string())]
    );
    assert_eq!(lib.attachments_with_pages().unwrap(), vec![pdf.clone()]);
    lib.set_attachment_pages(&pdf.id, &["only".into()]).unwrap();
    assert_eq!(lib.attachment_pages(&pdf.id).unwrap().len(), 1);
    lib.remove_attachment(&pdf.id).unwrap();
    assert!(lib.attachments_with_pages().unwrap().is_empty());
}

/// Copies what changed in `from` into `to`, as sync does.
fn sync_into(from: &mut Library, to: &mut Library) {
    let changed = from.changed_records().unwrap();
    let records: Vec<Record> = changed
        .iter()
        .map(|(kind, id)| Record { kind: *kind, id: id.clone(), fields: from.record(*kind, id).unwrap() })
        .collect();
    from.forget_changed(&changed).unwrap();
    to.apply_records(&records).unwrap();
}

#[test]
fn copies_changed_records_to_another_library() {
    use paperloom_model::{AnnotationKind, Rect};
    let mut here = Library::open_in_memory().unwrap();
    let mut there = Library::open_in_memory().unwrap();
    let a = article("Shared", Some("10.1000/shared"));
    here.add_item(&a).unwrap();
    let thesis = here.create_collection("Thesis", None).unwrap();
    let chapter = here.create_collection("Chapter 1", Some(&thesis.id)).unwrap();
    here.add_to_collection(&chapter.id, &a.id).unwrap();
    here.set_item_tags(&a.id, &["ML".into(), "to read".into()]).unwrap();
    here.set_tag_color("ML", Some("#ff0000")).unwrap();
    here.set_starred(&a.id, true).unwrap();
    let pdf = attachment(&a, "abc");
    here.add_attachment(&pdf).unwrap();
    let mark = Annotation {
        id: AnnotationId::new(),
        attachment_id: pdf.id.clone(),
        kind: AnnotationKind::Highlight,
        color: "#ffd400".into(),
        page_index: 2,
        rects: vec![Rect { x: 72.0, y: 700.0, width: 300.0, height: 12.0 }],
        text: Some("important".into()),
        comment: None,
        tags: Vec::new(),
        strokes: Vec::new(),
        font_size: None,
        date_added: now_ms(),
        date_modified: now_ms(),
    };
    let mut mark = mark;
    mark.tags = vec!["#auto".into(), "翻译".into()];
    here.add_annotation(&mark).unwrap();

    let changed = here.changed_records().unwrap();
    assert!(changed.contains(&(RecordKind::Item, a.id.to_string())));
    assert_eq!(changed.first().map(|c| c.0), Some(RecordKind::Tag), "tags come first");
    let fields = here.record(RecordKind::Item, a.id.as_str()).unwrap().unwrap();
    assert_eq!(fields["data.title"], "Shared");
    assert_eq!(fields["tag:ML"], true);
    assert_eq!(fields[&format!("in:{}", chapter.id)], true);
    assert_eq!(fields["starred"], true);

    sync_into(&mut here, &mut there);
    assert!(here.changed_records().unwrap().is_empty());
    assert!(there.changed_records().unwrap().is_empty(), "records from elsewhere are not noted again");
    assert_eq!(there.entry(&a.id).unwrap(), here.entry(&a.id).unwrap());
    assert_eq!(there.tags().unwrap(), here.tags().unwrap());
    assert_eq!(there.collection(&chapter.id).unwrap().unwrap().parent_id, Some(thesis.id.clone()));
    assert_eq!(there.items_in_collection(&chapter.id).unwrap(), vec![a.clone()]);
    assert_eq!(there.attachments(&a.id).unwrap(), vec![pdf.clone()]);
    assert_eq!(there.annotations(&pdf.id).unwrap(), vec![mark.clone()]);
    mark.tags.clear();
    here.update_annotation(&mark).unwrap();
    sync_into(&mut here, &mut there);
    assert!(there.annotation(&mark.id).unwrap().unwrap().tags.is_empty(), "clearing tags syncs too");

    // Changes and deletions travel too.
    here.set_item_tags(&a.id, &["ML".into()]).unwrap();
    here.remove_annotation(&mark.id).unwrap();
    here.rename_collection(&thesis.id, "PhD").unwrap();
    sync_into(&mut here, &mut there);
    assert_eq!(there.item_tags(&a.id).unwrap(), ["ML"]);
    assert!(there.annotations(&pdf.id).unwrap().is_empty());
    assert_eq!(there.collection(&thesis.id).unwrap().unwrap().name, "PhD");
    here.delete_item(&a.id).unwrap();
    sync_into(&mut here, &mut there);
    assert_eq!(there.item(&a.id).unwrap(), None);
    assert!(there.attachments(&a.id).unwrap().is_empty());
}

#[test]
fn keeps_both_copies_when_two_devices_add_one_paper() {
    let mut here = Library::open_in_memory().unwrap();
    let mut there = Library::open_in_memory().unwrap();
    let mine = article("Same paper", Some("10.1000/same"));
    let theirs = article("Same paper", Some("10.1000/same"));
    here.add_item(&mine).unwrap();
    there.add_item(&theirs).unwrap();
    sync_into(&mut here, &mut there);
    assert_eq!(there.item(&mine.id).unwrap(), Some(mine.clone()), "kept, its DOI still in its data");
    assert_eq!(there.find_by_doi(&Doi::parse("10.1000/same").unwrap()).unwrap(), Some(theirs));
}

#[test]
fn marks_everything_for_the_first_sync() {
    let mut lib = Library::open_in_memory().unwrap();
    let a = article("Old", None);
    lib.add_item(&a).unwrap();
    lib.forget_changed(&lib.changed_records().unwrap()).unwrap();
    assert!(lib.changed_records().unwrap().is_empty());
    lib.mark_all_changed().unwrap();
    assert_eq!(lib.changed_records().unwrap(), [(RecordKind::Item, a.id.to_string())]);
    assert_eq!(lib.record(RecordKind::Item, "missing").unwrap(), None);
}

#[test]
fn finds_and_merges_duplicates() {
    let mut lib = Library::open_in_memory().unwrap();
    let mut a = article("Quantum Sensing", Some("10.1000/qs"));
    a.year = Some(2020);
    let mut b = article("Quantum sensing.", None);
    (b.year, b.pages) = (Some(2021), Some("1-10".into()));
    let mut old = article("Quantum Sensing", None);
    old.year = Some(2010);
    let mut other_doi = article("Quantum Sensing", Some("10.1000/other"));
    other_doi.year = Some(2020);
    let mut stranger = article("Quantum Sensing", None);
    stranger.creators = vec![Creator::person("Degen", "Christian")];
    let mut book = Item::new(ItemType::Book, "A Book");
    book.isbn = Some("0-306-40615-2".into());
    let mut same_book = Item::new(ItemType::Book, "The Book, Second Printing");
    same_book.isbn = Some("978-0-306-40615-7".into());
    for item in [&a, &b, &old, &other_doi, &stranger, &book, &same_book] {
        lib.add_item(item).unwrap();
    }
    let groups = lib.duplicate_groups().unwrap();
    let sorted = |ids: &[ItemId]| {
        let mut v: Vec<String> = ids.iter().map(|i| i.to_string()).collect();
        v.sort();
        v
    };
    let mut expected_quantum = vec![a.id.to_string(), b.id.to_string(), other_doi.id.to_string()];
    expected_quantum.sort();
    let mut expected_books = vec![book.id.to_string(), same_book.id.to_string()];
    expected_books.sort();
    let found: Vec<Vec<String>> = groups.iter().map(|g| sorted(g.as_slice())).collect();
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found.contains(&expected_quantum), "{found:?}");
    assert!(found.contains(&expected_books), "{found:?}");

    let shelf = lib.create_collection("Shelf", None).unwrap();
    lib.add_to_collection(&shelf.id, &b.id).unwrap();
    lib.set_item_tags(&b.id, &["sensing".to_string()]).unwrap();
    lib.set_starred(&b.id, true).unwrap();
    let pdf = attachment(&b, "pdf");
    lib.add_attachment(&pdf).unwrap();
    let merged = lib.merge_items(&a.id, &[b.id.clone()]).unwrap();
    assert_eq!(merged.pages.as_deref(), Some("1-10"));
    assert_eq!(merged.year, Some(2020), "its own details stay");
    assert_eq!(lib.item(&a.id).unwrap(), Some(merged));
    assert_eq!(lib.attachments(&a.id).unwrap().len(), 1);
    assert_eq!(lib.item_tags(&a.id).unwrap(), ["sensing"]);
    assert_eq!(lib.items_in_collection(&shelf.id).unwrap().len(), 1);
    assert_eq!(lib.items_in_collection(&shelf.id).unwrap()[0].id, a.id);
    assert_eq!(lib.is_trashed(&b.id).unwrap(), Some(true));
    assert!(lib.entry(&a.id).unwrap().unwrap().starred);
    assert!(matches!(lib.merge_items(&ItemId::from_string("missing"), &[]), Err(Error::NotFound(_))));
}

#[test]
fn keeps_runs_and_syncs_saved_searches() {
    let mut lib = Library::open_in_memory().unwrap();
    let a = article("Quantum Sensing", None);
    let b = article("Classical Optics", None);
    lib.add_item(&a).unwrap();
    lib.add_item(&b).unwrap();
    lib.set_item_tags(&b.id, &["Review".to_string()]).unwrap();
    let condition = |field: &str, op: &str, value: &str| Condition {
        field: field.into(),
        op: op.into(),
        value: value.into(),
    };
    let search = SavedSearch {
        id: "s1".into(),
        name: " Quantum or reviews ".into(),
        match_all: false,
        conditions: vec![condition("title", "contains", "quantum"), condition("tag", "is", "review")],
        position: 0,
    };
    lib.save_search(&search).unwrap();
    let kept = lib.saved_search("s1").unwrap().unwrap();
    assert_eq!(kept.name, "Quantum or reviews");
    assert_eq!(lib.run_search(&kept).unwrap().len(), 2);
    let every = SavedSearch { match_all: true, ..kept.clone() };
    lib.save_search(&every).unwrap();
    assert!(lib.run_search(&every).unwrap().is_empty());
    assert_eq!(lib.saved_searches().unwrap(), vec![every.clone()]);

    // Sync carries it to another library, which does not send it back.
    let fields = lib.record(RecordKind::Search, "s1").unwrap().unwrap();
    assert_eq!(fields["name"], "Quantum or reviews");
    let mut there = Library::open_in_memory().unwrap();
    there
        .apply_records(&[Record { kind: RecordKind::Search, id: "s1".into(), fields: Some(fields) }])
        .unwrap();
    assert_eq!(there.saved_searches().unwrap(), [every]);
    assert!(there.changed_records().unwrap().is_empty());

    lib.delete_saved_search("s1").unwrap();
    assert!(lib.changed_records().unwrap().contains(&(RecordKind::Search, "s1".to_string())));
    assert_eq!(lib.record(RecordKind::Search, "s1").unwrap(), None);
    assert!(matches!(lib.delete_saved_search("s1"), Err(Error::NotFound(_))));
}

#[test]
fn relates_items_both_ways_and_syncs_them() {
    let mut lib = Library::open_in_memory().unwrap();
    let (a, b, c) = (article("A", None), article("B", None), article("C", None));
    for item in [&a, &b, &c] {
        lib.add_item(item).unwrap();
    }
    lib.relate_items(&a.id, &b.id).unwrap();
    lib.relate_items(&a.id, &c.id).unwrap();
    lib.relate_items(&a.id, &a.id).unwrap();
    let titles = |lib: &Library, id: &ItemId| -> Vec<String> {
        lib.related_entries(id).unwrap().into_iter().map(|e| e.item.title).collect()
    };
    assert_eq!(titles(&lib, &a.id).len(), 2);
    assert_eq!(titles(&lib, &b.id), ["A"]);
    assert!(lib.relate_items(&a.id, &ItemId::from_string("missing")).is_err());

    // Each side carries the link in its record.
    let fields = lib.record(RecordKind::Item, b.id.as_str()).unwrap().unwrap();
    assert_eq!(fields[&format!("related:{}", a.id)], true);

    // The trash hides a related item; deleting it for good unlinks it.
    lib.trash_item(&c.id).unwrap();
    assert_eq!(titles(&lib, &a.id), ["B"]);
    lib.delete_item(&c.id).unwrap();
    lib.unrelate_items(&b.id, &a.id).unwrap();
    assert!(titles(&lib, &a.id).is_empty());
    assert!(titles(&lib, &b.id).is_empty());
}

#[test]
fn merging_keeps_related_items() {
    let mut lib = Library::open_in_memory().unwrap();
    let (kept, copy, other) =
        (article("Same Work Here", None), article("Same Work Here", None), article("Other", None));
    for item in [&kept, &copy, &other] {
        lib.add_item(item).unwrap();
    }
    lib.relate_items(&copy.id, &other.id).unwrap();
    lib.merge_items(&kept.id, std::slice::from_ref(&copy.id)).unwrap();
    let related: Vec<ItemId> =
        lib.related_entries(&kept.id).unwrap().into_iter().map(|e| e.item.id).collect();
    assert_eq!(related, vec![other.id.clone()]);
    let back: Vec<ItemId> = lib.related_entries(&other.id).unwrap().into_iter().map(|e| e.item.id).collect();
    assert_eq!(back, [kept.id]);
}

#[test]
fn follows_feeds_and_keeps_what_they_announce() {
    let mut lib = Library::open_in_memory().unwrap();
    let feed = lib.add_feed("https://example.org/rss", " Nature ").unwrap();
    assert_eq!((feed.title.as_str(), feed.unread, feed.last_checked), ("Nature", 0, None));
    assert_eq!(lib.add_feed("https://example.org/rss", "Again").unwrap().id, feed.id, "followed once");

    let (a, b) = (article("First", None), article("Second", None));
    let entries = [
        NewFeedEntry { guid: "a", item: &a, link: Some("https://example.org/a") },
        NewFeedEntry { guid: "b", item: &b, link: None },
    ];
    assert_eq!(lib.store_feed_entries(&feed.id, &entries).unwrap(), 2);
    assert_eq!(lib.store_feed_entries(&feed.id, &entries[..1]).unwrap(), 0, "known entries are not new");
    let items = lib.feed_items(Some(&feed.id)).unwrap();
    assert_eq!(items.iter().map(|i| i.item.title.as_str()).collect::<Vec<_>>(), ["First", "Second"]);
    assert_eq!(lib.feed(&feed.id).unwrap().unwrap().unread, 2);
    assert!(lib.feed(&feed.id).unwrap().unwrap().last_checked.is_some());

    lib.set_feed_item_read(&items[0].id, true).unwrap();
    assert_eq!(lib.feeds().unwrap()[0].unread, 1);
    lib.mark_feed_read(None).unwrap();
    assert!(lib.feed_items(None).unwrap().iter().all(|i| i.read));
    lib.set_feed_problem(&feed.id, "403 Forbidden").unwrap();
    assert_eq!(lib.feed(&feed.id).unwrap().unwrap().problem.as_deref(), Some("403 Forbidden"));

    lib.rename_feed(&feed.id, "Nature Physics").unwrap();
    assert_eq!(lib.feed_item(&items[1].id).unwrap().unwrap().feed_id, feed.id);
    lib.remove_feed(&feed.id).unwrap();
    assert!(lib.feeds().unwrap().is_empty());
    assert!(lib.feed_items(None).unwrap().is_empty(), "its entries went with it");
}
