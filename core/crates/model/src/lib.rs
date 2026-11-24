//! Paperloom's data model.
//!
//! Items follow CSL-JSON closely enough to feed a citation processor, with
//! the extras Chinese literature needs (unsplit names, GB/T 7714 type codes).

mod annotation;
mod attachment;
mod collection;
mod creator;
pub mod csl;
mod doi;
mod id;
mod item;
mod status;

pub use annotation::{Annotation, AnnotationKind, Point, Rect, is_color};
pub use attachment::{Attachment, content_type_for};
pub use collection::Collection;
pub use creator::{Creator, CreatorRole, contains_cjk};
pub use doi::{Doi, InvalidDoi};
pub use id::{AnnotationId, AttachmentId, CollectionId, ItemId};
pub use item::{Item, ItemType};
pub use status::{ReadStatus, Tag};
