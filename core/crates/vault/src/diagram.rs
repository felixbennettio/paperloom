//! Diagrams, kept in the vault as `.drawio.svg` (a picture every app
//! shows, with the diagram inside it) or `.drawio` files, the way
//! diagrams.net and the draw.io plugins for Obsidian and VS Code keep them.
//! Paperloom reads, lays out, draws and edits them itself: the model, the
//! shapes, the routing of edges and the pictures are in the modules below.

mod edit;
mod model;
mod route;
mod scene;
mod shapes;
mod style;
mod svg;

use std::fs;

pub use edit::{DiagramEdit, EDGE_STYLE, PALETTE, PaletteRow, PaletteShape, edit_page};
pub use model::{Cell, Diagram, Geometry, Page};
pub use scene::{Item, Scene, label_text, scene};
pub use shapes::Rect;
pub use style::Style;
pub use svg::scene_svg;

use crate::xml::{escape, parse_xml};
use crate::{Error, Result, Vault, checked, io_error};

/// An empty diagram, as draw.io starts one.
pub const EMPTY_DIAGRAM: &str = "<mxfile><diagram id=\"page-1\" name=\"Page-1\"><mxGraphModel><root>\
<mxCell id=\"0\"/><mxCell id=\"1\" parent=\"0\"/></root></mxGraphModel></diagram></mxfile>";

/// Whether a path is a diagram's.
pub fn is_diagram(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with(".drawio") || lower.ends_with(".drawio.svg")
}

/// The diagram (draw.io's XML) a file holds: a `.drawio` file is that XML,
/// a `.drawio.svg` keeps it in its `content` attribute.
pub fn diagram_xml(text: &str) -> Option<String> {
    let root = parse_xml(text)?;
    match root.local() {
        "mxfile" | "mxGraphModel" => Some(text.trim_start_matches('\u{feff}').trim().to_string()),
        "svg" => root.attr("content").map(str::to_string).filter(|c| !c.trim().is_empty()),
        _ => None,
    }
}

/// A blank picture holding a diagram, for a `.drawio.svg` before draw.io
/// draws it.
pub fn picture_holding(xml: &str) -> String {
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" version=\"1.1\" width=\"1px\" height=\"1px\" \
         viewBox=\"-0.5 -0.5 1 1\" content=\"{}\"><defs/><g/></svg>",
        escape(xml)
    )
}

/// A diagram's first page drawn as a picture that holds the diagram, for a
/// `.drawio.svg`; a blank one when the XML is no diagram.
pub fn diagram_picture(xml: &str) -> String {
    match Diagram::parse(xml) {
        Some(diagram) => scene_svg(&scene(&diagram.pages[0]), Some(xml)),
        None => picture_holding(xml),
    }
}

fn checked_diagram(path: &str) -> Result<String> {
    let path = checked(path)?;
    if is_diagram(&path) { Ok(path) } else { Err(Error::InvalidPath(path)) }
}

impl Vault {
    /// The diagrams in the vault, by path.
    pub fn diagrams(&self) -> impl Iterator<Item = &String> {
        self.files.iter().filter(|f| is_diagram(f))
    }

    /// A diagram's draw.io XML.
    pub fn read_diagram(&self, path: &str) -> Result<String> {
        let path = checked_diagram(path)?;
        let full = self.full(&path);
        let text = fs::read_to_string(&full).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Error::NotFound(path.clone()),
            _ => io_error(&full, e),
        })?;
        diagram_xml(&text).ok_or_else(|| Error::Invalid(format!("{path} holds no draw.io diagram")))
    }

    /// Saves a diagram: a `.drawio` file as its XML, a `.drawio.svg` as the
    /// picture draw.io made of it (`svg`, which holds the XML), or a blank
    /// one holding the XML when there is none. Replaced in one step.
    pub fn write_diagram(&mut self, path: &str, xml: &str, svg: Option<&str>) -> Result<()> {
        let path = checked_diagram(path)?;
        let text = if path.to_lowercase().ends_with(".svg") {
            match svg.filter(|svg| diagram_xml(svg).is_some()) {
                Some(svg) => svg.to_string(),
                None => diagram_picture(xml),
            }
        } else {
            xml.to_string()
        };
        let full = self.full(&path);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
            self.add_folders(&path);
        }
        let temporary = full.with_extension("paperloom-tmp");
        fs::write(&temporary, text).map_err(|e| io_error(&temporary, e))?;
        fs::rename(&temporary, &full).map_err(|e| io_error(&full, e))?;
        self.files.insert(path);
        Ok(())
    }

    /// Makes an empty `.drawio.svg` in `folder` named `name` (or `name 2`,
    /// … when taken); its path.
    pub fn create_diagram(&mut self, folder: &str, name: &str) -> Result<String> {
        let name = name.trim().trim_end_matches(".svg").trim_end_matches(".drawio");
        let name = if name.is_empty() { "Untitled" } else { name };
        let folder = folder.trim_matches('/');
        let mut n = 1;
        let path = loop {
            let file = if n == 1 { format!("{name}.drawio.svg") } else { format!("{name} {n}.drawio.svg") };
            let path = if folder.is_empty() { file } else { format!("{folder}/{file}") };
            if !self.files.iter().any(|f| f.eq_ignore_ascii_case(&path)) {
                break path;
            }
            n += 1;
        };
        self.write_diagram(&path, EMPTY_DIAGRAM, None)?;
        Ok(path)
    }
}

#[cfg(test)]
mod engine_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_diagram_in_either_file() {
        let xml =
            "<?xml version=\"1.0\"?>\n<mxfile host=\"app\"><diagram name=\"A\">rVPBb</diagram></mxfile>";
        assert_eq!(diagram_xml(xml).unwrap(), xml);
        let svg = picture_holding("<mxfile><diagram name=\"B & C\">x</diagram></mxfile>");
        assert!(svg.contains("content=\"&lt;mxfile&gt;&lt;diagram name=&quot;B &amp; C&quot;&gt;"), "{svg}");
        assert_eq!(diagram_xml(&svg).unwrap(), "<mxfile><diagram name=\"B & C\">x</diagram></mxfile>");
        assert!(diagram_xml("<svg xmlns=\"http://www.w3.org/2000/svg\"><g/></svg>").is_none());
        assert!(diagram_xml("not xml").is_none());
        assert!(is_diagram("a/B.drawio.svg") && is_diagram("C.DRAWIO") && !is_diagram("d.svg"));
    }

    #[test]
    fn keeps_diagrams_in_the_vault() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = Vault::open(dir.path()).unwrap();
        let path = vault.create_diagram("Figures", "Setup").unwrap();
        assert_eq!(path, "Figures/Setup.drawio.svg");
        assert_eq!(
            vault.create_diagram("Figures", "Setup.drawio.svg").unwrap(),
            "Figures/Setup 2.drawio.svg"
        );
        assert_eq!(vault.diagrams().count(), 2);
        assert_eq!(vault.read_diagram(&path).unwrap(), EMPTY_DIAGRAM);
        let drawn =
            picture_holding("<mxfile><diagram>drawn</diagram></mxfile>").replace("<g/>", "<g><rect/></g>");
        vault.write_diagram(&path, "<mxfile><diagram>drawn</diagram></mxfile>", Some(&drawn)).unwrap();
        assert_eq!(fs::read_to_string(dir.path().join(&path)).unwrap(), drawn);
        assert_eq!(vault.read_diagram(&path).unwrap(), "<mxfile><diagram>drawn</diagram></mxfile>");
        // A picture without the diagram in it is not kept in its place.
        vault.write_diagram(&path, "<mxfile><diagram>again</diagram></mxfile>", Some("<svg/>")).unwrap();
        assert_eq!(vault.read_diagram(&path).unwrap(), "<mxfile><diagram>again</diagram></mxfile>");
        vault.write_diagram("Flow.drawio", "<mxfile><diagram>flow</diagram></mxfile>", None).unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("Flow.drawio")).unwrap(),
            "<mxfile><diagram>flow</diagram></mxfile>"
        );
        assert!(matches!(vault.write_diagram("Notes.md", "x", None), Err(Error::InvalidPath(_))));
        assert!(matches!(vault.read_diagram("Missing.drawio"), Err(Error::NotFound(_))));
    }
}
