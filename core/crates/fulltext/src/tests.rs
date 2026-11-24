use super::*;

fn doc(kind: Kind, key: &str, title: &str, body: &str) -> Document {
    Document {
        kind,
        key: key.into(),
        item: None,
        attachment: None,
        page: None,
        title: title.into(),
        body: body.into(),
    }
}

fn keys(hits: &[Hit]) -> Vec<&str> {
    hits.iter().map(|h| h.key.as_str()).collect()
}

fn sample() -> FullText {
    let index = FullText::in_memory().unwrap();
    index
        .put(&[
            doc(
                Kind::Item,
                "item-1",
                "Attention Is All You Need",
                "Vaswani; transformer models for translation",
            ),
            doc(Kind::Item, "item-2", "基于深度学习的文献分类", "张三；本文研究深度学习在文献分类中的应用"),
            Document {
                item: Some("item-1".into()),
                attachment: Some("pdf-1".into()),
                page: Some(3),
                ..doc(
                    Kind::Pdf,
                    "pdf-1/3",
                    "Attention Is All You Need",
                    "Multi-head attention allows the model to attend",
                )
            },
            Document {
                item: Some("item-1".into()),
                attachment: Some("pdf-1".into()),
                page: Some(3),
                ..doc(
                    Kind::Annotation,
                    "ann-1",
                    "Attention Is All You Need",
                    "multi-head attention is the key idea",
                )
            },
            doc(Kind::Note, "Reading/Transformers.md", "Transformers", "My notes on attention and 深度学习."),
        ])
        .unwrap();
    index
}

#[test]
fn finds_words_in_any_language() {
    let index = sample();
    assert_eq!(index.len(), 5);
    assert_eq!(keys(&index.search("translation", &[], 10).unwrap()), ["item-1"]);
    let chinese = index.search("深度学习", &[], 10).unwrap();
    assert_eq!(chinese.len(), 2, "{chinese:?}");
    assert_eq!(chinese[0].key, "item-2", "the title match ranks first");
    assert_eq!(keys(&index.search("文献分类", &[], 10).unwrap()), ["item-2"]);
}

#[test]
fn needs_every_word_and_filters_by_kind() {
    let index = sample();
    let both = index.search("multi-head attention", &[], 10).unwrap();
    assert_eq!(both.len(), 2);
    let pages = index.search("attention", &[Kind::Pdf], 10).unwrap();
    assert_eq!(keys(&pages), ["pdf-1/3"]);
    assert_eq!((pages[0].page, pages[0].item.as_deref()), (Some(3), Some("item-1")));
    assert!(pages[0].snippet.contains("<b>"), "{}", pages[0].snippet);
    assert!(index.search("attention translation", &[Kind::Note], 10).unwrap().is_empty());
}

#[test]
fn gives_back_what_it_indexed() {
    let index = sample();
    let body = index.body("Reading/Transformers.md").unwrap().unwrap();
    assert!(!body.is_empty());
    assert_eq!(index.body("nothing").unwrap(), None);
}

#[test]
fn lists_live_documents_and_resolves_source_metadata() {
    let index = sample();
    index.put(&[doc(Kind::Note, "Reading/Transformers.md", "Revised", "New body")]).unwrap();
    index.remove_keys(&["item-2".into()]).unwrap();
    let documents = index.documents().unwrap();
    assert_eq!(documents.len(), 4);
    assert!(documents.windows(2).all(|pair| pair[0].key < pair[1].key));
    let note = documents.iter().find(|doc| doc.key == "Reading/Transformers.md").unwrap();
    assert_eq!((note.title.as_str(), note.body.as_str()), ("Revised", "New body"));
    let hit = index.hit("pdf-1/3").unwrap().unwrap();
    assert_eq!(hit.kind, Kind::Pdf);
    assert_eq!(
        (hit.item.as_deref(), hit.attachment.as_deref(), hit.page),
        (Some("item-1"), Some("pdf-1"), Some(3))
    );
    assert!(index.hit("item-2").unwrap().is_none());
}

#[test]
fn replaces_and_removes() {
    let index = sample();
    index.put(&[doc(Kind::Note, "Reading/Transformers.md", "Transformers", "rewritten entirely")]).unwrap();
    assert!(index.search("深度学习", &[Kind::Note], 10).unwrap().is_empty());
    assert_eq!(index.len(), 5);
    assert!(index.has_pages("pdf-1").unwrap());
    index.remove_item("item-1").unwrap();
    assert_eq!(index.len(), 2, "the item, its page and its annotation are gone");
    assert!(!index.has_pages("pdf-1").unwrap());
    index.remove_kind(Kind::Note).unwrap();
    index.remove_keys(&["item-2".into()]).unwrap();
    assert!(index.is_empty());
}

#[test]
fn reopens_and_rebuilds_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("index");
    {
        let (index, fresh) = FullText::open(&folder).unwrap();
        assert!(fresh);
        index.put(&[doc(Kind::Item, "a", "Kept", "on disk")]).unwrap();
    }
    {
        let (index, fresh) = FullText::open(&folder).unwrap();
        assert!(!fresh);
        assert_eq!(keys(&index.search("disk", &[], 5).unwrap()), ["a"]);
    }
    fs::write(folder.join(FORMAT_FILE), "0").unwrap();
    let (index, fresh) = FullText::open(&folder).unwrap();
    assert!(fresh && index.is_empty(), "an older format starts over");
}

#[test]
fn finds_related_documents() {
    let index = sample();
    index
        .put(&[
            doc(Kind::Note, "Bread.md", "Bread", "Sourdough bread needs a starter and patience."),
            doc(
                Kind::Note,
                "Draft.md",
                "Draft",
                "Attention for translation: the transformer, and attention again.",
            ),
        ])
        .unwrap();
    let text = "Attention for translation: the transformer, and attention again.";
    let found = index.related(text, &[], &["Draft.md".to_string()], 10).unwrap();
    assert_eq!(found[0].key, "item-1", "{:?}", keys(&found));
    assert!(!keys(&found).contains(&"Draft.md") && !keys(&found).contains(&"Bread.md"), "{:?}", keys(&found));
    let notes = index.related(text, &[Kind::Note], &[], 10).unwrap();
    assert_eq!(keys(&notes), ["Draft.md", "Reading/Transformers.md"]);
    let chinese = index.related("关于深度学习的笔记", &[Kind::Item], &[], 10).unwrap();
    assert_eq!(keys(&chinese), ["item-2"]);
    assert!(index.related("the and of", &[], &[], 10).unwrap().is_empty(), "no telling words");
}

#[test]
fn snippets_are_one_line() {
    assert_eq!(
        one_line("Ashish Vaswani\nNoam <b>Shazeer</b>\n\nNature", Kind::Item),
        "Ashish Vaswani · Noam <b>Shazeer</b> · Nature"
    );
    assert_eq!(
        one_line("# Title\n\nSelf-<b>attention</b> wins", Kind::Note),
        "# Title Self-<b>attention</b> wins"
    );
}
