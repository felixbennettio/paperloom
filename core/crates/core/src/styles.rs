//! Citation styles beyond the built-in ones: searched for in and downloaded
//! from the CSL style repository (the one Zotero serves), or added from a
//! `.csl` file. They are kept in `styles/` beside the library and added
//! again each time it opens.

use std::fs;
use std::io;
use std::path::Path;
use std::time::{Duration, SystemTime};

use paperloom_citation as citation;
use paperloom_http::{Http, UreqHttp};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{CitationStyle, CoreError, PaperLibrary};

const INDEX_URL: &str = "https://www.zotero.org/styles-files/styles.json";
const STYLE_URL: &str = "https://www.zotero.org/styles/";
/// How long a downloaded list of the repository's styles is used.
const INDEX_AGE: Duration = Duration::from_secs(7 * 24 * 3600);

/// A style in the repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct StyleListing {
    pub id: String,
    pub name: String,
    /// `numeric`, `author-date`, `note`, `author` or `label`.
    pub format: String,
    pub fields: Vec<String>,
    /// Cites like another style, under a journal's own name.
    pub dependent: bool,
    /// Built in or added already.
    pub available: bool,
}

fn storage(err: io::Error) -> CoreError {
    CoreError::Storage { message: err.to_string() }
}

/// Adds the styles kept in `dir`, the ones others cite like first.
pub(crate) fn load_styles(dir: &Path) {
    let Ok(read) = fs::read_dir(dir) else { return };
    let mut files: Vec<(String, String, bool)> = read
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("csl")) {
                return None;
            }
            let id = path.file_stem()?.to_str()?.to_string();
            let xml = fs::read_to_string(&path).ok()?;
            let dependent = citation::read_style(&xml).ok()?.parent.is_some();
            Some((id, xml, dependent))
        })
        .collect();
    files.sort_by_key(|(_, _, dependent)| *dependent);
    for (id, xml, _) in files {
        let _ = citation::add_style(&id, &xml);
    }
}

fn fetch_style(http: &dyn Http, id: &str) -> Result<String, CoreError> {
    let url = format!("{STYLE_URL}{id}");
    let response = http
        .get(&url, "application/vnd.citationstyles.style+xml, application/xml;q=0.9")
        .map_err(|e| CoreError::Network { message: e.0 })?;
    match response.status {
        200..=299 => Ok(response.body),
        404 => Err(CoreError::NotFound { message: format!("The style repository has no style “{id}”.") }),
        status => Err(CoreError::Network { message: format!("The style repository answered {status}.") }),
    }
}

/// Better matches first: the query as the whole name or title, then titles
/// that start with it, then shorter titles.
fn rank(entry: &Value, query: &str) -> (bool, bool, usize) {
    let title = entry["title"].as_str().unwrap_or_default().to_lowercase();
    let name = entry["name"].as_str().unwrap_or_default();
    (name != query && title != query, !title.starts_with(query), title.len())
}

fn listing(entry: &Value) -> Option<StyleListing> {
    let id = entry["name"].as_str()?.to_string();
    Some(StyleListing {
        name: entry["title"].as_str().unwrap_or(&id).to_string(),
        format: entry["categories"]["format"].as_str().unwrap_or_default().to_string(),
        fields: entry["categories"]["fields"]
            .as_array()
            .map(|fields| fields.iter().filter_map(|f| f.as_str().map(str::to_string)).collect())
            .unwrap_or_default(),
        dependent: entry["dependent"].as_u64().is_some_and(|d| d != 0),
        available: citation::has_style(&id),
        id,
    })
}

#[uniffi::export]
impl PaperLibrary {
    /// Styles in the repository whose name holds every word of `query`;
    /// the list of them is downloaded once a week.
    pub fn search_citation_styles(&self, query: String, limit: u32) -> Result<Vec<StyleListing>, CoreError> {
        self.search_styles_through(&UreqHttp::new(), &query, limit as usize)
    }

    /// Downloads a style from the repository and adds it, with the style
    /// it cites like when that is not here yet.
    pub fn download_citation_style(&self, id: String) -> Result<CitationStyle, CoreError> {
        self.download_style_through(&UreqHttp::new(), &id)
    }

    /// Adds the style in a `.csl` file, named after the file (downloading
    /// the style it cites like when that is not here yet).
    pub fn add_citation_style_file(&self, path: String) -> Result<CitationStyle, CoreError> {
        let xml = fs::read_to_string(&path).map_err(storage)?;
        let id = Path::new(&path)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(citation::short_name)
            .unwrap_or_default();
        if id.is_empty() {
            return Err(CoreError::InvalidInput {
                message: format!("“{path}” is not a style file name.")
            });
        }
        self.add_style_through(&UreqHttp::new(), &id, &xml)
    }

    /// Removes an added style (a built-in one it replaced comes back).
    pub fn remove_citation_style(&self, id: String) -> Result<(), CoreError> {
        let id = citation::short_name(&id);
        citation::remove_style(&id);
        if let Some(dir) = &self.styles_dir {
            match fs::remove_file(dir.join(format!("{id}.csl"))) {
                Err(err) if err.kind() != io::ErrorKind::NotFound => return Err(storage(err)),
                _ => {}
            }
        }
        Ok(())
    }
}

impl PaperLibrary {
    pub(crate) fn search_styles_through(
        &self,
        http: &dyn Http,
        query: &str,
        limit: usize,
    ) -> Result<Vec<StyleListing>, CoreError> {
        let index = self.style_index(http)?;
        let query = query.trim().to_lowercase();
        let words: Vec<&str> = query.split_whitespace().collect();
        let mut found: Vec<&Value> = index
            .iter()
            .filter(|entry| {
                let text = format!(
                    "{} {} {}",
                    entry["title"].as_str().unwrap_or_default(),
                    entry["titleShort"].as_str().unwrap_or_default(),
                    entry["name"].as_str().unwrap_or_default()
                )
                .to_lowercase();
                words.iter().all(|w| text.contains(w))
            })
            .collect();
        found.sort_by_cached_key(|entry| rank(entry, &query));
        Ok(found.into_iter().filter_map(listing).take(limit).collect())
    }

    /// The repository's list of styles: the copy kept beside the library
    /// while it is fresh, else downloaded again (the old copy serves when
    /// the repository cannot be reached).
    fn style_index(&self, http: &dyn Http) -> Result<Vec<Value>, CoreError> {
        let kept = self.styles_dir.as_ref().map(|dir| dir.join("index.json"));
        let age = kept
            .as_ref()
            .and_then(|path| fs::metadata(path).ok()?.modified().ok())
            .and_then(|modified| SystemTime::now().duration_since(modified).ok());
        let text = if age.is_some_and(|age| age < INDEX_AGE) {
            kept.as_ref().and_then(|path| fs::read_to_string(path).ok())
        } else {
            None
        };
        let text = match text {
            Some(text) => text,
            None => {
                let downloaded = match http.get(INDEX_URL, "application/json") {
                    Ok(response) if response.is_success() => Ok(response.body),
                    Ok(response) => Err(format!("The style repository answered {}.", response.status)),
                    Err(err) => Err(err.0),
                };
                match (downloaded, &kept) {
                    (Ok(body), Some(path)) => {
                        // A copy that cannot be kept is downloaded again next time.
                        if let Some(dir) = path.parent() {
                            let _ = fs::create_dir_all(dir).and_then(|()| fs::write(path, &body));
                        }
                        body
                    }
                    (Ok(body), None) => body,
                    (Err(message), kept) => kept
                        .as_ref()
                        .and_then(|path| fs::read_to_string(path).ok())
                        .ok_or(CoreError::Network { message })?,
                }
            }
        };
        serde_json::from_str(&text).map_err(|e| CoreError::Network {
            message: format!("The style repository sent an unexpected list: {e}"),
        })
    }

    pub(crate) fn download_style_through(
        &self,
        http: &dyn Http,
        id: &str,
    ) -> Result<CitationStyle, CoreError> {
        let id = citation::short_name(id);
        let xml = fetch_style(http, &id)?;
        self.add_style_through(http, &id, &xml)
    }

    fn add_style_through(&self, http: &dyn Http, id: &str, xml: &str) -> Result<CitationStyle, CoreError> {
        let file = citation::read_style(xml)?;
        if let Some(parent) = file.parent.as_deref().filter(|p| !citation::has_style(p)) {
            let parent_xml = fetch_style(http, parent)?;
            self.keep_style(parent, &parent_xml)?;
        }
        self.keep_style(id, xml)
    }

    fn keep_style(&self, id: &str, xml: &str) -> Result<CitationStyle, CoreError> {
        let name = citation::add_style(id, xml)?;
        if let Some(dir) = &self.styles_dir {
            fs::create_dir_all(dir)
                .and_then(|()| fs::write(dir.join(format!("{id}.csl")), xml))
                .map_err(storage)?;
        }
        Ok(CitationStyle { id: id.to_string(), name, added: true })
    }
}

#[cfg(test)]
mod tests;
