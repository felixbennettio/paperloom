//! CSL-JSON: an array of items (a single object is accepted too).

use paperloom_model::{Item, csl};
use serde_json::Value;

use crate::Parsed;
use crate::text::citation_keys;

pub(crate) fn parse(text: &str) -> Parsed {
    let mut parsed = Parsed::default();
    let values = match serde_json::from_str::<Value>(text) {
        Ok(Value::Array(values)) => values,
        Ok(value @ Value::Object(_)) => vec![value],
        _ => {
            parsed.skipped = 1;
            return parsed;
        }
    };
    for value in &values {
        match csl::item_from_csl(value) {
            Some(item) => parsed.items.push(item),
            None => parsed.skipped += 1,
        }
    }
    parsed
}

pub(crate) fn write(items: &[Item]) -> String {
    let keys = citation_keys(items);
    let values: Vec<Value> = items.iter().zip(&keys).map(|(item, key)| csl::item_to_csl(item, key)).collect();
    serde_json::to_string_pretty(&values).unwrap_or_else(|_| "[]".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_arrays_and_single_items() {
        let parsed =
            parse(r#"[{"type":"book","title":"A"},{"type":"book"},{"type":"article-journal","title":"B"}]"#);
        assert_eq!(parsed.items.len(), 2);
        assert_eq!(parsed.skipped, 1);
        assert_eq!(parse(r#"{"type":"book","title":"Only"}"#).items[0].title, "Only");
        assert_eq!(parse("not json").skipped, 1);
    }

    #[test]
    fn round_trips() {
        let items = parse(r#"[{"type":"article-journal","title":"Thermometry","author":[{"family":"Kucsko","given":"G."}],"issued":{"date-parts":[[2013,7,31]]}}]"#).items;
        let text = write(&items);
        assert!(text.contains(r#""id": "kucsko2013thermometry""#), "{text}");
        let again = parse(&text).items;
        assert_eq!(
            (&again[0].title, &again[0].creators, &again[0].date),
            (&items[0].title, &items[0].creators, &items[0].date)
        );
    }
}
