use serde::{Deserialize, Serialize};

/// What a creator did for an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CreatorRole {
    #[default]
    Author,
    Editor,
    Translator,
    Contributor,
}

/// A person or organisation credited on an item.
///
/// Western names are split into family and given parts; Chinese, Japanese
/// and Korean names, and organisations, stay whole in `literal`, the way
/// GB/T 7714 prints them.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Creator {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub family: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub given: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub literal: String,
    #[serde(default)]
    pub role: CreatorRole,
}

impl Creator {
    pub fn person(family: impl Into<String>, given: impl Into<String>) -> Self {
        Self { family: family.into(), given: given.into(), ..Self::default() }
    }

    pub fn literal(name: impl Into<String>) -> Self {
        Self { literal: name.into(), ..Self::default() }
    }

    /// Reads a name as people write it in a form: `Family, Given`,
    /// `Given Family`, or a CJK name kept whole.
    pub fn parse(name: &str) -> Self {
        let name = name.trim();
        if name.is_empty() {
            return Self::default();
        }
        if contains_cjk(name) {
            return Self::literal(name);
        }
        if let Some((family, given)) = name.split_once(',') {
            return Self::person(family.trim(), given.trim());
        }
        match name.rsplit_once(char::is_whitespace) {
            Some((given, family)) => Self::person(family.trim(), given.trim()),
            None => Self::person(name, ""),
        }
    }

    /// The name as shown in lists: `Given Family`, or the literal name.
    pub fn display_name(&self) -> String {
        if !self.literal.is_empty() {
            return self.literal.clone();
        }
        match (self.given.is_empty(), self.family.is_empty()) {
            (true, _) => self.family.clone(),
            (false, true) => self.given.clone(),
            (false, false) => format!("{} {}", self.given, self.family),
        }
    }

    /// The part used in short references and sorting: the family name, or
    /// the literal name.
    pub fn short_name(&self) -> &str {
        if self.literal.is_empty() { &self.family } else { &self.literal }
    }

    pub fn is_cjk(&self) -> bool {
        contains_cjk(self.short_name())
    }
}

/// True if the text contains Chinese, Japanese or Korean characters.
pub fn contains_cjk(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(c,
            '\u{3040}'..='\u{30FF}'     // Hiragana, Katakana
            | '\u{3400}'..='\u{4DBF}'   // CJK Extension A
            | '\u{4E00}'..='\u{9FFF}'   // CJK Unified Ideographs
            | '\u{AC00}'..='\u{D7AF}'   // Hangul syllables
            | '\u{F900}'..='\u{FAFF}'   // CJK Compatibility Ideographs
            | '\u{20000}'..='\u{2A6DF}' // CJK Extension B
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_name_forms() {
        assert_eq!(Creator::parse("Curie, Marie"), Creator::person("Curie", "Marie"));
        assert_eq!(Creator::parse("Marie Curie"), Creator::person("Curie", "Marie"));
        assert_eq!(Creator::parse("Ludwig van Beethoven"), Creator::person("Beethoven", "Ludwig van"));
        assert_eq!(Creator::parse("Plato"), Creator::person("Plato", ""));
        assert_eq!(Creator::parse("  张三 "), Creator::literal("张三"));
        assert_eq!(Creator::parse("山田 太郎"), Creator::literal("山田 太郎"));
        assert_eq!(Creator::parse(""), Creator::default());
    }

    #[test]
    fn displays_and_shortens_names() {
        let curie = Creator::person("Curie", "Marie");
        assert_eq!(curie.display_name(), "Marie Curie");
        assert_eq!(curie.short_name(), "Curie");
        assert!(!curie.is_cjk());
        let zhang = Creator::literal("张三");
        assert_eq!(zhang.display_name(), "张三");
        assert_eq!(zhang.short_name(), "张三");
        assert!(zhang.is_cjk());
    }

    #[test]
    fn serializes_compactly() {
        let json = serde_json::to_string(&Creator::literal("WHO")).unwrap();
        assert_eq!(json, r#"{"literal":"WHO","role":"author"}"#);
    }
}
