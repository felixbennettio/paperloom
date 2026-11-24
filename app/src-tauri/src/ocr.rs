//! Text read from pictures of scanned PDF pages, for search and the AI, and
//! the lines with their places for an invisible text layer: Windows' own
//! text recognition, Tesseract where it is installed on Linux, and ML Kit on
//! Android (through the share plugin). The web UI draws a page that has no
//! text and hands it over.

use paperloom_ocr::Line;
use tauri::{AppHandle, Runtime};

use crate::CommandResult;

/// Whether this device can read text from pictures.
#[tauri::command]
pub(crate) async fn text_recognition_available() -> bool {
    // Android's recognizer comes with the app.
    if cfg!(target_os = "android") {
        return true;
    }
    tauri::async_runtime::spawn_blocking(paperloom_ocr::available).await.unwrap_or(false)
}

/// The text in a page's picture (a PNG, in base64), line by line; none if
/// nothing was read.
#[tauri::command]
pub(crate) async fn recognize_page_text<R: Runtime>(
    app: AppHandle<R>,
    png: String,
) -> CommandResult<Option<String>> {
    let lines = recognize_page_lines(app, png).await?;
    let text: Vec<String> = lines.into_iter().map(|line| line.text).collect();
    Ok((!text.is_empty()).then(|| text.join("\n")))
}

/// The lines in a page's picture (a PNG, in base64) with their boxes; empty
/// if nothing was read.
#[tauri::command]
pub(crate) async fn recognize_page_lines<R: Runtime>(
    app: AppHandle<R>,
    png: String,
) -> CommandResult<Vec<Line>> {
    tauri::async_runtime::spawn_blocking(move || {
        #[cfg(target_os = "android")]
        {
            use tauri::Manager as _;

            match app.state::<tauri_plugin_paperloom_share::Share<R>>().recognize_text(png) {
                Ok(read) => read
                    .into_iter()
                    .map(|line| Line {
                        text: paperloom_ocr::tidy(&line.text),
                        x: line.x,
                        y: line.y,
                        width: line.width,
                        height: line.height,
                    })
                    .filter(|line| !line.text.is_empty())
                    .collect(),
                Err(e) => {
                    eprintln!("text recognition failed: {e}");
                    Vec::new()
                }
            }
        }
        #[cfg(not(target_os = "android"))]
        {
            use base64::Engine as _;

            let _ = app;
            base64::engine::general_purpose::STANDARD
                .decode(png)
                .ok()
                .and_then(|png| paperloom_ocr::recognize(&png))
                .unwrap_or_default()
        }
    })
    .await
    .map_err(|e| e.to_string())
}

/// Why the web UI could not draw or read a page for recognition, for the
/// log (never what the page says).
#[tauri::command]
pub(crate) fn text_recognition_note(note: String) {
    eprintln!("text recognition: {note}");
}

/// `paperloom --recognize-text <picture.png> <text.txt>`: the installer
/// tests' check that text recognition works on a computer. Writes what it
/// read; exits 0 when it read something, 1 when not, 3 when this computer
/// has no recognizer, 2 when the arguments are wrong.
pub(crate) fn command_line() -> Option<i32> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) != Some("--recognize-text") {
        return None;
    }
    let (Some(picture), Some(out)) = (args.get(2), args.get(3)) else { return Some(2) };
    let Ok(png) = std::fs::read(picture) else { return Some(2) };
    if !paperloom_ocr::available() {
        return Some(3);
    }
    let lines = paperloom_ocr::recognize(&png).unwrap_or_default();
    let text: Vec<String> = lines.into_iter().map(|line| line.text).collect();
    let text = text.join("\n");
    Some(if std::fs::write(out, &text).is_ok() && !text.is_empty() { 0 } else { 1 })
}
