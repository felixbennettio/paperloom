//! A cell's style as draw.io writes it: `rounded=1;fillColor=#dae8fc;` or
//! `ellipse;whiteSpace=wrap;`, a bare word naming a shape or a named style.

/// Keys and values in order; a bare word has an empty key.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Style(Vec<(String, String)>);

impl Style {
    pub fn parse(text: &str) -> Style {
        Style(
            text.split(';')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(|part| match part.split_once('=') {
                    Some((key, value)) => (key.trim().to_string(), value.trim().to_string()),
                    None => (String::new(), part.to_string()),
                })
                .collect(),
        )
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.0.iter().rev().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
    }

    /// A number, or `default` when it is missing or not one.
    pub fn number(&self, key: &str, default: f64) -> f64 {
        self.get(key).and_then(|v| v.parse().ok()).unwrap_or(default)
    }

    /// Whether a flag is on (`1`), or `default` when it is missing.
    pub fn flag(&self, key: &str, default: bool) -> bool {
        self.get(key).map_or(default, |v| v == "1" || v == "true")
    }

    /// Whether a bare word is there: `ellipse`, `text`, `rhombus`…
    pub fn has(&self, word: &str) -> bool {
        self.0.iter().any(|(k, v)| k.is_empty() && v == word)
    }

    /// Sets a key (where it was, or at the end); an empty value removes it.
    pub fn set(&mut self, key: &str, value: &str) {
        if value.is_empty() {
            self.0.retain(|(k, _)| k != key);
        } else if let Some(entry) = self.0.iter_mut().find(|(k, _)| k == key) {
            entry.1 = value.to_string();
        } else {
            self.0.push((key.to_string(), value.to_string()));
        }
    }

    /// The shape it draws: its `shape`, or a bare word naming one.
    pub fn shape(&self) -> &str {
        if let Some(shape) = self.get("shape") {
            return shape;
        }
        const SHAPES: [&str; 11] = [
            "ellipse",
            "rhombus",
            "triangle",
            "text",
            "swimlane",
            "line",
            "label",
            "image",
            "cylinder",
            "hexagon",
            "doubleEllipse",
        ];
        self.0
            .iter()
            .find(|(k, v)| k.is_empty() && SHAPES.contains(&v.as_str()))
            .map_or("rectangle", |(_, v)| v.as_str())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Display for Style {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (key, value) in &self.0 {
            if key.is_empty() {
                write!(f, "{value};")?;
            } else {
                write!(f, "{key}={value};")?;
            }
        }
        Ok(())
    }
}
