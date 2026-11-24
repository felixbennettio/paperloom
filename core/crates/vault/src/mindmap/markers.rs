//! Topic markers, as XMind puts icons on topics: priorities, task progress,
//! a flag, a star and symbols. A note keeps them as the emoji they look
//! like at the start of a topic's text, so any editor shows them; XMind
//! files carry them as marker IDs.

/// A marker: XMind's ID for it, the emoji a note keeps, and its group (a
/// topic carries one marker of a group at most).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Marker {
    pub id: &'static str,
    pub emoji: &'static str,
    pub group: &'static str,
}

const fn marker(id: &'static str, emoji: &'static str, group: &'static str) -> Marker {
    Marker { id, emoji, group }
}

/// The markers, in the order a topic shows them.
pub const MARKERS: [Marker; 16] = [
    marker("priority-1", "1️⃣", "priority"),
    marker("priority-2", "2️⃣", "priority"),
    marker("priority-3", "3️⃣", "priority"),
    marker("priority-4", "4️⃣", "priority"),
    marker("priority-5", "5️⃣", "priority"),
    marker("priority-6", "6️⃣", "priority"),
    marker("priority-7", "7️⃣", "priority"),
    marker("priority-8", "8️⃣", "priority"),
    marker("priority-9", "9️⃣", "priority"),
    marker("task-start", "⬜", "task"),
    marker("task-half", "🌓", "task"),
    marker("task-done", "✅", "task"),
    marker("flag-red", "🚩", "flag"),
    marker("star-yellow", "⭐", "star"),
    marker("symbol-question", "❓", "symbol"),
    marker("symbol-exclam", "❗", "symbol"),
];

/// A topic's markers and the text after them.
pub fn split_markers(text: &str) -> (Vec<&'static Marker>, &str) {
    let mut found = Vec::new();
    let mut rest = text.trim_start();
    // The longest emoji first: a keycap is a digit and two more characters.
    while let Some(marker) = MARKERS.iter().find(|m| rest.starts_with(m.emoji)) {
        found.push(marker);
        rest = rest[marker.emoji.len()..].trim_start();
    }
    (found, rest)
}

/// A topic's text with its markers in their order before it.
pub fn with_markers(markers: &[&Marker], text: &str) -> String {
    let mut ordered: Vec<&Marker> = MARKERS.iter().filter(|m| markers.iter().any(|n| n.id == m.id)).collect();
    ordered.dedup_by_key(|m| m.id);
    let mut out: Vec<&str> = ordered.iter().map(|m| m.emoji).collect();
    if !text.is_empty() {
        out.push(text);
    }
    out.join(" ")
}

/// The marker with an XMind ID.
pub fn marker_named(id: &str) -> Option<&'static Marker> {
    MARKERS.iter().find(|m| m.id == id)
}

/// A topic's text with a marker put on, or taken off when it has it; it
/// takes the place of another of its group.
pub fn toggle_marker(text: &str, id: &str) -> Result<String, String> {
    let marker = marker_named(id).ok_or_else(|| format!("“{id}” is not a marker."))?;
    let (mut markers, rest) = split_markers(text);
    if markers.iter().any(|m| m.id == marker.id) {
        markers.retain(|m| m.id != marker.id);
    } else {
        markers.retain(|m| m.group != marker.group);
        markers.push(marker);
    }
    Ok(with_markers(&markers, rest))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn puts_markers_on_and_takes_them_off() {
        assert_eq!(toggle_marker("Task", "priority-1").unwrap(), "1️⃣ Task");
        assert_eq!(toggle_marker("1️⃣ Task", "priority-2").unwrap(), "2️⃣ Task", "one priority at a time");
        assert_eq!(toggle_marker("2️⃣ Task", "task-done").unwrap(), "2️⃣ ✅ Task");
        assert_eq!(toggle_marker("✅ 2️⃣ Task", "task-done").unwrap(), "2️⃣ Task", "in order, and off again");
        assert!(toggle_marker("Task", "nonsense").is_err());
        let (markers, rest) = split_markers("🚩⭐ Look here");
        assert_eq!(
            (markers.iter().map(|m| m.id).collect::<Vec<_>>(), rest),
            (vec!["flag-red", "star-yellow"], "Look here")
        );
        assert_eq!(with_markers(&[], "Plain"), "Plain");
    }
}
