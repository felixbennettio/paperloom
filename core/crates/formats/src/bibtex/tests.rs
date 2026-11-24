use super::*;

const SAMPLE: &str = r#"
% A comment line outside entries is ignored.
@string{nat = "Nature"}

@comment{ anything {at all} here }

@article{kucsko2013,
  author = {Kucsko, G. and Maurer, P. C. and Lukin, Mikhail D.},
  title = {Nanometre-scale thermometry in a {living} cell},
  journal = nat,
  year = 2013,
  month = jul,
  volume = {500},
  number = "7460",
  pages = {54--58},
  doi = {10.1038/nature12373},
  keywords = {thermometry, NV centres},
}

@inproceedings{schrodinger,
  author = {Schr{\"o}dinger, Erwin and {World Health Organization}},
  title = {On {\'e}l{\`e}ve \& the Stra\ss e},
  booktitle = {Proceedings of the 1st Conference},
  date = {2020-05-01},
  publisher = {ACM},
  address = {New York}
}

@misc{vaswani2017attention,
  title={Attention Is All You Need},
  author={Ashish Vaswani and Noam Shazeer},
  year={2017},
  eprint={1706.03762},
  archivePrefix={arXiv},
  primaryClass={cs.CL}
}

@article{zhang2021,
  author = {张三 and 李四},
  title = {基于深度学习的文献分类},
  journal = {计算机学报},
  year = {2021}
}

@article{broken, title = {Never closed
"#;

#[test]
fn reads_entries() {
    let parsed = parse(SAMPLE);
    assert_eq!(parsed.items.len(), 4);
    assert_eq!(parsed.skipped, 1);

    let a = &parsed.items[0];
    assert_eq!(a.item_type, ItemType::JournalArticle);
    assert_eq!(a.title, "Nanometre-scale thermometry in a living cell");
    assert_eq!(a.creators[2], Creator::person("Lukin", "Mikhail D."));
    assert_eq!(a.container_title.as_deref(), Some("Nature"), "@string macro");
    assert_eq!(a.date.as_deref(), Some("2013-07"), "month macro");
    assert_eq!(a.issue.as_deref(), Some("7460"));
    assert_eq!(a.pages.as_deref(), Some("54-58"));
    assert_eq!(a.keywords, ["thermometry", "NV centres"]);

    let b = &parsed.items[1];
    assert_eq!(b.item_type, ItemType::ConferencePaper);
    assert_eq!(b.creators[0], Creator::person("Schrödinger", "Erwin"));
    assert_eq!(b.creators[1], Creator::literal("World Health Organization"));
    assert_eq!(b.title, "On élève & the Straße");
    assert_eq!(b.date.as_deref(), Some("2020-05-01"));
    assert_eq!(b.year, Some(2020));
    assert_eq!(b.place.as_deref(), Some("New York"));

    let c = &parsed.items[2];
    assert_eq!(c.item_type, ItemType::Preprint);
    assert_eq!(c.arxiv_id.as_deref(), Some("1706.03762"));
    assert_eq!(c.creators[0], Creator::person("Vaswani", "Ashish"));

    let d = &parsed.items[3];
    assert_eq!(d.creators, vec![Creator::literal("张三"), Creator::literal("李四")]);
}

#[test]
fn decodes_latex() {
    assert_eq!(decode_latex(r#"Schr\"{o}dinger"#), "Schrödinger");
    assert_eq!(decode_latex(r#"G{\"o}del and Erd\H{o}s"#), "Gödel and Erdős");
    assert_eq!(decode_latex(r#"\c{C}elik \v{C}apek \aa{}ngstr\"om"#), "Çelik Čapek ångström");
    assert_eq!(
        decode_latex(r"\emph{Deep} learning: 50\% \& more $\alpha$"),
        "Deep learning: 50% & more alpha"
    );
    assert_eq!(decode_latex("non~breaking"), "non breaking");
}

#[test]
fn splits_names_at_top_level() {
    assert_eq!(split_names("A and B AND C"), ["A", "B", "C"]);
    assert_eq!(split_names("{Barnes and Noble} and Smith, J."), ["{Barnes and Noble}", "Smith, J."]);
}

#[test]
fn round_trips() {
    let items = parse(SAMPLE).items;
    let text = write(&items);
    assert!(text.contains("@article{kucsko2013nanometre,"), "{text}");
    assert!(text.contains("pages = {54--58}"), "{text}");
    assert!(text.contains("author = {{张三} and {李四}}"), "{text}");
    assert!(text.contains(r"title = {On élève \& the Straße}"), "{text}");
    let again = parse(&text).items;
    assert_eq!(again.len(), items.len());
    for (a, b) in items.iter().zip(&again) {
        assert_eq!((&a.title, &a.creators, &a.item_type), (&b.title, &b.creators, &b.item_type));
        assert_eq!((&a.year, &a.pages, &a.doi, &a.arxiv_id), (&b.year, &b.pages, &b.doi, &b.arxiv_id));
    }
}
