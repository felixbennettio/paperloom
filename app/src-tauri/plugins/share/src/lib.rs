//! What other apps share to Paperloom through Android's share sheet: text
//! and links as they came, files copied into the app's cache. Each share is
//! kept until the app takes it with [`Share::take`]; `share://received`
//! says a new one arrived while the app was open. Elsewhere nothing is ever
//! shared, so `take` answers nothing.
//!
//! The Android side also keeps the page clear of the system bars (see
//! SystemBars.kt), which the WebView does not report as safe areas, and
//! reads text in pictures of scanned pages ([`Share::recognize_text`]).

use serde::{Deserialize, Serialize};
use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Manager, Runtime};

/// The event sent when something is shared while the app is open.
pub const RECEIVED: &str = "share://received";

/// What was shared since the app last asked.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Shared {
    pub texts: Vec<SharedText>,
    pub files: Vec<SharedFile>,
}

/// Text another app shared: often a link, sometimes with the page's title.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SharedText {
    pub text: String,
    pub subject: Option<String>,
}

/// A shared file, copied into the app's cache.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SharedFile {
    pub path: String,
    /// The name the sharing app gave it.
    pub name: String,
    pub mime: Option<String>,
}

/// A line read in a picture, and its box as shares of the picture's width
/// and height from its top-left corner.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ReadLine {
    pub text: String,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// The shares the app has not taken yet.
pub struct Share<R: Runtime> {
    #[cfg(target_os = "android")]
    handle: tauri::plugin::PluginHandle<R>,
    #[cfg(not(target_os = "android"))]
    _runtime: std::marker::PhantomData<fn() -> R>,
}

impl<R: Runtime> Share<R> {
    /// Takes what was shared so far; files are copied first, so call it off
    /// the main thread.
    pub fn take(&self) -> Result<Shared, String> {
        #[cfg(target_os = "android")]
        {
            self.handle.run_mobile_plugin::<Shared>("takeShared", ()).map_err(|e| e.to_string())
        }
        #[cfg(not(target_os = "android"))]
        {
            Ok(Shared::default())
        }
    }

    /// The lines in a picture of a page (a PNG, in base64), read on the
    /// phone with ML Kit; none elsewhere. Waits for the answer, so call it
    /// off the main thread.
    pub fn recognize_text(&self, png_base64: String) -> Result<Vec<ReadLine>, String> {
        #[cfg(target_os = "android")]
        {
            #[derive(Serialize)]
            struct Picture {
                png: String,
            }
            #[derive(Deserialize)]
            struct Read {
                #[serde(default)]
                lines: Vec<ReadLine>,
            }
            let read = self
                .handle
                .run_mobile_plugin::<Read>("recognizeText", Picture { png: png_base64 })
                .map_err(|e| e.to_string())?;
            Ok(read.lines)
        }
        #[cfg(not(target_os = "android"))]
        {
            let _ = png_base64;
            Ok(Vec::new())
        }
    }
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("paperloom-share")
        .setup(|app, api| {
            #[cfg(target_os = "android")]
            {
                use tauri::Emitter;
                use tauri::ipc::Channel;

                #[derive(Serialize)]
                struct EventHandler {
                    handler: Channel,
                }

                let handle = api.register_android_plugin("io.felixbennett.paperloom.share", "SharePlugin")?;
                let app_handle = app.clone();
                handle.run_mobile_plugin::<()>(
                    "setEventHandler",
                    EventHandler {
                        handler: Channel::new(move |_| {
                            let _ = app_handle.emit(RECEIVED, ());
                            Ok(())
                        }),
                    },
                )?;
                app.manage(Share { handle });
            }
            #[cfg(not(target_os = "android"))]
            {
                let _ = api;
                app.manage(Share::<R> { _runtime: std::marker::PhantomData });
            }
            Ok(())
        })
        .build()
}
