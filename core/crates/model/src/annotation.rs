use serde::{Deserialize, Serialize};

use crate::{AnnotationId, AttachmentId};

/// What a mark on a page is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnnotationKind {
    Highlight,
    Underline,
    StrikeOut,
    /// A sticky note at a point; its rect is the icon's.
    Note,
    /// Freehand strokes (a pen or the mouse); its rect bounds them.
    Ink,
    /// A text box written on the page: its rect is the box, its comment
    /// the text.
    Text,
    /// A rectangle drawn on the page: its rect.
    Square,
    /// An ellipse drawn on the page, inside its rect.
    Circle,
    /// An arrow from its stroke's first point to its last; its rect bounds
    /// them.
    Arrow,
    /// A stamp (`Approved`, `Draft`…): its rect is the stamp, its comment
    /// the word on it.
    Stamp,
}

impl AnnotationKind {
    pub const ALL: [AnnotationKind; 10] = [
        AnnotationKind::Highlight,
        AnnotationKind::Underline,
        AnnotationKind::StrikeOut,
        AnnotationKind::Note,
        AnnotationKind::Ink,
        AnnotationKind::Text,
        AnnotationKind::Square,
        AnnotationKind::Circle,
        AnnotationKind::Arrow,
        AnnotationKind::Stamp,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            AnnotationKind::Highlight => "highlight",
            AnnotationKind::Underline => "underline",
            AnnotationKind::StrikeOut => "strikeout",
            AnnotationKind::Note => "note",
            AnnotationKind::Ink => "ink",
            AnnotationKind::Text => "text",
            AnnotationKind::Square => "square",
            AnnotationKind::Circle => "circle",
            AnnotationKind::Arrow => "arrow",
            AnnotationKind::Stamp => "stamp",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == value)
    }
}

/// A rectangle in PDF user space: points, origin at the bottom left of the
/// unrotated page. PDFKit uses these coordinates directly; pdf.js converts
/// through its viewport.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// A point in the same space as [`Rect`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Rect {
    /// The smallest rectangle around some points, or none for none.
    pub fn around(points: impl IntoIterator<Item = Point>) -> Option<Rect> {
        let mut points = points.into_iter();
        let first = points.next()?;
        let (mut x0, mut y0, mut x1, mut y1) = (first.x, first.y, first.x, first.y);
        for p in points {
            (x0, y0, x1, y1) = (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y));
        }
        Some(Rect { x: x0, y: y0, width: x1 - x0, height: y1 - y0 })
    }

    pub fn is_valid(&self) -> bool {
        [self.x, self.y, self.width, self.height].iter().all(|v| v.is_finite())
            && self.width >= 0.0
            && self.height >= 0.0
    }
}

/// A mark on one page of an attachment. Marks live in the library, not in
/// the PDF, so every platform shows the same ones and sync moves only them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Annotation {
    pub id: AnnotationId,
    pub attachment_id: AttachmentId,
    pub kind: AnnotationKind,
    /// `#rrggbb`.
    pub color: String,
    /// From 0.
    pub page_index: u32,
    /// One per line of marked text.
    pub rects: Vec<Rect>,
    /// The marked text, for lists and notes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// The strokes of an ink mark, each a line through its points; an
    /// arrow's one stroke, from its tail to its head.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub strokes: Vec<Vec<Point>>,
    /// A text box's font size, in points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f64>,
    /// Milliseconds since 1970.
    pub date_added: i64,
    pub date_modified: i64,
}

/// `#rrggbb` (either case).
pub fn is_color(value: &str) -> bool {
    value.len() == 7 && value.starts_with('#') && value[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_json_defaults_tags_and_empty_tags_stay_omitted() {
        let old = serde_json::json!({
            "id": AnnotationId::new(), "attachment_id": AttachmentId::new(),
            "kind": "highlight", "color": "#ffd400", "page_index": 0,
            "rects": [], "date_added": 0, "date_modified": 0,
        });
        let mut annotation: Annotation = serde_json::from_value(old.clone()).unwrap();
        assert!(annotation.tags.is_empty());
        assert_eq!(serde_json::to_value(&annotation).unwrap(), old);
        annotation.tags = vec!["#auto".into()];
        assert_eq!(
            serde_json::from_str::<Annotation>(&serde_json::to_string(&annotation).unwrap()).unwrap(),
            annotation
        );
    }

    #[test]
    fn kinds_round_trip() {
        for kind in AnnotationKind::ALL {
            assert_eq!(AnnotationKind::parse(kind.as_str()), Some(kind));
            assert_eq!(serde_json::to_value(kind).unwrap(), kind.as_str());
        }
    }

    #[test]
    fn checks_colors_and_rects() {
        assert!(is_color("#ffd400") && is_color("#5FB236"));
        assert!(!is_color("ffd400") && !is_color("#ffd40") && !is_color("#ggg000"));
        assert!(Rect { x: 1.0, y: 2.0, width: 3.0, height: 4.0 }.is_valid());
        assert!(!Rect { x: f64::NAN, y: 2.0, width: 3.0, height: 4.0 }.is_valid());
        assert!(!Rect { x: 1.0, y: 2.0, width: -3.0, height: 4.0 }.is_valid());
    }
}
