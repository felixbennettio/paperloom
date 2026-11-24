//! Writing plugins in Settings: the task panes the person let use the
//! library, the WPS add-in this app puts in WPS's add-in folder, and the
//! Word add-in's manifest it registers with Word on Windows
//! (`docs/writing-plugins.md`). The WPS add-in ships with the Linux and
//! Windows packages as the `wps-addin` resource (the WPS build,
//! `dist-wps`); the Word manifest with the Windows package as `word-addin`,
//! once the add-in has a hosting address (`scripts/word-manifest.py`).

use paperloom_core::ConnectedWriter;
use serde::Serialize;
use tauri::{AppHandle, Manager, Runtime, State};

use crate::{AppState, CommandResult};

#[tauri::command]
pub(crate) fn connected_writers(state: State<'_, AppState>) -> CommandResult<Vec<ConnectedWriter>> {
    state.library.connected_writers().map_err(|e| e.to_string())
}

#[tauri::command]
pub(crate) fn forget_connected_writers(state: State<'_, AppState>) -> CommandResult<()> {
    state.library.forget_connected_writers().map_err(|e| e.to_string())
}

/// The WPS add-in as Settings shows it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WpsAddin {
    /// This package carries the add-in (Linux and Windows).
    available: bool,
    /// The version WPS lists, if any.
    installed: Option<String>,
    /// This app's version, which installing puts there.
    version: String,
    folder: String,
}

struct Places {
    package: std::path::PathBuf,
    addons: std::path::PathBuf,
}

fn places<R: Runtime>(app: &AppHandle<R>) -> CommandResult<Places> {
    let paths = app.path();
    Ok(Places {
        package: paths.resource_dir().map_err(|e| e.to_string())?.join("wps-addin"),
        addons: paperloom_core::wps_addin_folder(&paths.data_dir().map_err(|e| e.to_string())?),
    })
}

fn state<R: Runtime>(app: &AppHandle<R>) -> CommandResult<WpsAddin> {
    let places = places(app)?;
    Ok(WpsAddin {
        available: cfg!(any(target_os = "linux", windows)) && places.package.join("ribbon.xml").is_file(),
        installed: paperloom_core::installed_wps_addin(&places.addons),
        version: app.package_info().version.to_string(),
        folder: places.addons.display().to_string(),
    })
}

#[tauri::command]
pub(crate) fn wps_addin<R: Runtime>(app: AppHandle<R>) -> CommandResult<WpsAddin> {
    state(&app)
}

/// Installs the add-in (or this version in place of an earlier one).
#[tauri::command]
pub(crate) fn install_wps_addin<R: Runtime>(app: AppHandle<R>) -> CommandResult<WpsAddin> {
    let places = places(&app)?;
    let version = app.package_info().version.to_string();
    paperloom_core::install_wps_addin(&places.package, &places.addons, &version)
        .map_err(|e| e.to_string())?;
    state(&app)
}

#[tauri::command]
pub(crate) fn remove_wps_addin<R: Runtime>(app: AppHandle<R>) -> CommandResult<WpsAddin> {
    paperloom_core::remove_wps_addin(&places(&app)?.addons).map_err(|e| e.to_string())?;
    state(&app)
}

/// The Word add-in as Settings shows it (Windows).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WordAddin {
    /// This package carries the manifest (Windows, once the add-in is
    /// hosted).
    available: bool,
    /// The version of the manifest Word is given, if any.
    installed: Option<String>,
    /// The version this package's manifest has.
    version: String,
}

/// The manifest this package carries, and the copy Word reads.
fn word_places<R: Runtime>(app: &AppHandle<R>) -> CommandResult<(std::path::PathBuf, std::path::PathBuf)> {
    let paths = app.path();
    Ok((
        paths.resource_dir().map_err(|e| e.to_string())?.join("word-addin").join("manifest.xml"),
        paths.app_data_dir().map_err(|e| e.to_string())?.join("word-addin").join("manifest.xml"),
    ))
}

/// The text of a manifest's first `<name>` element.
fn manifest_field(manifest: &str, name: &str) -> Option<String> {
    let start = manifest.find(&format!("<{name}>"))? + name.len() + 2;
    let end = start + manifest[start..].find(&format!("</{name}>"))?;
    Some(manifest[start..end].trim().to_string())
}

fn word_state<R: Runtime>(app: &AppHandle<R>) -> CommandResult<WordAddin> {
    let (package, copy) = word_places(app)?;
    let manifest = std::fs::read_to_string(&package).ok().filter(|_| cfg!(windows));
    // Installed: the copy is there and Word's list points at it.
    let installed = std::fs::read_to_string(&copy).ok().and_then(|text| {
        let listed = word_registry::get(&manifest_field(&text, "Id")?)?;
        if std::path::Path::new(&listed) == copy { manifest_field(&text, "Version") } else { None }
    });
    Ok(WordAddin {
        available: manifest.is_some(),
        installed,
        version: manifest.as_deref().and_then(|m| manifest_field(m, "Version")).unwrap_or_default(),
    })
}

#[tauri::command]
pub(crate) fn word_addin<R: Runtime>(app: AppHandle<R>) -> CommandResult<WordAddin> {
    word_state(&app)
}

/// Copies the manifest beside the app's data and lists it among Word's
/// developer add-ins (`HKCU\Software\Microsoft\Office\16.0\WEF\Developer`,
/// as Office's own sideloading tools do): Word adds the Paperloom group to
/// its Home tab when it next starts.
#[tauri::command]
pub(crate) fn install_word_addin<R: Runtime>(app: AppHandle<R>) -> CommandResult<WordAddin> {
    let (package, copy) = word_places(&app)?;
    let manifest = std::fs::read_to_string(&package)
        .map_err(|_| "The Word add-in is missing from this installation.".to_string())?;
    let id = manifest_field(&manifest, "Id").ok_or("The Word add-in's manifest has no id.")?;
    if let Some(folder) = copy.parent() {
        std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    }
    std::fs::write(&copy, &manifest).map_err(|e| e.to_string())?;
    word_registry::set(&id, &copy.display().to_string()).map_err(|e| e.to_string())?;
    word_state(&app)
}

#[tauri::command]
pub(crate) fn remove_word_addin<R: Runtime>(app: AppHandle<R>) -> CommandResult<WordAddin> {
    let (package, copy) = word_places(&app)?;
    for manifest in [&copy, &package] {
        if let Some(id) = std::fs::read_to_string(manifest).ok().and_then(|m| manifest_field(&m, "Id")) {
            word_registry::remove(&id).map_err(|e| e.to_string())?;
        }
    }
    if let Err(e) = std::fs::remove_file(&copy)
        && e.kind() != std::io::ErrorKind::NotFound
    {
        return Err(e.to_string());
    }
    word_state(&app)
}

/// Word's list of developer add-ins: value name the manifest's id, value
/// the manifest's path.
#[cfg(windows)]
mod word_registry {
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ, RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW,
    };

    const KEY: &str = r"Software\Microsoft\Office\16.0\WEF\Developer";

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }

    pub(super) fn get(name: &str) -> Option<String> {
        let (key, name) = (wide(KEY), wide(name));
        let mut buffer = vec![0u16; 1024];
        let mut size = u32::try_from(buffer.len() * 2).ok()?;
        // SAFETY: the key and name are NUL-terminated and outlive the call;
        // Windows writes at most `size` bytes into `buffer`.
        let status = unsafe {
            RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                RRF_RT_REG_SZ,
                std::ptr::null_mut(),
                buffer.as_mut_ptr().cast(),
                &mut size,
            )
        };
        if status != ERROR_SUCCESS {
            return None;
        }
        let text = &buffer[..(size as usize / 2).min(buffer.len())];
        let end = text.iter().position(|&c| c == 0).unwrap_or(text.len());
        Some(String::from_utf16_lossy(&text[..end]))
    }

    pub(super) fn set(name: &str, value: &str) -> std::io::Result<()> {
        let (key, name, value) = (wide(KEY), wide(name), wide(value));
        let size = u32::try_from(value.len() * 2).map_err(std::io::Error::other)?;
        // SAFETY: the strings are NUL-terminated and outlive the call, and
        // `size` is the length of `value` in bytes, its NUL included. The
        // key is made if missing.
        let status = unsafe {
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                name.as_ptr(),
                REG_SZ,
                value.as_ptr().cast(),
                size,
            )
        };
        if status == ERROR_SUCCESS { Ok(()) } else { Err(std::io::Error::from_raw_os_error(status as i32)) }
    }

    pub(super) fn remove(name: &str) -> std::io::Result<()> {
        let (key, name) = (wide(KEY), wide(name));
        // SAFETY: both strings are NUL-terminated and outlive the call.
        let status = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, key.as_ptr(), name.as_ptr()) };
        match status {
            ERROR_SUCCESS | ERROR_FILE_NOT_FOUND => Ok(()),
            _ => Err(std::io::Error::from_raw_os_error(status as i32)),
        }
    }
}

/// Elsewhere there is no Word for this app to register with.
#[cfg(not(windows))]
mod word_registry {
    pub(super) fn get(_name: &str) -> Option<String> {
        None
    }

    pub(super) fn set(_name: &str, _value: &str) -> std::io::Result<()> {
        Err(std::io::Error::other("Word add-ins are installed by this app only on Windows."))
    }

    pub(super) fn remove(_name: &str) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::manifest_field;

    #[test]
    fn reads_the_manifest_s_id_and_version() {
        let manifest = "<OfficeApp>\n  <Id>c6f575ad-1641</Id>\n  <Version>0.2.0.0</Version>\n</OfficeApp>";
        assert_eq!(manifest_field(manifest, "Id").as_deref(), Some("c6f575ad-1641"));
        assert_eq!(manifest_field(manifest, "Version").as_deref(), Some("0.2.0.0"));
        assert_eq!(manifest_field(manifest, "Name"), None);
    }
}
