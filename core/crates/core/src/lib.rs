//! The API every Paperloom app calls.
//!
//! The Apple apps reach it through UniFFI-generated Swift; the Tauri app
//! links it as a Rust crate and wraps the same calls in Tauri commands. Keep
//! the surface small and made of plain records so both stay easy to use.

mod account;
mod ai;
mod annotations;
mod backup;
mod canvas;
mod cite;
mod citeapi;
mod citedby;
mod cloud;
mod connector;
mod diagram;
mod docx;
mod drawing;
mod duplicates;
mod endnote;
mod feeds;
mod files;
mod findpdf;
mod fulltext;
mod html;
mod latex;
mod lookup;
mod messages;
mod mindmap;
mod notelinks;
mod organize;
mod pdf;
mod pdfedit;
mod pdfform;
mod picture;
mod plugins;
mod references;
mod related;
mod search;
mod searches;
mod semantic;
mod storage;
mod styles;
mod translators;
mod unzip;
mod vault;
mod webpage;
mod wordcite;
mod wpsaddin;
mod zotero;
mod zotero_plugins;

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use paperloom_model::{AttachmentId, Collection, CollectionId, Creator, Doi, Item, ItemId, ItemType};
use paperloom_store as store;
use serde::{Deserialize, Serialize};

pub use account::{AccountService, AccountSession, AccountTokens, AccountUser, SignInRequest};
pub use ai::{
    AiAnswer, AiLibraryAnswer, AiMindMap, AiModel, AiProvider, AiSource, ai_mindmap, ai_models, ai_transform,
};
pub use annotations::{AnnotationInfo, AnnotationRect, NewAnnotation};
pub use backup::{BackupReport, RestoreReport, finish_restore};
pub use canvas::{CanvasArrow, CanvasBoard, CanvasCard};
pub use cite::{CitationOutput, CitationStyle, citation_styles};
pub use citeapi::{CitationCluster, CiteCandidate, CiteMarker, CitedText, CitedWork};
pub use cloud::{SyncReport, SyncStatus};
pub use connector::{CONNECTOR_PORT, ConnectedWriter, ConnectorEvents, connector_port};
pub use diagram::{
    DiagramChange, DiagramChanged, DiagramDrawn, DiagramPoint, DiagramShape, DiagramView, diagram_change,
    diagram_page_svg, diagram_shapes, diagram_view,
};
pub use drawing::{DrawingItem, DrawingPage, DrawingPoint, drawing_erase};
pub use duplicates::DuplicateGroup;
pub use endnote::{EndNoteImport, find_endnote_data_folder};
pub use feeds::{FeedInfo, FeedItem};
pub use files::AttachmentInfo;
pub use findpdf::FoundPdf;
pub use fulltext::FullTextHit;
pub use lookup::{ItemDetail, lookup_identifier, recognize_identifier};
pub use messages::{localize, localize_message};
pub use mindmap::{
    MindMapChange, MindMapEdit, MindMapLayout, MindMapTheme, MindMarker, MindTopic, export_mindmap,
    is_mindmap, mindmap_edit, mindmap_layout, mindmap_markers, mindmap_svg, mindmap_themes, new_mindmap_text,
};
pub use notelinks::{AnnotationLocation, CitationCompletion};
pub use organize::{ItemFilter, TagInfo};
pub use pdf::PdfImport;
pub use pdfedit::{PageEdit, RecognizedLine, RecognizedPage};
pub use pdfform::{PdfFieldValue, PdfFormField, PdfWidget};
pub use picture::picture_pdf;
pub use plugins::{
    CommunityPlugin, ObsidianPlugin, ObsidianPluginCode, PluginHeader, PluginResponse, community_plugins,
    plugin_fetch,
};
pub use references::{ImportReport, ReferenceFormat, reference_formats};
pub use search::{
    CaptureSite, SearchHit, SearchPage, SearchSource, SruCatalogue, capture_script, capture_sites,
    catalogue_presets, search_sources,
};
pub use searches::{SavedSearch, SearchCondition, SearchField, search_fields};
pub use semantic::AiSemanticStatus;
pub use storage::{S3Place, SeafilePlace};
pub use styles::StyleListing;
pub use translators::{TranslatorResponse, TranslatorStatus, WebTranslator, translator_request};
pub use vault::{
    DiffLine, GraphEdge, GraphNodeInfo, LinkCompletion, LinkSuggestion, LocalTime, NoteGraph, NoteHeading,
    NoteInfo, NoteLinkInfo, NoteMention, NoteOutline, NoteProperty, NoteVersion, PaperVault, VaultSettings,
    VaultTag,
};
pub use wordcite::WordCitations;
pub use wpsaddin::{install_wps_addin, installed_wps_addin, remove_wps_addin, wps_addin_folder};
pub use zotero::ZoteroImport;
pub use zotero_plugins::{PluginFile, ZoteroPlugin, ZoteroPluginCode};

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum CoreError {
    #[error("{message}")]
    Storage { message: String },
    #[error("{message}")]
    InvalidInput { message: String },
    #[error("{message}")]
    Duplicate { message: String, existing_id: String },
    #[error("{message}")]
    NotFound { message: String },
    /// The account service could not be reached.
    #[error("{message}")]
    Network { message: String },
    /// Signing in failed or the account service refused a request.
    #[error("{message}")]
    Account { message: String },
    /// The person closed the sign-in page.
    #[error("{message}")]
    Cancelled { message: String },
    /// Looking up an identifier failed (unknown, or an unexpected reply).
    #[error("{message}")]
    Lookup { message: String },
    /// The AI provider refused or failed (bad key, unknown model…).
    #[error("{message}")]
    Ai { message: String },
}

impl CoreError {
    /// "item 0193… not found", as the store says it: `what` names the thing.
    pub(crate) fn not_found(what: impl std::fmt::Display) -> Self {
        CoreError::NotFound { message: format!("{what} not found") }
    }
}

impl From<store::Error> for CoreError {
    fn from(err: store::Error) -> Self {
        let message = err.to_string();
        match err {
            store::Error::DuplicateDoi { existing, .. } => {
                CoreError::Duplicate { message, existing_id: existing.to_string() }
            }
            store::Error::NotFound(_) => CoreError::NotFound { message },
            _ => CoreError::Storage { message },
        }
    }
}

/// An item as lists and detail panes show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct ItemSummary {
    pub id: String,
    pub item_type: String,
    pub title: String,
    /// Short author label: `Curie et al.`, `张三 等`.
    pub authors: String,
    pub container_title: Option<String>,
    pub year: Option<i32>,
    pub doi: Option<String>,
    pub starred: bool,
    /// `unread`, `reading` or `read`.
    pub read_status: String,
    pub tags: Vec<String>,
    pub has_pdf: bool,
    /// Milliseconds since 1970; 0 for an item not in the library.
    pub date_added: i64,
    /// How many works cite it, as OpenAlex last counted them.
    pub cited_by: Option<u32>,
}

impl From<&Item> for ItemSummary {
    /// The summary of an item just added: no stars or tags yet.
    fn from(item: &Item) -> Self {
        Self {
            id: item.id.to_string(),
            item_type: item.item_type.as_str().to_string(),
            title: item.title.clone(),
            authors: item.authors_short(),
            container_title: item.container_title.clone(),
            year: item.year,
            doi: item.doi.as_ref().map(|d| d.to_string()),
            starred: false,
            read_status: paperloom_model::ReadStatus::Unread.as_str().to_string(),
            tags: Vec::new(),
            has_pdf: false,
            date_added: 0,
            cited_by: item.cited_by,
        }
    }
}

impl From<&store::Entry> for ItemSummary {
    fn from(entry: &store::Entry) -> Self {
        Self {
            starred: entry.starred,
            read_status: entry.read_status.as_str().to_string(),
            tags: entry.tags.clone(),
            has_pdf: entry.pdf_count > 0,
            date_added: entry.date_added,
            ..Self::from(&entry.item)
        }
    }
}

/// What a person fills in to add an item by hand.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase", default)]
pub struct NewItem {
    /// One of [`item_types`]; empty means journal article.
    pub item_type: String,
    pub title: String,
    /// Names as typed: `Curie, Marie`, `Marie Curie` or `张三`.
    pub authors: Vec<String>,
    pub container_title: Option<String>,
    pub year: Option<i32>,
    /// A DOI in any common spelling (URL, `doi:` label…).
    pub doi: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct CollectionInfo {
    pub id: String,
    pub name: String,
    pub parent_id: Option<String>,
    pub position: i64,
}

impl From<Collection> for CollectionInfo {
    fn from(c: Collection) -> Self {
        Self {
            id: c.id.to_string(),
            name: c.name,
            parent_id: c.parent_id.map(|p| p.to_string()),
            position: c.position,
        }
    }
}

/// A library database. Safe to share between threads.
#[derive(uniffi::Object)]
pub struct PaperLibrary {
    inner: Mutex<store::Library>,
    kept: Mutex<search::KeptResults>,
    /// Attached files; none for a library kept only in memory.
    files: Option<files::FileStore>,
    /// Full-text index (in memory for a library kept in memory).
    fulltext: paperloom_fulltext::FullText,
    /// Opened only when semantic search is used.
    vectors: Mutex<Option<rusqlite::Connection>>,
    vectors_path: Option<std::path::PathBuf>,
    /// Where sync keeps its state; none for a library kept in memory.
    sync_dir: Option<std::path::PathBuf>,
    /// Added citation styles; none for a library kept in memory.
    styles_dir: Option<std::path::PathBuf>,
    /// Installed Zotero plugins; none for a library kept in memory.
    plugins_dir: Option<std::path::PathBuf>,
    translators_lock: Mutex<()>,
    /// One sync at a time.
    syncing: Mutex<()>,
    /// Seals what sync sends, with a key from the person's passphrase.
    sealer: Mutex<Option<Arc<paperloom_sync::seal::Sealer>>>,
    /// Writing plugins the person allowed to read the library.
    writers: Mutex<connector::Writers>,
    /// A writing plugin's pairing question is waiting for the person.
    pairing: std::sync::atomic::AtomicBool,
}

#[uniffi::export]
impl PaperLibrary {
    /// Opens (or creates) the library database at `path`.
    #[uniffi::constructor]
    pub fn new(path: String) -> Result<Arc<Self>, CoreError> {
        let files = files::FileStore::beside(Path::new(&path));
        let store = store::Library::open(&path)?;
        let folder =
            Path::new(&path).parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
        // Another copy of the app may hold the index; then this one keeps
        // its own in memory.
        let (fulltext, fresh) = match paperloom_fulltext::FullText::open(&folder.join("index")) {
            Ok(opened) => opened,
            Err(_) => (paperloom_fulltext::FullText::in_memory()?, true),
        };
        let mut library = Self::with_store(store, Some(files), fulltext);
        library.sync_dir = Some(folder.join("sync"));
        library.styles_dir = Some(folder.join("styles"));
        library.plugins_dir = Some(folder.join("plugins"));
        library.vectors_path = Some(folder.join("vectors.sqlite"));
        library.writers = Mutex::new(connector::Writers::beside(folder));
        styles::load_styles(&folder.join("styles"));
        if fresh {
            library.rebuild_index()?;
        }
        Ok(Arc::new(library))
    }

    /// A library kept only in memory, for previews and tests.
    #[uniffi::constructor]
    pub fn in_memory() -> Result<Arc<Self>, CoreError> {
        Ok(Arc::new(Self::with_store(
            store::Library::open_in_memory()?,
            None,
            paperloom_fulltext::FullText::in_memory()?,
        )))
    }

    pub fn add_item(&self, new_item: NewItem) -> Result<ItemSummary, CoreError> {
        let item = build_item(new_item)?;
        self.lock()?.add_item(&item)?;
        self.reindex_items(std::slice::from_ref(&item.id));
        self.stored_summary(&item)
    }

    /// All items not in the trash, most recently added first.
    pub fn items(&self) -> Result<Vec<ItemSummary>, CoreError> {
        self.list_items(ItemFilter::default())
    }

    pub fn items_in_collection(&self, collection_id: String) -> Result<Vec<ItemSummary>, CoreError> {
        self.list_items(ItemFilter { collection_id: Some(collection_id), ..ItemFilter::default() })
    }

    /// The items a filter selects (collection, tag, starred, reading status,
    /// or the trash), most recently added first.
    pub fn list_items(&self, filter: ItemFilter) -> Result<Vec<ItemSummary>, CoreError> {
        organize::list_items(self, filter)
    }

    /// Saves edited details. Tags, stars and reading status in `detail` are
    /// ignored; they have their own calls.
    pub fn update_item(&self, detail: ItemDetail) -> Result<ItemDetail, CoreError> {
        organize::update_item(self, detail)
    }

    pub fn set_starred(&self, item_id: String, starred: bool) -> Result<(), CoreError> {
        Ok(self.lock()?.set_starred(&ItemId::from_string(item_id), starred)?)
    }

    /// `status` is `unread`, `reading` or `read`.
    pub fn set_read_status(&self, item_id: String, status: String) -> Result<(), CoreError> {
        organize::set_read_status(self, item_id, &status)
    }

    /// Every tag, with how many items carry it.
    pub fn tags(&self) -> Result<Vec<TagInfo>, CoreError> {
        organize::tags(self)
    }

    /// Replaces an item's tags; new names become new tags.
    pub fn set_item_tags(&self, item_id: String, tags: Vec<String>) -> Result<(), CoreError> {
        let id = ItemId::from_string(item_id);
        self.lock()?.set_item_tags(&id, &tags)?;
        self.reindex_items(std::slice::from_ref(&id));
        Ok(())
    }

    /// `color` is `#rrggbb`, or none for a plain tag.
    pub fn set_tag_color(&self, name: String, color: Option<String>) -> Result<(), CoreError> {
        organize::set_tag_color(self, &name, color)
    }

    /// Renaming onto an existing tag merges the two.
    pub fn rename_tag(&self, name: String, new_name: String) -> Result<(), CoreError> {
        let tagged = self.tagged(&name)?;
        self.lock()?.rename_tag(&name, &new_name)?;
        self.reindex_items(&tagged);
        Ok(())
    }

    pub fn delete_tag(&self, name: String) -> Result<(), CoreError> {
        let tagged = self.tagged(&name)?;
        self.lock()?.delete_tag(&name)?;
        self.reindex_items(&tagged);
        Ok(())
    }

    /// Takes an item out of the trash.
    pub fn restore_item(&self, item_id: String) -> Result<(), CoreError> {
        Ok(self.lock()?.restore_item(&ItemId::from_string(item_id))?)
    }

    /// Deletes an item for good, with its files and annotations.
    pub fn delete_item(&self, item_id: String) -> Result<(), CoreError> {
        organize::delete_items(self, &[ItemId::from_string(item_id)]).map(|_| ())
    }

    /// Deletes everything in the trash for good; returns how many items.
    pub fn empty_trash(&self) -> Result<u32, CoreError> {
        organize::empty_trash(self)
    }

    /// Every field of an item, for its detail page.
    pub fn item_detail(&self, item_id: String) -> Result<Option<ItemDetail>, CoreError> {
        Ok(self.lock()?.entry(&ItemId::from_string(item_id))?.as_ref().map(ItemDetail::from))
    }

    /// Looks up a DOI, arXiv ID, PubMed ID or ISBN and adds the item with
    /// all its details (to the collection too, when given). Talks to the
    /// network: call it off the main thread.
    pub fn add_by_identifier(
        &self,
        input: String,
        collection_id: Option<String>,
    ) -> Result<ItemSummary, CoreError> {
        let item = lookup::resolve(&input)?;
        {
            let mut library = self.lock()?;
            library.add_item(&item)?;
            if let Some(collection) = collection_id {
                library.add_to_collection(&CollectionId::from_string(collection), &item.id)?;
            }
        }
        self.reindex_items(std::slice::from_ref(&item.id));
        self.stored_summary(&item)
    }

    /// The reference list entries and the in-text citation for some items,
    /// in their given order, in a citation style (see `citation_styles`).
    pub fn format_citations(
        &self,
        item_ids: Vec<String>,
        style_id: String,
        html: bool,
    ) -> Result<CitationOutput, CoreError> {
        let items = {
            let library = self.lock()?;
            let mut items = Vec::with_capacity(item_ids.len());
            for id in item_ids {
                let id = ItemId::from_string(id);
                items.push(library.item(&id)?.ok_or_else(|| CoreError::not_found(format!("item {id}")))?);
            }
            items
        };
        cite::format(&items, &style_id, html)
    }

    /// Imports a reference file (RIS, EndNote/CNKI, BibTeX, CSL-JSON, PubMed
    /// MEDLINE, Web of Science, Scopus CSV or EndNote XML, found from its
    /// content), into a collection too when given. Items whose DOI
    /// is already in the library are skipped as duplicates.
    pub fn import_references(
        &self,
        text: String,
        collection_id: Option<String>,
    ) -> Result<ImportReport, CoreError> {
        let (format, parsed) = paperloom_formats::parse(&text).ok_or_else(|| CoreError::InvalidInput {
            message: "This is not a reference file Paperloom reads (RIS, EndNote, BibTeX, CSL-JSON, PubMed MEDLINE, Web of Science, Scopus CSV or EndNote XML).".into(),
        })?;
        let collection = collection_id.map(CollectionId::from_string);
        let mut report = ImportReport {
            format: format.id().to_string(),
            skipped: parsed.skipped as u32,
            ..ImportReport::default()
        };
        {
            let mut library = self.lock()?;
            add_all(&mut library, &parsed.items, collection.as_ref(), &mut report)?;
        }
        let added: Vec<ItemId> = report.added_ids.iter().map(|id| ItemId::from_string(id.clone())).collect();
        self.reindex_items(&added);
        Ok(report)
    }

    /// Writes items as a reference file; `format` is one of
    /// `reference_formats()`' ids.
    pub fn export_references(&self, item_ids: Vec<String>, format: String) -> Result<String, CoreError> {
        let format = paperloom_formats::Format::parse_id(&format).ok_or_else(|| CoreError::InvalidInput {
            message: format!("Unknown reference format “{format}”."),
        })?;
        let library = self.lock()?;
        let mut items = Vec::with_capacity(item_ids.len());
        for id in item_ids {
            let id = ItemId::from_string(id);
            items.push(library.item(&id)?.ok_or_else(|| CoreError::not_found(format!("item {id}")))?);
        }
        Ok(paperloom_formats::write(format, &items))
    }

    /// Searches an online database, one of `search_sources()`; `page`
    /// counts from 0, and `api_key` is the person's key for a database that
    /// needs one. Results whose DOI the library has carry that item's ID.
    /// Talks to the network: call it off the main thread.
    pub fn search_online(
        &self,
        source_id: String,
        text: String,
        page: u32,
        api_key: Option<String>,
    ) -> Result<SearchPage, CoreError> {
        search::search_online(self, &source_id, text, page, api_key)
    }

    /// Searches a library catalogue the person added (see
    /// `catalogue_presets()`); `page` counts from 0. Results are kept for
    /// `add_search_results` like those of `search_online`. Talks to the
    /// network: call it off the main thread.
    pub fn search_catalogue(
        &self,
        catalogue: SruCatalogue,
        text: String,
        page: u32,
    ) -> Result<SearchPage, CoreError> {
        search::search_catalogue(self, catalogue, text, page)
    }

    /// Reads the works on a page in the in-app browser, from what
    /// `capture_script()` answered there, keeping them for
    /// `add_search_results` like search results. Works the page names by DOI
    /// alone are looked up: call it off the main thread.
    pub fn read_captured_page(&self, capture: String) -> Result<SearchPage, CoreError> {
        search::read_captured_page(self, &capture)
    }

    /// Reads the works embedded in a shared web page, keeping them for
    /// `add_search_results`. Talks to the network: call it off the main thread.
    pub fn read_web_page(&self, url: String) -> Result<SearchPage, CoreError> {
        webpage::read_web_page(self, &paperloom_http::UreqHttp::new(), &url)
    }

    /// Adds results of recent searches (their `key`s), to a collection too
    /// when given. Results whose DOI is already in the library count as
    /// duplicates.
    pub fn add_search_results(
        &self,
        keys: Vec<String>,
        collection_id: Option<String>,
    ) -> Result<ImportReport, CoreError> {
        search::add_search_results(self, &keys, collection_id)
    }

    /// Adds a PDF: recognises the paper from the first pages' text (a DOI,
    /// an `arXiv:` stamp) or the PDF's own title, adds it with its details,
    /// or finds it already in the library, and keeps the file with it.
    /// Talks to the network: call it off the main thread.
    pub fn import_pdf(
        &self,
        path: String,
        first_pages_text: String,
        pdf_title: Option<String>,
        collection_id: Option<String>,
    ) -> Result<PdfImport, CoreError> {
        pdf::import_pdf(self, &path, &first_pages_text, pdf_title, collection_id)
    }

    /// Copies a file into the library and attaches it to an item. The same
    /// file attached twice is kept once.
    pub fn attach_file(&self, item_id: String, path: String) -> Result<AttachmentInfo, CoreError> {
        let files = self.files()?;
        let mut library = self.lock()?;
        let item = ItemId::from_string(item_id);
        library.item(&item)?.ok_or_else(|| CoreError::not_found(format!("item {item}")))?;
        let attachment = files.attach(&mut library, &item, Path::new(&path))?;
        Ok(files.info(&attachment))
    }

    /// An item's attached files, oldest first.
    pub fn attachments(&self, item_id: String) -> Result<Vec<AttachmentInfo>, CoreError> {
        let Some(files) = &self.files else { return Ok(Vec::new()) };
        let attachments = self.lock()?.attachments(&ItemId::from_string(item_id))?;
        Ok(attachments.iter().map(|a| files.info(a)).collect())
    }

    /// One attached file, if it exists.
    pub fn attachment(&self, attachment_id: String) -> Result<Option<AttachmentInfo>, CoreError> {
        let Some(files) = &self.files else { return Ok(None) };
        let attachment = self.lock()?.attachment(&AttachmentId::from_string(attachment_id))?;
        Ok(attachment.as_ref().map(|a| files.info(a)))
    }

    /// Detaches a file and deletes the library's copy.
    pub fn remove_attachment(&self, attachment_id: String) -> Result<(), CoreError> {
        let files = self.files()?;
        let mut library = self.lock()?;
        let id = AttachmentId::from_string(attachment_id);
        let attachment =
            library.attachment(&id)?.ok_or_else(|| CoreError::not_found(format!("attachment {id}")))?;
        library.remove_attachment(&id)?;
        drop(library);
        self.unindex_attachment(&id);
        files.remove(&attachment)
    }

    /// Keeps the text of an attachment's pages (from 0) for full-text
    /// search. The apps read it with their PDF engines.
    pub fn set_attachment_text(&self, attachment_id: String, pages: Vec<String>) -> Result<(), CoreError> {
        fulltext::set_attachment_text(self, attachment_id, pages)
    }

    /// Whether an attachment's page text is kept (else the apps read it).
    pub fn attachment_has_text(&self, attachment_id: String) -> Result<bool, CoreError> {
        let id = AttachmentId::from_string(attachment_id);
        Ok(!self.lock()?.attachment_pages(&id)?.is_empty())
    }

    /// Searches items, annotations, PDF text and connected vault notes.
    /// `kinds` (`item`, `annotation`, `pdf`, `note`) narrows it; empty
    /// means all. All words must appear; `"phrases"`, `-word` and `OR` work.
    pub fn search_everything(
        &self,
        query: String,
        kinds: Vec<String>,
        limit: u32,
    ) -> Result<Vec<FullTextHit>, CoreError> {
        fulltext::search_everything(self, &query, &kinds, limit)
    }

    /// Rebuilds the full-text index from the library.
    pub fn rebuild_search_index(&self) -> Result<(), CoreError> {
        self.rebuild_index()
    }

    /// An attachment's highlights and notes, by page.
    pub fn annotations(&self, attachment_id: String) -> Result<Vec<AnnotationInfo>, CoreError> {
        annotations::annotations(self, attachment_id)
    }

    pub fn add_annotation(&self, annotation: NewAnnotation) -> Result<AnnotationInfo, CoreError> {
        annotations::add(self, annotation)
    }

    /// Changes a mark's color and comment.
    pub fn update_annotation(
        &self,
        annotation_id: String,
        color: String,
        comment: Option<String>,
    ) -> Result<AnnotationInfo, CoreError> {
        annotations::update(self, annotation_id, color, comment)
    }

    pub fn remove_annotation(&self, annotation_id: String) -> Result<(), CoreError> {
        annotations::remove(self, annotation_id)
    }

    pub fn trash_item(&self, item_id: String) -> Result<(), CoreError> {
        Ok(self.lock()?.trash_item(&ItemId::from_string(item_id))?)
    }

    pub fn create_collection(
        &self,
        name: String,
        parent_id: Option<String>,
    ) -> Result<CollectionInfo, CoreError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(CoreError::InvalidInput { message: "A collection needs a name.".into() });
        }
        let parent = parent_id.map(CollectionId::from_string);
        Ok(self.lock()?.create_collection(name, parent.as_ref())?.into())
    }

    pub fn collections(&self) -> Result<Vec<CollectionInfo>, CoreError> {
        Ok(self.lock()?.collections()?.into_iter().map(CollectionInfo::from).collect())
    }

    pub fn add_to_collection(&self, collection_id: String, item_id: String) -> Result<(), CoreError> {
        Ok(self
            .lock()?
            .add_to_collection(&CollectionId::from_string(collection_id), &ItemId::from_string(item_id))?)
    }
}

impl PaperLibrary {
    fn with_store(
        library: store::Library,
        files: Option<files::FileStore>,
        fulltext: paperloom_fulltext::FullText,
    ) -> Self {
        Self {
            inner: Mutex::new(library),
            kept: Mutex::default(),
            files,
            fulltext,
            vectors: Mutex::new(None),
            vectors_path: None,
            sync_dir: None,
            styles_dir: None,
            plugins_dir: None,
            translators_lock: Mutex::new(()),
            syncing: Mutex::new(()),
            sealer: Mutex::new(None),
            writers: Mutex::default(),
            pairing: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn files(&self) -> Result<&files::FileStore, CoreError> {
        self.files.as_ref().ok_or_else(|| CoreError::Storage {
            message: "This library is kept only in memory and cannot keep files.".into(),
        })
    }

    fn lock(&self) -> Result<MutexGuard<'_, store::Library>, CoreError> {
        self.inner
            .lock()
            .map_err(|_| CoreError::Storage { message: "The library is unavailable after a crash.".into() })
    }

    /// An item's summary as listed (with the date it was added).
    fn stored_summary(&self, item: &Item) -> Result<ItemSummary, CoreError> {
        Ok(self.lock()?.entry(&item.id)?.as_ref().map_or_else(|| ItemSummary::from(item), ItemSummary::from))
    }

    /// Items carrying a tag.
    fn tagged(&self, tag: &str) -> Result<Vec<ItemId>, CoreError> {
        let filter = store::Filter { tag: Some(tag.to_string()), ..store::Filter::default() };
        Ok(self.lock()?.entries(&filter)?.into_iter().map(|e| e.item.id).collect())
    }

    fn kept_results(&self) -> Result<MutexGuard<'_, search::KeptResults>, CoreError> {
        self.kept.lock().map_err(|_| CoreError::Storage {
            message: "Search results are unavailable after a crash.".into(),
        })
    }
}

/// Adds items (to a collection too), counting those whose DOI the library
/// already has as duplicates.
fn add_all(
    library: &mut store::Library,
    items: &[Item],
    collection: Option<&CollectionId>,
    report: &mut ImportReport,
) -> Result<(), CoreError> {
    for item in items {
        match library.add_item(item) {
            Ok(()) => {
                report.added += 1;
                report.added_ids.push(item.id.to_string());
                if let Some(collection) = collection {
                    library.add_to_collection(collection, &item.id)?;
                }
            }
            Err(store::Error::DuplicateDoi { .. }) => report.duplicates += 1,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

fn build_item(new_item: NewItem) -> Result<Item, CoreError> {
    let item_type = if new_item.item_type.is_empty() {
        ItemType::default()
    } else {
        ItemType::parse(&new_item.item_type).ok_or_else(|| CoreError::InvalidInput {
            message: format!("Unknown item type “{}”.", new_item.item_type),
        })?
    };
    let title = new_item.title.trim();
    if title.is_empty() {
        return Err(CoreError::InvalidInput { message: "An item needs a title.".into() });
    }
    let mut item = Item::new(item_type, title);
    item.creators = new_item
        .authors
        .iter()
        .map(|a| Creator::parse(a))
        .filter(|c| !c.short_name().is_empty() || !c.given.is_empty())
        .collect();
    item.container_title = new_item.container_title.map(|t| t.trim().to_string()).filter(|t| !t.is_empty());
    item.year = new_item.year;
    item.doi = match new_item.doi.as_deref().map(str::trim).filter(|d| !d.is_empty()) {
        Some(doi) => Some(Doi::parse(doi).map_err(|e| CoreError::InvalidInput { message: e.to_string() })?),
        None => None,
    };
    Ok(item)
}

/// The version of the shared core, shown in About panels.
#[uniffi::export]
pub fn core_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// The canonical form of a DOI typed or pasted by a person, or nothing if
/// it is not a DOI.
#[uniffi::export]
pub fn normalize_doi(input: String) -> Option<String> {
    Doi::parse(&input).ok().map(String::from)
}

/// Every distinct DOI in a piece of text, in order of appearance.
#[uniffi::export]
pub fn find_dois(text: String) -> Vec<String> {
    Doi::find_in_text(&text).into_iter().map(String::from).collect()
}

/// The item type identifiers [`NewItem::item_type`] accepts.
#[uniffi::export]
pub fn item_types() -> Vec<String> {
    ItemType::ALL.iter().map(|t| t.as_str().to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_item(title: &str, doi: Option<&str>) -> NewItem {
        NewItem {
            title: title.into(),
            authors: vec!["Curie, Marie".into(), "Pierre Curie".into(), "Henri Becquerel".into()],
            doi: doi.map(String::from),
            year: Some(1903),
            ..NewItem::default()
        }
    }

    #[test]
    fn adds_and_lists_items() {
        let lib = PaperLibrary::in_memory().unwrap();
        let added = lib.add_item(new_item("Radioactivity", Some("https://doi.org/10.1000/RADIO"))).unwrap();
        assert_eq!(added.title, "Radioactivity");
        assert_eq!(added.item_type, "journal-article");
        assert_eq!(added.authors, "Curie et al.");
        assert_eq!(added.doi.as_deref(), Some("10.1000/radio"));
        assert_eq!(lib.items().unwrap(), vec![added]);
    }

    #[test]
    fn reports_input_errors() {
        let lib = PaperLibrary::in_memory().unwrap();
        assert!(matches!(lib.add_item(new_item("  ", None)), Err(CoreError::InvalidInput { .. })));
        assert!(matches!(
            lib.add_item(new_item("X", Some("not a doi"))),
            Err(CoreError::InvalidInput { .. })
        ));
        let bad_type = NewItem { item_type: "scroll".into(), ..new_item("X", None) };
        assert!(matches!(lib.add_item(bad_type), Err(CoreError::InvalidInput { .. })));
        assert!(matches!(lib.create_collection(" ".into(), None), Err(CoreError::InvalidInput { .. })));
    }

    #[test]
    fn reports_duplicates_with_the_existing_item() {
        let lib = PaperLibrary::in_memory().unwrap();
        let first = lib.add_item(new_item("A", Some("10.1000/x"))).unwrap();
        match lib.add_item(new_item("B", Some("doi:10.1000/X"))) {
            Err(CoreError::Duplicate { existing_id, .. }) => assert_eq!(existing_id, first.id),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn manages_collections() {
        let lib = PaperLibrary::in_memory().unwrap();
        let thesis = lib.create_collection("Thesis".into(), None).unwrap();
        let chapter = lib.create_collection("Chapter 1".into(), Some(thesis.id.clone())).unwrap();
        assert_eq!(chapter.parent_id.as_deref(), Some(thesis.id.as_str()));
        let item = lib.add_item(new_item("A", None)).unwrap();
        lib.add_to_collection(chapter.id.clone(), item.id.clone()).unwrap();
        assert_eq!(lib.items_in_collection(chapter.id).unwrap(), vec![item.clone()]);
        assert_eq!(lib.collections().unwrap().len(), 2);
        lib.trash_item(item.id).unwrap();
        assert!(lib.items().unwrap().is_empty());
    }

    #[test]
    fn opens_a_file_library() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("library.sqlite").to_string_lossy().into_owned();
        PaperLibrary::new(path.clone()).unwrap().add_item(new_item("Kept", None)).unwrap();
        assert_eq!(PaperLibrary::new(path).unwrap().items().unwrap().len(), 1);
    }

    #[test]
    fn doi_helpers() {
        assert_eq!(normalize_doi("DOI: 10.1038/NATURE12373".into()).as_deref(), Some("10.1038/nature12373"));
        assert_eq!(normalize_doi("nope".into()), None);
        assert_eq!(find_dois("a 10.1000/x b 10.1000/y.".into()), ["10.1000/x", "10.1000/y"]);
        assert_eq!(item_types().len(), 12);
        assert!(!core_version().is_empty());
    }

    #[test]
    fn records_serialize_in_camel_case() {
        let info = CollectionInfo { id: "a".into(), name: "N".into(), parent_id: None, position: 0 };
        let json = serde_json::to_value(&info).unwrap();
        assert!(json.get("parentId").is_some());
        let item: NewItem = serde_json::from_str(r#"{"title":"T","containerTitle":"J"}"#).unwrap();
        assert_eq!(item.container_title.as_deref(), Some("J"));
        assert!(item.authors.is_empty());
    }
}
