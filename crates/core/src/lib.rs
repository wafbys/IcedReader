//! Format-agnostic types for IcedReader.
//!
//! UI and Tauri talk only to this crate. Format crates (EPUB now, others later)
//! implement [`Book`]. Persistence (library, bookmarks) will live here too.

mod annotations;
mod book_meta;
mod fonts;
mod progress;
mod publisher_fonts;
mod settings;

use std::path::Path;

use serde::{Deserialize, Serialize};

pub use annotations::{AnnotationStore, Highlight};
pub use book_meta::{clean_title, join_title, read_meta_file, resolved_title, write_meta_file, BookMeta, TITLE_JOIN_SEP};
pub use fonts::{
    apply_custom_fonts, font_override_css, rewrite_css_font_families, sniff_font, FontKind,
    FontUrls, CJK_UNICODE_RANGE, LATIN_UNICODE_RANGE,
};
pub use progress::{progress_key, ProgressRecord, ProgressStore};
pub(crate) use progress::same_book;
pub use publisher_fonts::{
    collect_publisher_fonts, ChapterView, PublisherFontDecl, PublisherFontReport,
};
pub use settings::{
    clamp_font_scale, FontFile, FontSettingsView, FontSlot, FontSlots, ReaderSettings,
    SettingsStore, FONT_SCALE_DEFAULT, FONT_SCALE_MAX, FONT_SCALE_MIN, FONT_SCALE_STEP,
};

pub const EPUB_FORMAT: &str = "epub";

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("{0}")]
    Message(String),
    #[error("unsupported format: {0}")]
    UnsupportedFormat(String),
    #[error("chapter not found: {0}")]
    ChapterNotFound(String),
    #[error("resource not found: {0}")]
    ResourceNotFound(String),
}

impl CoreError {
    pub fn msg(msg: impl Into<String>) -> Self {
        Self::Message(msg.into())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Metadata {
    pub title: String,
    pub authors: Vec<String>,
    pub language: Option<String>,
    pub publisher: Option<String>,
    pub identifiers: Vec<String>,
    pub description: Option<String>,
    pub cover_href: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TocNode {
    pub label: String,
    pub href: Option<String>,
    pub children: Vec<TocNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpineItem {
    pub id: String,
    pub href: String,
    pub media_type: String,
    /// TOC label when the adapter expands or annotates chapters.
    #[serde(default)]
    pub title: Option<String>,
}

/// Position that survives font-size and platform changes.
/// `fraction` is 0..=1 within the chapter; `cfi` is filled in once pagination exists.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Locator {
    pub href: String,
    pub fraction: f64,
    pub cfi: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Resource {
    pub href: String,
    pub media_type: String,
    pub data: Vec<u8>,
}

/// Opened publication. Implementors must be `Send + Sync` (Tauri app state).
pub trait Book: Send + Sync {
    fn format_id(&self) -> &'static str;
    fn metadata(&self) -> Metadata;
    fn toc(&self) -> Vec<TocNode>;
    fn spine(&self) -> Vec<SpineItem>;

    /// Chapter HTML with resource URLs rewritten using `resource_base`
    /// (e.g. `http://icedreader.localhost/book/{id}/`).
    fn chapter_html(&self, href: &str, resource_base: &str) -> Result<String, CoreError>;

    fn resource(&self, href: &str) -> Result<Resource, CoreError>;
}

pub trait BookOpener: Send + Sync {
    fn format_id(&self) -> &'static str;
    fn can_open(&self, path: &Path) -> bool;
    fn open(&self, path: &Path) -> Result<Box<dyn Book>, CoreError>;
}

pub fn extension_is(path: &Path, ext: &str) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case(ext))
}
