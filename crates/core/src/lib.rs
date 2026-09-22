//! Format-agnostic types for IcedReader.
//!
//! UI and Tauri talk only to this crate. Format crates implement [`Book`].
//! Persistence (library, bookmarks) will live here too.

mod annotations;
mod book_meta;
mod fonts;
mod progress;
mod publisher_fonts;
mod settings;

use std::path::Path;

use serde::{Deserialize, Serialize};

pub use annotations::{AnnotationStore, Highlight, COLOR_GREEN, COLOR_YELLOW};
pub use book_meta::{
    clean_person_list, clean_title, join_title, read_meta_file, resolved_title, write_meta_file,
    BookMeta, FIELD_SEP, TITLE_JOIN_SEP,
};
pub use fonts::{
    apply_custom_fonts, font_override_css, rewrite_css_font_families, sniff_font, FontKind,
    FontUrls, CJK_UNICODE_RANGE, LATIN_UNICODE_RANGE,
};
pub(crate) use progress::same_book;
pub use progress::{progress_key, ProgressRecord, ProgressStore};
pub use publisher_fonts::{
    collect_publisher_fonts, ChapterView, PublisherFontDecl, PublisherFontReport,
};
pub use settings::{
    clamp_font_scale, FontFile, FontSettingsView, FontSlot, FontSlots, ReaderSettings,
    SettingsStore, FONT_SCALE_DEFAULT, FONT_SCALE_MAX, FONT_SCALE_MIN, FONT_SCALE_STEP,
};

pub const EPUB_FORMAT: &str = "epub";
pub const PDF_FORMAT: &str = "pdf";

/// Extensions the reader can open, lowercase and without the dot — one source
/// of truth for the library scanner, the metadata renamer, the importer's
/// fallback name and the `lib:` progress key.
pub const BOOK_EXTENSIONS: [&str; 2] = [EPUB_FORMAT, PDF_FORMAT];

/// Strip a known book extension (`三体.EPUB` → `三体`). Names that are not a
/// book file come back unchanged, so a stray `.txt` keeps its full name.
pub fn book_stem(file_name: &str) -> &str {
    let stem = book_stem_len(file_name);
    &file_name[..stem]
}

/// Lowercase extension of a book file, or `None` for anything else.
pub fn book_extension(file_name: &str) -> Option<&'static str> {
    let (_, ext) = file_name.rsplit_once('.')?;
    BOOK_EXTENSIONS
        .iter()
        .copied()
        .find(|known| ext.eq_ignore_ascii_case(known))
}

fn book_stem_len(file_name: &str) -> usize {
    match file_name.rsplit_once('.') {
        Some((stem, _)) if book_extension(file_name).is_some() => stem.len(),
        _ => file_name.len(),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    #[error("{0}")]
    Message(String),
    #[error("不支持的格式：{0}")]
    UnsupportedFormat(String),
    #[error("找不到章节：{0}")]
    ChapterNotFound(String),
    #[error("找不到资源：{0}")]
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
/// `fraction` is 0..=1 within the chapter. `cfi` is reserved and must stay
/// `None` — pagination exists, but do not invent a fake CFI.
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

    /// Size of each spine unit in the format's own units, when the format has a
    /// **fixed layout** (PDF). The shell needs it to lay out a continuous page
    /// view — every page's placeholder must be the right shape before its bytes
    /// arrive, or the scrollbar length and page jumps would be wrong.
    ///
    /// Reflowable formats (EPUB) have no such notion and return an empty list;
    /// callers must treat an empty list as "no hints", never as "zero-sized".
    fn page_sizes(&self) -> Vec<(f32, f32)> {
        Vec::new()
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn book_stem_strips_only_known_book_extensions() {
        assert_eq!(book_stem("三体.epub"), "三体");
        assert_eq!(book_stem("三体.EPUB"), "三体");
        assert_eq!(book_stem("三体.pdf"), "三体");
        assert_eq!(book_stem("经济漩涡.PdF"), "经济漩涡");
        // Not a book file: keep the name intact so nothing is silently cut.
        assert_eq!(book_stem("notes.txt"), "notes.txt");
        assert_eq!(book_stem("no-extension"), "no-extension");
        // Dots inside the title are fine (last dot decides).
        assert_eq!(book_stem("第 1 卷. 上.pdf"), "第 1 卷. 上");
        assert_eq!(book_stem(".hidden.epub"), ".hidden");
    }

    #[test]
    fn book_extension_reports_the_canonical_lowercase_form() {
        assert_eq!(book_extension("三体.epub"), Some("epub"));
        assert_eq!(book_extension("三体.PDF"), Some("pdf"));
        assert_eq!(book_extension("三体.txt"), None);
        assert_eq!(book_extension("三体"), None);
    }
}
