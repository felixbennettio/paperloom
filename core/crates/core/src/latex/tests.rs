use super::*;
use crate::{NewItem, PaperLibrary, VaultSettings};

#[test]
fn writes_a_note_as_latex_with_its_bibliography() {
    let dir = tempfile::tempdir().unwrap();
    let library = PaperLibrary::new(dir.path().join("library.sqlite").display().to_string()).unwrap();
    library
        .add_item(NewItem {
            item_type: String::new(),
            title: "Nanometre-scale thermometry in a living cell".into(),
            authors: vec!["Georg Kucsko".into()],
            container_title: Some("Nature".into()),
            year: Some(2013),
            doi: Some("10.1038/nature12373".into()),
        })
        .unwrap();
    let vault = PaperVault::open(dir.path().join("Vault").display().to_string()).unwrap();
    vault.connect_library(library);
    let settings =
        VaultSettings { citation_style: "gb-7714-2015-numeric".into(), ..vault.settings().unwrap() };
    vault.set_settings(settings).unwrap();
    let note = "---\nstatus: draft\n---\n# Findings\n\nThe **key** idea [@10.1038/nature12373, p. 55] and \
                x < y & [[Other note|that one]], 50% of #1 a_b, *so* $E=mc^2$.\n\n\
                1. first\n2. second\n\n- [x] done\n\n\
                See [the paper](https://example.org/a%20b#c).\n\n```rust\nlet x = 1;\nlet y = 2;\n```\n\n\
                | a | b |\n|---|--:|\n| 1 | 2 |\n";
    vault.write_note("Draft.md".into(), note.into()).unwrap();

    let out = dir.path().join("Draft.tex");
    vault.export_latex("Draft.md".into(), out.display().to_string()).unwrap();
    let tex = std::fs::read_to_string(&out).unwrap();
    for expected in [
        "\\begin{filecontents*}[overwrite]{\\jobname.bib}\n@article{kucsko2013nanometre,",
        "\\documentclass{article}\n",
        "\\usepackage[backend=biber,style=gb7714-2015]{biblatex}\n\\addbibresource{\\jobname.bib}",
        "\\title{Draft}",
        "\\section{Findings}\n\n",
        "The \\textbf{key} idea \\autocite[55]{kucsko2013nanometre} and x < y \\& that one, 50\\% of \\#1 a\\_b, \
         \\emph{so} $E=mc^2$.",
        "\\begin{enumerate}\n\\item first\n\\item second\n\\end{enumerate}",
        "\\item $\\boxtimes$\\ done",
        "\\href{https://example.org/a\\%20b\\#c}{the paper}",
        "% rust\n\\begin{verbatim}\nlet x = 1;\nlet y = 2;\n\\end{verbatim}",
        "\\begin{tabular}{lr}\n\\hline\na & b \\\\\n\\hline\n1 & 2 \\\\\n\\hline\n\\end{tabular}",
        "\\printbibliography\n\\end{document}\n",
    ] {
        assert!(tex.contains(expected), "missing {expected:?} in:\n{tex}");
    }
    assert!(!tex.contains("status: draft"), "no front matter: {tex}");

    let alone = PaperVault::open(dir.path().join("Alone").display().to_string()).unwrap();
    alone.write_note("结论.md".into(), "中文 [@nobody] ~ ^ {x}".into()).unwrap();
    let tex = alone.note_to_latex("结论.md").unwrap();
    assert!(tex.contains("\\documentclass{ctexart}") && tex.contains("\\title{结论}"), "{tex}");
    assert!(tex.contains("中文 [@nobody] \\~{} \\^{} \\{x\\}"), "{tex}");
    assert!(!tex.contains("biblatex") && !tex.contains("filecontents"), "{tex}");
    assert_eq!(cite_command(&[("a".into(), Some("5".into())), ("b".into(), None)]), "\\autocites[5]{a}{b}");
    assert_eq!(biblatex_style("harvard-cite-them-right"), "authoryear");
}
