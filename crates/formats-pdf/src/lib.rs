//! PDF adapter — **spike stage** (see `docs/ideas/pdf.md`).
//!
//! This crate deliberately does **not** implement `Book` / `BookOpener` yet.
//! Before wiring PDF into the reader we have to answer three questions on real
//! files, and this crate exists to answer them:
//!
//! 1. does `hayro` build and render acceptably on Windows/MSVC (quality),
//! 2. how often do real PDFs rely on **non-embedded** fonts (hayro cannot
//!    resolve those: `FontQuery::Fallback` is not implemented upstream),
//! 3. how long does one page take, and how big is the PNG (cache design).
//!
//! Everything the reader will eventually need is already here in seed form:
//! page count, Info metadata, outline tree, per-font embedding audit, and
//! page rasterisation with timing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{RenderCache, RenderSettings};
use lopdf::{Dictionary, Document, Object, ObjectId};

mod book;
mod quality;

pub use book::{PdfBook, PdfOpener};
pub use quality::{analyze as analyze_quality, PdfQuality, TextLayer};

/// Media type of the generated page document. Not a real PDF media type: the
/// reader never serves `.pdf` bytes to the webview, only rendered pages.
pub const PDF_PAGE_MEDIA_TYPE: &str = "application/pdf-page";

/// Raster width (device pixels) used when a page URL does not ask for one.
/// 1440–1600 px is where the samples stop showing any visible loss at the
/// reader's column width (≤720 CSS px, DPR ≤ 2).
pub const DEFAULT_PAGE_WIDTH: u32 = 1600;

pub type Result<T> = std::result::Result<T, PdfError>;

#[derive(Debug, thiserror::Error)]
pub enum PdfError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("not a readable PDF: {0}")]
    Load(String),
    #[error("page {0} out of range (this file has {1} pages)")]
    PageOutOfRange(usize, usize),
    #[error("image encode: {0}")]
    Encode(String),
}

/// Document-level metadata (PDF Info dictionary; XMP is a later enhancement).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PdfInfo {
    pub title: Option<String>,
    pub author: Option<String>,
    pub subject: Option<String>,
    pub creator: Option<String>,
    pub producer: Option<String>,
    pub page_count: usize,
}

/// One outline (bookmark) node. `page` is 0-based; `None` when the destination
/// could not be resolved to a page of this document (external / broken link).
#[derive(Debug, Clone, PartialEq)]
pub struct OutlineNode {
    pub title: String,
    pub page: Option<usize>,
    pub children: Vec<OutlineNode>,
}

/// Per-font audit of a document — the R1 detector.
#[derive(Debug, Clone, PartialEq)]
pub struct FontAudit {
    /// `/BaseFont` (e.g. `SimSun`, `ABCDEF+SongtiSC-Bold`).
    pub name: String,
    /// `/Subtype` (e.g. `TrueType`, `Type0`, `Type1`, `Type3`).
    pub subtype: String,
    /// `false` = the renderer must find this font in the system (hayro cannot).
    pub embedded: bool,
    /// `/FontFile` family key that provided the glyphs, when embedded.
    pub file: Option<String>,
    /// Named `/Encoding` (e.g. `WinAnsiEncoding`, `Identity-H`), when the font
    /// dict names one instead of embedding a CMap stream. `Identity-H` is the
    /// bad case: codes are glyph ids of the missing font, so no substitute can
    /// stand in for it.
    pub encoding: Option<String>,
    /// Whether the font carries a `/ToUnicode` CMap (text extraction, and the
    /// only way a CID font could be re-mapped onto a substitute).
    pub to_unicode: bool,
}

/// An actual embedded font program found in the file (not just a font dict
/// that *names* a font): the difference between "text is real text" and
/// "text was converted to outlines".
#[derive(Debug, Clone, PartialEq)]
pub struct EmbeddedFont {
    pub font_name: String,
    /// `FontFile` / `FontFile2` / `FontFile3`.
    pub key: String,
    pub bytes: usize,
}

/// What one page's content stream actually draws — the honest answer to
/// "is there text here, or only paths/images?".
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PageContentStats {
    /// `Tj` / `TJ` / `'` / `"` operators (page stream **and** nested forms).
    pub text_ops: usize,
    /// Text ops with a painting render mode (0/1/2/4/5/6) — the ones whose
    /// glyphs must actually exist.
    pub visible_text_ops: usize,
    /// Text ops with `Tr` 3 or 7: an invisible OCR / clip text layer, which
    /// renders fine without any font program.
    pub invisible_text_ops: usize,
    /// Path construction/painting operators (`m`, `l`, `c`, `re`, `f`, `S`…).
    pub path_ops: usize,
    /// `Do` operators (XObjects: images and forms).
    pub xobject_ops: usize,
    /// Form XObjects walked (their own resources and operators are included
    /// in the counts above).
    pub forms: usize,
    /// Image XObjects referenced by the page (`Do` on an `/Image`).
    pub image_objects: usize,
    /// `BI` inline images (often a whole scanned page).
    pub inline_images: usize,
    /// Total encoded bytes of the images above.
    pub image_bytes: usize,
    /// Distinct `Tr` (text rendering mode) values seen. `3` = **invisible**,
    /// i.e. a hidden OCR text layer over a scanned bitmap.
    pub text_render_modes: Vec<i64>,
    /// Fonts the page really uses, including those of nested form XObjects.
    pub fonts: Vec<FontAudit>,
    pub unresolved_fonts: Vec<FontAudit>,
}

/// Encoding of a rasterised page.
///
/// **PNG is the default, and measurably the right one.** The obvious guess —
/// "a page is a picture, so use JPEG" — is wrong with this encoder: measured on
/// the sample books at 1440 px, PNG encoding costs 4–21 ms while baseline JPEG
/// costs **65–95 ms** (6–9× slower) for only ~2× fewer bytes. The reader is
/// served by an in-process protocol, so bytes are cheap and encoder time is
/// not. JPEG stays available for experiments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PageFormat {
    #[default]
    Png,
    Jpeg,
    /// Lossless WebP (`image` has no lossy encoder). Measured alongside the
    /// others; kept so the comparison can be repeated.
    WebP,
}

impl PageFormat {
    pub fn media_type(self) -> &'static str {
        match self {
            PageFormat::Png => "image/png",
            PageFormat::Jpeg => "image/jpeg",
            PageFormat::WebP => "image/webp",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            PageFormat::Png => "png",
            PageFormat::Jpeg => "jpg",
            PageFormat::WebP => "webp",
        }
    }
}

/// JPEG quality for page rasters. 82 keeps text crisp at reading sizes while
/// staying around 150–250 KB for a 1440px scanned page.
pub const JPEG_QUALITY: u8 = 82;

/// Everything a page raster request can vary.
#[derive(Debug, Clone)]
pub struct RenderOptions {
    pub width: u32,
    pub format: PageFormat,
    /// Font program used when the PDF does not embed the font it names
    /// (diagnostic; not a shipped setting).
    pub substitute_font: Option<(Vec<u8>, u32)>,
}

impl RenderOptions {
    pub fn png(width: u32) -> Self {
        Self {
            width,
            format: PageFormat::Png,
            substitute_font: None,
        }
    }

    pub fn jpeg(width: u32) -> Self {
        Self {
            width,
            format: PageFormat::Jpeg,
            substitute_font: None,
        }
    }

    /// The reader's page format: lossless WebP. Measured at 1440 px against
    /// PNG on the sample books, encoding costs 5–8 ms more (20.3 vs 15.6 ms on
    /// a text page, 27.5 vs 19.8 ms on a scan) but produces 55–69% fewer bytes
    /// (498→224 KB, 1042→324 KB) — and that encode happens on the prefetch
    /// worker, off the page-turn path, while the bytes are what the webview has
    /// to move, decode and hold in memory.
    pub fn webp(width: u32) -> Self {
        Self {
            width,
            format: PageFormat::WebP,
            substitute_font: None,
        }
    }
}

/// A rasterised page plus the numbers the cache design needs.
#[derive(Debug, Clone)]
pub struct RenderedPage {
    pub index: usize,
    pub width: u32,
    pub height: u32,
    /// Encoded image bytes in [`RenderedPage::format`].
    pub data: Vec<u8>,
    pub format: PageFormat,
    /// Fraction of non-white pixels: 0.0 means the page came out **blank**
    /// (the R1 symptom), a text page is typically 0.02–0.15.
    pub ink: f64,
    /// Per-call parse cost. Now always 0: the document is parsed once in
    /// [`PdfDoc::open`] and reused (see [`PdfDoc::parse_ms`] for that cost).
    pub parse_ms: f64,
    /// Raster + encode (what a page turn costs when it is not cached).
    pub render_ms: f64,
    /// `hayro` rasterising the page.
    pub raster_ms: f64,
    /// Encoding the raster (the half that used to be wasted on PNG).
    pub encode_ms: f64,
}

impl RenderedPage {
    /// Alias kept for callers that only ever asked for PNG.
    pub fn png(&self) -> &[u8] {
        &self.data
    }
}

/// An opened PDF, kept in memory.
///
/// Two parsers on purpose: `lopdf` owns structure (page tree, outline, fonts,
/// Info) because `hayro-syntax` does not expose outlines, and `hayro` owns
/// rasterisation. Both parse once here and are reused for every page: cloning
/// and re-parsing a 30 MB document per page turn cost 30 MB of copying plus
/// 5–88 ms, against ~20 ms of actual rasterising.
pub struct PdfDoc {
    path: PathBuf,
    doc: Document,
    pages: BTreeMap<u32, ObjectId>,
    /// The `hayro` document. Its parsed form is `Send + Sync` (asserted in the
    /// tests), so it can live behind the reader's `Book: Send + Sync` bound.
    /// Only the `RenderCache` is per-call: it borrows the document.
    pdf: Pdf,
    /// One-time parse cost (ms), measured at open.
    parse_ms: f64,
}

impl PdfDoc {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let bytes = std::fs::read(&path)?;
        let doc = Document::load_mem(&bytes).map_err(|e| PdfError::Load(e.to_string()))?;
        let pages = doc.get_pages();
        if pages.is_empty() {
            return Err(PdfError::Load("no pages".into()));
        }
        let t = Instant::now();
        // Takes ownership of the bytes; the raw copy is not kept, so a big PDF
        // is not held twice in memory.
        let pdf = Pdf::new(bytes).map_err(load_error)?;
        let parse_ms = t.elapsed().as_secs_f64() * 1000.0;
        Ok(Self {
            path,
            doc,
            pages,
            pdf,
            parse_ms,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Display size of one page (0-based) in PDF units — the aspect ratio the
    /// shell needs to reserve layout space for a window of pages before their
    /// bytes arrive. Falls back to a square when the page is unreadable.
    pub fn page_size(&self, index: usize) -> (f32, f32) {
        self.pdf
            .pages()
            .iter()
            .nth(index)
            .map(|page| page.render_dimensions())
            .unwrap_or((1.0, 1.0))
    }

    /// Cost of the one-time `hayro` parse (ms), paid at open.
    pub fn parse_ms(&self) -> f64 {
        self.parse_ms
    }

    pub fn info(&self) -> PdfInfo {
        let info = self
            .doc
            .trailer
            .get(b"Info")
            .ok()
            .and_then(|obj| self.resolve(obj))
            .and_then(|obj| obj.as_dict().ok());
        let field = |key: &[u8]| -> Option<String> {
            let dict = info?;
            let value = dict.get(key).ok()?;
            pdf_text(self.resolve(value)?)
        };
        PdfInfo {
            title: field(b"Title"),
            author: field(b"Author"),
            subject: field(b"Subject"),
            creator: field(b"Creator"),
            producer: field(b"Producer"),
            page_count: self.page_count(),
        }
    }

    /// Outline (bookmark) tree, rebuilt from lopdf's flattened
    /// `{level, title, page}` list. Empty when the file has none — matching
    /// the agreed behaviour: a PDF without an outline gets no sidebar TOC.
    pub fn outline(&self) -> Vec<OutlineNode> {
        let Ok(toc) = self.doc.get_toc() else {
            return Vec::new();
        };
        let mut roots: Vec<OutlineNode> = Vec::new();
        // Open ancestors, innermost last; each holds its already-mapped children.
        let mut stack: Vec<(usize, OutlineNode)> = Vec::new();
        for item in toc.toc {
            let node = OutlineNode {
                title: item.title,
                // lopdf reports 1-based page numbers.
                page: (item.page > 0).then(|| item.page - 1),
                children: Vec::new(),
            };
            while stack
                .last()
                .is_some_and(|(level, _)| *level >= item.level)
            {
                let (_, done) = stack.pop().expect("checked above");
                attach_outline(done, &mut stack, &mut roots);
            }
            stack.push((item.level, node));
        }
        while let Some((_, done)) = stack.pop() {
            attach_outline(done, &mut stack, &mut roots);
        }
        roots
    }

    /// Every embedded font program in the document. An empty list means the
    /// file carries **no** font outlines: any visible text must be vector
    /// paths (文字转曲) or an image, which is why such a PDF renders fine in
    /// hayro while still being un-extractable for search/highlights.
    pub fn embedded_font_objects(&self) -> Vec<EmbeddedFont> {
        let mut out = Vec::new();
        for (_, object) in self.doc.objects.iter() {
            let Object::Dictionary(dict) = object else {
                continue;
            };
            for key in [b"FontFile".as_slice(), b"FontFile2", b"FontFile3"] {
                let Ok(value) = dict.get(key) else {
                    continue;
                };
                if matches!(value, Object::Null) {
                    continue;
                }
                let bytes = self
                    .resolve(value)
                    .and_then(|obj| obj.as_stream().ok())
                    .map(|stream| stream.content.len())
                    .unwrap_or(0);
                out.push(EmbeddedFont {
                    font_name: dict
                        .get(b"FontName")
                        .ok()
                        .and_then(|obj| self.resolve(obj))
                        .and_then(|obj| obj.as_name().ok())
                        .map(|name| String::from_utf8_lossy(name).into_owned())
                        .unwrap_or_default(),
                    key: String::from_utf8_lossy(key).into_owned(),
                    bytes,
                });
            }
        }
        out
    }

    /// Operator census of one page (0-based): does it draw text, paths, images?
    /// Nested form XObjects are walked too — in a lot of "text as outlines" /
    /// CID-font PDFs the visible text lives inside a single form.
    pub fn page_content_stats(&self, index: usize) -> Result<PageContentStats> {
        let page_id = self
            .pages
            .get(&(index as u32 + 1))
            .copied()
            .ok_or(PdfError::PageOutOfRange(index, self.page_count()))?;
        // Bounded decode: PDFs are untrusted input, and a page stream is
        // normally a few KB (the limit only bites on decompression bombs).
        const MAX_PAGE_STREAM: usize = 32 * 1024 * 1024;
        let bytes = self
            .doc
            .get_page_content_with_limit(page_id, MAX_PAGE_STREAM)
            .map_err(|e| PdfError::Load(e.to_string()))?;

        let mut stats = PageContentStats::default();
        if let Ok(content) = lopdf::content::Content::decode(&bytes) {
            census(&content, &mut stats);
        }
        if let Ok((Some(resources), _)) = self.doc.get_page_resources(page_id) {
            self.walk_resources(resources, &mut stats, 0);
        }
        Ok(stats)
    }

    /// Collect the fonts and operators reachable from a resource dictionary,
    /// recursing into form XObjects (depth-capped: see `MAX_FORM_DEPTH`).
    fn walk_resources(&self, resources: &Dictionary, stats: &mut PageContentStats, depth: usize) {
        const MAX_FORM_DEPTH: usize = 6;
        if depth > MAX_FORM_DEPTH {
            return;
        }
        if let Some(fonts) = resources
            .get(b"Font")
            .ok()
            .and_then(|obj| self.resolve(obj))
            .and_then(|obj| obj.as_dict().ok())
        {
            for (_, value) in fonts.iter() {
                let Some(dict) = self.resolve(value).and_then(|obj| obj.as_dict().ok()) else {
                    continue;
                };
                let audit = self.audit_font(dict);
                if !stats.fonts.contains(&audit) {
                    if !audit.embedded && !is_standard_font_name(&audit.name) {
                        stats.unresolved_fonts.push(audit.clone());
                    }
                    stats.fonts.push(audit);
                }
            }
        }
        let Some(xobjects) = resources
            .get(b"XObject")
            .ok()
            .and_then(|obj| self.resolve(obj))
            .and_then(|obj| obj.as_dict().ok())
        else {
            return;
        };
        for (_, value) in xobjects.iter() {
            let Some(stream) = self.resolve(value).and_then(|obj| obj.as_stream().ok()) else {
                continue;
            };
            let subtype = stream
                .dict
                .get(b"Subtype")
                .ok()
                .and_then(|obj| obj.as_name().ok())
                .unwrap_or_default();
            if subtype != b"Form" {
                stats.image_objects += 1;
                stats.image_bytes += stream.content.len();
                continue;
            }
            stats.forms += 1;
            if let Ok(bytes) = stream.decompressed_content() {
                if let Ok(content) = lopdf::content::Content::decode(&bytes) {
                    census(&content, stats);
                }
            }
            if let Some(inner) = stream
                .dict
                .get(b"Resources")
                .ok()
                .and_then(|obj| self.resolve(obj))
                .and_then(|obj| obj.as_dict().ok())
            {
                self.walk_resources(inner, stats, depth + 1);
            }
        }
    }

    /// Every distinct font used in the document, with its embedding status.
    /// Deduplicated; stops early on absurdly long documents.
    pub fn fonts(&self) -> Vec<FontAudit> {
        const PAGE_SCAN_LIMIT: usize = 400;
        let mut seen: Vec<FontAudit> = Vec::new();
        for (_, page_id) in self.pages.iter().take(PAGE_SCAN_LIMIT) {
            let Ok(fonts) = self.doc.get_page_fonts(*page_id) else {
                continue;
            };
            for (_, dict) in fonts {
                let audit = self.audit_font(dict);
                if !seen.contains(&audit) {
                    seen.push(audit);
                }
            }
        }
        seen
    }

    /// Fonts that the renderer cannot supply itself: not embedded **and** not
    /// one of the 14 standard fonts. These are the R1 candidates; whether they
    /// actually matter depends on whether they draw **visible** text — see
    /// [`PdfDoc::visible_text_risk`].
    pub fn unresolved_fonts(&self) -> Vec<FontAudit> {
        self.fonts()
            .into_iter()
            .filter(|f| !f.embedded && !is_standard_font_name(&f.name))
            .collect()
    }

    /// The honest "will this PDF lose text?" verdict.
    ///
    /// Only **visible** text (`Tr` 0/1/2/4/5/6) drawn with an unresolved font
    /// goes blank; the fonts behind an invisible `Tr=3` OCR layer or a pure
    /// scan never need glyphs (measured: a scanned book with an OCR layer
    /// renders perfectly while listing 14 "unresolved" fonts). Samples up to
    /// [`TEXT_RISK_PAGES`] pages spread across the document.
    ///
    /// Returns the offending font names; empty means safe to read.
    pub fn visible_text_risk(&self) -> Vec<String> {
        const TEXT_RISK_PAGES: usize = 24;
        let pages = self.page_count();
        let step = (pages / TEXT_RISK_PAGES).max(1);
        let mut offenders: Vec<String> = Vec::new();
        for index in (0..pages).step_by(step).take(TEXT_RISK_PAGES) {
            let Ok(stats) = self.page_content_stats(index) else {
                continue;
            };
            if stats.visible_text_ops == 0 {
                continue;
            }
            for font in &stats.unresolved_fonts {
                if !offenders.contains(&font.name) {
                    offenders.push(font.name.clone());
                }
            }
        }
        offenders
    }

    fn audit_font(&self, font: &Dictionary) -> FontAudit {
        let name = self.dict_name(font, b"BaseFont").unwrap_or_default();
        let subtype = self.dict_name(font, b"Subtype").unwrap_or_default();
        let encoding = font
            .get(b"Encoding")
            .ok()
            .and_then(|obj| self.resolve(obj))
            .and_then(|obj| obj.as_name().ok())
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned());
        let to_unicode = font
            .get(b"ToUnicode")
            .map(|obj| !matches!(obj, Object::Null))
            .unwrap_or(false)
            || font
                .get(b"DescendantFonts")
                .ok()
                .and_then(|obj| self.resolve(obj))
                .and_then(|obj| obj.as_array().ok())
                .and_then(|items| items.first())
                .and_then(|item| self.resolve(item))
                .and_then(|item| item.as_dict().ok())
                .map(|dict| dict.get(b"ToUnicode").is_ok())
                .unwrap_or(false);
        // Type3 glyphs are procedures inside the file: self-contained.
        if subtype.eq_ignore_ascii_case("Type3") {
            return FontAudit {
                name,
                subtype,
                embedded: true,
                file: Some("Type3".into()),
                encoding,
                to_unicode,
            };
        }
        let file = self
            .font_file_key(font)
            .map(|(key, _)| String::from_utf8_lossy(key).into_owned());
        FontAudit {
            name,
            subtype,
            embedded: file.is_some(),
            file,
            encoding,
            to_unicode,
        }
    }

    /// `FontFile` / `FontFile2` / `FontFile3` wherever it lives for this font
    /// (simple font descriptor, or the Type0 descendant's descriptor).
    fn font_file_key<'a>(&'a self, font: &'a Dictionary) -> Option<(&'a [u8], &'a Object)> {
        let mut descriptors: Vec<&Dictionary> = Vec::new();
        if let Some(dict) = font
            .get(b"FontDescriptor")
            .ok()
            .and_then(|obj| self.dict_ref(obj))
        {
            descriptors.push(dict);
        }
        if let Some(descendants) = font.get(b"DescendantFonts").ok() {
            if let Some(descendants) = self.resolve(descendants) {
                if let Ok(items) = descendants.as_array() {
                    for item in items {
                        if let Some(dict) = self.resolve(item).and_then(|o| o.as_dict().ok()) {
                            if let Some(descriptor) = dict
                                .get(b"FontDescriptor")
                                .ok()
                                .and_then(|obj| self.resolve(obj))
                                .and_then(|o| o.as_dict().ok())
                            {
                                descriptors.push(descriptor);
                            }
                        }
                    }
                }
            }
        }
        for dict in descriptors {
            for key in [b"FontFile".as_slice(), b"FontFile2", b"FontFile3"] {
                if let Ok(value) = dict.get(key) {
                    if !matches!(value, Object::Null) {
                        return Some((key, value));
                    }
                }
            }
        }
        None
    }

    /// Follow one reference; non-references resolve to themselves.
    fn resolve<'a>(&'a self, obj: &'a Object) -> Option<&'a Object> {
        self.doc.dereference(obj).ok().map(|(_, value)| value)
    }

    fn dict_ref<'a>(&'a self, obj: &'a Object) -> Option<&'a Dictionary> {
        self.resolve(obj).and_then(|value| value.as_dict().ok())
    }

    fn dict_name(&self, dict: &Dictionary, key: &[u8]) -> Option<String> {
        let value = dict.get(key).ok()?;
        let bytes = self.resolve(value)?.as_name().ok()?;
        Some(String::from_utf8_lossy(bytes).into_owned())
    }

    /// Rasterise one page (0-based) at `target_width` device pixels, as PNG.
    pub fn render_page_png(&self, index: usize, target_width: u32) -> Result<RenderedPage> {
        self.render_page(index, &RenderOptions::png(target_width))
    }

    /// Same, with an optional **substitute font program** (`bytes`, TTC index)
    /// used whenever the PDF does not embed the font it names.
    ///
    /// This is the hook behind "can the reader choose a font for PDFs?".
    /// `hayro` calls its font resolver for `FontQuery::Fallback` (a font that is
    /// not embedded) and *uses* the returned program, mapping the PDF's codes
    /// through its `ToUnicode` CMap first — so supplying a CJK font renders
    /// text that the default standard-14 substitution leaves blank. A PDF
    /// without `ToUnicode` cannot be rescued this way: its codes are glyph ids
    /// of a font we do not have.
    ///
    /// **Not shipped as a user setting** (decided 2026-09-15): kept as the
    /// diagnostic entry point for the spike harness and the tests.
    pub fn render_page_png_with(
        &self,
        index: usize,
        target_width: u32,
        substitute: Option<(Vec<u8>, u32)>,
    ) -> Result<RenderedPage> {
        self.render_page(
            index,
            &RenderOptions {
                substitute_font: substitute,
                ..RenderOptions::png(target_width)
            },
        )
    }

    /// The general entry point: page → raster → encoded bytes.
    pub fn render_page(&self, index: usize, opts: &RenderOptions) -> Result<RenderedPage> {
        if index >= self.page_count() {
            return Err(PdfError::PageOutOfRange(index, self.page_count()));
        }
        let pages = self.pdf.pages();
        let page = pages
            .iter()
            .nth(index)
            .ok_or(PdfError::PageOutOfRange(index, self.page_count()))?;
        let (page_w, _page_h) = page.render_dimensions();
        let scale = opts.width as f32 / page_w.max(1.0);

        let t_render = Instant::now();
        let cache = RenderCache::new();
        let pixmap = hayro::render(
            page,
            &cache,
            &interpreter_settings(opts.substitute_font.clone()),
            &RenderSettings {
                x_scale: scale,
                y_scale: scale,
                width: None,
                height: None,
                bg_color: WHITE,
            },
        );
        let raster_ms = t_render.elapsed().as_secs_f64() * 1000.0;

        let (width, height) = (pixmap.width() as u32, pixmap.height() as u32);
        let rgba = pixmap.data_as_u8_slice();
        let ink = ink_ratio(rgba);

        // Encoding is not free: for a 1440×2159 page the PNG encoder is in the
        // same order of magnitude as the rasteriser, which is why the reader
        // serves JPEG by default (see `book.rs`) and why both halves are timed.
        let t_encode = Instant::now();
        let (data, format) = match opts.format {
            PageFormat::Png => {
                let image = image::RgbaImage::from_raw(width, height, rgba.to_vec())
                    .ok_or_else(|| PdfError::Encode("pixmap buffer size mismatch".into()))?;
                let mut png = Vec::new();
                image::DynamicImage::ImageRgba8(image)
                    .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
                    .map_err(|e| PdfError::Encode(e.to_string()))?;
                (png, PageFormat::Png)
            }
            PageFormat::Jpeg => {
                let rgb: Vec<u8> = rgba
                    .chunks_exact(4)
                    .flat_map(|px| [px[0], px[1], px[2]])
                    .collect();
                let mut jpeg = Vec::new();
                let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
                    &mut jpeg,
                    JPEG_QUALITY,
                );
                encoder
                    .encode(&rgb, width, height, image::ExtendedColorType::Rgb8)
                    .map_err(|e| PdfError::Encode(e.to_string()))?;
                (jpeg, PageFormat::Jpeg)
            }
            PageFormat::WebP => {
                let image = image::RgbaImage::from_raw(width, height, rgba.to_vec())
                    .ok_or_else(|| PdfError::Encode("pixmap buffer size mismatch".into()))?;
                let mut webp = Vec::new();
                image::DynamicImage::ImageRgba8(image)
                    .write_to(
                        &mut std::io::Cursor::new(&mut webp),
                        image::ImageFormat::WebP,
                    )
                    .map_err(|e| PdfError::Encode(e.to_string()))?;
                (webp, PageFormat::WebP)
            }
        };
        let encode_ms = t_encode.elapsed().as_secs_f64() * 1000.0;

        Ok(RenderedPage {
            index,
            width,
            height,
            data,
            format,
            ink,
            // The document was parsed once in `open`, so a page turn costs no
            // parsing at all (the field stays for the harness output shape).
            parse_ms: 0.0,
            // What a page turn actually waits for when it is not cached.
            render_ms: raster_ms + encode_ms,
            raster_ms,
            encode_ms,
        })
    }
}

/// Interpreter settings, optionally with a substitute font program used for
/// fonts the PDF does not embed (see [`PdfDoc::render_page_png_with`]).
fn interpreter_settings(substitute: Option<(Vec<u8>, u32)>) -> InterpreterSettings {
    let mut settings = InterpreterSettings::default();
    if let Some((bytes, ttc_index)) = substitute {
        let data: hayro::hayro_interpret::font::FontData = std::sync::Arc::new(bytes);
        settings.font_resolver = std::sync::Arc::new(
            move |_query: &hayro::hayro_interpret::font::FontQuery| {
                Some((data.clone(), ttc_index))
            },
        );
    }
    settings
}

/// Page 1 as a shelf cover thumbnail: `(media_type, bytes)`. Cheap enough to
/// run on demand — the shelf's per-revision cover cache keeps repeat visits
/// free — and small in bytes, which matters because a colour cover at 400 px is
/// the biggest image the shelf shows.
pub fn cover(path: &Path, width: u32) -> Result<(String, Vec<u8>)> {
    let doc = PdfDoc::open(path)?;
    let page = doc.render_page(0, &RenderOptions::webp(width))?;
    Ok((page.format.media_type().to_string(), page.data))
}

/// Path-level form of [`PdfDoc::visible_text_risk`] for callers that just want
/// the import-time verdict (the reader shell does exactly this when opening).
pub fn visible_text_risk(path: &Path) -> Result<Vec<String>> {
    Ok(PdfDoc::open(path)?.visible_text_risk())
}

/// Fraction of non-white pixels in an encoded RGBA buffer — a cheap "did any
/// ink land on the page" check for tests and the spike report.
pub fn ink_ratio(rgba: &[u8]) -> f64 {
    let pixels = rgba.len() / 4;
    if pixels == 0 {
        return 0.0;
    }
    let inked = rgba
        .chunks_exact(4)
        .filter(|px| px[0] < 250 || px[1] < 250 || px[2] < 250)
        .count();
    inked as f64 / pixels as f64
}

/// Count the operator kinds of one content stream into `stats`.
///
/// `Tr` is part of the graphics state, so the text ops of one stream are
/// classified with the mode in force when they run (a form starts from the
/// default state, hence the reset per call).
fn census(content: &lopdf::content::Content, stats: &mut PageContentStats) {
    let mut mode: i64 = 0;
    for op in &content.operations {
        match op.operator.as_str() {
            "Tr" => {
                if let Some(Object::Integer(next)) = op.operands.first() {
                    mode = *next;
                    if !stats.text_render_modes.contains(next) {
                        stats.text_render_modes.push(*next);
                    }
                }
            }
            "Tj" | "TJ" | "'" | "\"" => {
                stats.text_ops += 1;
                if mode == 3 || mode == 7 {
                    stats.invisible_text_ops += 1;
                } else {
                    stats.visible_text_ops += 1;
                }
            }
            "m" | "l" | "c" | "v" | "y" | "re" | "h" | "f" | "F" | "f*" | "B" | "B*" | "b"
            | "b*" | "S" | "s" | "n" | "W" | "W*" => stats.path_ops += 1,
            "Do" => stats.xobject_ops += 1,
            "BI" => stats.inline_images += 1,
            _ => {}
        }
    }
}

/// Attach a finished outline node to the innermost open ancestor (or to the
/// roots when the stack is empty).
fn attach_outline(
    done: OutlineNode,
    stack: &mut [(usize, OutlineNode)],
    roots: &mut Vec<OutlineNode>,
) {
    match stack.last_mut() {
        Some((_, parent)) => parent.children.push(done),
        None => roots.push(done),
    }
}

/// `hayro_syntax::LoadPdfError` carries no `Display`; keep the debug text so a
/// failure still says something useful in the UI.
fn load_error(err: hayro::hayro_syntax::LoadPdfError) -> PdfError {
    PdfError::Load(format!("{err:?}"))
}

/// The 14 fonts every PDF processor must be able to supply itself
/// (`hayro`'s `embed-fonts` feature covers them, so they are not R1 cases).
pub fn is_standard_font_name(base_font: &str) -> bool {
    // Subset prefixes look like `ABCDEF+Helvetica`.
    let name = base_font
        .split_once('+')
        .map(|(_, rest)| rest)
        .unwrap_or(base_font);
    let name = name.trim();
    const STANDARD: [&str; 14] = [
        "Times-Roman",
        "Times-Bold",
        "Times-Italic",
        "Times-BoldItalic",
        "Helvetica",
        "Helvetica-Bold",
        "Helvetica-Oblique",
        "Helvetica-BoldOblique",
        "Courier",
        "Courier-Bold",
        "Courier-Oblique",
        "Courier-BoldOblique",
        "Symbol",
        "ZapfDingBats",
    ];
    STANDARD.iter().any(|s| s.eq_ignore_ascii_case(name))
}

/// PDF text strings are PDFDocEncoding (≈ASCII for our purposes) or UTF-16BE
/// with a BOM.
fn pdf_text(obj: &Object) -> Option<String> {
    let bytes = obj.as_str().ok()?;
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
            .collect();
        return Some(String::from_utf16_lossy(&units));
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        return Some(String::from_utf16_lossy(&units));
    }
    let text: String = bytes.iter().map(|b| *b as char).collect();
    let text = text.trim_matches('\0').trim();
    (!text.is_empty()).then(|| text.to_string())
}

#[cfg(test)]
// Shared with ook.rs tests: the synthetic PDFs keep the suite runnable
// without committing any binary fixture.
pub(crate) mod tests {
    use super::*;

    /// Assemble objects into a PDF with a valid xref table (no external
    /// fixture, so the suite needs no committed binary sample).
    pub(crate) fn assemble(objects: &[String]) -> Vec<u8> {
        let mut out = String::from("%PDF-1.4\n");
        let mut offsets = Vec::new();
        for (i, body) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.push_str(&format!("{} 0 obj\n{}\nendobj\n", i + 1, body));
        }
        let xref_at = out.len();
        out.push_str(&format!("xref\n0 {}\n", objects.len() + 1));
        out.push_str("0000000000 65535 f \n");
        for offset in &offsets {
            out.push_str(&format!("{offset:010} 00000 n \n"));
        }
        out.push_str(&format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
            objects.len() + 1
        ));
        out.into_bytes()
    }

    /// One page, base-14 Helvetica text, Title in the Info dictionary.
    pub(crate) fn write_latin_pdf(path: &Path) {
        let content = "BT /F1 24 Tf 20 100 Td (Hello PDF) Tj ET";
        let objects = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] \
             /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>"
                .to_string(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
            format!("<< /Length {} >>\nstream\n{}\nendstream", content.len(), content),
            "<< /Title (Spike Sample) /Author (Tester) >>".to_string(),
        ];
        // The Info dictionary needs a trailer reference; rebuild with it.
        let mut pdf = assemble(&objects);
        let marker = b"/Root 1 0 R >>";
        if let Some(at) = pdf
            .windows(marker.len())
            .position(|w| w == marker.as_slice())
        {
            let insert = b"/Root 1 0 R /Info 6 0 R >>";
            pdf.splice(at..at + marker.len(), insert.iter().copied());
        }
        std::fs::write(path, pdf).unwrap();
    }

    /// One page whose only font is a **non-embedded** CJK font referenced as a
    /// Type0/Identity-H font with no `/FontFile2` — the R1 case.
    pub(crate) fn write_cjk_unembedded_pdf(path: &Path) {
        let content = "BT /F1 24 Tf 20 100 Td <4E00 4E8C 4E09> Tj ET";
        let objects = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] \
             /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>"
                .to_string(),
            "<< /Type /Font /Subtype /Type0 /BaseFont /SimSun \
             /Encoding /Identity-H /DescendantFonts [6 0 R] >>"
                .to_string(),
            format!("<< /Length {} >>\nstream\n{}\nendstream", content.len(), content),
            "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /SimSun \
             /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
             /FontDescriptor 7 0 R /DW 1000 >>"
                .to_string(),
            "<< /Type /FontDescriptor /FontName /SimSun /Flags 4 \
             /FontBBox [-20 -200 1000 900] /ItalicAngle 0 /Ascent 880 \
             /Descent -120 /CapHeight 700 /StemV 80 >>"
                .to_string(),
        ];
        std::fs::write(path, assemble(&objects)).unwrap();
    }

    /// Like [`write_cjk_unembedded_pdf`], but the font carries a `ToUnicode`
    /// CMap — the shape a real "CID font, not embedded" PDF has. This is the
    /// case a substitute font *can* rescue, because codes map to Unicode first.
    pub(crate) fn write_cjk_with_tounicode_pdf(path: &Path) {
        let content = "BT /F1 24 Tf 20 100 Td <0001 0002 0003> Tj ET";
        let cmap = "/CIDInit /ProcSet findresource begin\n\
                    12 dict begin\nbegincmap\n\
                    /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n\
                    /CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n\
                    1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n\
                    3 beginbfchar\n<0001> <4E00>\n<0002> <4E8C>\n<0003> <4E09>\nendbfchar\n\
                    endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend";
        let objects = vec![
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] \
             /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>"
                .to_string(),
            "<< /Type /Font /Subtype /Type0 /BaseFont /SimSun \
             /Encoding /Identity-H /DescendantFonts [6 0 R] /ToUnicode 8 0 R >>"
                .to_string(),
            format!("<< /Length {} >>\nstream\n{}\nendstream", content.len(), content),
            "<< /Type /Font /Subtype /CIDFontType2 /BaseFont /SimSun \
             /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> \
             /FontDescriptor 7 0 R /DW 1000 >>"
                .to_string(),
            "<< /Type /FontDescriptor /FontName /SimSun /Flags 4 \
             /FontBBox [-20 -200 1000 900] /ItalicAngle 0 /Ascent 880 \
             /Descent -120 /CapHeight 700 /StemV 80 >>"
                .to_string(),
            format!("<< /Length {} >>\nstream\n{}\nendstream", cmap.len(), cmap),
        ];
        std::fs::write(path, assemble(&objects)).unwrap();
    }

    /// A font file that can stand in for a missing CJK font, when this machine
    /// has one. Used by the substitution experiment below.
    fn cjk_substitute_font() -> Option<(Vec<u8>, u32)> {
        for candidate in [
            r"C:\Windows\Fonts\simsun.ttc",
            r"C:\Windows\Fonts\msyh.ttc",
            r"C:\Windows\Fonts\simhei.ttf",
        ] {
            if let Ok(bytes) = std::fs::read(candidate) {
                return Some((bytes, 0));
            }
        }
        None
    }

    /// "Can a reader choose the font for a PDF?" — the experiment.
    ///
    /// A PDF that names a font it does not embed, with a `ToUnicode` CMap, is
    /// drawn with the supplied font instead of the standard-14 stand-in. Both
    /// renders are written to the test directory (`plain.png` /
    /// `substituted.png`) because **ink alone cannot tell them apart** — the
    /// default stand-in also inks the page (with `.notdef` boxes). What this
    /// asserts is that the substitute actually reaches the renderer; the
    /// correct glyph shapes were confirmed by eye:
    /// `cargo test -p iced-reader-pdf -- --nocapture substitute_font` then look
    /// at the two PNGs. A bare `Identity-H` font *without* `ToUnicode` cannot be
    /// rescued at all (its codes are glyph ids we do not have — see the canary
    /// above).
    #[test]
    fn substitute_font_rescues_non_embedded_cjk_when_tounicode_exists() {
        let Some(font) = cjk_substitute_font() else {
            return; // no CJK font on this machine: nothing to assert
        };
        let dir = temp_dir("cjk-substitute");
        let path = dir.join("cjk-tounicode.pdf");
        write_cjk_with_tounicode_pdf(&path);
        let doc = PdfDoc::open(&path).unwrap();

        let plain = doc.render_page_png(0, 400).unwrap();
        let rescued = doc.render_page_png_with(0, 400, Some(font)).unwrap();
        std::fs::write(dir.join("plain.png"), &plain.data).unwrap();
        std::fs::write(dir.join("substituted.png"), &rescued.data).unwrap();
        println!(
            "plain: ink {:.5}, {} bytes | substituted: ink {:.5}, {} bytes -> {}",
            plain.ink,
            plain.data.len(),
            rescued.ink,
            rescued.data.len(),
            dir.display()
        );
        assert_ne!(
            plain.data, rescued.data,
            "the substitute font must change what is drawn"
        );
    }

    pub(crate) fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("icedreader-pdf-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn opened_document_is_send_and_sync() {
        // The reader stores books behind `Book: Send + Sync`, and `PdfDoc`
        // holds the parsed hayro document, so this must hold.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<PdfDoc>();
    }

    #[test]
    fn opens_and_reports_page_count_and_info() {
        let dir = temp_dir("open");
        let path = dir.join("latin.pdf");
        write_latin_pdf(&path);

        let doc = PdfDoc::open(&path).unwrap();
        assert_eq!(doc.page_count(), 1);
        let info = doc.info();
        assert_eq!(info.title.as_deref(), Some("Spike Sample"));
        assert_eq!(info.author.as_deref(), Some("Tester"));
        assert!(doc.outline().is_empty(), "no outline in a minimal PDF");
    }

    #[test]
    fn renders_standard_font_page_with_ink() {
        let dir = temp_dir("render");
        let path = dir.join("latin.pdf");
        write_latin_pdf(&path);

        let doc = PdfDoc::open(&path).unwrap();
        let page = doc.render_page_png(0, 400).unwrap();
        assert_eq!(page.width, 400);
        assert!(page.height > 0);
        assert_eq!(&page.data[..4], b"\x89PNG", "encoded as PNG");
        assert!(page.data.len() > 500, "non-trivial PNG: {} bytes", page.data.len());
        // `embed-fonts` must have supplied a stand-in for Helvetica, otherwise
        // the page would come out blank.
        assert!(
            page.ink > 0.001,
            "page should contain glyph ink, got {}",
            page.ink
        );
    }

    #[test]
    fn reports_standard_font_as_not_embedded_but_resolvable() {
        let dir = temp_dir("fonts");
        let path = dir.join("latin.pdf");
        write_latin_pdf(&path);

        let doc = PdfDoc::open(&path).unwrap();
        let fonts = doc.fonts();
        assert_eq!(fonts.len(), 1, "{fonts:?}");
        assert_eq!(fonts[0].name, "Helvetica");
        assert!(!fonts[0].embedded, "base-14 fonts are normally not embedded");
        assert!(is_standard_font_name(&fonts[0].name));
        assert!(
            doc.unresolved_fonts().is_empty(),
            "standard fonts are supplied by hayro's embed-fonts feature"
        );
    }

    #[test]
    fn flags_non_embedded_cjk_font_as_unresolved() {
        let dir = temp_dir("cjk-fonts");
        let path = dir.join("cjk.pdf");
        write_cjk_unembedded_pdf(&path);

        let doc = PdfDoc::open(&path).unwrap();
        let unresolved = doc.unresolved_fonts();
        assert_eq!(unresolved.len(), 1, "{:?}", doc.fonts());
        assert_eq!(unresolved[0].name, "SimSun");
        assert_eq!(unresolved[0].subtype, "Type0");
        // The unrenderable shape: glyph-id codes and nothing to map them with.
        assert_eq!(unresolved[0].encoding.as_deref(), Some("Identity-H"));
        assert!(!unresolved[0].to_unicode);
    }

    /// Canary for the R1 limitation: this page cannot render its text today
    /// because hayro's `FontQuery::Fallback` is unimplemented upstream.
    /// Ignored so the suite stays green; run it to see the current state:
    /// `cargo test -p iced-reader-pdf -- --ignored cjk_unembedded`.
    #[test]
    #[ignore = "documents an upstream hayro limitation; asserts nothing about quality"]
    fn cjk_unembedded_page_ink_ratio() {
        let dir = temp_dir("cjk-render");
        let path = dir.join("cjk.pdf");
        write_cjk_unembedded_pdf(&path);

        let doc = PdfDoc::open(&path).unwrap();
        let page = doc.render_page_png(0, 400).unwrap();
        println!(
            "non-embedded CJK: ink ratio {:.5}, render {:.1} ms, parse {:.1} ms",
            page.ink, page.render_ms, page.parse_ms
        );
        // Today this is 0.0 (nothing is drawn). When hayro implements
        // `FontQuery::Fallback` this test starts reporting ink instead.
        assert!(page.width > 0);
    }
}
