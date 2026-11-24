//! A note as a Word document (`.docx`): headings, paragraphs, emphasis,
//! lists, quotes, code, links and tables, with `[@citations]` formatted in
//! the vault's citation style and the reference list at the end. Word,
//! WPS, Pages and LibreOffice open it.

use std::fmt::Write as _;
use std::path::Path;

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use crate::backup::ZipWriter;
use crate::{CoreError, PaperVault};

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// How the text being written looks.
#[derive(Default, Clone, Copy)]
struct Look {
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
}

/// Builds `word/document.xml` from Markdown events.
#[derive(Default)]
struct Writer {
    body: String,
    /// The paragraph being written: its style and runs.
    paragraph: Option<(Option<String>, String)>,
    look: Look,
    /// External links, by relationship ID order.
    links: Vec<String>,
    /// Runs inside a link go into it.
    link: Option<(usize, String)>,
    /// Open lists: their next number (none for bullets).
    lists: Vec<Option<u64>>,
    /// A list item's bullet or number, written with its first text.
    marker: Option<String>,
    /// The run being written (its properties and text), so text that
    /// arrives in pieces looks the same goes into one run.
    run: Option<(String, String)>,
    quote: usize,
    code_block: bool,
    /// Table rows: cells being collected.
    row: Option<Vec<String>>,
    cell: Option<String>,
}

impl Writer {
    fn start_paragraph(&mut self, style: Option<&str>) {
        self.end_paragraph();
        let style = style.map(str::to_string).or_else(|| (self.quote > 0).then(|| "Quote".to_string()));
        self.paragraph = Some((style, String::new()));
    }

    /// Writes out the run being collected, into the link or paragraph.
    fn flush_run(&mut self) {
        let Some((properties, text)) = self.run.take() else { return };
        let xml = format!(
            r#"<w:r><w:rPr>{properties}</w:rPr><w:t xml:space="preserve">{}</w:t></w:r>"#,
            escape(&text)
        );
        match &mut self.link {
            Some((_, runs)) => runs.push_str(&xml),
            None => {
                if let Some((_, runs)) = &mut self.paragraph {
                    runs.push_str(&xml);
                }
            }
        }
    }

    fn end_paragraph(&mut self) {
        self.flush_run();
        let Some((style, runs)) = self.paragraph.take() else { return };
        let mut properties = String::new();
        if let Some(style) = style {
            let _ = write!(properties, r#"<w:pStyle w:val="{style}"/>"#);
        }
        if !self.lists.is_empty() {
            let indent = 360 * self.lists.len();
            let _ = write!(properties, r#"<w:ind w:left="{indent}" w:hanging="360"/>"#);
        }
        let _ = write!(self.body, "<w:p><w:pPr>{properties}</w:pPr>{runs}</w:p>");
    }

    fn text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        if let Some(cell) = &mut self.cell {
            cell.push_str(text);
            return;
        }
        if self.paragraph.is_none() {
            self.start_paragraph(None);
        }
        if let Some(marker) = self.marker.take() {
            self.flush_run();
            if let Some((_, runs)) = &mut self.paragraph {
                let _ = write!(runs, r#"<w:r><w:t xml:space="preserve">{}</w:t></w:r>"#, escape(&marker));
            }
        }
        let mut properties = String::new();
        if self.link.is_some() {
            properties.push_str(r#"<w:rStyle w:val="Hyperlink"/>"#);
        }
        if self.look.bold {
            properties.push_str("<w:b/>");
        }
        if self.look.italic {
            properties.push_str("<w:i/>");
        }
        if self.look.strike {
            properties.push_str("<w:strike/>");
        }
        if self.look.code || self.code_block {
            properties.push_str(r#"<w:rFonts w:ascii="Consolas" w:hAnsi="Consolas"/>"#);
        }
        match &mut self.run {
            Some((same, collected)) if *same == properties => collected.push_str(text),
            _ => {
                self.flush_run();
                self.run = Some((properties, text.to_string()));
            }
        }
    }

    fn line_break(&mut self) {
        self.flush_run();
        if let Some((_, runs)) = &mut self.paragraph {
            runs.push_str("<w:r><w:br/></w:r>");
        }
    }

    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                let n = match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    HeadingLevel::H3 => 3,
                    HeadingLevel::H4 => 4,
                    HeadingLevel::H5 => 5,
                    HeadingLevel::H6 => 6,
                };
                self.start_paragraph(Some(&format!("Heading{n}")));
            }
            Event::Start(Tag::Paragraph) => self.start_paragraph(None),
            Event::End(TagEnd::Paragraph | TagEnd::Heading(_)) => self.end_paragraph(),
            Event::Start(Tag::BlockQuote(_)) => self.quote += 1,
            Event::End(TagEnd::BlockQuote(_)) => {
                self.end_paragraph();
                self.quote = self.quote.saturating_sub(1);
            }
            Event::Start(Tag::List(start)) => {
                self.end_paragraph();
                self.lists.push(start);
            }
            Event::End(TagEnd::List(_)) => {
                self.end_paragraph();
                self.lists.pop();
            }
            Event::Start(Tag::Item) => {
                self.end_paragraph();
                self.marker = Some(match self.lists.last_mut() {
                    Some(Some(n)) => {
                        let marker = format!("{n}.\t");
                        *n += 1;
                        marker
                    }
                    _ => "•\t".to_string(),
                });
            }
            Event::End(TagEnd::Item) => self.end_paragraph(),
            Event::Start(Tag::CodeBlock(_)) => {
                self.code_block = true;
                self.start_paragraph(Some("Code"));
            }
            Event::End(TagEnd::CodeBlock) => {
                self.end_paragraph();
                self.code_block = false;
            }
            Event::Start(Tag::Strong) => self.look.bold = true,
            Event::End(TagEnd::Strong) => self.look.bold = false,
            Event::Start(Tag::Emphasis) => self.look.italic = true,
            Event::End(TagEnd::Emphasis) => self.look.italic = false,
            Event::Start(Tag::Strikethrough) => self.look.strike = true,
            Event::End(TagEnd::Strikethrough) => self.look.strike = false,
            Event::Start(Tag::Link { dest_url, .. }) => {
                if dest_url.starts_with("http") || dest_url.starts_with("mailto:") {
                    self.flush_run();
                    self.links.push(dest_url.to_string());
                    self.link = Some((self.links.len(), String::new()));
                }
            }
            Event::End(TagEnd::Link) => {
                self.flush_run();
                if let Some((n, runs)) = self.link.take() {
                    if self.paragraph.is_none() {
                        self.start_paragraph(None);
                    }
                    if let Some((_, paragraph)) = &mut self.paragraph {
                        let _ = write!(paragraph, r#"<w:hyperlink r:id="rIdLink{n}">{runs}</w:hyperlink>"#);
                    }
                }
            }
            Event::Start(Tag::Table(_)) => self.end_paragraph(),
            Event::Start(Tag::TableHead | Tag::TableRow) => self.row = Some(Vec::new()),
            Event::Start(Tag::TableCell) => self.cell = Some(String::new()),
            Event::End(TagEnd::TableCell) => {
                if let (Some(row), Some(cell)) = (&mut self.row, self.cell.take()) {
                    row.push(cell);
                }
            }
            Event::End(TagEnd::TableHead | TagEnd::TableRow) => {
                if let Some(row) = self.row.take() {
                    self.start_paragraph(None);
                    self.text(&row.join("\t"));
                    self.end_paragraph();
                }
            }
            Event::Text(text) => {
                if self.code_block {
                    let mut lines = text.lines().peekable();
                    while let Some(line) = lines.next() {
                        self.text(line);
                        if lines.peek().is_some() {
                            self.line_break();
                        }
                    }
                } else {
                    self.text(&text);
                }
            }
            Event::Code(text) | Event::InlineMath(text) | Event::DisplayMath(text) => {
                let code = self.look.code;
                self.look.code = true;
                self.text(&text);
                self.look.code = code;
            }
            Event::SoftBreak => self.text(" "),
            Event::HardBreak => self.line_break(),
            Event::Rule => {
                self.start_paragraph(None);
                self.end_paragraph();
            }
            Event::TaskListMarker(done) => self.text(if done { "☑ " } else { "☐ " }),
            _ => {}
        }
    }
}

const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:eastAsia="SimSun"/><w:sz w:val="22"/><w:lang w:val="en-US" w:eastAsia="zh-CN"/></w:rPr></w:rPrDefault>
<w:pPrDefault><w:pPr><w:spacing w:after="120" w:line="300" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>
<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>
<w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:after="240"/></w:pPr><w:rPr><w:b/><w:sz w:val="40"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:pPr><w:keepNext/><w:spacing w:before="360"/><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:b/><w:sz w:val="32"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:pPr><w:keepNext/><w:spacing w:before="240"/><w:outlineLvl w:val="1"/></w:pPr><w:rPr><w:b/><w:sz w:val="28"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading3"><w:name w:val="heading 3"/><w:basedOn w:val="Normal"/><w:pPr><w:keepNext/><w:spacing w:before="200"/><w:outlineLvl w:val="2"/></w:pPr><w:rPr><w:b/><w:sz w:val="24"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading4"><w:name w:val="heading 4"/><w:basedOn w:val="Normal"/><w:pPr><w:keepNext/><w:outlineLvl w:val="3"/></w:pPr><w:rPr><w:b/><w:i/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading5"><w:name w:val="heading 5"/><w:basedOn w:val="Normal"/><w:pPr><w:keepNext/><w:outlineLvl w:val="4"/></w:pPr><w:rPr><w:b/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading6"><w:name w:val="heading 6"/><w:basedOn w:val="Normal"/><w:pPr><w:keepNext/><w:outlineLvl w:val="5"/></w:pPr><w:rPr><w:i/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Quote"><w:name w:val="Quote"/><w:basedOn w:val="Normal"/><w:pPr><w:ind w:left="720"/></w:pPr><w:rPr><w:i/><w:color w:val="595959"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Code"><w:name w:val="Code"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:after="0" w:line="240" w:lineRule="auto"/><w:shd w:val="clear" w:color="auto" w:fill="F2F2F2"/></w:pPr><w:rPr><w:rFonts w:ascii="Consolas" w:hAnsi="Consolas"/><w:sz w:val="20"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Bibliography"><w:name w:val="Bibliography"/><w:basedOn w:val="Normal"/><w:pPr><w:ind w:left="480" w:hanging="480"/></w:pPr></w:style>
<w:style w:type="character" w:styleId="Hyperlink"><w:name w:val="Hyperlink"/><w:rPr><w:color w:val="0563C1"/><w:u w:val="single"/></w:rPr></w:style>
</w:styles>"#;

/// The note's text made ready for Word or LaTeX: no front matter, no
/// comments, `[[links]]` as their text, citations as given.
pub(crate) fn prepare(text: &str, citations: &std::collections::HashMap<usize, String>) -> String {
    let parsed = paperloom_vault::parse(text);
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    for link in &parsed.links {
        let shown = if link.embed {
            String::new()
        } else {
            link.alias.clone().unwrap_or_else(|| link.target.trim_start_matches('@').to_string())
        };
        edits.push((link.start, link.end, shown));
    }
    for &(start, end) in &parsed.comment_spans {
        edits.push((start, end, String::new()));
    }
    for citation in &parsed.citations {
        if let Some(cited) = citations.get(&citation.start) {
            edits.push((citation.start, citation.end, cited.clone()));
        }
    }
    edits.sort_by_key(|(start, end, _)| (*start, std::cmp::Reverse(*end)));
    let mut out = String::with_capacity(text.len());
    let mut at = parsed.body_start;
    for (start, end, replacement) in edits {
        if start < at {
            continue;
        }
        out.push_str(&text[at..start]);
        out.push_str(&replacement);
        at = end;
    }
    out.push_str(&text[at..]);
    out
}

/// The parts of a package, into a ZIP at `path`.
fn write_package(path: &Path, parts: &[(&str, String)]) -> std::io::Result<()> {
    let mut zip = ZipWriter::create(path)?;
    for (name, content) in parts {
        zip.add_bytes(name, content.as_bytes())?;
    }
    zip.finish().map(|_| ())
}

impl PaperVault {
    /// Writes a note as a Word document at `destination`, its citations
    /// formatted in the vault's citation style, with a reference list.
    pub(crate) fn note_to_docx(&self, path: &str, destination: &Path) -> Result<(), CoreError> {
        let (text, title, style) = {
            let vault = self.lock()?;
            let note = vault
                .note(path)
                .ok_or_else(|| CoreError::NotFound { message: format!("{path} not found") })?;
            (note.text.clone(), note.title.clone(), vault.settings().citation_style)
        };
        let cited = self
            .library()
            .map(|l| l.cite_note(&text, &style, paperloom_citation::Format::Text))
            .unwrap_or_default();
        let markdown = prepare(&text, &cited.citations);
        let mut writer = Writer::default();
        writer.start_paragraph(Some("Title"));
        writer.text(&title);
        writer.end_paragraph();
        let options = Options::ENABLE_TABLES
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_MATH;
        for event in Parser::new_ext(&markdown, options) {
            writer.event(event);
        }
        writer.end_paragraph();
        if !cited.entries.is_empty() {
            let heading = if style.starts_with("gb-") { "参考文献" } else { "References" };
            writer.start_paragraph(Some("Heading1"));
            writer.text(heading);
            writer.end_paragraph();
            for entry in &cited.entries {
                writer.start_paragraph(Some("Bibliography"));
                writer.text(entry);
                writer.end_paragraph();
            }
        }
        let document = format!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><w:body>{}<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="708" w:footer="708" w:gutter="0"/></w:sectPr></w:body></w:document>"#,
            writer.body
        );
        let mut relations = String::from(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rIdStyles" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>"#,
        );
        for (n, url) in writer.links.iter().enumerate() {
            let _ = write!(
                relations,
                r#"<Relationship Id="rIdLink{}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="{}" TargetMode="External"/>"#,
                n + 1,
                escape(url)
            );
        }
        relations.push_str("</Relationships>");
        let parts: [(&str, String); 6] = [
            (
                "[Content_Types].xml",
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/><Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/><Override PartName="/docProps/core.xml" ContentType="application/vnd.openxmlformats-package.core-properties+xml"/></Types>"#
                    .to_string(),
            ),
            (
                "_rels/.rels",
                r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties" Target="docProps/core.xml"/></Relationships>"#
                    .to_string(),
            ),
            (
                "docProps/core.xml",
                format!(
                    r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>{}</dc:title></cp:coreProperties>"#,
                    escape(&title)
                ),
            ),
            ("word/document.xml", document),
            ("word/styles.xml", STYLES.to_string()),
            ("word/_rels/document.xml.rels", relations),
        ];
        let partial = destination.with_extension("docx.partial");
        if let Err(e) = write_package(&partial, &parts) {
            let _ = std::fs::remove_file(&partial);
            return Err(CoreError::Storage { message: format!("Writing {}: {e}", destination.display()) });
        }
        std::fs::rename(&partial, destination)
            .map_err(|e| CoreError::Storage { message: format!("Saving {}: {e}", destination.display()) })
    }
}

#[uniffi::export]
impl PaperVault {
    /// Writes a note as a Word document (`.docx`) at `destination`: its
    /// citations formatted in the vault's citation style, a reference list
    /// at the end.
    pub fn export_docx(&self, path: String, destination: String) -> Result<(), CoreError> {
        self.note_to_docx(&path, Path::new(&destination))
    }
}

#[cfg(test)]
mod tests;
