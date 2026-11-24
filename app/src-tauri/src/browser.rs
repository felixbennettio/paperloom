//! The in-app browser (desktop): a window of its own, where a person signs
//! in to their library's databases (CNKI, Wanfang, VIP, publishers) and
//! finds works. The app reads the page it shows by running the core's
//! capture script there itself, and offers the PDFs it downloads for adding.
//! Pages in it get no access to the app: the most one can do is ask, through
//! its "Save to Paperloom" button, for the app to read it.

use serde::Serialize;
use tauri::{AppHandle, Runtime};

use crate::CommandResult;

/// The browser window's label.
#[cfg(desktop)]
const LABEL: &str = "browser";
/// What the main window hears from the browser.
#[cfg(desktop)]
const EVENT: &str = "browser";
/// The page's button asks by going here; `.invalid` names no real host, and
/// the visit is stopped before it starts.
#[cfg(desktop)]
const ASK_HOST: &str = "paperloom.invalid";

/// The button each page gets, out of the page's way (and its styles').
#[cfg(desktop)]
const SAVE_BUTTON: &str = r#"(function () {
  if (window.top !== window) return;
  function add() {
    if (!document.documentElement || document.getElementById("paperloom-save")) return;
    var host = document.createElement("div");
    host.id = "paperloom-save";
    host.style.cssText = "position:fixed;right:18px;bottom:18px;z-index:2147483647;";
    var root = host.attachShadow({ mode: "closed" });
    var button = document.createElement("button");
    button.textContent = "＋ Paperloom";
    button.title = "Save to Paperloom";
    button.style.cssText = "font:600 13px -apple-system,system-ui,sans-serif;padding:8px 14px;border:0;" +
      "border-radius:999px;background:#2f6fde;color:#fff;box-shadow:0 2px 10px rgba(0,0,0,.25);cursor:pointer;";
    button.addEventListener("click", function (event) {
      event.preventDefault();
      event.stopPropagation();
      window.location.href = "https://paperloom.invalid/save";
    });
    root.appendChild(button);
    document.documentElement.appendChild(host);
  }
  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", add);
  else add();
})();"#;

/// What the main window hears.
#[derive(Clone, Serialize)]
#[cfg_attr(mobile, allow(dead_code))]
#[serde(rename_all = "camelCase", tag = "kind")]
pub(crate) enum BrowserEvent {
    /// The browser shows a page.
    Showing {
        url: String,
        title: String,
    },
    /// The browser is reading the page.
    Reading,
    Navigating {
        url: String,
    },
    Html {
        url: String,
        html: String,
    },
    Read {
        page: paperloom_core::SearchPage,
    },
    Failed {
        message: String,
    },
    /// A PDF came down, ready for adding like a picked one.
    Downloaded {
        path: String,
        name: String,
    },
    /// Another kind of file came down, into the Downloads folder.
    Saved {
        path: String,
    },
}

/// An address someone typed: `cnki.net` is `https://cnki.net/`.
fn address(text: &str) -> CommandResult<tauri::Url> {
    let text = text.trim();
    let full = if text.contains("://") { text.to_string() } else { format!("https://{text}") };
    let url = tauri::Url::parse(&full).map_err(|_| format!("“{text}” is not a web address."))?;
    match url.scheme() {
        "http" | "https" => Ok(url),
        _ => Err(format!("“{text}” is not a web address.")),
    }
}

/// Opens the browser at an address, or shows that address in it.
/// Async: making a window inside a synchronous command can hang Windows.
#[tauri::command]
pub(crate) async fn browser_open<R: Runtime>(app: AppHandle<R>, address: String) -> CommandResult<()> {
    let url = self::address(&address)?;
    #[cfg(desktop)]
    {
        native::open(&app, url)
    }
    #[cfg(mobile)]
    {
        let _ = (app, url);
        Err("The in-app browser comes to phones in a later version.".into())
    }
}

/// Reads the page the browser shows; the works come as a `read` event.
#[tauri::command]
pub(crate) fn browser_read<R: Runtime>(app: AppHandle<R>) -> CommandResult<()> {
    #[cfg(desktop)]
    {
        native::read(&app)
    }
    #[cfg(mobile)]
    {
        let _ = app;
        Err("The in-app browser comes to phones in a later version.".into())
    }
}

/// Refreshes translator detection without running metadata lookups.
#[tauri::command]
pub(crate) fn browser_snapshot<R: Runtime>(app: AppHandle<R>) -> CommandResult<()> {
    #[cfg(desktop)]
    {
        native::snapshot(&app);
        Ok(())
    }
    #[cfg(mobile)]
    {
        let _ = app;
        Err("The in-app browser comes to phones in a later version.".into())
    }
}

/// `back`, `forward` or `reload`.
#[tauri::command]
pub(crate) fn browser_go<R: Runtime>(app: AppHandle<R>, to: String) -> CommandResult<()> {
    let script = match to.as_str() {
        "back" => "history.back()",
        "forward" => "history.forward()",
        "reload" => "location.reload()",
        _ => return Err(format!("unknown direction {to}")),
    };
    #[cfg(desktop)]
    {
        use tauri::Manager;
        let window = app.get_webview_window(LABEL).ok_or("The browser is closed.")?;
        window.eval(script).map_err(|e| e.to_string())
    }
    #[cfg(mobile)]
    {
        let _ = (app, script);
        Err("The in-app browser comes to phones in a later version.".into())
    }
}

#[cfg(desktop)]
mod native {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};

    use tauri::webview::{DownloadEvent, NewWindowResponse, PageLoadEvent};
    use tauri::{AppHandle, Emitter, Manager, Runtime, Url, WebviewUrl, WebviewWindowBuilder};
    use tauri_plugin_dialog::FilePath;

    use super::{ASK_HOST, BrowserEvent, EVENT, LABEL, SAVE_BUTTON};
    use crate::pdf::Picked;
    use crate::{AppState, CommandResult};

    fn tell<R: Runtime>(app: &AppHandle<R>, event: BrowserEvent) {
        let _ = app.emit_to("main", EVENT, event);
    }

    pub(super) fn open<R: Runtime>(app: &AppHandle<R>, url: Url) -> CommandResult<()> {
        if let Some(window) = app.get_webview_window(LABEL) {
            window.navigate(url).map_err(|e| e.to_string())?;
            let _ = window.unminimize();
            let _ = window.set_focus();
            return Ok(());
        }
        let asks = app.clone();
        let opens = app.clone();
        let loads = app.clone();
        let titles = app.clone();
        let downloads = app.clone();
        // Where each download was sent: macOS does not say when it ends.
        let sent: Arc<Mutex<HashMap<String, PathBuf>>> = Arc::default();
        let builder = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::External(url));
        builder
            .title("Paperloom")
            .inner_size(1200.0, 860.0)
            .initialization_script(SAVE_BUTTON)
            .on_navigation(move |url| {
                if url.host_str() == Some(ASK_HOST) {
                    let _ = read(&asks);
                    return false;
                }
                matches!(url.scheme(), "http" | "https" | "about" | "blob" | "data")
            })
            .on_new_window(move |url, _| {
                // Links meant for a new tab open here instead.
                let app = opens.clone();
                tauri::async_runtime::spawn(async move {
                    if let Some(window) = app.get_webview_window(LABEL) {
                        let _ = window.navigate(url);
                    }
                });
                NewWindowResponse::Deny
            })
            .on_page_load(move |window, payload| {
                if matches!(payload.event(), PageLoadEvent::Started) {
                    tell(&loads, BrowserEvent::Navigating { url: payload.url().to_string() });
                }
                if matches!(payload.event(), PageLoadEvent::Finished) {
                    let title = window.title().unwrap_or_default();
                    tell(&loads, BrowserEvent::Showing { url: payload.url().to_string(), title });
                    snapshot(&loads);
                }
            })
            .on_document_title_changed(move |window, title| {
                let _ = window.set_title(&title);
                let url = window.url().map(|u| u.to_string()).unwrap_or_default();
                tell(&titles, BrowserEvent::Showing { url, title });
            })
            .on_download(move |_, event| {
                match event {
                    DownloadEvent::Requested { url, destination } => match place_for(&downloads, &url) {
                        Ok(path) => {
                            *destination = path.clone();
                            if let Ok(mut sent) = sent.lock() {
                                sent.insert(url.to_string(), path);
                            }
                        }
                        Err(message) => {
                            tell(&downloads, BrowserEvent::Failed { message });
                            return false;
                        }
                    },
                    DownloadEvent::Finished { url, path, success } => {
                        let known = sent.lock().ok().and_then(|mut sent| sent.remove(url.as_str()));
                        match path.or(known) {
                            Some(path) if success => arrived(&downloads, path),
                            _ => tell(
                                &downloads,
                                BrowserEvent::Failed {
                                    message: format!("Downloading {url} did not finish."),
                                },
                            ),
                        }
                    }
                    _ => {}
                }
                true
            })
            .build()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Runs the capture script in the page and reads what it answers, off
    /// the main thread (works named only by DOI are looked up).
    pub(super) fn read<R: Runtime>(app: &AppHandle<R>) -> CommandResult<()> {
        let window = app.get_webview_window(LABEL).ok_or("The browser is closed.")?;
        tell(app, BrowserEvent::Reading);
        snapshot(app);
        let reader = app.clone();
        window
            .eval_with_callback(paperloom_core::capture_script(), move |answer| {
                let app = reader.clone();
                tauri::async_runtime::spawn_blocking(move || {
                    // The script answers a string, handed over as JSON.
                    let json = serde_json::from_str::<String>(&answer).unwrap_or(answer);
                    let library = app.state::<AppState>().library.clone();
                    let event = match library.read_captured_page(json) {
                        Ok(page) => BrowserEvent::Read { page },
                        Err(e) => BrowserEvent::Failed { message: e.to_string() },
                    };
                    tell(&app, event);
                });
            })
            .map_err(|e| e.to_string())
    }

    /// Reads only the current page's HTML, capped before it crosses into the app.
    pub(super) fn snapshot<R: Runtime>(app: &AppHandle<R>) {
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let library = app.state::<AppState>().library.clone();
            if !library.translators_status().is_ok_and(|status| status.count > 0) {
                return;
            }
            let Some(window) = app.get_webview_window(LABEL) else { return };
            let reader = app.clone();
            let _ = window.eval_with_callback(
                r#"(() => {
                const limit = 5 * 1024 * 1024;
                const text = (document.documentElement?.outerHTML || "").slice(0, limit);
                const bytes = new TextEncoder().encode(text);
                const html = new TextDecoder().decode(bytes.subarray(0, limit), { stream: true });
                return JSON.stringify({ url: location.href, html });
            })()"#,
                move |answer| {
                    let json = serde_json::from_str::<String>(&answer).unwrap_or(answer);
                    let Ok(value) = serde_json::from_str::<serde_json::Value>(&json) else { return };
                    let (Some(url), Some(html)) = (value["url"].as_str(), value["html"].as_str()) else {
                        return;
                    };
                    if html.len() <= 5 * 1024 * 1024 {
                        tell(&reader, BrowserEvent::Html { url: url.into(), html: html.into() });
                    }
                },
            );
        });
    }

    /// A new file in the app's own downloads folder, named as the address
    /// names it.
    fn place_for<R: Runtime>(app: &AppHandle<R>, url: &Url) -> Result<PathBuf, String> {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis());
        let folder =
            app.path().app_cache_dir().map_err(|e| e.to_string())?.join("downloads").join(stamp.to_string());
        std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
        let name = url
            .path_segments()
            .and_then(|mut parts| parts.next_back())
            .map(|name| name.chars().filter(|c| !"/\\:*?\"<>|".contains(*c)).collect::<String>())
            .filter(|name| !name.is_empty() && name != "." && name != "..")
            .unwrap_or_else(|| "download".into());
        Ok(folder.join(name))
    }

    /// A finished download: a PDF is offered for adding; anything else goes
    /// to the Downloads folder.
    fn arrived<R: Runtime>(app: &AppHandle<R>, path: PathBuf) {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if is_pdf(&path) {
            let named = if name.to_ascii_lowercase().ends_with(".pdf") { path } else { with_pdf_name(path) };
            match app.state::<Picked>().remember(vec![FilePath::Path(named)]) {
                Ok(keys) => {
                    for key in keys {
                        tell(app, BrowserEvent::Downloaded { path: key, name: name.clone() });
                    }
                }
                Err(message) => tell(app, BrowserEvent::Failed { message }),
            }
            return;
        }
        let destination = app.path().download_dir();
        let moved = destination.map_err(|e| e.to_string()).and_then(|folder| {
            let target = folder.join(&name);
            std::fs::rename(&path, &target)
                .or_else(|_| std::fs::copy(&path, &target).map(|_| ()))
                .map(|()| target)
                .map_err(|e| e.to_string())
        });
        match moved {
            Ok(target) => tell(app, BrowserEvent::Saved { path: target.display().to_string() }),
            Err(message) => tell(app, BrowserEvent::Failed { message }),
        }
    }

    fn is_pdf(path: &Path) -> bool {
        use std::io::Read;
        let mut start = [0u8; 5];
        std::fs::File::open(path).and_then(|mut f| f.read_exact(&mut start)).is_ok() && &start == b"%PDF-"
    }

    /// A PDF whose name does not say so, renamed to end in `.pdf`.
    fn with_pdf_name(path: PathBuf) -> PathBuf {
        let renamed = path.with_extension("pdf");
        match std::fs::rename(&path, &renamed) {
            Ok(()) => renamed,
            Err(_) => path,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_addresses() {
        assert_eq!(address("cnki.net").unwrap().as_str(), "https://cnki.net/");
        assert_eq!(
            address(" https://www.wanfangdata.com.cn/ ").unwrap().host_str(),
            Some("www.wanfangdata.com.cn")
        );
        assert!(address("file:///etc/passwd").is_err());
        assert!(address("javascript:alert(1)").is_err());
    }
}
