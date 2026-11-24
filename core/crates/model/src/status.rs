use serde::{Deserialize, Serialize};

/// How far someone has got with an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReadStatus {
    #[default]
    Unread,
    Reading,
    Read,
}

impl ReadStatus {
    pub const ALL: [ReadStatus; 3] = [ReadStatus::Unread, ReadStatus::Reading, ReadStatus::Read];

    pub fn as_str(self) -> &'static str {
        match self {
            ReadStatus::Unread => "unread",
            ReadStatus::Reading => "reading",
            ReadStatus::Read => "read",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == value)
    }
}

/// A label people put on items. `color` is `#rrggbb`, or none for a plain
/// tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tag {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_round_trip() {
        for status in ReadStatus::ALL {
            assert_eq!(ReadStatus::parse(status.as_str()), Some(status));
        }
        assert_eq!(ReadStatus::parse("skimmed"), None);
    }
}
