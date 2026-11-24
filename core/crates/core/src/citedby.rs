//! How often works are cited, as OpenAlex counts them: looked up by DOI,
//! fifty to a request, and kept with the items.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::time::Duration;

use paperloom_http::{Http, UreqHttp};
use paperloom_model::{Item, ItemId};
use serde_json::Value;

use crate::{CoreError, PaperLibrary};

/// How many DOIs one request asks about.
const BATCH: usize = 50;

/// A DOI as an OpenAlex filter value: what a URL query may not hold
/// percent-encoded.
fn encode(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~/:".contains(&b) {
            out.push(char::from(b));
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

/// The counts OpenAlex gives for some DOIs, by DOI (lower case).
fn counts(http: &dyn Http, dois: &[&str]) -> Result<HashMap<String, u32>, CoreError> {
    let filter = dois.iter().copied().map(encode).collect::<Vec<_>>().join("%7C");
    let url = format!(
        "https://api.openalex.org/works?filter=doi:{filter}&select=doi,cited_by_count&per-page={BATCH}"
    );
    let failed = |detail: String| CoreError::Lookup { message: format!("OpenAlex: {detail}") };
    let response = http.get(&url, "application/json").map_err(|e| failed(e.to_string()))?;
    if !response.is_success() {
        return Err(failed(format!("answered {}", response.status)));
    }
    let body: Value = serde_json::from_str(&response.body).map_err(|e| failed(e.to_string()))?;
    Ok(body["results"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|work| {
            let doi = work["doi"].as_str()?.to_ascii_lowercase();
            let doi = doi.strip_prefix("https://doi.org/").unwrap_or(&doi).to_string();
            let count = u32::try_from(work["cited_by_count"].as_u64()?).ok()?;
            Some((doi, count))
        })
        .collect())
}

impl PaperLibrary {
    pub(crate) fn count_citations_with(
        &self,
        http: &dyn Http,
        item_ids: Vec<String>,
    ) -> Result<u32, CoreError> {
        let items: Vec<Item> = {
            let library = self.lock()?;
            if item_ids.is_empty() {
                library.items()?
            } else {
                let mut items = Vec::new();
                for id in item_ids {
                    items.extend(library.item(&ItemId::from_string(id))?);
                }
                items
            }
        };
        // DOIs that would break the filter (`,` and `|` part its values) are left out.
        let with_doi: Vec<Item> = items
            .into_iter()
            .filter(|i| i.doi.as_ref().is_some_and(|d| !d.as_str().contains([',', '|'])))
            .collect();
        let mut counted = 0;
        for batch in with_doi.chunks(BATCH) {
            let dois: Vec<&str> = batch.iter().filter_map(|i| i.doi.as_ref().map(|d| d.as_str())).collect();
            let found = counts(http, &dois)?;
            let mut library = self.lock()?;
            for item in batch {
                let Some(doi) = &item.doi else { continue };
                let Some(&count) = found.get(&doi.as_str().to_ascii_lowercase()) else { continue };
                if item.cited_by != Some(count) {
                    let mut changed = item.clone();
                    changed.cited_by = Some(count);
                    library.update_item(&changed)?;
                }
                counted += 1;
            }
        }
        Ok(counted)
    }
}

#[uniffi::export]
impl PaperLibrary {
    /// Looks up how many works cite some items (all of them when none are
    /// named) by their DOIs, as OpenAlex counts them, and keeps the counts;
    /// how many items were counted. Talks to the network: call it off the
    /// main thread.
    pub fn count_citations(&self, item_ids: Vec<String>) -> Result<u32, CoreError> {
        self.count_citations_with(&UreqHttp::with_timeout(Duration::from_secs(30)), item_ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NewItem;
    use paperloom_http::{NetworkError, Response};
    use std::sync::Mutex;

    /// OpenAlex, answering with counts for the DOIs it is asked about.
    struct OpenAlex(Mutex<Vec<String>>);

    impl Http for OpenAlex {
        fn get(&self, url: &str, _accept: &str) -> Result<Response, NetworkError> {
            self.0.lock().unwrap().push(url.to_string());
            let body = r#"{"results": [
                {"doi": "https://doi.org/10.1000/one", "cited_by_count": 42},
                {"doi": "https://doi.org/10.1000/TWO", "cited_by_count": 7}]}"#;
            Ok(Response::new(200, body))
        }

        fn post_form(&self, _url: &str, _form: &[(&str, &str)]) -> Result<Response, NetworkError> {
            Err(NetworkError("not used".into()))
        }
    }

    #[test]
    fn counts_citations_by_doi() {
        let lib = PaperLibrary::in_memory().unwrap();
        let add = |title: &str, doi: Option<&str>| {
            let item = NewItem { title: title.into(), doi: doi.map(str::to_string), ..NewItem::default() };
            lib.add_item(item).unwrap().id
        };
        let one = add("One", Some("10.1000/one"));
        add("Two", Some("10.1000/two"));
        add("None", None);
        let alex = OpenAlex(Mutex::new(Vec::new()));
        assert_eq!(lib.count_citations_with(&alex, vec![]).unwrap(), 2);
        let asked = alex.0.lock().unwrap().clone();
        assert_eq!(asked.len(), 1, "one request for both");
        assert!(asked[0].contains("filter=doi:10.1000/") && asked[0].contains("%7C"), "{}", asked[0]);
        assert_eq!(lib.item_detail(one.clone()).unwrap().unwrap().cited_by, Some(42));
        let listed = lib.list_items(crate::ItemFilter::default()).unwrap();
        assert!(listed.iter().any(|i| i.cited_by == Some(7)));
    }
}
