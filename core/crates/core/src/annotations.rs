//! Marks on attachments (highlights, underlines, notes, ink, text boxes,
//! rectangles, ellipses, arrows and stamps), stored in the library in PDF
//! page coordinates so every app draws the same ones.

use paperloom_model::{Annotation, AnnotationId, AnnotationKind, AttachmentId, Point, Rect, is_color};
use paperloom_store as store;
use serde::{Deserialize, Serialize};

use crate::{CoreError, PaperLibrary};

/// A rectangle in PDF user space: points from the bottom left of the
/// unrotated page.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, uniffi::Record)]
pub struct AnnotationRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A point in the same space.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, uniffi::Record)]
pub struct AnnotationPoint {
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct AnnotationInfo {
    pub id: String,
    pub attachment_id: String,
    /// `highlight`, `underline`, `strikeout`, `note`, `ink`, `text`,
    /// `square`, `circle`, `arrow` or `stamp`.
    pub kind: String,
    /// `#rrggbb`.
    pub color: String,
    /// From 0.
    pub page_index: u32,
    pub rects: Vec<AnnotationRect>,
    pub text: Option<String>,
    pub comment: Option<String>,
    pub tags: Vec<String>,
    /// An ink mark's strokes, each a line through its points; an arrow's
    /// one stroke, from its tail to its head.
    pub strokes: Vec<Vec<AnnotationPoint>>,
    /// A text box's font size, in points.
    pub font_size: Option<f64>,
    pub date_added: i64,
}

/// A mark someone just made.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct NewAnnotation {
    pub attachment_id: String,
    pub kind: String,
    pub color: String,
    pub page_index: u32,
    pub rects: Vec<AnnotationRect>,
    pub text: Option<String>,
    pub comment: Option<String>,
    /// For ink: the strokes (the rectangle around them is made when no
    /// rectangles are given). For an arrow: one stroke, tail to head.
    #[serde(default)]
    pub strokes: Vec<Vec<AnnotationPoint>>,
    /// For a text box (whose comment is its text): the font size in points;
    /// 14 when none is given.
    #[serde(default)]
    #[uniffi(default = None)]
    pub font_size: Option<f64>,
}

impl From<&Annotation> for AnnotationInfo {
    fn from(a: &Annotation) -> Self {
        Self {
            id: a.id.to_string(),
            attachment_id: a.attachment_id.to_string(),
            kind: a.kind.as_str().to_string(),
            color: a.color.clone(),
            page_index: a.page_index,
            rects: a
                .rects
                .iter()
                .map(|r| AnnotationRect { x: r.x, y: r.y, width: r.width, height: r.height })
                .collect(),
            text: a.text.clone(),
            comment: a.comment.clone(),
            tags: a.tags.clone(),
            strokes: a
                .strokes
                .iter()
                .map(|stroke| stroke.iter().map(|p| AnnotationPoint { x: p.x, y: p.y }).collect())
                .collect(),
            font_size: a.font_size,
            date_added: a.date_added,
        }
    }
}

fn invalid(message: impl Into<String>) -> CoreError {
    CoreError::InvalidInput { message: message.into() }
}

fn checked_color(color: &str) -> Result<String, CoreError> {
    if is_color(color) {
        Ok(color.to_ascii_lowercase())
    } else {
        Err(invalid(format!("“{color}” is not a #rrggbb color.")))
    }
}

/// Empty or blank text counts as none.
fn text(value: Option<String>) -> Option<String> {
    value.map(|t| t.trim().to_string()).filter(|t| !t.is_empty())
}

pub(crate) fn annotations(
    library: &PaperLibrary,
    attachment_id: String,
) -> Result<Vec<AnnotationInfo>, CoreError> {
    let marks = library.lock()?.annotations(&AttachmentId::from_string(attachment_id))?;
    Ok(marks.iter().map(AnnotationInfo::from).collect())
}

pub(crate) fn add(library: &PaperLibrary, new: NewAnnotation) -> Result<AnnotationInfo, CoreError> {
    let kind = AnnotationKind::parse(&new.kind)
        .ok_or_else(|| invalid(format!("Unknown annotation kind “{}”.", new.kind)))?;
    let strokes: Vec<Vec<Point>> = new
        .strokes
        .iter()
        .map(|stroke| stroke.iter().map(|p| Point { x: p.x, y: p.y }).collect::<Vec<_>>())
        .filter(|stroke| !stroke.is_empty())
        .collect();
    if !strokes.iter().flatten().all(|p| p.x.is_finite() && p.y.is_finite()) {
        return Err(invalid("A stroke has a point that is not a number."));
    }
    let strokes = match kind {
        AnnotationKind::Ink if strokes.is_empty() => {
            return Err(invalid("An ink mark needs at least one stroke."));
        }
        AnnotationKind::Ink => strokes,
        // An arrow is its first stroke's ends.
        AnnotationKind::Arrow => match strokes.first() {
            Some(stroke) if stroke.len() >= 2 => vec![vec![stroke[0], stroke[stroke.len() - 1]]],
            _ => return Err(invalid("An arrow needs a start and an end.")),
        },
        _ => Vec::new(),
    };
    let comment = text(new.comment);
    if kind == AnnotationKind::Text && comment.is_none() {
        return Err(invalid("A text box needs some text."));
    }
    if kind == AnnotationKind::Stamp && comment.is_none() {
        return Err(invalid("A stamp needs its word."));
    }
    let font_size = (kind == AnnotationKind::Text)
        .then(|| new.font_size.filter(|s| s.is_finite()).unwrap_or(14.0).clamp(6.0, 72.0));
    let mut rects: Vec<Rect> =
        new.rects.iter().map(|r| Rect { x: r.x, y: r.y, width: r.width, height: r.height }).collect();
    if rects.is_empty() {
        rects.extend(Rect::around(strokes.iter().flatten().copied()));
    }
    if rects.is_empty() || !rects.iter().all(Rect::is_valid) {
        return Err(invalid("An annotation needs at least one valid rectangle."));
    }
    let attachment_id = AttachmentId::from_string(new.attachment_id);
    let mut store = library.lock()?;
    store
        .attachment(&attachment_id)?
        .ok_or_else(|| CoreError::not_found(format!("attachment {attachment_id}")))?;
    let now = store::now_ms();
    let annotation = Annotation {
        id: AnnotationId::new(),
        attachment_id,
        kind,
        color: checked_color(&new.color)?,
        page_index: new.page_index,
        rects,
        text: text(new.text),
        comment,
        tags: Vec::new(),
        strokes,
        font_size,
        date_added: now,
        date_modified: now,
    };
    store.add_annotation(&annotation)?;
    drop(store);
    library.index_annotation(&annotation);
    Ok(AnnotationInfo::from(&annotation))
}

pub(crate) fn update(
    library: &PaperLibrary,
    annotation_id: String,
    color: String,
    comment: Option<String>,
) -> Result<AnnotationInfo, CoreError> {
    let id = AnnotationId::from_string(annotation_id);
    let mut store = library.lock()?;
    let mut annotation =
        store.annotation(&id)?.ok_or_else(|| CoreError::not_found(format!("annotation {id}")))?;
    annotation.color = checked_color(&color)?;
    annotation.comment = text(comment);
    annotation.date_modified = store::now_ms();
    store.update_annotation(&annotation)?;
    drop(store);
    library.index_annotation(&annotation);
    Ok(AnnotationInfo::from(&annotation))
}

pub(crate) fn remove(library: &PaperLibrary, annotation_id: String) -> Result<(), CoreError> {
    library.lock()?.remove_annotation(&AnnotationId::from_string(annotation_id.clone()))?;
    library.unindex_keys(&[annotation_id]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn library_with_pdf() -> (tempfile::TempDir, std::sync::Arc<PaperLibrary>, String) {
        let dir = tempfile::tempdir().unwrap();
        let lib = PaperLibrary::new(dir.path().join("library.sqlite").display().to_string()).unwrap();
        let pdf = dir.path().join("paper.pdf");
        std::fs::write(&pdf, b"%PDF-1.7").unwrap();
        let imported = lib.import_pdf(pdf.display().to_string(), String::new(), None, None).unwrap();
        (dir, lib, imported.attachment.id)
    }

    fn highlight(attachment_id: &str) -> NewAnnotation {
        NewAnnotation {
            attachment_id: attachment_id.to_string(),
            kind: "highlight".into(),
            color: "#FFD400".into(),
            page_index: 0,
            rects: vec![AnnotationRect { x: 72.0, y: 700.0, width: 200.0, height: 12.0 }],
            text: Some("  marked text ".into()),
            comment: Some("   ".into()),
            strokes: Vec::new(),
            font_size: None,
        }
    }

    #[test]
    fn keeps_ink_strokes() {
        let (_dir, lib, attachment) = library_with_pdf();
        let stroke =
            |points: &[(f64, f64)]| points.iter().map(|&(x, y)| AnnotationPoint { x, y }).collect::<Vec<_>>();
        let ink = NewAnnotation {
            kind: "ink".into(),
            rects: vec![],
            text: None,
            strokes: vec![stroke(&[(10.0, 20.0), (30.0, 25.0)]), stroke(&[(15.0, 40.0)])],
            ..highlight(&attachment)
        };
        let mark = lib.add_annotation(ink.clone()).unwrap();
        assert_eq!(mark.kind, "ink");
        assert_eq!(mark.strokes.len(), 2);
        assert_eq!(
            mark.rects,
            [AnnotationRect { x: 10.0, y: 20.0, width: 20.0, height: 20.0 }],
            "the box around them"
        );
        assert_eq!(lib.annotations(attachment.clone()).unwrap()[0].strokes, mark.strokes);
        let no_strokes = NewAnnotation { strokes: vec![], ..ink };
        assert!(matches!(lib.add_annotation(no_strokes), Err(CoreError::InvalidInput { .. })));
        let highlight_with_strokes = lib
            .add_annotation(NewAnnotation { strokes: vec![stroke(&[(1.0, 1.0)])], ..highlight(&attachment) })
            .unwrap();
        assert!(highlight_with_strokes.strokes.is_empty(), "only ink keeps strokes");
    }

    #[test]
    fn keeps_text_boxes_shapes_and_arrows() {
        let (_dir, lib, attachment) = library_with_pdf();
        let point = |x, y| AnnotationPoint { x, y };
        let text = NewAnnotation {
            kind: "text".into(),
            text: None,
            comment: Some(" See §3 ".into()),
            font_size: Some(200.0),
            ..highlight(&attachment)
        };
        let saved = lib.add_annotation(text.clone()).unwrap();
        assert_eq!((saved.comment.as_deref(), saved.font_size), (Some("See §3"), Some(72.0)));
        assert!(
            lib.add_annotation(NewAnnotation { comment: None, ..text }).is_err(),
            "a text box needs text"
        );

        let square =
            lib.add_annotation(NewAnnotation { kind: "square".into(), ..highlight(&attachment) }).unwrap();
        assert_eq!((square.kind.as_str(), square.font_size), ("square", None));

        let arrow = NewAnnotation {
            kind: "arrow".into(),
            rects: vec![],
            strokes: vec![vec![point(10.0, 10.0), point(20.0, 15.0), point(40.0, 30.0)]],
            ..highlight(&attachment)
        };
        let saved = lib.add_annotation(arrow.clone()).unwrap();
        assert_eq!(saved.strokes, [vec![point(10.0, 10.0), point(40.0, 30.0)]], "tail and head");
        assert_eq!(saved.rects, [AnnotationRect { x: 10.0, y: 10.0, width: 30.0, height: 20.0 }]);
        let one_point = NewAnnotation { strokes: vec![vec![point(1.0, 1.0)]], ..arrow };
        assert!(lib.add_annotation(one_point).is_err());
    }

    #[test]
    fn adds_updates_and_removes_marks() {
        let (_dir, lib, attachment) = library_with_pdf();
        let mark = lib.add_annotation(highlight(&attachment)).unwrap();
        assert_eq!(
            (mark.color.as_str(), mark.text.as_deref(), mark.comment.as_deref()),
            ("#ffd400", Some("marked text"), None)
        );
        let changed = lib.update_annotation(mark.id.clone(), "#5fb236".into(), Some("Why?".into())).unwrap();
        assert_eq!(changed.comment.as_deref(), Some("Why?"));
        assert_eq!(lib.annotations(attachment.clone()).unwrap(), vec![changed]);
        lib.remove_annotation(mark.id).unwrap();
        assert!(lib.annotations(attachment).unwrap().is_empty());
    }

    #[test]
    fn rejects_bad_marks() {
        let (_dir, lib, attachment) = library_with_pdf();
        let bad_kind = NewAnnotation { kind: "scribble".into(), ..highlight(&attachment) };
        assert!(matches!(lib.add_annotation(bad_kind), Err(CoreError::InvalidInput { .. })));
        let bad_color = NewAnnotation { color: "yellow".into(), ..highlight(&attachment) };
        assert!(matches!(lib.add_annotation(bad_color), Err(CoreError::InvalidInput { .. })));
        let no_rects = NewAnnotation { rects: vec![], ..highlight(&attachment) };
        assert!(matches!(lib.add_annotation(no_rects), Err(CoreError::InvalidInput { .. })));
        assert!(matches!(lib.add_annotation(highlight("missing")), Err(CoreError::NotFound { .. })));
    }
}
