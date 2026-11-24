//! Pictures the apps draw (mind maps, diagrams, drawings) as PDF pages.

use lopdf::{Dictionary, Document, Object, Stream};

use crate::CoreError;

/// A JPEG's width, height and colour components, from its frame header.
fn jpeg_size(jpeg: &[u8]) -> Option<(u32, u32, u8)> {
    if !jpeg.starts_with(&[0xff, 0xd8]) {
        return None;
    }
    let mut at = 2;
    while at + 4 <= jpeg.len() {
        if jpeg[at] != 0xff {
            at += 1;
            continue;
        }
        let marker = jpeg[at + 1];
        let length = usize::from(u16::from_be_bytes([jpeg[at + 2], jpeg[at + 3]]));
        // Start of frame (any kind but the huffman and arithmetic tables).
        if (0xc0..=0xcf).contains(&marker) && !matches!(marker, 0xc4 | 0xc8 | 0xcc) {
            let frame = jpeg.get(at + 4..at + 10)?;
            let height = u32::from(u16::from_be_bytes([frame[1], frame[2]]));
            let width = u32::from(u16::from_be_bytes([frame[3], frame[4]]));
            return Some((width, height, frame[5]));
        }
        at += 2 + length;
    }
    None
}

/// A picture (a JPEG drawn at twice its size) as a one-page PDF, the page
/// as big as the picture at its own size.
#[uniffi::export]
pub fn picture_pdf(jpeg: Vec<u8>) -> Result<Vec<u8>, CoreError> {
    let (width, height, components) = jpeg_size(&jpeg)
        .filter(|(w, h, _)| *w > 0 && *h > 0)
        .ok_or_else(|| CoreError::InvalidInput { message: "The picture is not a JPEG.".into() })?;
    let (page_width, page_height) = (f64::from(width) / 2.0, f64::from(height) / 2.0);
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();

    let mut image = Dictionary::new();
    image.set("Type", "XObject");
    image.set("Subtype", "Image");
    image.set("Width", i64::from(width));
    image.set("Height", i64::from(height));
    image.set("ColorSpace", if components == 1 { "DeviceGray" } else { "DeviceRGB" });
    image.set("BitsPerComponent", 8_i64);
    image.set("Filter", "DCTDecode");
    let mut picture = Stream::new(image, jpeg);
    // Already compressed: kept as it is.
    picture.allows_compression = false;
    let picture_id = doc.add_object(picture);

    let drawing = format!("q {page_width:.2} 0 0 {page_height:.2} 0 0 cm /Im0 Do Q");
    let content_id = doc.add_object(Stream::new(Dictionary::new(), drawing.into_bytes()));
    let mut objects = Dictionary::new();
    objects.set("Im0", Object::Reference(picture_id));
    let mut resources = Dictionary::new();
    resources.set("XObject", objects);
    let mut page = Dictionary::new();
    page.set("Type", "Page");
    page.set("Parent", Object::Reference(pages_id));
    page.set("MediaBox", [0.0, 0.0, page_width, page_height].map(Object::from).to_vec());
    page.set("Contents", Object::Reference(content_id));
    page.set("Resources", resources);
    let page_id = doc.add_object(page);

    let mut pages = Dictionary::new();
    pages.set("Type", "Pages");
    pages.set("Kids", vec![Object::Reference(page_id)]);
    pages.set("Count", 1_i64);
    doc.objects.insert(pages_id, Object::Dictionary(pages));
    let mut catalog = Dictionary::new();
    catalog.set("Type", "Catalog");
    catalog.set("Pages", Object::Reference(pages_id));
    let catalog_id = doc.add_object(catalog);
    doc.trailer.set("Root", Object::Reference(catalog_id));

    let mut out = Vec::new();
    doc.save_to(&mut out).map_err(|e| CoreError::Storage { message: format!("Writing the PDF: {e}") })?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The start of a JPEG: its SOI, an APP0 segment, and a baseline frame
    /// of 640 by 480 in three colours.
    fn jpeg() -> Vec<u8> {
        let mut bytes = vec![0xff, 0xd8, 0xff, 0xe0, 0x00, 0x04, 0x00, 0x00];
        bytes.extend([0xff, 0xc0, 0x00, 0x11, 0x08, 0x01, 0xe0, 0x02, 0x80, 0x03]);
        bytes.extend([0; 12]);
        bytes.extend([0xff, 0xd9]);
        bytes
    }

    #[test]
    fn wraps_pictures_in_a_page() {
        assert_eq!(jpeg_size(&jpeg()), Some((640, 480, 3)));
        let pdf = picture_pdf(jpeg()).unwrap();
        assert!(pdf.starts_with(b"%PDF-1.5"));
        let doc = Document::load_mem(&pdf).unwrap();
        let page = doc.get_pages()[&1];
        let size = doc.get_dictionary(page).unwrap().get(b"MediaBox").unwrap().as_array().unwrap().clone();
        assert_eq!(size[2].as_float().unwrap(), 320.0);
        assert!(matches!(picture_pdf(b"\x89PNG".to_vec()), Err(CoreError::InvalidInput { .. })));
    }
}
