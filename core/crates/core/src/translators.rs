//! Optional Zotero translators, downloaded into the library rather than shipped
//! with the app. Their code runs only in the separate translation frame.

use std::fs;
use std::path::{Path, PathBuf};

use paperloom_http::{Request, UreqHttp};
use paperloom_search::Hit;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{CoreError, PaperLibrary, PluginHeader, SearchPage, search, unzip, zotero};

const ARCHIVE: &str = "https://github.com/zotero/translators/archive/refs/heads/master.zip";
const MAX_CODE: u64 = 4 << 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct TranslatorStatus {
    pub count: u32,
    pub downloaded: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct WebTranslator {
    pub id: String,
    pub label: String,
    pub target: String,
    pub priority: u32,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Header {
    #[serde(rename = "translatorID")]
    id: String,
    label: String,
    #[serde(default)]
    target: Option<String>,
    priority: u32,
    translator_type: u32,
}

fn invalid(message: impl Into<String>) -> CoreError {
    CoreError::InvalidInput { message: message.into() }
}

fn storage(error: impl std::fmt::Display) -> CoreError {
    CoreError::Storage { message: format!("Translators: {error}") }
}

fn header(code: &str) -> Result<Header, CoreError> {
    let text = code.trim_start_matches('\u{feff}').trim_start();
    if !text.starts_with('{') {
        return Err(invalid("The translator has no JSON header."));
    }
    let mut parsed = serde_json::Deserializer::from_str(text)
        .into_iter::<Header>()
        .next()
        .ok_or_else(|| invalid("The translator has no JSON header."))?
        .map_err(|e| invalid(format!("The translator header: {e}")))?;
    // Zotero takes ids as written; one in the repository has a stray space
    // ("…-46bd- bad4-…"), which no other translator refers to.
    parsed.id.retain(|c| !c.is_whitespace());
    crate::zotero_plugins::checked_id(&parsed.id)?;
    Ok(parsed)
}

fn files(dir: &Path) -> Result<Vec<(PathBuf, Header)>, CoreError> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    if fs::symlink_metadata(dir).map_err(storage)?.file_type().is_symlink() {
        return Err(invalid("The translators folder must not be a symbolic link."));
    }
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).map_err(storage)? {
        let entry = entry.map_err(storage)?;
        if entry.file_type().map_err(storage)?.is_file()
            && entry.path().extension().is_some_and(|ext| ext == "js")
            && entry.metadata().map_err(storage)?.len() <= MAX_CODE
        {
            let code = fs::read_to_string(entry.path()).map_err(storage)?;
            if let Ok(parsed) = header(&code) {
                found.push((entry.path(), parsed));
            }
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(found)
}

impl PaperLibrary {
    fn translators_dir(&self) -> Result<PathBuf, CoreError> {
        self.plugins_dir
            .as_ref()
            .and_then(|p| p.parent())
            .map(|p| p.join("translators"))
            .ok_or_else(|| storage("This library is kept only in memory."))
    }

    fn install_translators(&self, bytes: &[u8]) -> Result<u32, CoreError> {
        let dir = self.translators_dir()?;
        let parent = dir.parent().ok_or_else(|| storage("No library folder."))?;
        let stage = tempfile::tempdir_in(parent).map_err(storage)?;
        let entries = unzip::entries(bytes).ok_or_else(|| invalid("The download is not a ZIP archive."))?;
        let mut ids = std::collections::HashSet::new();
        let mut size = 0;
        // One translator that can't be read must not keep the others out.
        for entry in entries {
            let Some(name) = entry.name.strip_prefix("translators-master/") else { continue };
            if name.contains(['/', '\\']) || !name.ends_with(".js") || name.starts_with('.') {
                continue;
            }
            let Some(code) = entry.contents(MAX_CODE) else { continue };
            size += code.len();
            if size > 128 << 20 {
                return Err(invalid("The translators are too large."));
            }
            let Ok(code) = String::from_utf8(code) else { continue };
            let Ok(parsed) = header(&code) else { continue };
            if !ids.insert(parsed.id) {
                continue;
            }
            fs::write(stage.path().join(name), code).map_err(storage)?;
        }
        if ids.is_empty() {
            return Err(invalid("The download contains no translators."));
        }
        fs::write(stage.path().join("downloaded.txt"), paperloom_capture::today()).map_err(storage)?;
        // A failed update leaves the previous installation available.
        let backup = tempfile::tempdir_in(parent).map_err(storage)?;
        let old = backup.path().join("previous");
        if dir.exists() {
            files(&dir)?;
            fs::rename(&dir, &old).map_err(storage)?;
        }
        if let Err(e) = fs::rename(stage.path(), &dir) {
            if old.exists() {
                let _ = fs::rename(&old, &dir);
            }
            return Err(storage(e));
        }
        Ok(ids.len() as u32)
    }
}

#[uniffi::export]
impl PaperLibrary {
    pub fn download_translators(&self) -> Result<u32, CoreError> {
        let _guard = self.translators_lock.lock().map_err(storage)?;
        let (response, _) = UreqHttp::new()
            .send_with_limit(
                Request {
                    method: "GET",
                    url: ARCHIVE.into(),
                    headers: vec![("Accept".into(), "application/zip".into())],
                    body: None,
                },
                64 << 20,
            )
            .map_err(|e| CoreError::Network { message: e.0 })?;
        if !response.is_success() {
            return Err(CoreError::Network {
                message: format!("Downloading translators: HTTP {}", response.status),
            });
        }
        self.install_translators(&response.body)
    }

    pub fn translators_status(&self) -> Result<TranslatorStatus, CoreError> {
        let _guard = self.translators_lock.lock().map_err(storage)?;
        let dir = self.translators_dir()?;
        let count = files(&dir)?.len() as u32;
        let downloaded = (count > 0).then(|| fs::read_to_string(dir.join("downloaded.txt")).ok()).flatten();
        Ok(TranslatorStatus { count, downloaded })
    }

    pub fn remove_translators(&self) -> Result<(), CoreError> {
        let _guard = self.translators_lock.lock().map_err(storage)?;
        let dir = self.translators_dir()?;
        if dir.exists() {
            files(&dir)?;
            fs::remove_dir_all(dir).map_err(storage)?;
        }
        Ok(())
    }

    pub fn translator_catalog(&self) -> Result<Vec<WebTranslator>, CoreError> {
        let _guard = self.translators_lock.lock().map_err(storage)?;
        Ok(files(&self.translators_dir()?)?
            .into_iter()
            .map(|(_, h)| WebTranslator {
                id: h.id,
                label: h.label,
                target: h.target.unwrap_or_default(),
                priority: h.priority,
            })
            .collect())
    }

    pub fn web_translators(&self) -> Result<Vec<WebTranslator>, CoreError> {
        let _guard = self.translators_lock.lock().map_err(storage)?;
        let mut list: Vec<_> = files(&self.translators_dir()?)?
            .into_iter()
            .filter(|(_, h)| h.translator_type & 4 != 0)
            .map(|(_, h)| WebTranslator {
                id: h.id,
                label: h.label,
                target: h.target.unwrap_or_default(),
                priority: h.priority,
            })
            .collect();
        list.sort_by(|a, b| a.priority.cmp(&b.priority).then(a.id.cmp(&b.id)));
        Ok(list)
    }

    pub fn translator_code(&self, id: String) -> Result<String, CoreError> {
        crate::zotero_plugins::checked_id(&id)?;
        let _guard = self.translators_lock.lock().map_err(storage)?;
        let file = files(&self.translators_dir()?)?
            .into_iter()
            .find(|(_, h)| h.id == id)
            .ok_or_else(|| CoreError::not_found(format!("Translator {id}")))?
            .0;
        fs::read_to_string(file).map_err(storage)
    }

    pub fn read_translated_items(&self, json: String, page_url: String) -> Result<SearchPage, CoreError> {
        let values: Vec<Value> =
            serde_json::from_str(&json).map_err(|e| invalid(format!("Translated items: {e}")))?;
        let mut hits = Vec::new();
        let mut tags = Vec::new();
        for value in values {
            let Some((item, labels)) = converted(&value, &page_url) else { continue };
            let key = format!("translator:{}", item.id);
            let pdf_url = value["attachments"].as_array().and_then(|attachments| {
                attachments.iter().find_map(|a| {
                    let url = a["url"].as_str()?.trim();
                    let pdf = a["mimeType"]
                        .as_str()
                        .is_some_and(|mime| mime.eq_ignore_ascii_case("application/pdf"))
                        || url
                            .split(['?', '#'])
                            .next()
                            .is_some_and(|path| path.to_ascii_lowercase().ends_with(".pdf"));
                    pdf.then(|| paperloom_capture::absolute_url(&page_url, url)).flatten()
                })
            });
            tags.push((key.clone(), labels));
            hits.push(Hit { key, item, pdf_url, cited_by: None });
        }
        let total = hits.len() as u64;
        let mut page = search::keep(self, hits, Some(total), false)?;
        let tags: std::collections::HashMap<_, _> = tags.into_iter().collect();
        for hit in &mut page.hits {
            if let Some(labels) = tags.get(&hit.key) {
                hit.detail.tags = labels.clone();
            }
        }
        self.kept_results()?.1.extend(tags);
        Ok(page)
    }
}

fn converted(value: &Value, page_url: &str) -> Option<(paperloom_model::Item, Vec<String>)> {
    let object = value.as_object()?;
    let type_name = value["itemType"].as_str().unwrap_or("webpage");
    if matches!(type_name, "note" | "attachment" | "annotation") {
        return None;
    }
    let mut z = zotero::ZItem { type_name: type_name.into(), ..Default::default() };
    z.fields =
        object.iter().filter_map(|(key, v)| v.as_str().map(|text| (key.clone(), text.to_string()))).collect();
    if let Some(creators) = value["creators"].as_array() {
        for c in creators {
            let name = c["name"].as_str();
            z.creators.push((
                c["firstName"].as_str().unwrap_or_default().into(),
                name.or_else(|| c["lastName"].as_str()).unwrap_or_default().into(),
                name.is_some() || c["fieldMode"].as_u64() == Some(1),
                c["creatorType"].as_str().unwrap_or("author").into(),
            ));
        }
    }
    let tags = value["tags"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|tag| {
            tag.as_str()
                .or_else(|| tag["tag"].as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
        })
        .collect();
    let mut item = zotero::to_item(&z)?;
    if let Some(date) = z.fields.get("date").map(|d| d.trim()).filter(|d| !d.is_empty()) {
        if item.date.is_none() {
            item.date = Some(date.to_string());
        }
        if item.year.is_none() {
            item.year = date
                .split(|c: char| !c.is_ascii_digit())
                .find(|part| part.len() == 4)
                .and_then(|part| part.parse().ok());
        }
    }
    item.url = item
        .url
        .as_deref()
        .and_then(|url| paperloom_capture::absolute_url(page_url, url))
        .or_else(|| paperloom_capture::absolute_url(page_url, page_url));
    Some((item, tags))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
pub struct TranslatorResponse {
    pub status: u16,
    pub text: String,
    pub url: String,
}

/// Translator requests use the same HTTP client as the rest of the core.
#[uniffi::export]
pub fn translator_request(
    url: String,
    method: String,
    headers: Vec<PluginHeader>,
    body: Option<String>,
) -> Result<TranslatorResponse, CoreError> {
    let url = paperloom_capture::absolute_url(&url, &url)
        .ok_or_else(|| invalid("Translators may only request HTTP or HTTPS addresses."))?;
    let method = match method.to_ascii_uppercase().as_str() {
        "GET" => "GET",
        "POST" => "POST",
        _ => return Err(invalid("Translators may only send GET or POST requests.")),
    };
    let (response, final_url) = UreqHttp::new()
        .send_with_limit(
            Request {
                method,
                url: url.clone(),
                headers: headers.into_iter().map(|h| (h.name, h.value)).collect(),
                body: body.map(String::into_bytes),
            },
            16 << 20,
        )
        .map_err(|e| CoreError::Network { message: e.0 })?;
    Ok(TranslatorResponse {
        status: response.status,
        text: String::from_utf8_lossy(&response.body).into_owned(),
        url: final_url,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(id: &str, kind: u32) -> String {
        let header = serde_json::json!({
            "translatorID": id, "label": "Braces } and \"quotes\"", "target": r"^https?://example\.org/",
            "priority": 100, "translatorType": kind,
        });
        format!("{header}\nfunction detectWeb() {{ return \"book\"; }}")
    }

    #[test]
    fn parses_only_the_leading_header_and_checks_ids() {
        let text = code("a-b", 4);
        let h = header(&text).unwrap();
        assert_eq!(h.target.as_deref(), Some(r"^https?://example\.org/"));
        assert!(header(&format!("\u{feff}  {text}")).is_ok());
        for id in ["", " ", "..", ".hidden", "../outside", "a/b", "a\\b", "bad*id"] {
            assert!(header(&code(id, 4)).is_err(), "{id}");
        }
        // Spaces inside an id are dropped (one translator in Zotero's repository has one).
        assert_eq!(header(&code("a- b", 4)).unwrap().id, "a-b");
        assert!(header("function detectWeb() {}").is_err());
        assert!(header("{broken}").is_err());
    }

    #[test]
    fn installs_only_top_level_files_and_filters_web_bits() {
        let folder = tempfile::tempdir().unwrap();
        let lib = PaperLibrary::new(folder.path().join("library.sqlite").display().to_string()).unwrap();
        let web = code("web", 4);
        let combined = code("combined", 13);
        let import = code("import", 1);
        let archive = unzip::tests::deflated_zip(&[
            ("translators-master/Web.js", web.as_bytes()),
            ("translators-master/Combined.js", combined.as_bytes()),
            ("translators-master/Import.js", import.as_bytes()),
            ("translators-master/test/nested.js", web.as_bytes()),
            ("translators-master/../escape.js", web.as_bytes()),
            ("translators-master/README.md", b"readme"),
        ]);
        assert_eq!(lib.install_translators(&archive).unwrap(), 3);
        assert_eq!(lib.web_translators().unwrap().len(), 2);
        assert_eq!(lib.translators_status().unwrap().count, 3);
        assert!(lib.translators_status().unwrap().downloaded.is_some());
        assert_eq!(lib.translator_code("import".into()).unwrap(), import);
        assert!(lib.translator_code("../escape".into()).is_err());
        assert!(!folder.path().join("escape.js").exists());
        assert!(lib.install_translators(b"bad archive").is_err());
        assert_eq!(lib.translators_status().unwrap().count, 3);
        lib.remove_translators().unwrap();
        assert_eq!(lib.translators_status().unwrap(), TranslatorStatus { count: 0, downloaded: None });
    }

    /// The repository's JurPC translator has a space in its id; one odd file
    /// must not keep the other translators out.
    #[test]
    fn installs_around_translators_that_cannot_be_read() {
        let folder = tempfile::tempdir().unwrap();
        let lib = PaperLibrary::new(folder.path().join("library.sqlite").display().to_string()).unwrap();
        let web = code("web", 4);
        let spaced = code("b662c6eb-e478-46bd- bad4-23cdfd0c9d67", 4);
        let duplicate = code("web", 4);
        let archive = unzip::tests::deflated_zip(&[
            ("translators-master/Web.js", web.as_bytes()),
            ("translators-master/JurPC.js", spaced.as_bytes()),
            ("translators-master/Broken.js", b"function detectWeb() {}"),
            ("translators-master/Again.js", duplicate.as_bytes()),
            ("translators-master/Binary.js", &[0xff, 0xfe, 0x00]),
        ]);
        assert_eq!(lib.install_translators(&archive).unwrap(), 2);
        let ids: Vec<String> = lib.web_translators().unwrap().into_iter().map(|t| t.id).collect();
        assert!(ids.contains(&"b662c6eb-e478-46bd-bad4-23cdfd0c9d67".to_string()));
        assert_eq!(lib.translator_code("b662c6eb-e478-46bd-bad4-23cdfd0c9d67".into()).unwrap(), spaced);
        let nothing = unzip::tests::deflated_zip(&[("translators-master/Broken.js", b"nothing")]);
        assert!(lib.install_translators(&nothing).is_err());
        assert_eq!(lib.translators_status().unwrap().count, 2);
    }

    #[test]
    fn converts_items_and_keeps_names_tags_and_pdf_for_adding() {
        let lib = PaperLibrary::in_memory().unwrap();
        let page = lib.read_translated_items(r#"[{"itemType":"bookSection","title":"章节","creators":[{"firstName":"三","lastName":"张","creatorType":"author"},{"name":"研究所","creatorType":"editor"}],"date":"2024-02-03","publicationTitle":"论文集","volume":"2","issue":"1","pages":"3–8","DOI":"10.1000/test","ISBN":"123","ISSN":"456","abstractNote":"摘要","language":"zh","publisher":"出版社","place":"北京","tags":[{"tag":"研究"},"文献"],"notes":[{"note":"ignored"}],"attachments":[{"url":"https://example.org/page","mimeType":"text/html"},{"url":"/full","mimeType":"application/pdf"},{"url":"https://example.org/other.pdf"}]},{"itemType":"note","title":"ignored"}]"#.into(), "https://example.org/".into()).unwrap();
        assert_eq!(page.hits.len(), 1);
        let hit = &page.hits[0];
        assert_eq!(hit.detail.item_type, "book-chapter");
        assert_eq!(hit.pdf_url.as_deref(), Some("https://example.org/full"));
        assert_eq!(hit.detail.tags, ["研究", "文献"]);
        let key = hit.key.clone();
        let report = lib.add_search_results(vec![key], None).unwrap();
        assert_eq!(report.added, 1);
        let store = lib.lock().unwrap();
        let item = store.items().unwrap().pop().unwrap();
        assert_eq!(item.creators[0].literal, "张三");
        assert_eq!(item.creators[1].literal, "研究所");
        assert_eq!(item.container_title.as_deref(), Some("论文集"));
        assert_eq!(item.url.as_deref(), Some("https://example.org/"));
        assert_eq!(store.item_tags(&item.id).unwrap().len(), 2);
        assert!(lib.read_translated_items("{}".into(), String::new()).is_err());
    }

    #[test]
    fn preserves_textual_dates_and_single_field_names() {
        let value = serde_json::json!({
            "itemType": "report", "title": "Report", "date": "October 14, 2024",
            "creators": [{ "lastName": "研究所", "fieldMode": 1, "creatorType": "author" }],
            "url": "../record",
        });
        let (item, _) = converted(&value, "https://example.org/").unwrap();
        assert_eq!(item.date.as_deref(), Some("October 14, 2024"));
        assert_eq!(item.url.as_deref(), Some("https://example.org/record"));
        assert_eq!(item.year, Some(2024));
        assert_eq!(item.creators[0].literal, "研究所");
    }

    #[cfg(unix)]
    #[test]
    fn ignores_symlinked_code_and_rejects_symlinked_folders() {
        use std::os::unix::fs::symlink;
        let folder = tempfile::tempdir().unwrap();
        let lib = PaperLibrary::new(folder.path().join("library.sqlite").display().to_string()).unwrap();
        let dir = lib.translators_dir().unwrap();
        fs::create_dir(&dir).unwrap();
        let outside = folder.path().join("Outside.js");
        fs::write(&outside, code("outside", 4)).unwrap();
        symlink(&outside, dir.join("Inside.js")).unwrap();
        assert!(lib.translator_code("outside".into()).is_err());
        lib.remove_translators().unwrap();
        assert!(outside.exists());
        symlink(folder.path(), &dir).unwrap();
        assert!(lib.web_translators().is_err());
        assert!(lib.remove_translators().is_err());
        assert!(outside.exists());
    }

    #[test]
    fn rejects_non_web_requests_and_other_methods() {
        for url in ["file:///etc/passwd", "javascript:alert(1)"] {
            assert!(translator_request(url.into(), "GET".into(), vec![], None).is_err());
        }
        assert!(translator_request("https://example.org".into(), "DELETE".into(), vec![], None).is_err());
    }
}
