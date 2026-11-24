//! Items that are the same work, found as Zotero finds them (the same DOI,
//! book ISBN, arXiv or PubMed ID; or the same title with close years and a
//! shared author), and merged into one.

use std::collections::HashMap;

use paperloom_model::{Doi, Item, ItemId, ItemType};
use rusqlite::params;

use crate::{Error, Library, Result, now_ms};

/// A title as compared: letters and digits, lower case, single spaces.
fn title_key(title: &str) -> String {
    let plain: String = title
        .chars()
        .flat_map(char::to_lowercase)
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect();
    plain.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// An ISBN as ISBN-13 digits; an ISBN-10 gains its 978 and check digit.
fn isbn13(isbn: &str) -> Option<String> {
    let digits: String = isbn.chars().filter(|c| c.is_ascii_digit() || *c == 'X' || *c == 'x').collect();
    match digits.len() {
        13 => Some(digits),
        10 => {
            let body = format!("978{}", &digits[..9]);
            let sum: u32 = body
                .chars()
                .filter_map(|c| c.to_digit(10))
                .enumerate()
                .map(|(i, d)| if i % 2 == 0 { d } else { d * 3 })
                .sum();
            Some(format!("{body}{}", (10 - sum % 10) % 10))
        }
        _ => None,
    }
}

/// What names a work for sure: its DOI, a book's ISBN, its arXiv or
/// PubMed ID.
fn identifiers(item: &Item) -> Vec<String> {
    let mut keys = Vec::new();
    if let Some(doi) = &item.doi {
        keys.push(format!("doi:{}", doi.as_str()));
    }
    if item.item_type == ItemType::Book
        && let Some(isbn) = item.isbn.as_deref().and_then(isbn13)
    {
        keys.push(format!("isbn:{isbn}"));
    }
    if let Some(id) = item.arxiv_id.as_deref().map(str::trim).filter(|i| !i.is_empty()) {
        keys.push(format!("arxiv:{}", id.to_lowercase()));
    }
    if let Some(id) = item.pmid.as_deref().map(str::trim).filter(|i| !i.is_empty()) {
        keys.push(format!("pmid:{id}"));
    }
    keys
}

/// Whether two items with the same title are the same work: not two
/// different DOIs, years at most one apart, and a shared author when both
/// name some.
fn alike(a: &Item, b: &Item) -> bool {
    if let (Some(x), Some(y)) = (&a.doi, &b.doi)
        && x != y
    {
        return false;
    }
    if let (Some(x), Some(y)) = (a.year, b.year)
        && (x - y).abs() > 1
    {
        return false;
    }
    let names = |item: &Item| -> Vec<String> {
        item.creators.iter().map(|c| c.short_name().trim().to_lowercase()).filter(|n| !n.is_empty()).collect()
    };
    let (x, y) = (names(a), names(b));
    x.is_empty() || y.is_empty() || x.iter().any(|n| y.contains(n))
}

/// The first other item's value for a field the kept one lacks.
fn gap(slot: &mut Option<String>, others: &[Item], get: impl Fn(&Item) -> &Option<String>) {
    if slot.is_none() {
        *slot = others.iter().find_map(|o| get(o).clone());
    }
}

/// The kept item with its gaps filled from the others, and their keywords.
fn filled(mut kept: Item, others: &[Item]) -> Item {
    gap(&mut kept.container_title, others, |o| &o.container_title);
    gap(&mut kept.container_short_title, others, |o| &o.container_short_title);
    gap(&mut kept.date, others, |o| &o.date);
    gap(&mut kept.volume, others, |o| &o.volume);
    gap(&mut kept.issue, others, |o| &o.issue);
    gap(&mut kept.pages, others, |o| &o.pages);
    gap(&mut kept.publisher, others, |o| &o.publisher);
    gap(&mut kept.place, others, |o| &o.place);
    gap(&mut kept.edition, others, |o| &o.edition);
    gap(&mut kept.number, others, |o| &o.number);
    gap(&mut kept.isbn, others, |o| &o.isbn);
    gap(&mut kept.issn, others, |o| &o.issn);
    gap(&mut kept.arxiv_id, others, |o| &o.arxiv_id);
    gap(&mut kept.pmid, others, |o| &o.pmid);
    gap(&mut kept.url, others, |o| &o.url);
    gap(&mut kept.accessed, others, |o| &o.accessed);
    gap(&mut kept.abstract_note, others, |o| &o.abstract_note);
    gap(&mut kept.language, others, |o| &o.language);
    if kept.year.is_none() {
        kept.year = others.iter().find_map(|o| o.year);
    }
    if kept.cited_by.is_none() {
        kept.cited_by = others.iter().find_map(|o| o.cited_by);
    }
    if kept.doi.is_none() {
        kept.doi = others.iter().find_map(|o| o.doi.clone());
    }
    if kept.creators.is_empty()
        && let Some(other) = others.iter().find(|o| !o.creators.is_empty())
    {
        kept.creators = other.creators.clone();
    }
    for keyword in others.iter().flat_map(|o| &o.keywords) {
        if !kept.keywords.contains(keyword) {
            kept.keywords.push(keyword.clone());
        }
    }
    kept
}

impl Library {
    /// Groups of items (not in the trash) that look like the same work,
    /// each newest first; groups in the order of their newest item.
    pub fn duplicate_groups(&self) -> Result<Vec<Vec<ItemId>>> {
        let items = self.items()?;
        let mut parent: Vec<usize> = (0..items.len()).collect();
        fn root(parent: &mut [usize], i: usize) -> usize {
            let mut r = i;
            while parent[r] != r {
                r = parent[r];
            }
            let mut j = i;
            while parent[j] != r {
                let next = parent[j];
                parent[j] = r;
                j = next;
            }
            r
        }
        fn join(parent: &mut [usize], a: usize, b: usize) {
            let (ra, rb) = (root(parent, a), root(parent, b));
            if ra != rb {
                parent[ra.max(rb)] = ra.min(rb);
            }
        }
        let mut seen: HashMap<String, usize> = HashMap::new();
        let mut titles: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, item) in items.iter().enumerate() {
            for key in identifiers(item) {
                match seen.get(&key) {
                    Some(&j) => join(&mut parent, i, j),
                    None => {
                        seen.insert(key, i);
                    }
                }
            }
            let title = title_key(&item.title);
            // Too short to tell works apart.
            if title.chars().count() >= 8 {
                titles.entry(title).or_default().push(i);
            }
        }
        for same in titles.values() {
            for (k, &i) in same.iter().enumerate() {
                for &j in &same[k + 1..] {
                    if alike(&items[i], &items[j]) {
                        join(&mut parent, i, j);
                    }
                }
            }
        }
        let mut groups: Vec<Vec<ItemId>> = Vec::new();
        let mut index: HashMap<usize, usize> = HashMap::new();
        for (i, item) in items.iter().enumerate() {
            let r = root(&mut parent, i);
            let at = *index.entry(r).or_insert_with(|| {
                groups.push(Vec::new());
                groups.len() - 1
            });
            groups[at].push(item.id.clone());
        }
        groups.retain(|g| g.len() > 1);
        Ok(groups)
    }

    /// Merges items into `master`: its own details stay and its gaps are
    /// filled from the others; their files (with their marks), tags,
    /// collections and star come to it; and they go to the trash. The
    /// merged item.
    pub fn merge_items(&mut self, master: &ItemId, others: &[ItemId]) -> Result<Item> {
        let kept = self.item(master)?.ok_or_else(|| Error::NotFound(format!("item {master}")))?;
        let mut merged = Vec::new();
        for id in others.iter().filter(|id| *id != master) {
            merged.push(self.item(id)?.ok_or_else(|| Error::NotFound(format!("item {id}")))?);
        }
        let kept = filled(kept, &merged);
        // The DOI it takes may be no other item's but the merged ones'.
        if let Some(doi) = &kept.doi
            && let Some(holder) = self.find_by_doi(doi)?
            && holder.id != kept.id
            && !merged.iter().any(|m| m.id == holder.id)
        {
            return Err(Error::DuplicateDoi { doi: doi.clone(), existing: holder.id });
        }
        let now = now_ms();
        let tx = self.conn.transaction()?;
        for other in &merged {
            let (to, from) = (master.as_str(), other.id.as_str());
            tx.execute("UPDATE attachments SET item_id = ?1 WHERE item_id = ?2", params![to, from])?;
            tx.execute(
                "INSERT OR IGNORE INTO item_tags (item_id, tag) SELECT ?1, tag FROM item_tags WHERE item_id = ?2",
                params![to, from],
            )?;
            tx.execute(
                "INSERT OR IGNORE INTO collection_items (collection_id, item_id)
                 SELECT collection_id, ?1 FROM collection_items WHERE item_id = ?2",
                params![to, from],
            )?;
            tx.execute(
                "UPDATE items SET starred = 1 WHERE id = ?1 AND EXISTS (SELECT 1 FROM items WHERE id = ?2 AND starred = 1)",
                params![to, from],
            )?;
            // Its related items become the kept one's.
            tx.execute(
                "INSERT OR IGNORE INTO item_relations (item_id, related_id)
                 SELECT ?1, related_id FROM item_relations WHERE item_id = ?2 AND related_id != ?1",
                params![to, from],
            )?;
            tx.execute(
                "INSERT OR IGNORE INTO item_relations (item_id, related_id)
                 SELECT item_id, ?1 FROM item_relations WHERE related_id = ?2 AND item_id != ?1",
                params![to, from],
            )?;
            tx.execute("DELETE FROM item_relations WHERE item_id = ?1 OR related_id = ?1", params![from])?;
            tx.execute("UPDATE items SET deleted = 1, date_modified = ?2 WHERE id = ?1", params![from, now])?;
        }
        tx.execute(
            "UPDATE items SET item_type = ?2, title = ?3, doi = ?4, year = ?5, data = ?6, date_modified = ?7
             WHERE id = ?1",
            params![
                kept.id.as_str(),
                kept.item_type.as_str(),
                kept.title,
                kept.doi.as_ref().map(Doi::as_str),
                kept.year,
                serde_json::to_string(&kept)?,
                now,
            ],
        )?;
        tx.commit()?;
        Ok(kept)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_titles_and_isbns() {
        assert_eq!(title_key("  Quantum Sensing: A Review. "), "quantum sensing a review");
        assert_eq!(title_key("量子传感——综述"), "量子传感 综述");
        assert_eq!(isbn13("0-306-40615-2").as_deref(), Some("9780306406157"));
        assert_eq!(isbn13("978-0-306-40615-7").as_deref(), Some("9780306406157"));
        assert_eq!(isbn13("12345"), None);
    }
}
