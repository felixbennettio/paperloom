//! What other apps share to Paperloom (Android's share sheet) and the files
//! opened with it (a file manager's Open With on Windows and Linux), sorted
//! for the importers: PDFs join the files picked in a dialog, reference
//! files (RIS, BibTeX, EndNote, PubMed…) are read as text, and shared text
//! comes as it was.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use paperloom_core::SearchPage;
use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime, State};
use tauri_plugin_dialog::FilePath;
use tauri_plugin_paperloom_share::{Share, SharedText};

use crate::pdf::Picked;
use crate::{AppState, CommandResult};

/// The largest reference file read.
const MOST_TEXT: u64 = 16 * 1024 * 1024;

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Received {
    texts: Vec<SharedText>,
    /// For `read_picked_file` and `import_pdf`.
    pdfs: Vec<String>,
    references: Vec<ReferenceFile>,
    /// Names of files that are neither.
    unsupported: Vec<String>,
    /// Files were opened with Paperloom, rather than shared to it.
    opened: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReferenceFile {
    name: String,
    text: String,
}

/// Files opened with Paperloom, waiting for `take_shared`.
#[derive(Default)]
pub(crate) struct Opened(Mutex<Vec<PathBuf>>);

/// Keeps the files among a launch's arguments (the program's path first)
/// for `take_shared` and tells the window: the first launch's, and a second
/// launch's that the single-instance plugin hands over.
#[cfg(desktop)]
pub(crate) fn open_files<R: Runtime>(app: &AppHandle<R>, args: &[String], cwd: &Path) {
    use tauri::Emitter;

    let files = opened_files(args, cwd);
    let Some(opened) = app.try_state::<Opened>() else { return };
    if files.is_empty() {
        return;
    }
    if let Ok(mut waiting) = opened.0.lock() {
        waiting.extend(files);
    }
    let _ = app.emit(tauri_plugin_paperloom_share::RECEIVED, ());
}

/// The existing files among a launch's arguments: paths (relative ones from
/// `cwd`) and file:// URLs, as Linux file managers pass them (`%U`).
/// Options and other URLs, such as a sign-in callback, are not files.
#[cfg(desktop)]
fn opened_files(args: &[String], cwd: &Path) -> Vec<PathBuf> {
    args.iter()
        .skip(1)
        .filter_map(|arg| {
            let path = if arg.starts_with("file://") {
                tauri::Url::parse(arg).ok()?.to_file_path().ok()?
            } else if arg.starts_with('-') || arg.contains("://") {
                return None;
            } else {
                cwd.join(arg)
            };
            path.is_file().then_some(path)
        })
        .collect()
}

/// Takes what was shared or opened since last asked.
#[tauri::command]
pub(crate) async fn take_shared<R: Runtime>(app: AppHandle<R>) -> CommandResult<Received> {
    let handle = app.clone();
    let shared = tauri::async_runtime::spawn_blocking(move || handle.state::<Share<R>>().take())
        .await
        .map_err(|e| e.to_string())??;
    let mut received = Received { texts: shared.texts, ..Received::default() };
    let mut pdfs = Vec::new();
    for file in shared.files {
        let path = PathBuf::from(&file.path);
        if is_pdf(&file.name, file.mime.as_deref(), &path) {
            pdfs.push(FilePath::Path(path));
            continue;
        }
        match read_text(&path) {
            Some(text) => received.references.push(ReferenceFile { name: file.name, text }),
            None => received.unsupported.push(file.name),
        }
        // A shared file is the app's own copy.
        let _ = std::fs::remove_file(&path);
    }
    // An opened file is the person's own: read, never removed.
    let opened = std::mem::take(&mut *app.state::<Opened>().0.lock().map_err(|e| e.to_string())?);
    received.opened = !opened.is_empty();
    for path in opened {
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        if is_pdf(&name, None, &path) {
            pdfs.push(FilePath::Path(path));
            continue;
        }
        match read_text(&path) {
            Some(text) => received.references.push(ReferenceFile { name, text }),
            None => received.unsupported.push(name),
        }
    }
    received.pdfs = app.state::<Picked>().remember(pdfs)?;
    Ok(received)
}

/// Reads the works on a shared web page, kept for `add_search_results`.
/// Network, so off the main thread.
#[tauri::command]
pub(crate) async fn read_web_page(state: State<'_, AppState>, url: String) -> CommandResult<SearchPage> {
    let library = state.library.clone();
    tauri::async_runtime::spawn_blocking(move || library.read_web_page(url))
        .await
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())
}

fn is_pdf(name: &str, mime: Option<&str>, path: &Path) -> bool {
    if mime == Some("application/pdf") || name.to_lowercase().ends_with(".pdf") {
        return true;
    }
    let mut head = [0u8; 5];
    std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut head)).is_ok() && head == *b"%PDF-"
}

/// A file's text: UTF-8, or UTF-16 with a byte order mark; nothing for
/// files too large or not text.
fn read_text(path: &Path) -> Option<String> {
    if std::fs::metadata(path).ok()?.len() > MOST_TEXT {
        return None;
    }
    decode(&std::fs::read(path).ok()?)
}

fn decode(bytes: &[u8]) -> Option<String> {
    let utf16 = |big: bool| {
        let units = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|&pair| if big { u16::from_be_bytes(pair) } else { u16::from_le_bytes(pair) });
        char::decode_utf16(units).collect::<Result<String, _>>().ok()
    };
    let text = match bytes {
        [0xFF, 0xFE, ..] => utf16(false)?,
        [0xFE, 0xFF, ..] => utf16(true)?,
        _ => {
            if bytes.iter().take(8192).any(|&b| b == 0) {
                return None;
            }
            String::from_utf8_lossy(bytes).into_owned()
        }
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    (!text.trim().is_empty()).then(|| text.to_string())
}

#[cfg(test)]
mod tests {
    use super::decode;

    #[test]
    fn reads_utf8_and_utf16_text() {
        assert_eq!(decode(b"\xEF\xBB\xBFTY  - JOUR\n").as_deref(), Some("TY  - JOUR\n"));
        let utf16: Vec<u8> =
            [0xFF, 0xFE].into_iter().chain("TY  - JOUR".encode_utf16().flat_map(u16::to_le_bytes)).collect();
        assert_eq!(decode(&utf16).as_deref(), Some("TY  - JOUR"));
        let big: Vec<u8> = [0xFE, 0xFF]
            .into_iter()
            .chain("@article{a,}".encode_utf16().flat_map(u16::to_be_bytes))
            .collect();
        assert_eq!(decode(&big).as_deref(), Some("@article{a,}"));
    }

    #[test]
    fn skips_binary_and_empty_files() {
        assert_eq!(decode(b"PK\x03\x04\x00\x00zip"), None);
        assert_eq!(decode(b"  \n"), None);
    }
}
