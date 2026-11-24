//! Text read from pictures of pages, line by line with each line's box:
//! Windows' own text recognition (`Windows.Media.Ocr`), or Tesseract where
//! it is installed on Linux. Elsewhere there is none (Android reads pictures
//! with ML Kit in the app's share plugin, Apple with Vision).

use serde::{Deserialize, Serialize};

/// A line read in a picture, and its box as shares of the picture's width
/// and height from its top-left corner.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Line {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Whether this computer can read text from pictures.
pub fn available() -> bool {
    system::available()
}

/// The lines in a picture (a PNG); none if this computer cannot read it.
pub fn recognize(png: &[u8]) -> Option<Vec<Line>> {
    system::recognize(png)
}

/// Recognizers put spaces between Chinese and Japanese characters; the
/// words read better, and are found, without them.
pub fn tidy(line: &str) -> String {
    let chars: Vec<char> = line.trim().chars().collect();
    let mut out = String::with_capacity(line.len());
    for (index, &c) in chars.iter().enumerate() {
        let between =
            index > 0 && index + 1 < chars.len() && wide(chars[index - 1]) && wide(chars[index + 1]);
        if c == ' ' && between {
            continue;
        }
        out.push(c);
    }
    out
}

fn wide(c: char) -> bool {
    matches!(c, '\u{3000}'..='\u{30FF}' | '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}' | '\u{F900}'..='\u{FAFF}' | '\u{FF00}'..='\u{FFEF}')
}

#[cfg(windows)]
mod system {
    use windows::Globalization::Language;
    use windows::Graphics::Imaging::{BitmapDecoder, BitmapPixelFormat, SoftwareBitmap};
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};
    use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
    use windows::core::HSTRING;

    /// A recognizer for the person's languages, else English.
    fn engine() -> windows::core::Result<OcrEngine> {
        // Worker threads join the multithreaded apartment; one already in an
        // apartment stays where it is.
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        OcrEngine::TryCreateFromUserProfileLanguages().or_else(|_| {
            OcrEngine::TryCreateFromLanguage(&Language::CreateLanguage(&HSTRING::from("en-US"))?)
        })
    }

    pub(super) fn available() -> bool {
        engine().is_ok()
    }

    pub(super) fn recognize(png: &[u8]) -> Option<Vec<super::Line>> {
        read(png).ok()
    }

    fn read(png: &[u8]) -> windows::core::Result<Vec<super::Line>> {
        let engine = engine()?;
        let stream = InMemoryRandomAccessStream::new()?;
        let writer = DataWriter::CreateDataWriter(&stream)?;
        writer.WriteBytes(png)?;
        writer.StoreAsync()?.join()?;
        // The stream outlives the writer.
        writer.DetachStream()?;
        stream.Seek(0)?;
        let decoder = BitmapDecoder::CreateAsync(&stream)?.join()?;
        let mut bitmap = decoder.GetSoftwareBitmapAsync()?.join()?;
        if bitmap.BitmapPixelFormat()? != BitmapPixelFormat::Bgra8 {
            bitmap = SoftwareBitmap::Convert(&bitmap, BitmapPixelFormat::Bgra8)?;
        }
        let (width, height) = (f64::from(bitmap.PixelWidth()?), f64::from(bitmap.PixelHeight()?));
        let lines = engine.RecognizeAsync(&bitmap)?.join()?.Lines()?;
        let mut read = Vec::new();
        for index in 0..lines.Size()? {
            let line = lines.GetAt(index)?;
            let text = super::tidy(&line.Text()?.to_string());
            // The line's box: all its words' boxes together.
            let words = line.Words()?;
            let (mut left, mut top, mut right, mut bottom) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
            for word in 0..words.Size()? {
                let r = words.GetAt(word)?.BoundingRect()?;
                let (x, y) = (f64::from(r.X), f64::from(r.Y));
                (left, top) = (left.min(x), top.min(y));
                (right, bottom) = (right.max(x + f64::from(r.Width)), bottom.max(y + f64::from(r.Height)));
            }
            if !text.is_empty() && right > left && bottom > top && width > 0.0 && height > 0.0 {
                read.push(super::Line {
                    text,
                    x: left / width,
                    y: top / height,
                    width: (right - left) / width,
                    height: (bottom - top) / height,
                });
            }
        }
        Ok(read)
    }
}

#[cfg(target_os = "linux")]
mod system {
    use std::io::Write as _;
    use std::process::{Command, Stdio};

    const WIDE: [&str; 4] = ["chi_sim", "chi_tra", "jpn", "kor"];
    const LATIN: [&str; 5] = ["eng", "fra", "deu", "rus", "spa"];

    /// The installed Tesseract languages to read with.
    fn languages() -> Option<String> {
        let out = Command::new("tesseract").arg("--list-langs").output().ok()?;
        // Older versions list them on standard error.
        let listed =
            format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        choose(&listed, own_language())
    }

    /// From the languages Tesseract lists, a Chinese, Japanese or Korean
    /// one alone (the person's first), else the Latin ones (theirs first):
    /// with English in the set Tesseract reads those scripts as Latin
    /// nonsense (纳米金刚石 came out as "NARS SF Mim" with `chi_sim+eng`),
    /// while their models read English lines well.
    pub(super) fn choose(listed: &str, own: Option<&'static str>) -> Option<String> {
        let installed = |name: &str| listed.lines().any(|line| line.trim() == name);
        if let Some(wide) = own.filter(|l| WIDE.contains(l)).into_iter().chain(WIDE).find(|&l| installed(l)) {
            return Some(wide.to_string());
        }
        let mut latin: Vec<&str> = Vec::new();
        for language in own.into_iter().chain(LATIN) {
            if installed(language) && !latin.contains(&language) {
                latin.push(language);
            }
        }
        (!latin.is_empty()).then(|| latin.join("+"))
    }

    /// The Tesseract name of the person's language, from the locale.
    fn own_language() -> Option<&'static str> {
        let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
            .into_iter()
            .find_map(|name| std::env::var(name).ok().filter(|value| !value.is_empty()))?
            .to_ascii_lowercase();
        // zh_TW.UTF-8, zh-Hant, en_US@euro…
        let tag = locale.split(['.', '@']).next().unwrap_or_default();
        let parts: Vec<&str> = tag.split(['_', '-']).collect();
        Some(match parts[0] {
            "zh" if parts[1..].iter().any(|p| matches!(*p, "tw" | "hk" | "mo" | "hant")) => "chi_tra",
            "zh" => "chi_sim",
            "ja" => "jpn",
            "ko" => "kor",
            "fr" => "fra",
            "de" => "deu",
            "ru" => "rus",
            "es" => "spa",
            "en" => "eng",
            _ => return None,
        })
    }

    pub(super) fn available() -> bool {
        languages().is_some()
    }

    pub(super) fn recognize(png: &[u8]) -> Option<Vec<super::Line>> {
        let languages = languages()?;
        let mut child = Command::new("tesseract")
            .args(["stdin", "stdout", "-l", &languages, "tsv"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        // Tesseract reads the whole picture before it writes; closing its
        // input (dropping it) lets it start.
        child.stdin.take()?.write_all(png).ok()?;
        let out = child.wait_with_output().ok()?;
        if !out.status.success() {
            return None;
        }
        Some(lines_of_tsv(&String::from_utf8_lossy(&out.stdout)))
    }

    /// A line being gathered from its words.
    struct Gathered {
        /// Block, paragraph and line numbers.
        key: (u32, u32, u32),
        words: Vec<String>,
        edges: [f64; 4],
    }

    /// Tesseract's TSV: the page's size (level 1), then words (level 5),
    /// gathered into their lines (block, paragraph and line numbers).
    pub(super) fn lines_of_tsv(tsv: &str) -> Vec<super::Line> {
        let (mut width, mut height) = (0.0, 0.0);
        let mut lines: Vec<Gathered> = Vec::new();
        for row in tsv.lines().skip(1) {
            let cells: Vec<&str> = row.split('\t').collect();
            if cells.len() < 12 {
                continue;
            }
            let number = |i: usize| cells[i].trim().parse::<f64>().unwrap_or(0.0);
            let id = |i: usize| cells[i].trim().parse::<u32>().unwrap_or(0);
            match cells[0].trim() {
                "1" => (width, height) = (number(8), number(9)),
                "5" if !cells[11].trim().is_empty() => {
                    let key = (id(2), id(3), id(4));
                    let word = cells[11].trim().to_string();
                    let (left, top) = (number(6), number(7));
                    let (right, bottom) = (left + number(8), top + number(9));
                    match lines.iter_mut().find(|line| line.key == key) {
                        Some(line) => {
                            line.words.push(word);
                            let e = &mut line.edges;
                            *e = [e[0].min(left), e[1].min(top), e[2].max(right), e[3].max(bottom)];
                        }
                        None => {
                            lines.push(Gathered { key, words: vec![word], edges: [left, top, right, bottom] })
                        }
                    }
                }
                _ => {}
            }
        }
        if width <= 0.0 || height <= 0.0 {
            return Vec::new();
        }
        lines
            .into_iter()
            .map(|Gathered { words, edges: [left, top, right, bottom], .. }| super::Line {
                text: super::tidy(&words.join(" ")),
                x: left / width,
                y: top / height,
                width: (right - left) / width,
                height: (bottom - top) / height,
            })
            .filter(|line| !line.text.is_empty())
            .collect()
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod system {
    pub(super) fn available() -> bool {
        false
    }

    pub(super) fn recognize(_png: &[u8]) -> Option<Vec<super::Line>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::tidy;

    /// With `OCR_SAMPLE_PNG` set (CI draws an English and a Chinese line),
    /// the installed Tesseract reads the picture.
    #[cfg(target_os = "linux")]
    #[test]
    fn reads_a_picture_when_one_is_given() {
        let Ok(path) = std::env::var("OCR_SAMPLE_PNG") else { return };
        assert!(super::available(), "Tesseract and its languages are installed");
        let lines = super::recognize(&std::fs::read(path).unwrap()).expect("Tesseract ran");
        let text = lines.iter().map(|line| line.text.as_str()).collect::<Vec<_>>().join("\n");
        assert!(text.contains("Thermometry"), "{text}");
        // Read as Chinese: Tesseract's model may still mistake a character or two.
        let chinese = "纳米金刚石量子测温";
        assert!(chinese.chars().filter(|&c| text.contains(c)).count() >= 7, "{text}");
        assert!(
            lines.iter().all(|l| (0.0..1.0).contains(&l.x) && l.width > 0.0 && l.height > 0.0),
            "{lines:?}"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn reads_chinese_japanese_and_korean_without_english() {
        use super::system::choose;

        let listed = "List of available languages in \"/usr/share/tesseract-ocr/5/tessdata/\" (4):\nchi_sim\nchi_tra\neng\nosd\n";
        assert_eq!(choose(listed, None).as_deref(), Some("chi_sim"));
        assert_eq!(choose(listed, Some("eng")).as_deref(), Some("chi_sim"));
        assert_eq!(choose(listed, Some("chi_tra")).as_deref(), Some("chi_tra"));
        assert_eq!(choose(listed, Some("jpn")).as_deref(), Some("chi_sim"));
        let latin = "List of available languages (3):\neng\nfra\nosd\n";
        assert_eq!(choose(latin, None).as_deref(), Some("eng+fra"));
        assert_eq!(choose(latin, Some("fra")).as_deref(), Some("fra+eng"));
        assert_eq!(choose("List of available languages (1):\nosd\n", Some("eng")), None);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn gathers_tesseract_words_into_lines_with_boxes() {
        let tsv = "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n\
                   1\t1\t0\t0\t0\t0\t0\t0\t1000\t500\t-1\t\n\
                   4\t1\t1\t1\t1\t0\t100\t50\t400\t40\t-1\t\n\
                   5\t1\t1\t1\t1\t1\t100\t50\t150\t40\t96\tQuantum\n\
                   5\t1\t1\t1\t1\t2\t300\t55\t200\t30\t95\tsensing\n\
                   5\t1\t1\t1\t2\t1\t100\t200\t20\t30\t90\t纳\n\
                   5\t1\t1\t1\t2\t2\t125\t200\t20\t30\t90\t米\n";
        let lines = super::system::lines_of_tsv(tsv);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "Quantum sensing");
        assert_eq!((lines[0].x, lines[0].y, lines[0].width, lines[0].height), (0.1, 0.1, 0.4, 0.08));
        assert_eq!(lines[1].text, "纳米");
    }

    #[test]
    fn joins_chinese_characters_and_keeps_spaces_between_words() {
        assert_eq!(tidy(" 纳 米 金 刚 石 测 温 "), "纳米金刚石测温");
        assert_eq!(tidy("Quantum sensing in 活 细 胞 cells"), "Quantum sensing in 活细胞 cells");
        assert_eq!(tidy("日本語 の テキスト"), "日本語のテキスト");
    }
}
