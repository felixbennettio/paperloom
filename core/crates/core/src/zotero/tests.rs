use std::fs;

use paperloom_model::Creator;

use super::*;

/// A small Zotero data folder: the tables the import reads, two papers, a
/// deleted one, a file (and one gone), a highlight and a note.
fn zotero_folder() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let db = Connection::open(dir.path().join("zotero.sqlite")).unwrap();
    db.execute_batch(
        r#"
        CREATE TABLE itemTypes (itemTypeID INTEGER PRIMARY KEY, typeName TEXT);
        CREATE TABLE items (itemID INTEGER PRIMARY KEY, itemTypeID INT, dateAdded TEXT, dateModified TEXT,
            libraryID INT, key TEXT);
        CREATE TABLE fields (fieldID INTEGER PRIMARY KEY, fieldName TEXT);
        CREATE TABLE itemDataValues (valueID INTEGER PRIMARY KEY, value);
        CREATE TABLE itemData (itemID INT, fieldID INT, valueID INT);
        CREATE TABLE creators (creatorID INTEGER PRIMARY KEY, firstName TEXT, lastName TEXT, fieldMode INT);
        CREATE TABLE creatorTypes (creatorTypeID INTEGER PRIMARY KEY, creatorType TEXT);
        CREATE TABLE itemCreators (itemID INT, creatorID INT, creatorTypeID INT, orderIndex INT);
        CREATE TABLE tags (tagID INTEGER PRIMARY KEY, name TEXT);
        CREATE TABLE itemTags (itemID INT, tagID INT, type INT);
        CREATE TABLE collections (collectionID INTEGER PRIMARY KEY, collectionName TEXT, parentCollectionID INT,
            libraryID INT, key TEXT);
        CREATE TABLE collectionItems (collectionID INT, itemID INT, orderIndex INT);
        CREATE TABLE deletedItems (itemID INTEGER PRIMARY KEY, dateDeleted TEXT);
        CREATE TABLE itemAttachments (itemID INTEGER PRIMARY KEY, parentItemID INT, linkMode INT,
            contentType TEXT, path TEXT);
        CREATE TABLE itemNotes (itemID INTEGER PRIMARY KEY, parentItemID INT, note TEXT, title TEXT);
        CREATE TABLE itemAnnotations (itemID INTEGER PRIMARY KEY, parentItemID INT, type INT, text TEXT,
            comment TEXT, color TEXT, position TEXT);

        INSERT INTO itemTypes VALUES (1, 'journalArticle'), (2, 'attachment'), (3, 'note'), (4, 'annotation'),
            (5, 'thesis');
        INSERT INTO items VALUES
            (1, 1, '2024-01-01 00:00:00', '2024-01-01 00:00:00', 1, 'AAAA1111'),
            (2, 5, '2024-01-01 00:00:00', '2024-01-01 00:00:00', 1, 'BBBB2222'),
            (3, 1, '2024-01-01 00:00:00', '2024-01-01 00:00:00', 1, 'CCCC3333'),
            (10, 2, '2024-01-01 00:00:00', '2024-01-01 00:00:00', 1, 'ATTACH10'),
            (11, 2, '2024-01-01 00:00:00', '2024-01-01 00:00:00', 1, 'ATTACH11'),
            (20, 3, '2024-01-01 00:00:00', '2024-01-01 00:00:00', 1, 'NOTE0020'),
            (30, 4, '2024-05-01 08:30:00', '2024-05-02 09:00:00', 1, 'ANNOT030');
        INSERT INTO fields VALUES (1, 'title'), (2, 'date'), (3, 'DOI'), (4, 'publicationTitle'), (5, 'extra'),
            (6, 'university'), (7, 'pages');
        INSERT INTO itemDataValues VALUES
            (1, 'Nanometre-scale thermometry in a living cell'), (2, '2013-07-31 July 31, 2013'),
            (3, '10.1038/nature12373'), (4, 'Nature'), (5, '54-58'),
            (6, '基于深度学习的文献分类研究'), (7, '2021-00-00 2021'), (8, '某某大学'), (9, 'PMID: 123
Read_Status: Read
Citation Key: thesis2021'),
            (10, 'Deleted paper');
        INSERT INTO itemData VALUES (1, 1, 1), (1, 2, 2), (1, 3, 3), (1, 4, 4), (1, 7, 5),
            (2, 1, 6), (2, 2, 7), (2, 6, 8), (2, 5, 9), (3, 1, 10);
        INSERT INTO creators VALUES (1, 'G.', 'Kucsko', 0), (2, '三', '张', 0), (3, NULL, 'CERN', 1);
        INSERT INTO creatorTypes VALUES (1, 'author'), (2, 'editor');
        INSERT INTO itemCreators VALUES (1, 1, 1, 0), (1, 3, 2, 1), (2, 2, 1, 0);
        INSERT INTO tags VALUES (1, 'nv'), (2, 'thermometry');
        INSERT INTO itemTags VALUES (1, 1, 0), (1, 2, 0);
        INSERT INTO collections VALUES (1, 'Quantum', 2, 1, 'COLL0001'), (2, 'Physics', NULL, 1, 'COLL0002');
        INSERT INTO collectionItems VALUES (1, 1, 0), (2, 2, 0);
        INSERT INTO deletedItems VALUES (3, '2024-02-01 00:00:00');
        INSERT INTO itemAttachments VALUES (10, 1, 0, 'application/pdf', 'storage:paper.pdf'),
            (11, 1, 0, 'application/pdf', 'storage:gone.pdf');
        INSERT INTO itemNotes VALUES (20, 1, '<div data-schema-version="8"><h1>Reading</h1><p>The <strong>key</strong> idea, see <a href="https://example.org">this</a> &amp; that.</p><ul><li>one</li><li>two</li></ul></div>', 'Reading');
        INSERT INTO itemAnnotations VALUES (30, 10, 1, 'nitrogen-vacancy', 'nice', '#5fb236',
            '{"pageIndex":1,"rects":[[100,200,300,212]]}');
        "#,
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("storage/ATTACH10")).unwrap();
    fs::write(dir.path().join("storage/ATTACH10/paper.pdf"), b"%PDF-1.4 thermometry").unwrap();
    dir
}

#[test]
fn moves_a_zotero_library_in() {
    let zotero = zotero_folder();
    let here = tempfile::tempdir().unwrap();
    let library = PaperLibrary::new(here.path().join("library.sqlite").display().to_string()).unwrap();
    let vault = PaperVault::open(here.path().join("Vault").display().to_string()).unwrap();

    let report = library.import_zotero_from(zotero.path(), Some(&*vault)).unwrap();
    assert_eq!(
        (report.added, report.duplicates, report.collections, report.files, report.missing_files),
        (2, 0, 2, 1, 1),
        "{report:?}"
    );
    assert_eq!((report.annotations, report.notes, report.from_backup), (1, 1, false));

    let store = library.lock().unwrap();
    let items = store.items().unwrap();
    assert_eq!(items.len(), 2, "the deleted paper stays out");
    let paper = items.iter().find(|i| i.title.starts_with("Nanometre")).unwrap();
    assert_eq!(paper.creators[0], Creator::person("Kucsko", "G."));
    assert_eq!((paper.creators[1].literal.as_str(), paper.creators[1].role), ("CERN", CreatorRole::Editor));
    assert_eq!(paper.container_title.as_deref(), Some("Nature"));
    assert_eq!((paper.year, paper.date.as_deref()), (Some(2013), Some("2013-07-31")));
    assert_eq!(store.item_tags(&paper.id).unwrap(), ["nv", "thermometry"]);
    let thesis = items.iter().find(|i| i.item_type == ItemType::Thesis).unwrap();
    assert_eq!(thesis.creators, vec![Creator::literal("张三")]);
    assert_eq!((thesis.year, thesis.date.as_deref()), (Some(2021), None));
    assert_eq!((thesis.publisher.as_deref(), thesis.pmid.as_deref()), (Some("某某大学"), Some("123")));
    assert_eq!(thesis.extra.as_deref(), Some("PMID: 123\nRead_Status: Read\nCitation Key: thesis2021"));

    let collections = store.collections().unwrap();
    let physics = collections.iter().find(|c| c.name == "Physics").unwrap();
    let quantum = collections.iter().find(|c| c.name == "Quantum").unwrap();
    assert_eq!(quantum.parent_id.as_ref(), Some(&physics.id), "children come after their parents");
    assert_eq!(store.items_in_collection(&quantum.id).unwrap()[0].id, paper.id);

    let attachment = &store.attachments(&paper.id).unwrap()[0];
    let marks = store.annotations(&attachment.id).unwrap();
    assert_eq!(marks.len(), 1);
    let mark = &marks[0];
    assert_eq!((mark.kind, mark.page_index, mark.color.as_str()), (AnnotationKind::Highlight, 1, "#5fb236"));
    assert_eq!(mark.rects, [Rect { x: 100.0, y: 200.0, width: 200.0, height: 12.0 }]);
    assert_eq!((mark.text.as_deref(), mark.comment.as_deref()), (Some("nitrogen-vacancy"), Some("nice")));
    assert_eq!(mark.date_added, 1_714_552_200_000);
    drop(store);

    let note = vault.read_note("Zotero/Reading.md".into()).unwrap();
    assert_eq!(
        note,
        "# Reading\n\nThe **key** idea, see [this](https://example.org) & that.\n\n- one\n- two\n\n\
         [[@10.1038/nature12373|Kucsko 2013]]\n"
    );

    // Again: the paper with a DOI is joined, its highlight not doubled.
    let again = library.import_zotero_from(zotero.path(), None).unwrap();
    assert_eq!((again.duplicates, again.annotations, again.collections), (1, 0, 0));
    assert!(again.problems[0].contains("no vault"), "{:?}", again.problems);
}

#[test]
fn brings_files_kept_on_their_own_in_as_items() {
    let zotero = zotero_folder();
    let db = Connection::open(zotero.path().join("zotero.sqlite")).unwrap();
    db.execute_batch(
        r#"
        INSERT INTO items VALUES (12, 2, '2024-01-01 00:00:00', '2024-01-01 00:00:00', 1, 'ATTACH12'),
            (13, 2, '2024-01-01 00:00:00', '2024-01-01 00:00:00', 1, 'ATTACH13');
        INSERT INTO itemAttachments VALUES (12, NULL, 0, 'application/pdf', 'storage:Lecture notes.pdf'),
            (13, NULL, 0, 'application/pdf', 'storage:missing.pdf');
        INSERT INTO itemDataValues VALUES (11, 'Quantum sensing lecture');
        INSERT INTO itemData VALUES (12, 1, 11);
        INSERT INTO collectionItems VALUES (2, 12, 1);
        "#,
    )
    .unwrap();
    drop(db);
    fs::create_dir_all(zotero.path().join("storage/ATTACH12")).unwrap();
    fs::write(zotero.path().join("storage/ATTACH12/Lecture notes.pdf"), b"%PDF-1.4 lecture").unwrap();
    let here = tempfile::tempdir().unwrap();
    let library = PaperLibrary::new(here.path().join("library.sqlite").display().to_string()).unwrap();

    let report = library.import_zotero_from(zotero.path(), None).unwrap();
    assert_eq!((report.added, report.files, report.missing_files), (3, 2, 2), "{report:?}");
    let store = library.lock().unwrap();
    let lecture = store.items().unwrap().into_iter().find(|i| i.title == "Quantum sensing lecture").unwrap();
    assert_eq!(lecture.item_type, ItemType::Other);
    assert_eq!(store.attachments(&lecture.id).unwrap().len(), 1);
    let physics = store.collections().unwrap().into_iter().find(|c| c.name == "Physics").unwrap();
    assert!(store.items_in_collection(&physics.id).unwrap().iter().any(|i| i.id == lecture.id));
}

#[test]
fn reads_zotero_dates_and_refuses_other_folders() {
    assert_eq!(zotero_date("2013-07-31 July 31, 2013"), (Some(2013), Some("2013-07-31".into())));
    assert_eq!(zotero_date("2013-07-00 July 2013"), (Some(2013), Some("2013-07".into())));
    assert_eq!(zotero_date("0000-00-00 n.d."), (None, None));
    assert_eq!(zotero_time("1970-01-02 00:00:01"), Some(86_401_000));
    let empty = tempfile::tempdir().unwrap();
    let library = PaperLibrary::in_memory().unwrap();
    assert!(matches!(library.import_zotero_from(empty.path(), None), Err(CoreError::InvalidInput { .. })));
}
