use serde::{Deserialize, Serialize};

use crate::CollectionId;

/// A folder in the library tree. An item can sit in any number of
/// collections; collections nest through `parent_id`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Collection {
    pub id: CollectionId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<CollectionId>,
    /// Order among siblings.
    #[serde(default)]
    pub position: i64,
}

impl Collection {
    pub fn new(name: impl Into<String>, parent_id: Option<CollectionId>) -> Self {
        Self { id: CollectionId::new(), name: name.into(), parent_id, position: 0 }
    }
}
