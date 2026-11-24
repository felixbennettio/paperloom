//! Daily notes and templates, set up the way Obsidian keeps them, so the two
//! apps share a vault's settings: `.obsidian/daily-notes.json` (folder, name
//! format, template) and `.obsidian/templates.json` (folder, date and time
//! formats). Paperloom's own (the citation style) are in
//! `.paperloom/settings.json`.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::template::{self, Moment};
use crate::{Error, Note, Result, Vault, io_error};

/// How a vault makes daily notes and fills templates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// Where daily notes go; empty for the vault's top.
    pub daily_folder: String,
    /// The Moment.js format that names a daily note.
    pub daily_format: String,
    /// The note a new daily note starts from; empty for none.
    pub daily_template: String,
    pub templates_folder: String,
    pub date_format: String,
    pub time_format: String,
    /// The style `[@citations]` show in (a hayagriva style name).
    pub citation_style: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            daily_folder: String::new(),
            daily_format: "YYYY-MM-DD".into(),
            daily_template: String::new(),
            templates_folder: "Templates".into(),
            date_format: "YYYY-MM-DD".into(),
            time_format: "HH:mm".into(),
            citation_style: "apa".into(),
        }
    }
}

fn read_json(path: &Path) -> Map<String, Value> {
    fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default()
}

fn text(map: &Map<String, Value>, key: &str) -> Option<String> {
    map.get(key).and_then(Value::as_str).map(|s| s.trim().trim_matches('/').to_string())
}

impl Vault {
    fn obsidian(&self, name: &str) -> PathBuf {
        self.root().join(".obsidian").join(name)
    }

    fn own_settings(&self) -> PathBuf {
        self.root().join(".paperloom").join("settings.json")
    }

    pub fn settings(&self) -> Settings {
        let defaults = Settings::default();
        let daily = read_json(&self.obsidian("daily-notes.json"));
        let templates = read_json(&self.obsidian("templates.json"));
        let own = read_json(&self.own_settings());
        let some = |value: Option<String>| value.filter(|v| !v.is_empty());
        Settings {
            daily_folder: text(&daily, "folder").unwrap_or_default(),
            daily_format: some(text(&daily, "format")).unwrap_or(defaults.daily_format),
            daily_template: text(&daily, "template").unwrap_or_default(),
            templates_folder: some(text(&templates, "folder")).unwrap_or(defaults.templates_folder),
            date_format: some(text(&templates, "dateFormat")).unwrap_or(defaults.date_format),
            time_format: some(text(&templates, "timeFormat")).unwrap_or(defaults.time_format),
            citation_style: some(text(&own, "citationStyle")).unwrap_or(defaults.citation_style),
        }
    }

    /// Keeps the settings where Obsidian reads them, leaving its other
    /// settings in those files as they were.
    pub fn set_settings(&self, settings: &Settings) -> Result<()> {
        let daily = [
            ("folder", &settings.daily_folder),
            ("format", &settings.daily_format),
            ("template", &settings.daily_template),
        ];
        let templates = [
            ("folder", &settings.templates_folder),
            ("dateFormat", &settings.date_format),
            ("timeFormat", &settings.time_format),
        ];
        let own = [("citationStyle", &settings.citation_style)];
        let files: [(PathBuf, &[(&str, &String)]); 3] = [
            (self.obsidian("daily-notes.json"), &daily),
            (self.obsidian("templates.json"), &templates),
            (self.own_settings(), &own),
        ];
        for (path, values) in files {
            let mut map = read_json(&path);
            for (key, value) in values {
                map.insert(key.to_string(), Value::String(value.trim().to_string()));
            }
            if let Some(folder) = path.parent() {
                fs::create_dir_all(folder).map_err(|e| io_error(folder, e))?;
            }
            let text = serde_json::to_string_pretty(&Value::Object(map)).unwrap_or_default();
            fs::write(&path, text).map_err(|e| io_error(&path, e))?;
        }
        Ok(())
    }

    /// The notes in the templates folder.
    pub fn templates(&self) -> Vec<&Note> {
        let folder = self.settings().templates_folder;
        let prefix = format!("{}/", folder.to_lowercase());
        self.notes().filter(|n| n.path.to_lowercase().starts_with(&prefix)).collect()
    }

    /// A template's text filled in for a note named `title`.
    pub fn fill_template(&self, template: &str, title: &str, at: &Moment) -> Result<String> {
        let path = self.resolve(template, "").ok_or_else(|| Error::NotFound(template.to_string()))?;
        let note = self.note(&path).ok_or_else(|| Error::NotFound(path.clone()))?;
        let settings = self.settings();
        // A template's own properties belong to the new note too.
        Ok(template::fill(&note.text, title, at, &settings.date_format, &settings.time_format))
    }

    /// Creates a note from a template (or empty), like `create_note`. The
    /// template is filled with the name the note gets, which may have gained
    /// a number.
    pub fn create_from_template(
        &mut self,
        folder: &str,
        title: &str,
        template: Option<&str>,
        at: &Moment,
    ) -> Result<String> {
        let template = match template.map(str::trim).filter(|t| !t.is_empty()) {
            Some(name) => Some(self.resolve(name, "").ok_or_else(|| Error::NotFound(name.to_string()))?),
            None => None,
        };
        let path = self.create_note(folder, title)?;
        if let Some(template) = template {
            let text = self.fill_template(&template, &crate::title_of(&path), at)?;
            self.write(&path, &text)?;
        }
        Ok(path)
    }

    /// The daily note for a day, made (from the daily template, if any)
    /// when it is not there yet. Returns its path and whether it is new.
    pub fn daily_note(&mut self, at: &Moment) -> Result<(String, bool)> {
        let settings = self.settings();
        let name = template::format(&settings.daily_format, at);
        let path = if settings.daily_folder.is_empty() {
            format!("{name}.md")
        } else {
            format!("{}/{name}.md", settings.daily_folder)
        };
        let path = crate::checked_note(&path)?;
        if self.note(&path).is_some() {
            return Ok((path, false));
        }
        let title = crate::title_of(&path);
        let text = if settings.daily_template.is_empty() {
            String::new()
        } else {
            self.fill_template(&settings.daily_template, &title, at)?
        };
        self.write(&path, &text)?;
        Ok((path, true))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AT: Moment = Moment { year: 2026, month: 10, day: 12, hour: 9, minute: 30, second: 0 };

    #[test]
    fn shares_settings_with_obsidian() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        assert_eq!(vault.settings(), Settings::default());
        fs::create_dir_all(dir.path().join(".obsidian")).unwrap();
        fs::write(dir.path().join(".obsidian/daily-notes.json"), r#"{"folder":"Journal/","autorun":true}"#)
            .unwrap();
        let mut settings = vault.settings();
        assert_eq!(settings.daily_folder, "Journal");
        settings.daily_format = "YYYY/MM/YYYY-MM-DD".into();
        vault.set_settings(&settings).unwrap();
        let written = fs::read_to_string(dir.path().join(".obsidian/daily-notes.json")).unwrap();
        assert!(written.contains("\"autorun\": true"), "other settings stay: {written}");
        assert_eq!(vault.settings().daily_format, "YYYY/MM/YYYY-MM-DD");
    }

    #[test]
    fn makes_daily_notes_and_notes_from_templates() {
        let dir = tempfile::tempdir().unwrap();
        let mut vault = Vault::open(dir.path()).unwrap();
        vault.write("Templates/Daily.md", "# {{date:dddd, MMMM D}}\n\n## Done\n").unwrap();
        vault
            .write("Templates/Paper.md", "---\nstatus: reading\n---\n# {{title}}\nAdded {{date}} {{time}}\n")
            .unwrap();
        let settings = Settings {
            daily_folder: "Daily".into(),
            daily_template: "Templates/Daily".into(),
            ..vault.settings()
        };
        vault.set_settings(&settings).unwrap();

        let (path, new) = vault.daily_note(&AT).unwrap();
        assert_eq!((path.as_str(), new), ("Daily/2026-10-12.md", true));
        assert_eq!(vault.note(&path).unwrap().text, "# Monday, October 12\n\n## Done\n");
        assert_eq!(vault.daily_note(&AT).unwrap(), (path, false), "made once");

        let titles: Vec<&str> = vault.templates().iter().map(|n| n.title.as_str()).collect();
        assert_eq!(titles, ["Daily", "Paper"]);
        let made = vault.create_from_template("Reading", "Attention", Some("Paper"), &AT).unwrap();
        assert_eq!(made, "Reading/Attention.md");
        assert_eq!(
            vault.note(&made).unwrap().text,
            "---\nstatus: reading\n---\n# Attention\nAdded 2026-10-12 09:30\n"
        );
        assert!(vault.create_from_template("", "X", Some("Missing"), &AT).is_err());
    }
}
