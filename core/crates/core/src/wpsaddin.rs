//! The WPS add-in in WPS's add-in folder (`docs/writing-plugins.md`). WPS
//! loads the add-ins listed in `publish.xml` there, each from the folder
//! `<name>_<version>` beside the list, and asks the person the first time
//! whether to allow and trust it. It rewrites the list itself (attribute
//! order, `enable`), so other add-ins' entries are kept as they are.

use std::fs;
use std::path::{Path, PathBuf};

use crate::{CONNECTOR_PORT, CoreError};

const NAME: &str = "paperloom";
const LIST: &str = "publish.xml";

/// WPS's add-in folder in the user's data folder (`~/.local/share` on Linux,
/// `%APPDATA%` on Windows).
pub fn wps_addin_folder(data: &Path) -> PathBuf {
    data.join(if cfg!(windows) { "kingsoft" } else { "Kingsoft" }).join("wps").join("jsaddons")
}

/// The version of this app's add-in that WPS lists, when its folder is there.
pub fn installed_wps_addin(addons: &Path) -> Option<String> {
    let list = fs::read_to_string(addons.join(LIST)).ok()?;
    let version = entries(&list)
        .into_iter()
        .find(|entry| attribute(entry, "name").as_deref() == Some(NAME))
        .and_then(|entry| attribute(entry, "version"))?;
    addons.join(format!("{NAME}_{version}")).is_dir().then_some(version)
}

/// Copies the add-in (`package`, the folder the WPS build makes) into WPS's
/// add-in folder as `version`, in place of an earlier one, and lists it.
pub fn install_wps_addin(package: &Path, addons: &Path, version: &str) -> Result<(), CoreError> {
    if !package.join("ribbon.xml").is_file() {
        return Err(CoreError::NotFound {
            message: "The WPS add-in is missing from this installation.".into(),
        });
    }
    remove_folders(addons)?;
    copy(package, &addons.join(format!("{NAME}_{version}"))).map_err(storage)?;
    // WPS downloads from `url` only when the add-in's folder is missing.
    let entry = format!(
        r#"<jsplugin name="{NAME}" type="wps" url="http://127.0.0.1:{CONNECTOR_PORT}/wps/{NAME}.7z" version="{}" enable="enable" install="null"/>"#,
        escape(version)
    );
    let list = fs::read_to_string(addons.join(LIST)).unwrap_or_default();
    fs::write(addons.join(LIST), listed(&list, Some(&entry))).map_err(storage)
}

/// Takes the add-in out of WPS's list and folder; other add-ins stay.
pub fn remove_wps_addin(addons: &Path) -> Result<(), CoreError> {
    remove_folders(addons)?;
    match fs::read_to_string(addons.join(LIST)) {
        Ok(list) => fs::write(addons.join(LIST), listed(&list, None)).map_err(storage),
        Err(_) => Ok(()),
    }
}

fn storage(err: std::io::Error) -> CoreError {
    CoreError::Storage { message: err.to_string() }
}

fn remove_folders(addons: &Path) -> Result<(), CoreError> {
    let Ok(folders) = fs::read_dir(addons) else { return Ok(()) };
    for folder in folders.flatten() {
        let path = folder.path();
        if path.is_dir() && folder.file_name().to_string_lossy().starts_with(&format!("{NAME}_")) {
            fs::remove_dir_all(&path).map_err(storage)?;
        }
    }
    Ok(())
}

fn copy(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// The list again: every other entry as it was, then `entry`.
fn listed(list: &str, entry: Option<&str>) -> String {
    let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<jsplugins>\n");
    for other in entries(list).into_iter().filter(|e| attribute(e, "name").as_deref() != Some(NAME)) {
        out.push_str("  ");
        out.push_str(other.trim());
        out.push('\n');
    }
    if let Some(entry) = entry {
        out.push_str("  ");
        out.push_str(entry);
        out.push('\n');
    }
    out.push_str("</jsplugins>\n");
    out
}

/// The list's `<jsplugin …/>` and `<jspluginonline …/>` elements.
fn entries(list: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut rest = list;
    let mut offset = 0;
    while let Some(at) = rest.find("<jsplugin") {
        let start = offset + at;
        let after = &list[start + "<jsplugin".len()..];
        let Some(open_end) = after.find('>') else { break };
        let tag_end = start + "<jsplugin".len() + open_end + 1;
        let end = if after.starts_with('s') {
            // The `<jsplugins>` list itself.
            None
        } else if after[..open_end].ends_with('/') {
            Some(tag_end)
        } else {
            let name_len = after.find(|c: char| c.is_whitespace() || c == '>' || c == '/').unwrap_or(0);
            let close = format!("</jsplugin{}>", &after[..name_len]);
            list[tag_end..].find(&close).map(|at| tag_end + at + close.len())
        };
        if let Some(end) = end {
            found.push(&list[start..end]);
        }
        offset = end.unwrap_or(tag_end);
        rest = &list[offset..];
    }
    found
}

fn attribute(element: &str, name: &str) -> Option<String> {
    let mut rest = element;
    loop {
        let at = rest.find(&format!("{name}="))?;
        let before = rest[..at].chars().last();
        let value = &rest[at + name.len() + 1..];
        let quote = value.chars().next()?;
        if before.is_some_and(char::is_whitespace) && (quote == '"' || quote == '\'') {
            let value = &value[1..];
            return Some(unescape(&value[..value.find(quote)?]));
        }
        rest = &rest[at + name.len() + 1..];
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;").replace('>', "&gt;")
}

fn unescape(text: &str) -> String {
    text.replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("ribbon.xml"), "<customUI/>").unwrap();
        fs::create_dir(dir.path().join("icons")).unwrap();
        fs::write(dir.path().join("icons/icon-32.png"), b"png").unwrap();
        dir
    }

    #[test]
    fn installs_lists_and_replaces_the_add_in() {
        let package = package();
        let addons = tempfile::tempdir().unwrap();
        assert_eq!(installed_wps_addin(addons.path()), None);
        install_wps_addin(package.path(), addons.path(), "0.1.0").unwrap();
        assert_eq!(installed_wps_addin(addons.path()).as_deref(), Some("0.1.0"));
        assert!(addons.path().join("paperloom_0.1.0/icons/icon-32.png").is_file());

        install_wps_addin(package.path(), addons.path(), "0.2.0").unwrap();
        assert_eq!(installed_wps_addin(addons.path()).as_deref(), Some("0.2.0"));
        assert!(!addons.path().join("paperloom_0.1.0").exists());
        let list = fs::read_to_string(addons.path().join(LIST)).unwrap();
        assert_eq!(list.matches("name=\"paperloom\"").count(), 1, "{list}");
    }

    #[test]
    fn keeps_other_add_ins_as_wps_wrote_them() {
        let package = package();
        let addons = tempfile::tempdir().unwrap();
        // As WPS rewrote the list after the person allowed an add-in.
        let other = r#"<jsplugin type="wps" enable="enable_dev" version="1.0.0" name="other" url="http://example.com/other.7z" install="null"/>"#;
        let online =
            r#"<jspluginonline name="online" type="wps" url="http://127.0.0.1:3889/"></jspluginonline>"#;
        fs::write(
            addons.path().join(LIST),
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<jsplugins>\n    {other}\n    {online}\n    <jsplugin type=\"wps\" version=\"0.0.9\" name=\"paperloom\" url=\"x\"/>\n</jsplugins>\n"
            ),
        )
        .unwrap();
        fs::create_dir(addons.path().join("other_1.0.0")).unwrap();
        fs::create_dir(addons.path().join("paperloom_0.0.9")).unwrap();
        assert_eq!(installed_wps_addin(addons.path()).as_deref(), Some("0.0.9"));

        install_wps_addin(package.path(), addons.path(), "0.1.0").unwrap();
        let list = fs::read_to_string(addons.path().join(LIST)).unwrap();
        assert!(list.contains(other) && list.contains(online), "{list}");
        assert!(!list.contains("0.0.9") && !addons.path().join("paperloom_0.0.9").exists(), "{list}");

        remove_wps_addin(addons.path()).unwrap();
        let list = fs::read_to_string(addons.path().join(LIST)).unwrap();
        assert!(list.contains(other) && list.contains(online) && !list.contains("paperloom"), "{list}");
        assert!(addons.path().join("other_1.0.0").is_dir());
        assert_eq!(installed_wps_addin(addons.path()), None);
    }

    #[test]
    fn needs_the_package_and_a_listed_folder() {
        let addons = tempfile::tempdir().unwrap();
        let empty = tempfile::tempdir().unwrap();
        assert!(install_wps_addin(empty.path(), addons.path(), "0.1.0").is_err());
        assert!(remove_wps_addin(addons.path()).is_ok());
        let package = package();
        install_wps_addin(package.path(), addons.path(), "0.1.0").unwrap();
        fs::remove_dir_all(addons.path().join("paperloom_0.1.0")).unwrap();
        assert_eq!(installed_wps_addin(addons.path()), None);
    }

    #[test]
    fn folder_is_where_wps_looks() {
        let folder = wps_addin_folder(Path::new("/data"));
        assert!(folder.ends_with(Path::new("wps").join("jsaddons")), "{folder:?}");
    }
}
