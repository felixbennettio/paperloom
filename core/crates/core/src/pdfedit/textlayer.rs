//! Invisible words over a page's picture, in the page's own coordinates.

use std::collections::BTreeMap;

use lopdf::content::{Content, Operation};

use super::*;

/// The nearest value in the page tree, including indirect values.
fn inherited<'a>(doc: &'a Document, page: ObjectId, key: &[u8]) -> Option<&'a Object> {
    let mut current = Some(page);
    for _ in 0..64 {
        let dict = doc.get_dictionary(current?).ok()?;
        if let Ok(value) = dict.get(key) {
            return doc.dereference(value).ok().map(|(_, value)| value);
        }
        current = dict.get(b"Parent").and_then(Object::as_reference).ok();
    }
    None
}

struct PageSpace {
    bounds: [f64; 4],
    rotation: i64,
}

impl PageSpace {
    fn read(doc: &Document, page: ObjectId) -> Option<Self> {
        let bounds = inherited(doc, page, b"CropBox")
            .or_else(|| inherited(doc, page, b"MediaBox"))?
            .as_array()
            .ok()?;
        let [x0, y0, x1, y1] = bounds.as_slice() else { return None };
        let bounds = [number(x0)?, number(y0)?, number(x1)?, number(y1)?];
        if !bounds.iter().all(|n| n.is_finite()) || bounds[2] <= bounds[0] || bounds[3] <= bounds[1] {
            return None;
        }
        let rotation = inherited(doc, page, b"Rotate").and_then(number).unwrap_or(0.0);
        if !rotation.is_finite() || rotation % 90.0 != 0.0 {
            return None;
        }
        Some(Self { bounds, rotation: (rotation as i64).rem_euclid(360) })
    }

    /// Font size, horizontal scale, and a matrix undoing the viewer's turn.
    fn placement(&self, line: &RecognizedLine) -> (f64, f64, [f64; 6]) {
        let [x0, y0, x1, y1] = self.bounds;
        let (w, h) = (x1 - x0, y1 - y0);
        let (shown_w, shown_h) = if self.rotation % 180 == 0 { (w, h) } else { (h, w) };
        let size = line.height * shown_h;
        let x = line.x * shown_w;
        // The font's descent is 0.2 em; its ascent is 0.8 em.
        let y = (1.0 - line.y) * shown_h - size * 0.8;
        let matrix = match self.rotation {
            90 => [0.0, 1.0, -1.0, 0.0, x0 + w - y, y0 + x],
            180 => [-1.0, 0.0, 0.0, -1.0, x0 + w - x, y0 + h - y],
            270 => [0.0, -1.0, 1.0, 0.0, x0 + y, y0 + h - x],
            _ => [1.0, 0.0, 0.0, 1.0, x0 + x, y0 + y],
        };
        // Each UTF-16 unit has an advance of 500 / 1000 em.
        let scale = 100.0 * line.width * shown_w / (0.5 * size * line.text.encode_utf16().count() as f64);
        (size, scale, matrix)
    }
}

fn usable(line: &RecognizedLine) -> bool {
    !line.text.trim().is_empty()
        && [line.x, line.y, line.width, line.height].iter().all(|n| n.is_finite())
        && line.x >= 0.0
        && line.y >= 0.0
        && line.width > 0.0
        && line.height > 0.0
        && line.x + line.width <= 1.0
        && line.y + line.height <= 1.0
}

/// Copies resources before changing them: other pages may share them.
fn resources(doc: &Document, page: ObjectId, font: ObjectId) -> Result<(Dictionary, String), CoreError> {
    let mut resources = inherited(doc, page, b"Resources")
        .map(Object::as_dict)
        .transpose()
        .map_err(unreadable)?
        .cloned()
        .unwrap_or_default();
    let mut fonts = resources
        .get(b"Font")
        .ok()
        .map(|fonts| doc.dereference(fonts).and_then(|(_, fonts)| fonts.as_dict()))
        .transpose()
        .map_err(unreadable)?
        .cloned()
        .unwrap_or_default();
    let mut index = 0;
    let name = loop {
        let name = format!("PaperloomText{index}");
        if !fonts.has(name.as_bytes()) {
            break name;
        }
        index += 1;
    };
    fonts.set(name.as_bytes(), font);
    resources.set("Font", fonts);
    Ok((resources, name))
}

fn content(space: &PageSpace, lines: &[RecognizedLine], font: &str) -> Result<Vec<u8>, CoreError> {
    let mut operations = vec![Operation::new("q", vec![])];
    for line in lines {
        let (size, scale, matrix) = space.placement(line);
        operations.extend([
            Operation::new("BT", vec![]),
            Operation::new("Tr", vec![3.into()]),
            Operation::new("Tc", vec![0.into()]),
            Operation::new("Tw", vec![0.into()]),
            Operation::new("Ts", vec![0.into()]),
            Operation::new("Tf", vec![Object::Name(font.as_bytes().to_vec()), size.into()]),
            Operation::new("Tz", vec![scale.into()]),
            Operation::new("Tm", matrix.map(Object::from).to_vec()),
            Operation::new(
                "Tj",
                vec![Object::String(
                    line.text.encode_utf16().flat_map(u16::to_be_bytes).collect(),
                    StringFormat::Hexadecimal,
                )],
            ),
            Operation::new("ET", vec![]),
        ]);
    }
    operations.push(Operation::new("Q", vec![]));
    // Separate this stream's first token from the preceding stream's last.
    let mut bytes = vec![b'\n'];
    bytes.extend(Content { operations }.encode().map_err(unreadable)?);
    bytes.push(b'\n');
    Ok(bytes)
}

pub(super) fn apply(doc: &mut Document, pages: Vec<RecognizedPage>) -> Result<bool, CoreError> {
    let listed: BTreeMap<_, _> = pages.into_iter().map(|page| (page.page_index, page.lines)).collect();
    let ids = doc.get_pages();
    let mut font = None;
    let mut changed = false;
    for (index, lines) in listed {
        let Some(page_number) = index.checked_add(1) else { continue };
        let Some(&page) = ids.get(&page_number) else { continue };
        let lines: Vec<_> = lines.into_iter().filter(usable).collect();
        if lines.is_empty() {
            continue;
        }
        let text = doc.extract_text(&[page_number]).map_err(unreadable)?;
        if text.chars().filter(|c| c.is_alphabetic()).take(20).count() >= 20 {
            continue;
        }
        let Some(space) = PageSpace::read(doc, page) else { continue };
        let font = *font.get_or_insert_with(|| add_font(doc));
        let (resources, name) = resources(doc, page, font)?;
        let bytes = content(&space, &lines, &name)?;
        // lopdf's append helper needs a direct array, not a reference to one.
        let contents = doc.get_dictionary(page).map_err(unreadable)?.get(b"Contents").ok();
        let array = contents
            .map(|value| doc.dereference(value))
            .transpose()
            .map_err(unreadable)?
            .and_then(|(_, value)| value.as_array().ok().cloned());
        let dict = doc.get_dictionary_mut(page).map_err(unreadable)?;
        if let Some(array) = array {
            dict.set("Contents", array);
        }
        dict.set("Resources", resources);
        doc.add_page_contents(page, bytes).map_err(unreadable)?;
        changed = true;
    }
    Ok(changed)
}

const TO_UNICODE: &[u8] = b"/CIDInit /ProcSet findresource begin
12 dict begin
begincmap
/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def
/CMapName /PaperloomUnicode def
/CMapType 2 def
1 begincodespacerange
<0000> <FFFF>
endcodespacerange
1 beginbfrange
<0000> <FFFF> <0000>
endbfrange
endcmap
CMapName currentdict /CMap defineresource pop
end
end
";

/// One shared glyphless font; every CID maps to the sole empty glyph.
fn add_font(doc: &mut Document) -> ObjectId {
    if let Some((&id, _)) = doc.objects.iter().find(|(_, object)| {
        object.as_dict().is_ok_and(|dict| {
            dict.get(b"PaperloomTextLayer").and_then(Object::as_bool).unwrap_or(false)
                && dict.get(b"Subtype").and_then(Object::as_name).ok() == Some(b"Type0".as_slice())
        })
    }) {
        return id;
    }
    let bytes = glyphless_font();
    let mut file = Dictionary::new();
    file.set("Length1", bytes.len() as i64);
    let file = doc.add_object(Stream::new(file, bytes));
    let mut descriptor = Dictionary::new();
    descriptor.set("Type", "FontDescriptor");
    descriptor.set("FontName", "PaperloomGlyphless");
    descriptor.set("Flags", 5_i64);
    descriptor.set("FontBBox", vec![0.into(), (-200).into(), 500.into(), 800.into()]);
    descriptor.set("ItalicAngle", 0_i64);
    descriptor.set("Ascent", 800_i64);
    descriptor.set("Descent", -200_i64);
    descriptor.set("CapHeight", 800_i64);
    descriptor.set("StemV", 0_i64);
    descriptor.set("FontFile2", file);
    let descriptor = doc.add_object(descriptor);
    let mut system = Dictionary::new();
    system.set("Registry", Object::string_literal("Adobe"));
    system.set("Ordering", Object::string_literal("Identity"));
    system.set("Supplement", 0_i64);
    let mut map = Stream::new(Dictionary::new(), vec![0; 2 * 65536]);
    let _ = map.compress();
    let map = doc.add_object(map);
    let mut descendant = Dictionary::new();
    descendant.set("Type", "Font");
    descendant.set("Subtype", "CIDFontType2");
    descendant.set("BaseFont", "PaperloomGlyphless");
    descendant.set("CIDSystemInfo", system);
    descendant.set("FontDescriptor", descriptor);
    descendant.set("CIDToGIDMap", map);
    descendant.set("DW", 500_i64);
    let descendant = doc.add_object(descendant);
    let unicode = doc.add_object(Stream::new(Dictionary::new(), TO_UNICODE.to_vec()));
    let mut font = Dictionary::new();
    font.set("Type", "Font");
    font.set("Subtype", "Type0");
    font.set("BaseFont", "PaperloomGlyphless");
    font.set("Encoding", "Identity-H");
    font.set("DescendantFonts", vec![Object::Reference(descendant)]);
    font.set("ToUnicode", unicode);
    font.set("PaperloomTextLayer", true);
    doc.add_object(font)
}

fn put16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_be_bytes());
}

fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_be_bytes());
}

fn checksum(bytes: &[u8]) -> u32 {
    bytes.chunks(4).fold(0_u32, |sum, chunk| {
        let mut word = [0; 4];
        word[..chunk.len()].copy_from_slice(chunk);
        sum.wrapping_add(u32::from_be_bytes(word))
    })
}

/// A TrueType sfnt built here, with a single empty .notdef glyph.
fn glyphless_font() -> Vec<u8> {
    let mut head = vec![0; 54];
    put32(&mut head, 0, 0x0001_0000);
    put32(&mut head, 4, 0x0001_0000);
    put32(&mut head, 12, 0x5f0f_3cf5);
    put16(&mut head, 18, 1000);
    put16(&mut head, 46, 8);
    put16(&mut head, 48, 2);
    let mut hhea = vec![0; 36];
    put32(&mut hhea, 0, 0x0001_0000);
    put16(&mut hhea, 4, 800);
    put16(&mut hhea, 6, (-200_i16) as u16);
    put16(&mut hhea, 10, 500);
    put16(&mut hhea, 14, 500);
    put16(&mut hhea, 18, 1);
    put16(&mut hhea, 34, 1);
    let mut maxp = vec![0; 32];
    put32(&mut maxp, 0, 0x0001_0000);
    put16(&mut maxp, 4, 1);
    put16(&mut maxp, 14, 1);
    let mut os2 = vec![0; 78];
    put16(&mut os2, 2, 500);
    put16(&mut os2, 4, 400);
    put16(&mut os2, 6, 5);
    os2[58..62].copy_from_slice(b"PLUM");
    put16(&mut os2, 62, 0x40);
    put16(&mut os2, 66, 0xffff);
    put16(&mut os2, 68, 800);
    put16(&mut os2, 70, (-200_i16) as u16);
    put16(&mut os2, 74, 800);
    put16(&mut os2, 76, 200);
    let mut cmap = vec![0; 36];
    put16(&mut cmap, 2, 1);
    put16(&mut cmap, 4, 3);
    put16(&mut cmap, 6, 1);
    put32(&mut cmap, 8, 12);
    put16(&mut cmap, 12, 4);
    put16(&mut cmap, 14, 24);
    put16(&mut cmap, 18, 2);
    put16(&mut cmap, 20, 2);
    put16(&mut cmap, 26, 0xffff);
    put16(&mut cmap, 30, 0xffff);
    put16(&mut cmap, 32, 1);
    let names = [
        (1, "PaperloomGlyphless"),
        (2, "Regular"),
        (3, "PaperloomGlyphless-1"),
        (4, "PaperloomGlyphless"),
        (5, "Version 1.0"),
        (6, "PaperloomGlyphless"),
    ];
    let mut name = vec![0; 6 + names.len() * 12];
    let storage = name.len();
    put16(&mut name, 2, names.len() as u16);
    put16(&mut name, 4, storage as u16);
    for (index, (id, text)) in names.into_iter().enumerate() {
        let at = 6 + index * 12;
        let offset = name.len() - storage;
        let bytes: Vec<_> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
        put16(&mut name, at, 3);
        put16(&mut name, at + 2, 1);
        put16(&mut name, at + 4, 0x0409);
        put16(&mut name, at + 6, id);
        put16(&mut name, at + 8, bytes.len() as u16);
        put16(&mut name, at + 10, offset as u16);
        name.extend(bytes);
    }
    let mut post = vec![0; 32];
    put32(&mut post, 0, 0x0003_0000);
    put32(&mut post, 12, 1);
    let tables = [
        (*b"OS/2", os2),
        (*b"cmap", cmap),
        (*b"glyf", vec![0; 12]),
        (*b"head", head),
        (*b"hhea", hhea),
        (*b"hmtx", vec![1, 244, 0, 0]),
        (*b"loca", vec![0, 0, 0, 6]),
        (*b"maxp", maxp),
        (*b"name", name),
        (*b"post", post),
    ];
    let mut font = vec![0; 12 + tables.len() * 16];
    put32(&mut font, 0, 0x0001_0000);
    put16(&mut font, 4, tables.len() as u16);
    put16(&mut font, 6, 128);
    put16(&mut font, 8, 3);
    put16(&mut font, 10, 32);
    let mut head_offset = 0;
    for (index, (tag, table)) in tables.into_iter().enumerate() {
        let at = 12 + index * 16;
        let offset = font.len();
        font[at..at + 4].copy_from_slice(&tag);
        put32(&mut font, at + 4, checksum(&table));
        put32(&mut font, at + 8, offset as u32);
        put32(&mut font, at + 12, table.len() as u32);
        if tag == *b"head" {
            head_offset = offset;
        }
        font.extend(table);
        font.resize(font.len().next_multiple_of(4), 0);
    }
    let adjustment = 0xb1b0_afba_u32.wrapping_sub(checksum(&font));
    put32(&mut font, head_offset + 8, adjustment);
    font
}

#[cfg(test)]
mod tests;
