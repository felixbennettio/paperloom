//! Citing while writing: what the Word and WPS task panes ask of the
//! library through the connector (`docs/writing-plugins.md`). A document's
//! citations are formatted together, in document order, so their numbers
//! and the reference list follow the text.

use std::collections::HashMap;

use paperloom_citation::{self as citation, Cite, Format};
use paperloom_model::{Item, ItemId};
use paperloom_store::Filter;
use serde::{Deserialize, Serialize};

use crate::notelinks::{CitationResolver, key_of, short_cite};
use crate::{CoreError, PaperLibrary};

/// One work cited at one place, and the page if given.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct CitedWork {
    pub item_id: String,
    #[serde(default)]
    pub locator: Option<String>,
}

/// The works cited at one place in a document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct CitationCluster {
    pub works: Vec<CitedWork>,
}

/// A document's citations formatted in order, and its reference list.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct CitedText {
    /// One per cluster; empty for a cluster none of whose works is left.
    pub citations: Vec<String>,
    pub bibliography: Vec<String>,
    /// Item ids cited but no longer in the library, each once.
    pub missing: Vec<String>,
}

/// An item a writer can cite.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct CiteCandidate {
    pub id: String,
    pub key: String,
    pub title: String,
    /// `Kucsko et al. 2013`.
    pub cite: String,
    pub year: Option<i32>,
}

#[uniffi::export]
impl PaperLibrary {
    /// Items whose title, authors, year or citation key hold every word of
    /// `query`, newest first; the newest when it is empty.
    pub fn cite_candidates(&self, query: String, limit: u32) -> Result<Vec<CiteCandidate>, CoreError> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        let store = self.lock()?;
        let mut found = Vec::new();
        for entry in store.entries(&Filter::default())? {
            let item = entry.item;
            let key = key_of(&item);
            let names: Vec<String> = item.creators.iter().map(|c| c.display_name()).collect();
            let year = item.year.map(|y| y.to_string()).unwrap_or_default();
            let haystack = format!("{} {} {year} {key}", item.title, names.join(" ")).to_lowercase();
            if words.iter().all(|w| haystack.contains(w.as_str())) {
                found.push(CiteCandidate {
                    id: item.id.to_string(),
                    key,
                    title: item.title.clone(),
                    cite: short_cite(&item),
                    year: item.year,
                });
                if found.len() >= limit as usize {
                    break;
                }
            }
        }
        Ok(found)
    }

    /// Formats a document's citations (`clusters`, in document order) and
    /// its reference list in `style_id`, as HTML (italics, super- and
    /// subscripts kept) or text.
    pub fn format_citation_clusters(
        &self,
        style_id: String,
        clusters: Vec<CitationCluster>,
        html: bool,
    ) -> Result<CitedText, CoreError> {
        // Each item once, in the order it is first cited.
        let mut items: Vec<Item> = Vec::new();
        let mut index: HashMap<String, usize> = HashMap::new();
        let mut missing: Vec<String> = Vec::new();
        {
            let store = self.lock()?;
            for work in clusters.iter().flat_map(|c| &c.works) {
                if index.contains_key(&work.item_id) || missing.contains(&work.item_id) {
                    continue;
                }
                match store.item(&ItemId::from_string(work.item_id.clone()))? {
                    Some(item) => {
                        index.insert(work.item_id.clone(), items.len());
                        items.push(item);
                    }
                    None => missing.push(work.item_id.clone()),
                }
            }
        }
        // Clusters with nothing left are not given to the formatter, which
        // would skip them and shift the rest.
        let groups: Vec<(usize, Vec<Cite>)> = clusters
            .iter()
            .enumerate()
            .map(|(at, cluster)| {
                let cites = cluster
                    .works
                    .iter()
                    .filter_map(|w| {
                        Some(Cite {
                            index: *index.get(&w.item_id)?,
                            page: w.locator.clone().filter(|l| !l.is_empty()),
                        })
                    })
                    .collect::<Vec<_>>();
                (at, cites)
            })
            .filter(|(_, cites)| !cites.is_empty())
            .collect();
        let mut text =
            CitedText { citations: vec![String::new(); clusters.len()], missing, ..CitedText::default() };
        if groups.is_empty() {
            return Ok(text);
        }
        let format = if html { Format::Html } else { Format::Text };
        let wanted: Vec<Vec<Cite>> = groups.iter().map(|(_, cites)| cites.clone()).collect();
        let document = citation::format_document(&items, &wanted, &style_id, format)?;
        let tags = |text: String| if html { plugin_html(&text) } else { text };
        for ((at, _), citation) in groups.iter().zip(document.citations) {
            text.citations[*at] = tags(citation);
        }
        text.bibliography = document.entries.into_iter().map(tags).collect();
        Ok(text)
    }
}

/// A Pandoc citation (`[@key, p. 3]`) in a document's text, which a
/// writing plugin turns into a live citation: the `[@…]` markers that Cite
/// in Word Document reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CiteMarker {
    /// The citation as written, brackets included.
    pub text: String,
    /// The works it names; empty when the library lacks one of them.
    pub works: Vec<CitedWork>,
    /// The keys (citation keys, DOIs or item IDs) the library has no work
    /// for.
    pub missing: Vec<String>,
}

impl PaperLibrary {
    /// The Pandoc citations in `text`, in order, with the works they name.
    pub fn cite_markers(&self, text: &str) -> Result<Vec<CiteMarker>, CoreError> {
        let resolver = {
            let store = self.lock()?;
            CitationResolver::new(&store)?
        };
        Ok(paperloom_vault::plain_citations(text)
            .into_iter()
            .map(|citation| {
                let mut works = Vec::new();
                let mut missing = Vec::new();
                for work in citation.works {
                    match resolver.find(&work.key) {
                        Some(item) => {
                            works.push(CitedWork { item_id: item.id.to_string(), locator: work.page })
                        }
                        None => missing.push(work.key),
                    }
                }
                if !missing.is_empty() {
                    works.clear();
                }
                CiteMarker { text: text[citation.start..citation.end].to_string(), works, missing }
            })
            .collect())
    }
}

/// The formatter marks italics, bold and super/subscripts with
/// `<span style="…">`; the writing plugins take `<i>`, `<b>`, `<sup>` and
/// `<sub>` (`docs/writing-plugins.md`) and drop other markup, so each span
/// becomes those tags. Links and text pass through.
fn plugin_html(html: &str) -> String {
    const STYLES: [(&str, &str); 4] = [
        ("font-style: italic;", "i"),
        ("font-weight: bold;", "b"),
        ("vertical-align: super;", "sup"),
        ("vertical-align: sub;", "sub"),
    ];
    let mut out = String::with_capacity(html.len());
    let mut open: Vec<Vec<&str>> = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find('<') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let end = rest.find('>').map_or(rest.len(), |end| end + 1);
        let tag = &rest[..end];
        if let Some(style) = tag.strip_prefix("<span style=\"").and_then(|t| t.strip_suffix("\">")) {
            let tags: Vec<&str> =
                STYLES.iter().filter(|(css, _)| style.contains(css)).map(|(_, tag)| *tag).collect();
            for tag in &tags {
                out.push_str(&format!("<{tag}>"));
            }
            open.push(tags);
        } else if tag == "</span>" {
            for tag in open.pop().unwrap_or_default().iter().rev() {
                out.push_str(&format!("</{tag}>"));
            }
        } else {
            out.push_str(tag);
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use crate::NewItem;

    use super::*;

    fn add(library: &PaperLibrary, title: &str, author: &str, year: i32) -> String {
        library
            .add_item(NewItem {
                title: title.into(),
                authors: vec![author.into()],
                container_title: Some("Nature".into()),
                year: Some(year),
                ..NewItem::default()
            })
            .unwrap()
            .id
    }

    fn cluster(ids: &[&str]) -> CitationCluster {
        CitationCluster {
            works: ids.iter().map(|id| CitedWork { item_id: (*id).into(), locator: None }).collect(),
        }
    }

    #[test]
    fn numbers_citations_in_document_order() {
        let library = PaperLibrary::in_memory().unwrap();
        let curie = add(&library, "Radioactivity", "Marie Curie", 1903);
        let kucsko = add(&library, "Nanometre-scale thermometry in a living cell", "Georg Kucsko", 2013);
        let text = library
            .format_citation_clusters(
                "ieee".into(),
                vec![cluster(&[&kucsko]), cluster(&[&curie]), cluster(&[&kucsko])],
                false,
            )
            .unwrap();
        assert_eq!(text.citations, ["[1]", "[2]", "[1]"]);
        assert_eq!(text.bibliography.len(), 2);
        assert!(text.bibliography[0].contains("Nanometre-scale"), "{:?}", text.bibliography);
        assert!(text.missing.is_empty());
    }

    #[test]
    fn leaves_out_works_no_longer_in_the_library() {
        let library = PaperLibrary::in_memory().unwrap();
        let curie = add(&library, "Radioactivity", "Marie Curie", 1903);
        let text = library
            .format_citation_clusters(
                "ieee".into(),
                vec![cluster(&["gone"]), cluster(&[&curie, "gone"])],
                false,
            )
            .unwrap();
        assert_eq!(text.citations, ["", "[1]"]);
        assert_eq!(text.missing, ["gone"]);
        assert_eq!(text.bibliography.len(), 1);
        let empty = library.format_citation_clusters("ieee".into(), vec![cluster(&["gone"])], true).unwrap();
        assert_eq!(empty.citations, [""]);
        assert!(empty.bibliography.is_empty());
    }

    #[test]
    fn formats_html_with_the_tags_plugins_take() {
        let library = PaperLibrary::in_memory().unwrap();
        let kucsko = add(&library, "Nanometre-scale thermometry in a living cell", "Georg Kucsko", 2013);
        let apa = library.format_citation_clusters("apa".into(), vec![cluster(&[&kucsko])], true).unwrap();
        assert!(
            apa.bibliography[0].contains("<i>") && apa.bibliography[0].contains("Nature"),
            "{:?}",
            apa.bibliography
        );
        assert!(!apa.bibliography[0].contains("<span"), "{:?}", apa.bibliography);
        let gb = library
            .format_citation_clusters("gb-7714-2015-numeric".into(), vec![cluster(&[&kucsko])], true)
            .unwrap();
        assert!(gb.citations[0].contains("<sup>"), "{:?}", gb.citations);
        assert_eq!(gb.citations[0].replace("<sup>", "").replace("</sup>", ""), "[1]");
        assert_eq!(
            plugin_html(
                r#"<span style="font-style: italic;font-weight: bold;">A <a href="x">B</a></span> C"#
            ),
            r#"<i><b>A <a href="x">B</a></b></i> C"#
        );
    }

    #[test]
    fn finds_candidates_by_every_word() {
        let library = PaperLibrary::in_memory().unwrap();
        add(&library, "Radioactivity", "Marie Curie", 1903);
        let kucsko = add(&library, "Nanometre-scale thermometry in a living cell", "Georg Kucsko", 2013);
        let found = library.cite_candidates("kucsko 2013".into(), 10).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, kucsko);
        assert!(found[0].cite.contains("Kucsko") && found[0].cite.contains("2013"), "{:?}", found[0]);
        assert_eq!(library.cite_candidates(String::new(), 1).unwrap().len(), 1);
    }

    #[test]
    fn finds_the_markers_a_document_holds() {
        let library = PaperLibrary::in_memory().unwrap();
        let curie = add(&library, "Radioactivity", "Marie Curie", 1903);
        let kucsko = add(&library, "Nanometre-scale thermometry in a living cell", "Georg Kucsko", 2013);
        let key = library.cite_candidates("kucsko".into(), 1).unwrap().remove(0).key;
        let text = format!(
            "Heat [@{key}, p. 3] and rays [see @{curie}; @{key}].\rNot this: [@nobody]; nor [a link](x) or [[@{key}]]."
        );
        let markers = library.cite_markers(&text).unwrap();
        assert_eq!(markers.len(), 3, "{markers:?}");
        assert_eq!(markers[0].text, format!("[@{key}, p. 3]"));
        assert_eq!(markers[0].works, [CitedWork { item_id: kucsko.clone(), locator: Some("3".into()) }]);
        assert_eq!(
            markers[1].works.iter().map(|w| w.item_id.as_str()).collect::<Vec<_>>(),
            [curie.as_str(), kucsko.as_str()]
        );
        assert_eq!(markers[2].text, "[@nobody]");
        assert!(markers[2].works.is_empty());
        assert_eq!(markers[2].missing, ["nobody"]);
    }
}
