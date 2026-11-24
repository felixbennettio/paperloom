use std::fs;

use super::*;

fn vault_with(files: &[(&str, &str)]) -> (tempfile::TempDir, Vault) {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in files {
        let full = dir.path().join(path);
        fs::create_dir_all(full.parent().unwrap()).unwrap();
        fs::write(full, text).unwrap();
    }
    let vault = Vault::open(dir.path()).unwrap();
    (dir, vault)
}

const FILES: &[(&str, &str)] = &[
    (
        "Index.md",
        "# Index\nStart with [[Transformer]] and [[papers/Transformer|the paper note]].\nAlso [[Missing note]].\n",
    ),
    (
        "papers/Transformer.md",
        "---\naliases: [Attention]\n---\n# Transformer\nSee ![[figure.png]] and #ml/nlp.\n",
    ),
    ("papers/figure.png", "png"),
    ("drafts/Transformer.md", "A different note with the same name.\n"),
    ("Thoughts.md", "The transformer idea is everywhere; attention too.\n[[Index]]\n"),
    (".obsidian/app.json", "{}"),
];

#[test]
fn scans_notes_files_and_folders() {
    let (_dir, vault) = vault_with(FILES);
    let paths: Vec<&str> = vault.notes().map(|n| n.path.as_str()).collect();
    assert_eq!(paths, ["Index.md", "Thoughts.md", "drafts/Transformer.md", "papers/Transformer.md"]);
    assert_eq!(vault.files().collect::<Vec<_>>(), ["papers/figure.png"]);
    assert_eq!(vault.folders().collect::<Vec<_>>(), ["drafts", "papers"]);
    assert_eq!(vault.note("papers/Transformer.md").unwrap().parsed.aliases, ["Attention"]);
}

#[test]
fn resolves_links_like_obsidian() {
    let (_dir, vault) = vault_with(FILES);
    // Same folder first, else the shortest path.
    assert_eq!(vault.resolve("Transformer", "papers/Other.md").as_deref(), Some("papers/Transformer.md"));
    assert_eq!(vault.resolve("transformer", "Index.md").as_deref(), Some("drafts/Transformer.md"));
    assert_eq!(vault.resolve("papers/Transformer", "Index.md").as_deref(), Some("papers/Transformer.md"));
    assert_eq!(vault.resolve("Transformer.md", "drafts/x.md").as_deref(), Some("drafts/Transformer.md"));
    assert_eq!(vault.resolve("figure.png", "Index.md").as_deref(), Some("papers/figure.png"));
    assert_eq!(vault.resolve("Missing note", "Index.md"), None);
    assert_eq!(vault.resolve("", "Index.md").as_deref(), Some("Index.md"));
}

#[test]
fn finds_backlinks_and_unlinked_mentions() {
    let (_dir, vault) = vault_with(FILES);
    let back: Vec<(String, usize)> =
        vault.backlinks("papers/Transformer.md").into_iter().map(|m| (m.path, m.line)).collect();
    assert_eq!(back, [("Index.md".to_string(), 1)]);
    let index_back = vault.backlinks("Index.md");
    assert_eq!(index_back[0].context, "[[Index]]");
    let unlinked: Vec<String> =
        vault.unlinked_mentions("papers/Transformer.md").into_iter().map(|m| m.path).collect();
    assert_eq!(unlinked, ["Thoughts.md"], "Index.md only names it inside links");
}

#[test]
fn renames_and_updates_links() {
    let (dir, mut vault) = vault_with(FILES);
    let changed = vault.rename("papers/Transformer.md", "papers/Attention Is All You Need.md").unwrap();
    assert_eq!(changed, 1);
    let index = fs::read_to_string(dir.path().join("Index.md")).unwrap();
    assert_eq!(
        index,
        "# Index\nStart with [[Transformer]] and [[Attention Is All You Need|the paper note]].\nAlso [[Missing note]].\n",
        "the link that meant drafts/Transformer.md is untouched"
    );
    assert!(dir.path().join("papers/Attention Is All You Need.md").exists());
    assert!(matches!(vault.rename("Index.md", "Thoughts.md"), Err(Error::Exists(_))));
    assert!(matches!(vault.rename("Nope.md", "x.md"), Err(Error::NotFound(_))));
}

#[test]
fn writes_creates_and_deletes() {
    let (dir, mut vault) = vault_with(&[]);
    let first = vault.create_note("", "  New: idea?  ").unwrap();
    assert_eq!(first, "New idea.md");
    assert_eq!(vault.create_note("", "New idea").unwrap(), "New idea 2.md");
    assert_eq!(vault.create_note("inbox/today", "").unwrap(), "inbox/today/Untitled.md");
    assert_eq!(vault.folders().collect::<Vec<_>>(), ["inbox", "inbox/today"]);
    vault.write("New idea", "Links to [[New idea 2]]").unwrap();
    assert_eq!(vault.backlinks("New idea 2.md").len(), 1);
    vault.delete("New idea.md").unwrap();
    assert!(dir.path().join(".trash/New idea.md").exists());
    assert!(vault.backlinks("New idea 2.md").is_empty());
    for bad in ["../outside.md", "/etc/passwd", ".obsidian/x.md", "a//b.md", ""] {
        assert!(matches!(vault.write(bad, "x"), Err(Error::InvalidPath(_))), "{bad}");
    }
}

#[test]
fn refresh_picks_up_outside_edits() {
    let (dir, mut vault) = vault_with(&[("A.md", "one")]);
    assert!(!vault.refresh().unwrap());
    fs::write(dir.path().join("B.md"), "[[A]]").unwrap();
    fs::remove_file(dir.path().join("A.md")).unwrap();
    assert!(vault.refresh().unwrap());
    let paths: Vec<&str> = vault.notes().map(|n| n.path.as_str()).collect();
    assert_eq!(paths, ["B.md"]);
}

#[test]
fn switcher_and_search() {
    let (_dir, vault) = vault_with(FILES);
    let titles = |q: &str| vault.switcher(q, 10).into_iter().map(|n| n.path.clone()).collect::<Vec<_>>();
    assert_eq!(titles("ind"), ["Index.md"]);
    assert_eq!(titles("attention")[0], "papers/Transformer.md", "by alias");
    assert!(titles("tfmr").contains(&"papers/Transformer.md".to_string()), "fuzzy");
    assert!(titles("zzz").is_empty());

    let hits = |q: &str| vault.search(q, 10).into_iter().map(|m| (m.path, m.line)).collect::<Vec<_>>();
    assert_eq!(hits("everywhere attention"), [("Thoughts.md".to_string(), 0)]);
    assert_eq!(hits("tag:ml"), [("papers/Transformer.md".to_string(), 0)]);
    assert_eq!(hits("path:drafts name"), [("drafts/Transformer.md".to_string(), 0)]);
    assert_eq!(hits("\"same name\""), [("drafts/Transformer.md".to_string(), 0)]);
    assert_eq!(vault.tags(), [("ml/nlp".to_string(), 1)]);
}

#[test]
fn renders_links_tags_embeds_and_comments() {
    let (_dir, vault) = vault_with(FILES);
    let text = "# Notes & Ideas\n[[Transformer#Method|method]] [[Nowhere]] #idea %%secret%%\n\n![[figure.png]]\n\n<span style=\"color:#e03e3e\">red</span>\n\n![[papers/Transformer]]\n";
    let html = vault.render("Index.md", text);
    let has = |needle: &str| assert!(html.contains(needle), "{needle} in {html}");
    has(r#"<h1 id="notes-ideas">"#);
    has(
        r#"<a class="wikilink" href="paperloom:note/drafts/Transformer.md#method" data-note="drafts/Transformer.md" data-subpath="Method">method</a>"#,
    );
    has(r#"<a class="wikilink unresolved" href="paperloom:new/Nowhere" data-target="Nowhere">Nowhere</a>"#);
    has(r#"<a class="tag" href="paperloom:tag/idea" data-tag="idea">#idea</a>"#);
    has(
        r#"<img class="embed-image" src="paperloom-vault:///papers/figure.png" data-src="papers/figure.png" alt="figure.png">"#,
    );
    has(r#"<span style="color:#e03e3e">red</span>"#);
    has(r#"<div class="embed" data-note="papers/Transformer.md">"#);
    has(r#"<h1 id="transformer">Transformer</h1>"#);
    assert!(!html.contains("secret"), "{html}");
}

#[test]
fn renders_highlightr_marks_without_indexing_the_colour() {
    let text = r##"The <mark style="background: #FFF3A3A6;">quick</mark> brown fox #animal"##;
    let (_dir, vault) = vault_with(&[("Highlight.md", text)]);
    assert_eq!(
        vault.render("Highlight.md", text),
        "<p>The <mark style=\"background:#FFF3A3A6\">quick</mark> brown fox <a class=\"tag\" href=\"paperloom:tag/animal\" data-tag=\"animal\">#animal</a></p>\n"
    );
    assert_eq!(vault.note("Highlight.md").unwrap().parsed.tags, ["animal"]);
    assert_eq!(vault.tags(), [("animal".into(), 1)]);
}

#[test]
fn renders_the_allowed_html_tags_and_inline_markdown() {
    let (_dir, vault) = vault_with(&[]);
    for tag in [
        "mark", "span", "sub", "sup", "kbd", "u", "s", "del", "ins", "small", "abbr", "b", "i", "em",
        "strong", "code", "details", "summary", "div", "p",
    ] {
        let source = format!("<{tag}>text</{tag}>");
        let html = vault.render("A.md", &source);
        assert!(html.contains(&source), "{tag}: {html}");
    }
    assert_eq!(vault.render("A.md", "a<br>b<br />c"), "<p>a<br>b<br>c</p>\n");
    let html = vault.render("A.md", "A <mark>**quick** and *brown* [[Fox]] #animal</mark>.");
    assert!(html.contains("<mark><strong>quick</strong> and <em>brown</em>"), "{html}");
    assert!(html.contains("class=\"wikilink unresolved\""), "{html}");
    assert!(html.contains("#animal</a></mark>"), "{html}");
    let html =
        vault.render("A.md", "<details>\n<summary>Title</summary>\n<div><p>Body</p></div>\n</details>");
    assert!(
        html.contains("<details>\n<summary>Title</summary>\n<div><p>Body</p></div>\n</details>"),
        "{html}"
    );
}

#[test]
fn restricts_raw_html_attributes_and_styles() {
    let (_dir, vault) = vault_with(&[]);
    let html = vault.render(
        "A.md",
        r##"<SPAN title='A "title"' class="highlightr yellow_1" onclick="alert(1)" ONMOUSEOVER="bad()" id="x" data-note="x" style="background: #fff3; position: fixed; color: RebeccaPurple; text-decoration: underline wavy; background-image: url(https://bad)">text</SPAN>"##,
    );
    assert_eq!(
        html,
        "<p><span title=\"A &quot;title&quot;\" class=\"highlightr yellow_1\" style=\"background:#fff3;color:RebeccaPurple;text-decoration:underline wavy\">text</span></p>\n"
    );
    for value in [
        "#abc",
        "#abcd",
        "#aBcDeF",
        "#FFF3A3A6",
        "rgb(1, 2, 255)",
        "rgba(1, 2, 3, .5)",
        "rgba(1.5, 2, 3, 1)",
        "red",
        "LightGoldenRodYellow",
        "transparent",
    ] {
        for property in ["background", "background-color", "color"] {
            let source = format!("<mark style=\"{property}: {value};\">x</mark>");
            let html = vault.render("A.md", &source);
            assert!(html.contains(&format!("style=\"{property}:{value}\"")), "{source}: {html}");
        }
    }
    for value in [
        "javascript:alert(1)",
        "url(javascript:alert(1))",
        "URL (https://bad)",
        "expression(alert(1))",
        "var(--color)",
        "red !important",
        "notacolour",
        "#ab",
        "#abcde",
        "#abcdefg",
        "#123456789",
        "rgb(1,2)",
        "rgb(1,2,3,4)",
        "rgb(256,0,0)",
        "rgba(1,2,3,2)",
        "rgb(-1,0,0)",
        "rgb(1e2,0,0)",
        "rgb(10%,0,0)",
        "red/**/",
        "r\\65 d",
        "&#114;ed",
        "red;background-image:url(x)",
    ] {
        let source = format!("<mark style=\"color:{value}\">x</mark>");
        let html = vault.render("A.md", &source);
        if value.starts_with("red;") {
            assert!(html.contains("style=\"color:red\""), "{html}");
        } else {
            assert_eq!(html, "<p><mark>x</mark></p>\n", "{source}");
        }
    }
    for style in [
        "position:fixed",
        "behavior:expression(alert(1))",
        "text-decoration:url(x)",
        "text-decoration:underline red",
        "text-decoration:none underline",
        "color red",
        "color:",
    ] {
        let html = vault.render("A.md", &format!("<span style=\"{style}\">x</span>"));
        assert_eq!(html, "<p><span>x</span></p>\n", "{style}");
    }
    for class in ["ok bad:name", "x.y", "x&#32;y", "123", "x/y"] {
        let html = vault.render("A.md", &format!("<span class=\"{class}\">x</span>"));
        assert_eq!(html, "<p><span>x</span></p>\n", "{class}");
    }
}

#[test]
fn escapes_unknown_malformed_and_unbalanced_html() {
    let (_dir, vault) = vault_with(&[]);
    for source in [
        "<widget>x</widget>",
        "<script>alert(1)</script>",
        "<iframe src=\"https://bad\"></iframe>",
        "<img src=x onerror=alert(1)>",
        "<svg><script>alert(1)</script></svg>",
        "<!-- <script>alert(1)</script> -->",
        "<mark>x",
        "x</mark>",
        "<span/>",
        "<mark><span>x</mark></span>",
        "<span title=\"unterminated>x</span>",
        "<span title=\"x\"onclick=\"bad()\">x</span>",
    ] {
        let html = vault.render("A.md", source);
        for tag in ["widget", "script", "iframe", "img", "svg", "mark", "span"] {
            assert!(!html.contains(&format!("<{tag}")), "{source}: {html}");
        }
        assert!(html.contains("&lt;"), "{source}: {html}");
    }
    let html =
        vault.render("A.md", "<div>\n<script>alert(1)</script>\n<span onclick=\"bad()\">safe</span>\n</div>");
    assert!(html.contains("<div>\n&lt;script&gt;alert(1)&lt;/script&gt;"), "{html}");
    assert!(html.contains("<span>safe</span>\n</div>"), "{html}");
    let html = vault.render("A.md", "`<mark>x</mark>`\n\n```html\n<span>x</span>\n```\n\n    <u>x</u>");
    assert!(html.contains("<code>&lt;mark&gt;x&lt;/mark&gt;</code>"), "{html}");
    assert!(html.contains("&lt;span&gt;x&lt;/span&gt;"), "{html}");
    assert!(html.contains("&lt;u&gt;x&lt;/u&gt;"), "{html}");
    let html = vault.render("A.md", "<mark>visible %% </mark> %%");
    assert!(html.contains("&lt;mark&gt;visible") && !html.contains("<mark>"), "{html}");
    let html = vault.render("A.md", "[[Fox|<mark>]] visible</mark>");
    assert!(!html.contains("</mark>"), "{html}");
    // Backticks inside an escaped attribute must not change which later
    // HTML is protected by an inline code span.
    let html = vault.render("A.md", "<widget title=\"`\"> `<script>alert(1)</script>`");
    assert!(!html.contains("<script>"), "{html}");
    assert!(html.contains("<code>&lt;script&gt;alert(1)&lt;/script&gt;</code>"), "{html}");
}

#[test]
fn encodes_link_urls() {
    let (_dir, vault) = vault_with(&[("读书 笔记.md", "x")]);
    let html = vault.render("Index.md", "[[读书 笔记]] #标签");
    assert!(html.contains(r#"href="paperloom:note/%E8%AF%BB%E4%B9%A6%20%E7%AC%94%E8%AE%B0.md""#), "{html}");
    assert!(html.contains(r#"href="paperloom:tag/%E6%A0%87%E7%AD%BE""#), "{html}");
}

#[test]
fn makes_heading_ids() {
    assert_eq!(render::heading_id("Method & Results"), "method-results");
    assert_eq!(render::heading_id("  方法 与 结果 "), "方法-与-结果");
}

#[test]
fn hooks_render_their_own_links() {
    let (_dir, vault) = vault_with(&[]);
    let hook = |link: &Link| {
        link.target.starts_with('@').then(|| External {
            html: format!("<blockquote>{}</blockquote>", link.subpath.clone().unwrap_or_default()),
            block: link.embed,
        })
    };
    let html = vault.render_with("A.md", "Inline [[@paper]] and\n\n![[@paper#hl-1]]\n", &hook);
    assert!(html.contains("Inline <blockquote></blockquote> and"), "{html}");
    assert!(
        html.contains("<blockquote>hl-1</blockquote>") && !html.contains("<p><blockquote>hl-1"),
        "{html}"
    );
}

#[test]
fn keeps_recordings_and_plays_them_in_notes() {
    let (_dir, mut vault) = vault_with(FILES);
    let first = vault.add_file("Recordings", "Lecture.m4a", b"audio").unwrap();
    let second = vault.add_file("/Recordings/", "Lecture.m4a", b"more").unwrap();
    assert_eq!((first.as_str(), second.as_str()), ("Recordings/Lecture.m4a", "Recordings/Lecture 2.m4a"));
    assert_eq!(std::fs::read(vault.root().join(&second)).unwrap(), b"more");
    assert!(vault.add_file("../outside", "x.m4a", b"").is_err());
    let html = vault.render("Index.md", "![[Lecture.m4a]]");
    assert!(
        html.contains(
            r#"<span class="embed-audio" data-audio="Recordings/Lecture.m4a"><a class="audio-link" href="paperloom:audio/Recordings/Lecture.m4a">Lecture.m4a</a></span>"#
        ),
        "{html}"
    );
}
