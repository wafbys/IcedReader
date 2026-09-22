//! PDF quality signals — the PDF counterpart of `src-tauri`'s EPUB signals.
//!
//! Grading a PDF is a different question from grading an EPUB. An EPUB dump is
//! judged on text cleanliness and apparatus; a PDF is judged on **what the
//! reader can actually do with it**, because that is what varies wildly between
//! the two shapes real PDFs come in:
//!
//! * a born-digital PDF carries real text (searchable, and — from phase 2 —
//!   selectable and highlightable);
//! * a scanned book with an OCR layer *looks* the same and is searchable, but
//!   the visible page is a bitmap;
//! * a plain scan has no text at all (no search, no highlights, ever).
//!
//! So the signals are: text layer kind, font embedding, outline, metadata — all
//! measured, none inferred. Everything here runs once per file revision at
//! import; `list_library` never recomputes it (AGENTS: 质量不走热路径).

use serde::{Deserialize, Serialize};

use crate::PdfDoc;

/// How many pages the text-layer census samples.
const SAMPLE_PAGES: usize = 24;

/// What a PDF's text layer supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TextLayer {
    /// Visible text drawn with real fonts: searchable and selectable.
    MachineReadable,
    /// Scanned pages plus an invisible OCR layer (`Tr=3`): searchable, but the
    /// visible page is a bitmap.
    OcrLayer,
    /// Scanned pages with no text layer at all.
    ScanOnly,
    /// Text on some sampled pages and not others (mixed books, appended scans).
    Mixed,
}

impl TextLayer {
    /// Whether anything can be searched/selected at all (phase-2 capability).
    pub fn is_searchable(self) -> bool {
        matches!(
            self,
            TextLayer::MachineReadable | TextLayer::OcrLayer | TextLayer::Mixed
        )
    }
}

/// Measured, format-specific facts about one PDF.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PdfQuality {
    pub pages: usize,
    pub outline_entries: usize,
    pub text_layer: TextLayer,
    /// Pages inspected by the census.
    pub sampled_pages: usize,
    /// Sampled pages that draw visible text.
    pub text_pages: usize,
    /// Distinct fonts used, and how many of them carry a font program.
    pub fonts: usize,
    pub embedded_fonts: usize,
    /// Fonts used for **visible** text that are neither embedded nor one of the
    /// standard 14 — the pages these draw are the ones that come out wrong.
    pub unresolved_visible_fonts: Vec<String>,
    pub has_title: bool,
    pub has_author: bool,
    /// Encrypted PDFs cannot be opened by `hayro` at all (see `PdfError`), so
    /// this is only ever true for a file the opener accepted.
    pub encrypted: bool,
}

/// Measure one opened document. Cheap enough to run at import: it decodes page
/// content streams (no rasterising).
pub fn analyze(doc: &PdfDoc) -> PdfQuality {
    let pages = doc.page_count();
    let step = (pages / SAMPLE_PAGES).max(1);
    let mut sampled = 0usize;
    let mut visible_pages = 0usize;
    let mut invisible_pages = 0usize;
    for index in (0..pages).step_by(step).take(SAMPLE_PAGES) {
        let Ok(stats) = doc.page_content_stats(index) else {
            continue;
        };
        sampled += 1;
        if stats.visible_text_ops > 0 {
            visible_pages += 1;
        } else if stats.invisible_text_ops > 0 {
            invisible_pages += 1;
        }
    }
    // A page that draws both counts as visible text (the reader can select it);
    // the OCR shape is "no visible text anywhere, but a hidden layer exists".
    let text_layer = if visible_pages == 0 && invisible_pages == 0 {
        // No sampled page drew text at all (also covers `sampled == 0`).
        TextLayer::ScanOnly
    } else if visible_pages == sampled {
        TextLayer::MachineReadable
    } else if visible_pages == 0 {
        TextLayer::OcrLayer
    } else if visible_pages >= sampled / 2 {
        // Mostly text with a few image-only pages (covers, plates).
        TextLayer::MachineReadable
    } else {
        TextLayer::Mixed
    };

    let info = doc.info();
    PdfQuality {
        pages,
        outline_entries: count_outline(&doc.outline()),
        text_layer,
        sampled_pages: sampled,
        text_pages: visible_pages,
        fonts: doc.fonts().len(),
        embedded_fonts: doc.embedded_font_objects().len(),
        unresolved_visible_fonts: doc.visible_text_risk(),
        has_title: info.title.map(|t| !t.trim().is_empty()).unwrap_or(false),
        has_author: info.author.map(|a| !a.trim().is_empty()).unwrap_or(false),
        encrypted: false,
    }
}

fn count_outline(nodes: &[crate::OutlineNode]) -> usize {
    nodes
        .iter()
        .map(|node| 1 + count_outline(&node.children))
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::{temp_dir, write_cjk_unembedded_pdf, write_latin_pdf};

    #[test]
    fn machine_readable_pdf_is_reported_as_text() {
        let dir = temp_dir("quality-text");
        let path = dir.join("latin.pdf");
        write_latin_pdf(&path);
        let doc = PdfDoc::open(&path).unwrap();
        let quality = analyze(&doc);
        assert_eq!(quality.pages, 1);
        assert_eq!(quality.text_layer, TextLayer::MachineReadable);
        assert!(quality.text_layer.is_searchable());
        assert!(quality.has_title, "the synthetic file has an Info title");
        assert_eq!(quality.outline_entries, 0);
    }

    /// A non-embedded CJK font drawing visible text is the case the renderer
    /// cannot draw, and the census must name it.
    #[test]
    fn non_embedded_visible_text_is_flagged() {
        let dir = temp_dir("quality-risk");
        let path = dir.join("cjk.pdf");
        write_cjk_unembedded_pdf(&path);
        let doc = PdfDoc::open(&path).unwrap();
        let quality = analyze(&doc);
        assert_eq!(quality.text_layer, TextLayer::MachineReadable);
        assert_eq!(quality.unresolved_visible_fonts, vec!["SimSun".to_string()]);
    }
}
