//! A note as a LaTeX document: headings, paragraphs, emphasis, lists,
//! quotes, code, links, tables and math, with `[@citations]` as biblatex
//! citations in the vault's citation style. The cited works' BibTeX goes
//! inside the file, so one `.tex` compiles on its own with XeLaTeX and
//! Biber (and on Overleaf).

use std::collections::HashMap;
use std::fmt::Write as _;
use std::path::Path;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::docx::prepare;
use crate::{CoreError, PaperVault};

/// Marks where a citation goes until the text is escaped (private-use
/// characters never appear in notes in practice).
const CITE_OPEN: char = '\u{E001}';
const CITE_CLOSE: char = '\u{E002}';

/// Text with LaTeX's special characters spelled out.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str(r"\textbackslash{}"),
            '{' | '}' | '$' | '&' | '#' | '_' | '%' => {
                out.push('\\');
                out.push(c);
            }
            '^' => out.push_str(r"\^{}"),
            '~' => out.push_str(r"\~{}"),
            c => out.push(c),
        }
    }
    out
}

/// A URL as `\href` takes it.
fn escape_url(url: &str) -> String {
    let mut out = String::with_capacity(url.len());
    for c in url.chars() {
        if matches!(c, '%' | '#' | '{' | '}' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// The biblatex style closest to a citation style.
fn biblatex_style(style: &str) -> &'static str {
    match style {
        "gb-7714-2015-numeric" | "gb-7714-2015-note" => "gb7714-2015",
        "gb-7714-2015-author-date" => "gb7714-2015ay",
        "apa" => "apa",
        "ieee" => "ieee",
        "mla" => "mla",
        "nature" => "nature",
        "chicago-author-date" => "chicago-authordate",
        "chicago-notes" => "chicago-notes",
        s if s.contains("author-date") || s.contains("harvard") => "authoryear",
        _ => "numeric-comp",
    }
}

/// One citation: `\autocite[55]{kucsko2013}` or, for several works,
/// `\autocites[55]{a}{b}`.
fn cite_command(works: &[(String, Option<String>)]) -> String {
    let mut out = String::from(if works.len() == 1 { r"\autocite" } else { r"\autocites" });
    for (key, page) in works {
        if let Some(page) = page {
            let _ = write!(out, "[{}]", escape(page));
        }
        let _ = write!(out, "{{{key}}}");
    }
    out
}

/// Builds the document body from Markdown events.
#[derive(Default)]
struct Writer {
    out: String,
    /// Open lists: whether each is numbered.
    lists: Vec<bool>,
    code_block: bool,
    /// The table's next cell is its row's first.
    first_cell: bool,
    /// The LaTeX for each citation token.
    cites: Vec<String>,
}

impl Writer {
    fn text(&mut self, text: &str) {
        if self.code_block {
            self.out.push_str(text);
            return;
        }
        let mut escaped = escape(text);
        if escaped.contains(CITE_OPEN) {
            for (n, command) in self.cites.iter().enumerate() {
                escaped = escaped.replace(&format!("{CITE_OPEN}{n}{CITE_CLOSE}"), command);
            }
        }
        self.out.push_str(&escaped);
    }

    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.text(&text),
            Event::Code(code) => {
                let _ = write!(self.out, r"\texttt{{{}}}", escape(&code));
            }
            Event::InlineMath(math) => {
                let _ = write!(self.out, "${math}$");
            }
            Event::DisplayMath(math) => {
                let _ = write!(self.out, "\\[{math}\\]");
            }
            Event::SoftBreak => self.out.push('\n'),
            Event::HardBreak => self.out.push_str("\\\\\n"),
            Event::Rule => self.out.push_str("\\par\\noindent\\rule{\\linewidth}{0.4pt}\n\n"),
            Event::TaskListMarker(done) => {
                self.out.push_str(if done { r"$\boxtimes$\ " } else { r"$\square$\ " });
            }
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        let out = &mut self.out;
        match tag {
            Tag::Heading { level, .. } => out.push_str(match level {
                HeadingLevel::H1 => r"\section{",
                HeadingLevel::H2 => r"\subsection{",
                HeadingLevel::H3 => r"\subsubsection{",
                HeadingLevel::H4 => r"\paragraph{",
                _ => r"\subparagraph{",
            }),
            Tag::Emphasis => out.push_str(r"\emph{"),
            Tag::Strong => out.push_str(r"\textbf{"),
            Tag::Strikethrough => out.push_str(r"\sout{"),
            Tag::BlockQuote(_) => out.push_str("\\begin{quote}\n"),
            Tag::CodeBlock(kind) => {
                if let CodeBlockKind::Fenced(info) = kind
                    && !info.trim().is_empty()
                {
                    let _ = writeln!(out, "% {}", info.trim());
                }
                out.push_str("\\begin{verbatim}\n");
                self.code_block = true;
            }
            Tag::List(start) => {
                out.push_str(if start.is_some() { "\\begin{enumerate}\n" } else { "\\begin{itemize}\n" });
                if let Some(n) = start.filter(|n| *n != 1) {
                    let _ = writeln!(out, "\\setcounter{{enumi}}{{{}}}", n.saturating_sub(1));
                }
                self.lists.push(start.is_some());
            }
            Tag::Item => out.push_str(r"\item "),
            Tag::Link { dest_url, .. } => {
                let _ = write!(out, r"\href{{{}}}{{", escape_url(&dest_url));
            }
            // An image shows as its description.
            Tag::Image { .. } => out.push_str(r"\emph{"),
            Tag::Table(alignments) => {
                let columns: String = alignments
                    .iter()
                    .map(|a| match a {
                        Alignment::Center => 'c',
                        Alignment::Right => 'r',
                        Alignment::Left | Alignment::None => 'l',
                    })
                    .collect();
                let _ = writeln!(out, "\\begin{{center}}\n\\begin{{tabular}}{{{columns}}}\n\\hline");
            }
            Tag::TableHead | Tag::TableRow => self.first_cell = true,
            Tag::TableCell => {
                if !self.first_cell {
                    out.push_str(" & ");
                }
                self.first_cell = false;
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        let out = &mut self.out;
        match tag {
            TagEnd::Paragraph => out.push_str("\n\n"),
            TagEnd::Heading(_) => out.push_str("}\n\n"),
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough | TagEnd::Link | TagEnd::Image => {
                out.push('}');
            }
            TagEnd::BlockQuote(_) => out.push_str("\\end{quote}\n\n"),
            TagEnd::CodeBlock => {
                if !out.ends_with('\n') {
                    out.push('\n');
                }
                out.push_str("\\end{verbatim}\n\n");
                self.code_block = false;
            }
            TagEnd::List(_) => {
                let numbered = self.lists.pop().unwrap_or(false);
                out.push_str(if numbered { "\\end{enumerate}\n\n" } else { "\\end{itemize}\n\n" });
            }
            TagEnd::Item => {
                if !out.ends_with('\n') {
                    out.push('\n');
                }
            }
            TagEnd::TableHead => out.push_str(" \\\\\n\\hline\n"),
            TagEnd::TableRow => out.push_str(" \\\\\n"),
            TagEnd::Table => out.push_str("\\hline\n\\end{tabular}\n\\end{center}\n\n"),
            _ => {}
        }
    }
}

fn has_cjk(text: &str) -> bool {
    text.chars()
        .any(|c| matches!(c, '\u{3040}'..='\u{30ff}' | '\u{3400}'..='\u{9fff}' | '\u{ac00}'..='\u{d7af}'))
}

impl PaperVault {
    /// A note as a LaTeX document, its citations in the vault's citation
    /// style (as the biblatex style closest to it).
    pub(crate) fn note_to_latex(&self, path: &str) -> Result<String, CoreError> {
        let (text, title, style) = {
            let vault = self.lock()?;
            let note = vault
                .note(path)
                .ok_or_else(|| CoreError::NotFound { message: format!("{path} not found") })?;
            (note.text.clone(), note.title.clone(), vault.settings().citation_style)
        };
        let works = self.library().map(|l| l.note_works(&text)).unwrap_or_default();
        let keys = paperloom_formats::citation_keys(&works.items);
        let mut writer = Writer::default();
        let mut tokens: HashMap<usize, String> = HashMap::new();
        for (n, (start, cites)) in works.groups.iter().enumerate() {
            let cited: Vec<(String, Option<String>)> =
                cites.iter().map(|c| (keys[c.index].clone(), c.page.clone())).collect();
            writer.cites.push(cite_command(&cited));
            tokens.insert(*start, format!("{CITE_OPEN}{n}{CITE_CLOSE}"));
        }
        let markdown = prepare(&text, &tokens);
        let options = Options::ENABLE_TABLES
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_MATH;
        for event in Parser::new_ext(&markdown, options) {
            writer.event(event);
        }

        let mut doc = String::new();
        let _ = writeln!(doc, "% “{}”, written by Paperloom.", title.replace('\n', " "));
        doc.push_str("% Compile with XeLaTeX and Biber (on Overleaf, set the compiler to XeLaTeX).\n");
        let cites = !works.items.is_empty();
        if cites {
            let bib = paperloom_formats::write(paperloom_formats::Format::BibTex, &works.items);
            let _ = writeln!(
                doc,
                "\\begin{{filecontents*}}[overwrite]{{\\jobname.bib}}\n{}\n\\end{{filecontents*}}",
                bib.trim()
            );
        }
        doc.push_str(if has_cjk(&text) || has_cjk(&title) {
            "\\documentclass{ctexart}\n"
        } else {
            "\\documentclass{article}\n"
        });
        doc.push_str("\\usepackage{amsmath,amssymb}\n\\usepackage[normalem]{ulem}\n\\usepackage{hyperref}\n");
        if cites {
            let _ = writeln!(doc, "\\usepackage[backend=biber,style={}]{{biblatex}}", biblatex_style(&style));
            doc.push_str("\\addbibresource{\\jobname.bib}\n");
        }
        let _ =
            writeln!(doc, "\\title{{{}}}\n\\date{{}}\n\n\\begin{{document}}\n\\maketitle\n", escape(&title));
        doc.push_str(writer.out.trim_end());
        doc.push_str("\n\n");
        if cites {
            doc.push_str("\\printbibliography\n");
        }
        doc.push_str("\\end{document}\n");
        Ok(doc)
    }
}

#[uniffi::export]
impl PaperVault {
    /// Writes a note as a LaTeX document (`.tex`) at `destination`: its
    /// citations as biblatex citations in the style closest to the vault's,
    /// with the cited works' BibTeX inside, so the file compiles on its own
    /// (XeLaTeX and Biber).
    pub fn export_latex(&self, path: String, destination: String) -> Result<(), CoreError> {
        let latex = self.note_to_latex(&path)?;
        std::fs::write(Path::new(&destination), latex)
            .map_err(|e| CoreError::Storage { message: format!("Saving {destination}: {e}") })
    }
}

#[cfg(test)]
mod tests;
