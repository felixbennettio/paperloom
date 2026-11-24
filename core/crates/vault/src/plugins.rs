//! Obsidian plugins installed in the vault, kept where Obsidian keeps them:
//! each in `.obsidian/plugins/<id>/` (`manifest.json`, `main.js`, perhaps
//! `styles.css` and the plugin's own `data.json`), and the ones turned on
//! listed in `.obsidian/community-plugins.json`. Obsidian and Paperloom
//! open the same vault, so either can install, turn on or set up a plugin.

use std::fs;
use std::path::PathBuf;

use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::parse::Link;
use crate::{Error, Result, Vault, io_error};

/// What a plugin's `manifest.json` says about it.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub min_app_version: String,
    pub description: String,
    pub author: String,
    pub author_url: String,
    /// It needs Node or Electron, so not on phones.
    pub is_desktop_only: bool,
}

/// A plugin in the vault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPlugin {
    pub manifest: PluginManifest,
    /// Listed in `community-plugins.json`.
    pub enabled: bool,
    /// It has a `styles.css`.
    pub has_styles: bool,
}

/// What it takes to run a plugin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginCode {
    pub main: String,
    pub styles: Option<String>,
    /// Its settings, as it saved them (`data.json`).
    pub data: Option<String>,
}

/// Where something is in a note, as Obsidian's caches give it (by line).
fn position(line: usize) -> Value {
    let at = json!({ "line": line, "col": 0, "offset": 0 });
    json!({ "start": at, "end": at })
}

fn link_cache(link: &Link) -> Value {
    let target = match &link.subpath {
        Some(subpath) => format!("{}#{subpath}", link.target),
        None => link.target.clone(),
    };
    let alias = link.alias.as_ref().map(|a| format!("|{a}")).unwrap_or_default();
    json!({
        "link": target,
        "original": format!("{}[[{target}{alias}]]", if link.embed { "!" } else { "" }),
        "displayText": link.alias.clone().unwrap_or_else(|| target.clone()),
        "position": position(link.line),
    })
}

/// A plugin id is a folder name: letters, digits, `-` and `_`.
fn checked_id(id: &str) -> Result<&str> {
    let plain = !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if plain { Ok(id) } else { Err(Error::InvalidPath(id.to_string())) }
}

impl Vault {
    fn obsidian_folder(&self) -> PathBuf {
        self.full(".obsidian")
    }

    fn plugin_folder(&self, id: &str) -> Result<PathBuf> {
        Ok(self.obsidian_folder().join("plugins").join(checked_id(id)?))
    }

    /// The ids in `community-plugins.json`, in its order.
    fn enabled_plugins(&self) -> Vec<String> {
        fs::read_to_string(self.obsidian_folder().join("community-plugins.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<Vec<String>>(&text).ok())
            .unwrap_or_default()
    }

    /// The plugins in the vault, by name; a folder without a readable
    /// manifest is left out.
    pub fn obsidian_plugins(&self) -> Vec<InstalledPlugin> {
        let enabled = self.enabled_plugins();
        let Ok(entries) = fs::read_dir(self.obsidian_folder().join("plugins")) else { return Vec::new() };
        let mut found: Vec<InstalledPlugin> = entries
            .flatten()
            .filter(|entry| entry.path().join("main.js").is_file())
            .filter_map(|entry| {
                let text = fs::read_to_string(entry.path().join("manifest.json")).ok()?;
                let mut manifest: PluginManifest = serde_json::from_str(&text).ok()?;
                let folder = entry.file_name().to_string_lossy().into_owned();
                if manifest.id.is_empty() {
                    manifest.id = folder;
                }
                checked_id(&manifest.id).ok()?;
                if manifest.name.is_empty() {
                    manifest.name = manifest.id.clone();
                }
                Some(InstalledPlugin {
                    enabled: enabled.contains(&manifest.id),
                    has_styles: entry.path().join("styles.css").is_file(),
                    manifest,
                })
            })
            .collect();
        found.sort_by_key(|p| p.manifest.name.to_lowercase());
        found
    }

    /// Turns a plugin on or off, as Obsidian does: by listing it in
    /// `community-plugins.json` (other ids there stay as they are).
    pub fn set_obsidian_plugin_enabled(&mut self, id: &str, enabled: bool) -> Result<()> {
        let id = checked_id(id)?;
        let mut list = self.enabled_plugins();
        list.retain(|other| other != id);
        if enabled {
            list.push(id.to_string());
        }
        let folder = self.obsidian_folder();
        fs::create_dir_all(&folder).map_err(|e| io_error(&folder, e))?;
        let path = folder.join("community-plugins.json");
        let text = serde_json::to_string_pretty(&list).unwrap_or_else(|_| "[]".into());
        fs::write(&path, text).map_err(|e| io_error(&path, e))
    }

    /// A plugin's code, styles and saved settings.
    pub fn obsidian_plugin_code(&self, id: &str) -> Result<PluginCode> {
        let folder = self.plugin_folder(id)?;
        let main_path = folder.join("main.js");
        let main = fs::read_to_string(&main_path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => Error::NotFound(id.to_string()),
            _ => io_error(&main_path, e),
        })?;
        Ok(PluginCode {
            main,
            styles: fs::read_to_string(folder.join("styles.css")).ok(),
            data: fs::read_to_string(folder.join("data.json")).ok(),
        })
    }

    /// Puts a plugin's release files (`manifest.json`, `main.js` and perhaps
    /// `styles.css`) into `.obsidian/plugins/<id>/`, as Obsidian installs
    /// one; it stays off until turned on. Its saved settings stay.
    pub fn install_obsidian_plugin(
        &mut self,
        manifest: &str,
        main: &str,
        styles: Option<&str>,
    ) -> Result<PluginManifest> {
        let parsed: PluginManifest = serde_json::from_str(manifest)
            .map_err(|e| Error::Invalid(format!("The plugin's manifest: {e}")))?;
        let folder = self.plugin_folder(&parsed.id)?;
        fs::create_dir_all(&folder).map_err(|e| io_error(&folder, e))?;
        for (name, text) in
            [("manifest.json", Some(manifest)), ("main.js", Some(main)), ("styles.css", styles)]
        {
            let path = folder.join(name);
            match text {
                Some(text) => fs::write(&path, text).map_err(|e| io_error(&path, e))?,
                None => {
                    let _ = fs::remove_file(&path);
                }
            }
        }
        Ok(parsed)
    }

    /// Every note's metadata, by path, as Obsidian's `metadataCache` gives
    /// it to plugins (`CachedMetadata`): front matter, tags, links, embeds
    /// and headings; and under `resolved`, the notes each links to and how
    /// often.
    pub fn obsidian_metadata(&self) -> Value {
        let mut all = Map::new();
        for note in self.notes() {
            let parsed = &note.parsed;
            let frontmatter: Map<String, Value> = parsed
                .properties
                .iter()
                .map(|(key, value)| {
                    let list = matches!(key.to_lowercase().as_str(), "tags" | "aliases" | "cssclasses");
                    let value = if list {
                        json!(value.split(',').map(str::trim).filter(|v| !v.is_empty()).collect::<Vec<_>>())
                    } else {
                        json!(value)
                    };
                    (key.clone(), value)
                })
                .collect();
            let mut resolved = Map::new();
            for link in &parsed.links {
                if let Some(target) = self.resolve(&link.target, &note.path) {
                    let count = resolved.entry(target).or_insert(json!(0));
                    *count = json!(count.as_u64().unwrap_or(0) + 1);
                }
            }
            let mut cache = json!({
                "tags": parsed.tags.iter().map(|t| json!({ "tag": format!("#{t}"), "position": position(0) })).collect::<Vec<_>>(),
                "links": parsed.links.iter().filter(|l| !l.embed).map(link_cache).collect::<Vec<_>>(),
                "embeds": parsed.links.iter().filter(|l| l.embed).map(link_cache).collect::<Vec<_>>(),
                "headings": parsed
                    .headings
                    .iter()
                    .map(|h| json!({ "heading": h.text, "level": h.level, "position": position(h.line) }))
                    .collect::<Vec<_>>(),
                "resolved": resolved,
            });
            if !frontmatter.is_empty() {
                cache["frontmatter"] = Value::Object(frontmatter);
            }
            all.insert(note.path.clone(), cache);
        }
        Value::Object(all)
    }

    /// Keeps a plugin's settings (`saveData`), which must be JSON.
    pub fn save_obsidian_plugin_data(&self, id: &str, data: &str) -> Result<()> {
        if serde_json::from_str::<Value>(data).is_err() {
            return Err(Error::Invalid(format!("{id}'s settings are not JSON")));
        }
        let folder = self.plugin_folder(id)?;
        if !folder.is_dir() {
            return Err(Error::NotFound(id.to_string()));
        }
        let path = folder.join("data.json");
        fs::write(&path, data).map_err(|e| io_error(&path, e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plugin(root: &std::path::Path, folder: &str, manifest: &str, styles: bool) {
        let dir = root.join(".obsidian/plugins").join(folder);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("manifest.json"), manifest).unwrap();
        fs::write(dir.join("main.js"), "module.exports = class {};").unwrap();
        if styles {
            fs::write(dir.join("styles.css"), ".x {}").unwrap();
        }
    }

    #[test]
    fn lists_turns_on_and_runs_obsidian_plugins() {
        let dir = tempfile::tempdir().unwrap();
        plugin(
            dir.path(),
            "dataview",
            r#"{"id": "dataview", "name": "Dataview", "version": "0.5.67", "minAppVersion": "0.13.11",
               "description": "Queries.", "author": "Michael Brenan", "isDesktopOnly": false, "fundingUrl": "x"}"#,
            true,
        );
        plugin(
            dir.path(),
            "calendar",
            r#"{"name": "Calendar", "version": "1.5.10", "isDesktopOnly": true}"#,
            false,
        );
        plugin(dir.path(), "broken", "not json", false);
        fs::create_dir_all(dir.path().join(".obsidian/plugins/empty")).unwrap();
        fs::write(dir.path().join(".obsidian/community-plugins.json"), r#"["calendar", "gone"]"#).unwrap();
        let mut vault = Vault::open(dir.path()).unwrap();
        let plugins = vault.obsidian_plugins();
        let names: Vec<&str> = plugins.iter().map(|p| p.manifest.name.as_str()).collect();
        assert_eq!(names, ["Calendar", "Dataview"]);
        let (calendar, dataview) = (&plugins[0], &plugins[1]);
        assert_eq!(calendar.manifest.id, "calendar", "the folder names it");
        assert!(calendar.enabled && calendar.manifest.is_desktop_only && !calendar.has_styles);
        assert!(!dataview.enabled && dataview.has_styles);
        assert_eq!(dataview.manifest.min_app_version, "0.13.11");

        vault.set_obsidian_plugin_enabled("dataview", true).unwrap();
        vault.set_obsidian_plugin_enabled("calendar", false).unwrap();
        let listed: Vec<String> = serde_json::from_str(
            &fs::read_to_string(dir.path().join(".obsidian/community-plugins.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(listed, ["gone", "dataview"]);

        let code = vault.obsidian_plugin_code("dataview").unwrap();
        assert_eq!(
            (code.main.as_str(), code.styles.as_deref(), code.data),
            ("module.exports = class {};", Some(".x {}"), None)
        );
        vault.save_obsidian_plugin_data("dataview", r#"{"refresh": 2500}"#).unwrap();
        assert_eq!(
            vault.obsidian_plugin_code("dataview").unwrap().data.as_deref(),
            Some(r#"{"refresh": 2500}"#)
        );
        assert!(vault.save_obsidian_plugin_data("dataview", "{oops").is_err());
        assert!(matches!(vault.obsidian_plugin_code("../notes"), Err(Error::InvalidPath(_))));
        assert!(matches!(vault.obsidian_plugin_code("missing"), Err(Error::NotFound(_))));
        // The vault's notes leave `.obsidian` alone.
        assert_eq!(vault.notes().count(), 0);

        let installed = vault
            .install_obsidian_plugin(
                r#"{"id": "dataview", "name": "Dataview", "version": "0.5.68"}"#,
                "new()",
                None,
            )
            .unwrap();
        assert_eq!(installed.version, "0.5.68");
        let code = vault.obsidian_plugin_code("dataview").unwrap();
        assert_eq!((code.main.as_str(), code.styles, code.data.is_some()), ("new()", None, true));
        assert!(vault.install_obsidian_plugin(r#"{"id": "../x"}"#, "", None).is_err());
        assert!(vault.install_obsidian_plugin("nope", "", None).is_err());

        vault
            .write(
                "A.md",
                "---\ntags: [ml, nlp]\nstatus: draft\n---\n# Title\nSee [[B|bee]] and ![[B]] #idea",
            )
            .unwrap();
        vault.write("B.md", "text").unwrap();
        let metadata = vault.obsidian_metadata();
        let a = &metadata["A.md"];
        assert_eq!(a["frontmatter"]["tags"], json!(["ml", "nlp"]));
        assert_eq!(a["frontmatter"]["status"], "draft");
        assert_eq!(a["headings"][0]["heading"], "Title");
        assert_eq!(a["links"][0]["displayText"], "bee");
        assert_eq!(a["links"][0]["original"], "[[B|bee]]");
        assert_eq!(a["embeds"][0]["link"], "B");
        assert!(a["tags"].as_array().unwrap().iter().any(|t| t["tag"] == "#idea"));
        assert_eq!(a["resolved"]["B.md"], 2);
        assert!(metadata["B.md"].get("frontmatter").is_none());
    }
}
