use std::fmt;

use serde::{Deserialize, Serialize};

macro_rules! id_type {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// A new, time-ordered identifier (UUID v7).
            pub fn new() -> Self {
                Self(uuid::Uuid::now_v7().to_string())
            }

            /// Wraps an identifier read back from storage or another device.
            pub fn from_string(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

id_type!(
    /// Identifies an item (a paper, book, thesis…) across devices.
    ItemId
);
id_type!(
    /// Identifies a collection across devices.
    CollectionId
);
id_type!(
    /// Identifies a file attached to an item.
    AttachmentId
);
id_type!(
    /// Identifies a highlight, note or other mark on an attachment.
    AnnotationId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_ids_are_unique_and_ordered() {
        let a = ItemId::new();
        let b = ItemId::new();
        assert_ne!(a, b);
        assert!(a < b, "UUID v7 sorts by creation time");
        assert_eq!(a.as_str().len(), 36);
    }

    #[test]
    fn ids_serialize_as_plain_strings() {
        let id = CollectionId::from_string("abc");
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"abc\"");
    }
}
