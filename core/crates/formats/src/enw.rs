//! EndNote tagged format (Refer): `%0 Journal Article`, `%A Author`, …
//! It is what CNKI, Wanfang and VIP export as "EndNote", which matters for
//! Chinese literature: CNKI puts several authors or keywords into one field,
//! separated by semicolons.

use paperloom_model::{Creator, CreatorRole, Doi, Item, ItemType};

use crate::Parsed;
use crate::text::{inverted, iso_date, split_list, year_in};

fn item_type(name: &str) -> ItemType {
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "journal article" | "magazine article" | "newspaper article" | "electronic article" => {
            ItemType::JournalArticle
        }
        "conference proceedings" | "conference paper" => ItemType::ConferencePaper,
        "book" | "edited book" | "electronic book" => ItemType::Book,
        "book section" | "electronic book section" => ItemType::BookChapter,
        "thesis" | "dissertation" => ItemType::Thesis,
        "report" => ItemType::Report,
        "patent" => ItemType::Patent,
        "standard" => ItemType::Standard,
        "dataset" => ItemType::Dataset,
        "web page" => ItemType::Webpage,
        "unpublished work" | "manuscript" => ItemType::Preprint,
        _ => ItemType::Other,
    }
}

fn type_name(ty: ItemType) -> &'static str {
    match ty {
        ItemType::JournalArticle => "Journal Article",
        ItemType::ConferencePaper => "Conference Proceedings",
        ItemType::Book => "Book",
        ItemType::BookChapter => "Book Section",
        ItemType::Thesis => "Thesis",
        ItemType::Report => "Report",
        ItemType::Preprint => "Unpublished Work",
        ItemType::Patent => "Patent",
        ItemType::Standard => "Standard",
        ItemType::Dataset => "Dataset",
        ItemType::Webpage => "Web Page",
        ItemType::Other => "Generic",
    }
}

/// `%X value` → (`X`, value); the tag is one character after `%`.
fn tag_line(line: &str) -> Option<(char, &str)> {
    let rest = line.strip_prefix('%')?;
    let mut chars = rest.chars();
    let tag = chars.next()?;
    let value = chars.as_str();
    (value.is_empty() || value.starts_with(' ')).then(|| (tag, value.trim()))
}

pub(crate) fn parse(text: &str) -> Parsed {
    let mut parsed = Parsed::default();
    let mut record: Vec<(char, String)> = Vec::new();
    let flush = |record: &mut Vec<(char, String)>, parsed: &mut Parsed| {
        if !record.is_empty() {
            match build(record) {
                Some(item) => parsed.items.push(item),
                None => parsed.skipped += 1,
            }
            record.clear();
        }
    };
    for line in text.lines() {
        let line = line.trim_end();
        match tag_line(line.trim_start()) {
            Some(('0', value)) => {
                flush(&mut record, &mut parsed);
                record.push(('0', value.to_string()));
            }
            Some((tag, value)) => record.push((tag, value.to_string())),
            None if line.trim().is_empty() => flush(&mut record, &mut parsed),
            None => {
                if let Some((_, value)) = record.last_mut() {
                    value.push(' ');
                    value.push_str(line.trim());
                }
            }
        }
    }
    flush(&mut record, &mut parsed);
    parsed
}

fn build(record: &[(char, String)]) -> Option<Item> {
    let first = |tag: char| record.iter().find(|(t, v)| *t == tag && !v.is_empty()).map(|(_, v)| v.clone());
    let all = |tag: char| -> Vec<String> {
        record.iter().filter(|(t, _)| *t == tag).flat_map(|(_, v)| split_list(v)).collect()
    };
    let item_type = item_type(first('0').as_deref().unwrap_or(""));
    let mut item = Item::new(item_type, first('T')?);
    for (tag, role) in
        [('A', CreatorRole::Author), ('E', CreatorRole::Editor), ('Y', CreatorRole::Translator)]
    {
        for name in all(tag) {
            // CNKI ends lists with a stray comma or semicolon.
            let name = name.trim_end_matches([',', '，']).trim();
            if name.is_empty() {
                continue;
            }
            let mut creator = Creator::parse(name);
            creator.role = role;
            item.creators.push(creator);
        }
    }
    item.container_title = first('J').or_else(|| first('B'));
    item.year = first('D').as_deref().and_then(year_in);
    item.date = first('8')
        .as_deref()
        .and_then(iso_date)
        .or_else(|| first('D').as_deref().and_then(iso_date))
        .or_else(|| item.year.map(|y| y.to_string()));
    item.volume = first('V');
    item.issue = first('N');
    item.pages = first('P').map(|p| p.replace(['–', '—'], "-"));
    item.doi = first('R').and_then(|d| Doi::parse(&d).ok());
    if let Some(sn) = first('@') {
        if matches!(item_type, ItemType::Book | ItemType::BookChapter) {
            item.isbn = Some(sn);
        } else {
            item.issn = Some(sn);
        }
    }
    item.publisher = first('I');
    item.place = first('C');
    item.url = first('U');
    item.abstract_note = first('X');
    item.keywords = all('K');
    item.language = first('G');
    item.edition = first('7');
    if item_type == ItemType::Thesis {
        // Degree type (硕士 / 博士 / PhD) goes where GB/T 7714 shows it.
        item.number = first('9');
    }
    Some(item)
}

pub(crate) fn write(items: &[Item]) -> String {
    let mut out = String::new();
    for item in items {
        let mut line = |tag: char, value: &str| {
            if !value.trim().is_empty() {
                out.push('%');
                out.push(tag);
                out.push(' ');
                out.push_str(&value.replace(['\r', '\n'], " "));
                out.push_str("\r\n");
            }
        };
        line('0', type_name(item.item_type));
        line('T', &item.title);
        for creator in &item.creators {
            let tag = match creator.role {
                CreatorRole::Author | CreatorRole::Contributor => 'A',
                CreatorRole::Editor => 'E',
                CreatorRole::Translator => 'Y',
            };
            line(tag, &inverted(creator));
        }
        let container_tag = if item.item_type == ItemType::JournalArticle { 'J' } else { 'B' };
        line(container_tag, item.container_title.as_deref().unwrap_or(""));
        line('D', &item.year.map(|y| y.to_string()).unwrap_or_default());
        line('8', item.date.as_deref().filter(|d| d.len() > 4).unwrap_or(""));
        line('V', item.volume.as_deref().unwrap_or(""));
        line('N', item.issue.as_deref().unwrap_or(""));
        line('P', item.pages.as_deref().unwrap_or(""));
        line('I', item.publisher.as_deref().unwrap_or(""));
        line('C', item.place.as_deref().unwrap_or(""));
        line('7', item.edition.as_deref().unwrap_or(""));
        line('@', item.isbn.as_deref().or(item.issn.as_deref()).unwrap_or(""));
        line('R', &item.doi.as_ref().map(|d| d.to_string()).unwrap_or_default());
        line('U', item.url.as_deref().unwrap_or(""));
        line('X', item.abstract_note.as_deref().unwrap_or(""));
        line('K', &item.keywords.join(";"));
        line('G', item.language.as_deref().unwrap_or(""));
        if item.item_type == ItemType::Thesis {
            line('9', item.number.as_deref().unwrap_or(""));
        }
        out.push_str("\r\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape of a CNKI "EndNote" export.
    const CNKI: &str = "%0 Journal Article\r\n\
%A 张三;李四;王五;\r\n\
%+ 清华大学计算机系;\r\n\
%T 基于深度学习的文献分类方法\r\n\
%J 计算机学报\r\n\
%D 2021\r\n\
%V 44\r\n\
%N 03\r\n\
%K 深度学习;文献分类;注意力机制\r\n\
%X 本文提出一种基于深度学习的文献分类方法，\r\n\
在多个数据集上取得了较好效果。\r\n\
%P 512-525\r\n\
%@ 0254-4164\r\n\
%L 11-1826/TP\r\n\
%W CNKI\r\n\
\r\n\
%0 Thesis\r\n\
%A 赵六\r\n\
%T 文献管理系统的设计与实现\r\n\
%I 北京大学\r\n\
%9 硕士\r\n\
%D 2020\r\n\
%W CNKI\r\n";

    #[test]
    fn reads_cnki_exports() {
        let parsed = parse(CNKI);
        assert_eq!(parsed.items.len(), 2);
        let a = &parsed.items[0];
        assert_eq!(a.item_type, ItemType::JournalArticle);
        assert_eq!(
            a.creators,
            vec![Creator::literal("张三"), Creator::literal("李四"), Creator::literal("王五")]
        );
        assert_eq!(a.container_title.as_deref(), Some("计算机学报"));
        assert_eq!((a.year, a.volume.as_deref(), a.issue.as_deref()), (Some(2021), Some("44"), Some("03")));
        assert_eq!(a.keywords, ["深度学习", "文献分类", "注意力机制"]);
        assert_eq!(a.issn.as_deref(), Some("0254-4164"));
        assert!(a.abstract_note.as_deref().unwrap().ends_with("取得了较好效果。"));
        let thesis = &parsed.items[1];
        assert_eq!(thesis.item_type, ItemType::Thesis);
        assert_eq!(thesis.publisher.as_deref(), Some("北京大学"));
        assert_eq!(thesis.number.as_deref(), Some("硕士"));
    }

    #[test]
    fn round_trips() {
        let items = parse(CNKI).items;
        let again = parse(&write(&items)).items;
        assert_eq!(again.len(), 2);
        for (a, b) in items.iter().zip(&again) {
            assert_eq!((&a.title, &a.creators, &a.keywords), (&b.title, &b.creators, &b.keywords));
            assert_eq!(
                (&a.year, &a.pages, &a.publisher, &a.number),
                (&b.year, &b.pages, &b.publisher, &b.number)
            );
        }
    }

    #[test]
    fn reads_english_names() {
        let parsed = parse(
            "%0 Book\n%A Cormen, Thomas H.\n%A Leiserson, Charles E.\n%T Introduction to Algorithms\n%I MIT Press\n%C Cambridge, MA\n%D 2009\n%@ 978-0-262-03384-8\n",
        );
        let book = &parsed.items[0];
        assert_eq!(book.creators[0], Creator::person("Cormen", "Thomas H."));
        assert_eq!(book.isbn.as_deref(), Some("978-0-262-03384-8"));
        assert_eq!(book.place.as_deref(), Some("Cambridge, MA"));
    }
}
