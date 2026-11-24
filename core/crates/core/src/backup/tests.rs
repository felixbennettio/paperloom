use super::*;
use crate::NewItem;

fn paper(title: &str) -> NewItem {
    NewItem {
        item_type: String::new(),
        title: title.into(),
        authors: vec!["Marie Curie".into()],
        container_title: None,
        year: Some(1903),
        doi: None,
    }
}

#[test]
fn crc_and_dates_are_zips() {
    assert_eq!(crc32_update(0, b"123456789"), 0xcbf4_3926);
    assert_eq!(crc32_update(crc32_update(0, b"1234"), b"56789"), 0xcbf4_3926, "in pieces too");
    // 2026-10-12 15:04:06 UTC.
    let (time, date) = dos_time(1_791_817_446_000);
    assert_eq!((time >> 11, (time >> 5) & 0x3f, (time & 0x1f) * 2), (15, 4, 6));
    assert_eq!((date >> 9) + 1980, 2026);
    assert_eq!(((date >> 5) & 0x0f, date & 0x1f), (10, 12));
}

#[test]
fn writes_zips_it_can_read() {
    let dir = tempfile::tempdir().unwrap();
    let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.bin"));
    fs::write(&a, "hello").unwrap();
    fs::write(&b, vec![7u8; 100_000]).unwrap();
    let zip = dir.path().join("out.zip");
    let mut writer = ZipWriter::create(&zip).unwrap();
    writer.add_file("a.txt", &a).unwrap();
    writer.add_file("folder/中文.bin", &b).unwrap();
    let length = writer.finish().unwrap();
    assert_eq!(length, fs::metadata(&zip).unwrap().len());

    let mut file = File::open(&zip).unwrap();
    let entries = read_entries(&mut file).unwrap();
    let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["a.txt", "folder/中文.bin"]);
    let out = dir.path().join("back.bin");
    extract(&mut file, &entries[1], &out).unwrap();
    assert_eq!(fs::read(&out).unwrap(), vec![7u8; 100_000]);
    assert!(inside(dir.path(), "../escape").is_none());
    assert!(inside(dir.path(), "/etc/passwd").is_none());
    assert!(inside(dir.path(), "files/a/b.pdf").is_some());
}

#[test]
fn backs_up_and_puts_a_library_back() {
    let here = tempfile::tempdir().unwrap();
    let path = here.path().join("Library/library.sqlite");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let library = PaperLibrary::new(path.display().to_string()).unwrap();
    let vault = PaperVault::open(here.path().join("Vault").display().to_string()).unwrap();
    let item = library.add_item(paper("Recherches sur les substances radioactives")).unwrap();
    let pdf = here.path().join("these.pdf");
    fs::write(&pdf, b"%PDF-1.4 these").unwrap();
    library.attach_file(item.id.clone(), pdf.display().to_string()).unwrap();
    vault.write_note("Reading.md".into(), "# Radium".into()).unwrap();
    fs::create_dir_all(here.path().join("Library/styles")).unwrap();
    fs::write(here.path().join("Library/styles/lab.csl"), "<style/>").unwrap();
    fs::write(here.path().join("Library/styles/index.json"), "[]").unwrap();

    let zip = here.path().join("Backup.zip");
    let report = library.back_up(zip.display().to_string(), Some(vault.clone())).unwrap();
    assert_eq!(report.files, 5, "manifest, database, one file, one style, one note");
    assert!(report.bytes > 0 && zip.exists());
    assert!(!here.path().join("Backup.zip.partial").exists());

    // Elsewhere: an empty library and vault take the backup.
    let there = tempfile::tempdir().unwrap();
    let other_path = there.path().join("library.sqlite");
    let other = PaperLibrary::new(other_path.display().to_string()).unwrap();
    let other_vault = PaperVault::open(there.path().join("Vault").display().to_string()).unwrap();
    other_vault.write_note("Reading.md".into(), "# Mine".into()).unwrap();
    let restored = other.restore_backup(zip.display().to_string(), Some(other_vault.clone())).unwrap();
    assert_eq!((restored.notes, restored.attachments), (1, 1));
    assert_eq!(other_vault.read_note("Reading.md".into()).unwrap(), "# Radium");
    assert_eq!(other_vault.note_versions("Reading.md".into()).unwrap().len(), 1, "what it replaced is kept");
    assert!(other.item_detail(item.id.clone()).unwrap().is_none(), "the library follows on restart");
    assert_eq!(fs::read_to_string(there.path().join("styles/lab.csl")).unwrap(), "<style/>");
    assert!(!there.path().join("styles/index.json").exists());
    drop(other);

    assert!(finish_restore(other_path.display().to_string()).unwrap());
    assert!(!finish_restore(other_path.display().to_string()).unwrap(), "once");
    let reopened = PaperLibrary::new(other_path.display().to_string()).unwrap();
    assert_eq!(
        reopened.item_detail(item.id.clone()).unwrap().unwrap().title,
        "Recherches sur les substances radioactives"
    );
    let attachments = reopened.attachments(item.id.clone()).unwrap();
    assert_eq!(fs::read(&attachments[0].file_path).unwrap(), b"%PDF-1.4 these");
    let aside = fs::read_dir(there.path())
        .unwrap()
        .flatten()
        .any(|e| e.file_name().to_string_lossy().starts_with("before-restore-"));
    assert!(aside, "the library that was there is kept");

    let not_backup = there.path().join("other.zip");
    let mut writer = ZipWriter::create(&not_backup).unwrap();
    writer.add_file("a.txt", &pdf).unwrap();
    writer.finish().unwrap();
    assert!(matches!(
        reopened.restore_backup(not_backup.display().to_string(), None),
        Err(CoreError::InvalidInput { .. })
    ));
}

#[test]
fn backs_up_on_a_schedule_and_keeps_the_newest() {
    let here = tempfile::tempdir().unwrap();
    let path = here.path().join("library.sqlite");
    let library = PaperLibrary::new(path.display().to_string()).unwrap();
    library.add_item(paper("Radioactivity")).unwrap();

    let made = library.back_up_if_due(None, None, 24, 3).unwrap().expect("the first is due");
    assert!(
        made.path.starts_with(&here.path().join("backups").join("Paperloom ").display().to_string()),
        "{}",
        made.path
    );
    assert!(made.path.ends_with(".zip") && Path::new(&made.path).exists());
    assert!(library.back_up_if_due(None, None, 24, 3).unwrap().is_none(), "not due again within a day");

    let folder = here.path().join("elsewhere");
    fs::create_dir_all(&folder).unwrap();
    for old in ["Paperloom 2020-01-01 000000.zip", "Paperloom 2021-01-01 000000.zip", "notes.txt"] {
        fs::write(folder.join(old), b"old").unwrap();
    }
    let folder_name = Some(folder.display().to_string());
    assert!(library.back_up_if_due(folder_name.clone(), None, 0, 2).unwrap().is_some());
    let mut left: Vec<String> = fs::read_dir(&folder)
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    left.sort();
    assert_eq!(left.len(), 3, "two backups and the other file: {left:?}");
    assert_eq!(left[0], "Paperloom 2021-01-01 000000.zip", "{left:?}");
    assert_eq!(left[2], "notes.txt");
}
