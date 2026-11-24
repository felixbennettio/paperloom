//! A page as an SVG picture, drawn from its scene: the picture a
//! `.drawio.svg` file shows, and what a diagram's pictures are made of.

use std::fmt::Write as _;

use super::model::num;
use super::scene::{Item, Scene};
use crate::xml::escape;

const FONT: &str = "Helvetica, Arial, 'PingFang SC', 'Microsoft YaHei', sans-serif";

/// A label's lines, wrapped to `room` when it wraps; wide characters count
/// a whole em, others a little over half.
fn lines(text: &str, size: f64, room: f64, wrap: bool) -> Vec<String> {
    let mut out = Vec::new();
    for paragraph in text.split('\n') {
        if !wrap || room <= 0.0 {
            out.push(paragraph.to_string());
            continue;
        }
        let mut line = String::new();
        let mut width = 0.0;
        for c in paragraph.chars() {
            let w = if (c as u32) >= 0x1100 && !c.is_ascii() { size } else { size * 0.58 };
            if width + w > room + 0.5 && !line.is_empty() {
                out.push(std::mem::take(&mut line));
                width = 0.0;
            }
            line.push(c);
            width += w;
        }
        out.push(line);
    }
    out
}

fn paint(colour: &str) -> &str {
    if colour.is_empty() { "none" } else { colour }
}

fn item_svg(item: &Item, out: &mut String) {
    let (cx, cy) = (item.x + item.width / 2.0, item.y + item.height / 2.0);
    out.push_str("<g");
    if item.rotation.abs() > 1e-6 {
        let _ = write!(out, " transform=\"rotate({} {} {})\"", num(item.rotation), num(cx), num(cy));
    }
    if item.opacity < 1.0 {
        let _ = write!(out, " opacity=\"{}\"", num(item.opacity));
    }
    out.push('>');
    let dash = if item.dashed { " stroke-dasharray=\"6 4\"" } else { "" };
    let width = num(item.stroke_width);
    if !item.image.is_empty() {
        let fit = if item.image_aspect { "xMidYMid meet" } else { "none" };
        let _ = write!(
            out,
            "<image href=\"{}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" preserveAspectRatio=\"{fit}\"/>",
            escape(&item.image),
            num(item.x),
            num(item.y),
            num(item.width),
            num(item.height)
        );
    }
    if !item.path.is_empty() {
        let fill = if item.edge { "none" } else { paint(&item.fill) };
        let _ = write!(
            out,
            "<path d=\"{}\" fill=\"{fill}\" stroke=\"{}\" stroke-width=\"{width}\" stroke-linejoin=\"round\"{dash}/>",
            item.path,
            paint(&item.stroke)
        );
    }
    if !item.inner.is_empty() {
        let _ = write!(
            out,
            "<path d=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{width}\"/>",
            item.inner,
            paint(&item.stroke)
        );
    }
    if !item.arrows.is_empty() {
        let colour = paint(&item.stroke);
        let _ = write!(
            out,
            "<path d=\"{}\" fill=\"{colour}\" stroke=\"{colour}\" stroke-width=\"{width}\"/>",
            item.arrows
        );
    }
    if !item.open_arrows.is_empty() {
        let _ = write!(
            out,
            "<path d=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"{width}\"/>",
            item.open_arrows,
            paint(&item.stroke)
        );
    }
    if !item.text.is_empty() {
        label_svg(item, out);
    }
    out.push_str("</g>\n");
}

fn label_svg(item: &Item, out: &mut String) {
    let size = item.font_size;
    let lines = lines(&item.text, size, item.text_width, item.wrap);
    let line_height = size * 1.2;
    let block = line_height * lines.len() as f64;
    let top = match item.vertical_align.as_str() {
        "top" => item.text_y,
        "bottom" => item.text_y + item.text_height - block,
        _ => item.text_y + (item.text_height - block) / 2.0,
    };
    let (x, anchor) = match item.align.as_str() {
        "left" => (item.text_x, "start"),
        "right" => (item.text_x + item.text_width, "end"),
        _ => (item.text_x + item.text_width / 2.0, "middle"),
    };
    if !item.label_background.is_empty() {
        let widest = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as f64 * size * 0.6;
        let _ = write!(
            out,
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>",
            num(x - widest / 2.0 - 2.0),
            num(top),
            num(widest + 4.0),
            num(block),
            item.label_background
        );
    }
    let mut style = String::new();
    if item.bold {
        style.push_str(" font-weight=\"bold\"");
    }
    if item.italic {
        style.push_str(" font-style=\"italic\"");
    }
    if item.underline {
        style.push_str(" text-decoration=\"underline\"");
    }
    let _ = write!(
        out,
        "<text font-size=\"{}\" fill=\"{}\" text-anchor=\"{anchor}\"{style}>",
        num(size),
        paint(&item.font_color)
    );
    for (i, line) in lines.iter().enumerate() {
        let baseline = top + line_height * (i as f64 + 1.0) - size * 0.25;
        let _ = write!(out, "<tspan x=\"{}\" y=\"{}\">{}</tspan>", num(x), num(baseline), escape(line));
    }
    out.push_str("</text>");
}

/// The scene as an SVG picture, `content` (when given) kept in its
/// `content` attribute the way `.drawio.svg` files hold their diagram.
pub fn scene_svg(scene: &Scene, content: Option<&str>) -> String {
    let margin = 10.0;
    let (x, y) = (scene.x - margin, scene.y - margin);
    let (width, height) = ((scene.width + 2.0 * margin).max(1.0), (scene.height + 2.0 * margin).max(1.0));
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" version=\"1.1\" width=\"{}px\" height=\"{}px\" viewBox=\"{} {} {} {}\" \
         font-family=\"{FONT}\"",
        num(width.ceil()),
        num(height.ceil()),
        num(x),
        num(y),
        num(width),
        num(height)
    );
    if let Some(content) = content {
        let _ = write!(out, " content=\"{}\"", escape(content));
    }
    out.push_str(">\n");
    if !scene.background.is_empty() {
        let _ = writeln!(
            out,
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"/>",
            num(x),
            num(y),
            num(width),
            num(height),
            scene.background
        );
    }
    for item in &scene.items {
        item_svg(item, &mut out);
    }
    out.push_str("</svg>\n");
    out
}
