//! `Book` / `BookOpener` adapter: **one PDF page = one spine unit**.
//!
//! Why one unit per page (and not one unit for the whole document): the reader
//! shell already navigates a book by spine units — chapter jumps, TOC entries,
//! chapter-boundary paging, `notes.md` section titles and the per-chapter char
//! weights all key off them. Making a page a unit lets PDF reuse every one of
//! those paths unchanged, and keeps `Locator` (`href` + fraction) meaningful:
//! the href names the page, the fraction stays 0.
//!
//! ## Who lays pages out
//!
//! The reading shell renders PDF pages itself, as a continuous strip of page
//! rasters (`ui/src/PdfView.tsx`), and persists progress as the page identity
//! `page/0007`. This adapter therefore serves **one raster per request**
//! ([`PdfBook::resource`]) and `chapter_html` exists only because the `Book`
//! trait requires it.
//!
//! ## Cost
//!
//! Page rasters are served as **JPEG**: a page is a picture of text, PNG costs
//! about as much as rasterising it and 5–10× the bytes. Every page request also
//! queues its neighbours on a prefetch worker, so turning the page is a cache
//! hit instead of 15–90 ms of rasterising plus encoding.
//!
//! The page HTML this adapter produces is deliberately bare — a single
//! `<div id="iced-reader-pdf-page">` holding one or more `<img>` with their
//! intrinsic size — with **no** styling. Page layout (fit page / fit width /
//! spread / continuous) is the parent's job, exactly like `flowLayout.ts` is
//! for EPUB content; see `ui/src/pdfLayout.ts`.

use std::collections::{HashSet, VecDeque};
use std::path::Path;
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};

use iced_reader_core::{
    extension_is, Book, BookOpener, CoreError, Metadata, Resource, SpineItem, TocNode, PDF_FORMAT,
};

use crate::{
    OutlineNode, PdfDoc, PdfError, RenderOptions, RenderedPage, DEFAULT_PAGE_WIDTH,
    PDF_PAGE_MEDIA_TYPE,
};

/// Rasterised pages held in memory. A 1440px page is ~150–250 KB of JPEG, so a
/// dozen pages is a couple of MB — and since the prefetcher keeps the pages
/// around the reader warm, this is what makes a page turn instant. Deliberately
/// **not** a whole-book disk cache: 570 pages × ~200 KB would still be ~110 MB.
const CACHE_PAGES: usize = 16;

/// Pages rendered ahead/behind the page being read.
const PREFETCH_RADIUS: usize = 2;

/// Requested raster widths are rounded **up** to this step.
///
/// The shell derives the width from the container and the display scale, so a
/// window resize or a zoom nudge would otherwise ask for a slightly different
/// width every time and miss the cache (re-rasterising the page each time).
/// Bucketing keeps the cache useful; the cost is at most `step` extra pixels.
const WIDTH_STEP: u32 = 128;

/// Identity of one cached raster.
type CachedPage = (String, u32, RenderedPage);

/// Round a requested raster width up to the cache bucket.
fn bucket_width(width: u32) -> u32 {
    let stepped = width.div_ceil(WIDTH_STEP) * WIDTH_STEP;
    stepped.clamp(WIDTH_STEP, 6000)
}

pub struct PdfOpener;

impl BookOpener for PdfOpener {
    fn format_id(&self) -> &'static str {
        PDF_FORMAT
    }

    fn can_open(&self, path: &Path) -> bool {
        extension_is(path, "pdf")
    }

    fn open(&self, path: &Path) -> Result<Box<dyn Book>, CoreError> {
        let book = PdfBook::open(path).map_err(|e| CoreError::msg(e.to_string()))?;
        Ok(Box::new(book))
    }
}

/// An open PDF: page list, outline, a small raster cache and a prefetch worker.
pub struct PdfBook {
    doc: Arc<PdfDoc>,
    spine: Vec<SpineItem>,
    toc: Vec<TocNode>,
    cache: Arc<Mutex<VecDeque<CachedPage>>>,
    /// Sends `(page, width)` to the prefetch worker. Dropped with the book,
    /// which ends the worker thread.
    prefetch: Option<Sender<(usize, u32)>>,
}

impl PdfBook {
    pub fn open(path: &Path) -> Result<Self, PdfError> {
        let doc = Arc::new(PdfDoc::open(path)?);
        let pages = doc.page_count();
        let outline = doc.outline();
        let titles = page_titles(&outline, pages);
        let spine: Vec<SpineItem> = (1..=pages)
            .map(|number| SpineItem {
                id: format!("p{number}"),
                href: page_href(number),
                media_type: PDF_PAGE_MEDIA_TYPE.to_string(),
                title: titles.get(number - 1).cloned().flatten(),
            })
            .collect();
        let toc = outline_to_toc(&outline);
        let cache = Arc::new(Mutex::new(VecDeque::new()));
        let prefetch = spawn_prefetch_worker(Arc::clone(&doc), Arc::clone(&cache));
        // No open-time warm-up on purpose: the raster width depends on the
        // shell's container and display scale (see `WIDTH_STEP`), so a guess
        // here would render pages the first request then re-renders at its own
        // width. The first request warms its neighbourhood at *its* width.
        Ok(Self {
            doc,
            spine,
            toc,
            cache,
            prefetch: Some(prefetch),
        })
    }

    /// Page metadata (Info dictionary), as the reader expects it.
    pub fn info(&self) -> crate::PdfInfo {
        self.doc.info()
    }

    /// Render a page (1-based), reusing the in-memory cache when possible.
    fn cached_page(&self, page: usize, width: u32) -> Result<RenderedPage, PdfError> {
        let href = page_href(page);
        {
            let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(index) = cache
                .iter()
                .position(|(cached, w, _)| cached == &href && *w == width)
            {
                let hit = cache.remove(index).expect("index just found");
                let rendered = hit.2.clone();
                cache.push_front(hit);
                return Ok(rendered);
            }
        }
        self.render_into_cache(page, width)
    }

    /// Rasterise a page and put it in the cache (used by requests and the
    /// prefetch worker).
    fn render_into_cache(&self, page: usize, width: u32) -> Result<RenderedPage, PdfError> {
        let rendered = self
            .doc
            .render_page(page - 1, &RenderOptions::webp(width))?;
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.push_front((page_href(page), width, rendered.clone()));
        cache.truncate(CACHE_PAGES);
        Ok(rendered)
    }

    /// Queue one page for the worker (no-op when there is no worker).
    fn send_prefetch(&self, page: usize, width: u32) -> bool {
        match &self.prefetch {
            Some(tx) => tx.send((page, width)).is_ok(),
            None => false,
        }
    }

    /// Queue the pages around `page` (1-based) for the worker: nearest first,
    /// so a page turn has its neighbour ready even if the queue is long.
    fn prefetch_around(&self, page: usize, width: u32) {
        for delta in 1..=PREFETCH_RADIUS {
            if let Some(back) = page.checked_sub(delta) {
                if back >= 1 {
                    self.send_prefetch(back, width);
                }
            }
            let forward = page + delta;
            if forward <= self.spine.len() {
                self.send_prefetch(forward, width);
            }
        }
    }
}

/// Long-lived worker rendering prefetch requests into the shared cache.
///
/// Requests are drained in batches (newest neighbours first, as sent) and pages
/// already cached are skipped, so a fast scroller cannot queue the same page
/// twice.
fn spawn_prefetch_worker(
    doc: Arc<PdfDoc>,
    cache: Arc<Mutex<VecDeque<CachedPage>>>,
) -> Sender<(usize, u32)> {
    let (tx, rx) = mpsc::channel::<(usize, u32)>();
    std::thread::spawn(move || {
        let mut requested: HashSet<(usize, u32)> = HashSet::new();
        while let Ok(first) = rx.recv() {
            let mut batch = vec![first];
            while let Ok(next) = rx.try_recv() {
                batch.push(next);
            }
            for (page, width) in batch {
                if !requested.insert((page, width)) {
                    continue;
                }
                let href = page_href(page);
                let cached = {
                    let cache = cache.lock().unwrap_or_else(|e| e.into_inner());
                    cache.iter().any(|(cached_href, cached_w, _)| {
                        cached_href == &href && *cached_w == width
                    })
                };
                if !cached {
                    if let Ok(rendered) = doc.render_page(page - 1, &RenderOptions::webp(width)) {
                        let mut cache = cache.lock().unwrap_or_else(|e| e.into_inner());
                        cache.push_front((href, width, rendered));
                        cache.truncate(CACHE_PAGES);
                    }
                }
                requested.remove(&(page, width));
            }
        }
    });
    tx
}

impl Book for PdfBook {
    fn format_id(&self) -> &'static str {
        PDF_FORMAT
    }

    fn metadata(&self) -> Metadata {
        let info = self.doc.info();
        let title = info
            .title
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty() && t != "Untitled")
            .unwrap_or_default();
        Metadata {
            title,
            // The Info dictionary holds one free-form author string; a `;`
            // separated list is the common convention for several authors.
            authors: info
                .author
                .map(|a| {
                    a.split(';')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
            language: None,
            publisher: None,
            // PDFs carry no portable identifier we trust for progress keys, so
            // the key falls back to `lib:<file name>.pdf` (see AGENTS).
            identifiers: Vec::new(),
            description: info.subject,
            cover_href: None,
        }
    }

    fn toc(&self) -> Vec<TocNode> {
        self.toc.clone()
    }

    fn spine(&self) -> Vec<SpineItem> {
        self.spine.clone()
    }

    /// Every page's size in PDF units — the shell lays out one continuous strip
    /// of page placeholders from this (see `ui/src/PdfView.tsx`).
    fn page_sizes(&self) -> Vec<(f32, f32)> {
        (0..self.spine.len())
            .map(|index| self.doc.page_size(index))
            .collect()
    }

    /// A bare page document holding one page when the
    /// One page as a minimal document — the `Book` trait requires this, but the
    /// reading shell no longer uses it for PDF: the continuous page strip
    /// (`ui/src/PdfView.tsx`) loads page rasters straight from [`Self::resource`]
    /// URLs and lays them out itself. Kept correct and small for other callers.
    fn chapter_html(&self, href: &str, resource_base: &str) -> Result<String, CoreError> {
        let page = parse_page_ref(href.split(['?', '#']).next().unwrap_or(href))
            .ok_or_else(|| CoreError::ChapterNotFound(href.to_string()))?;
        if page == 0 || page > self.spine.len() {
            return Err(CoreError::ChapterNotFound(href.to_string()));
        }
        let (w, h) = page_display_size(self.doc.as_ref(), page, DEFAULT_PAGE_WIDTH);
        self.send_prefetch(page, DEFAULT_PAGE_WIDTH);
        Ok(format!(
            "<!DOCTYPE html>\n<html>\n<head><meta charset=\"utf-8\"></head>\n\
             <body><div id=\"iced-reader-pdf-page\">\
             <img data-page=\"{page}\" width=\"{w}\" height=\"{h}\" \
             src=\"{resource_base}{}.webp?w={DEFAULT_PAGE_WIDTH}\" alt=\"第 {page} 页\">\
             </div></body>\n</html>\n",
            page_href(page)
        ))
    }

    fn resource(&self, href: &str) -> Result<Resource, CoreError> {
        let path = href.split('#').next().unwrap_or(href);
        let (path, query) = match path.split_once('?') {
            Some((path, query)) => (path, Some(query)),
            None => (path, None),
        };
        let page = parse_page_ref(path.trim_start_matches('/'))
            .ok_or_else(|| CoreError::ResourceNotFound(href.to_string()))?;
        if page == 0 || page > self.spine.len() {
            return Err(CoreError::ResourceNotFound(href.to_string()));
        }
        let width = query
            .and_then(|q| {
                q.split('&')
                    .find_map(|pair| pair.strip_prefix("w="))
                    .and_then(|w| w.parse::<u32>().ok())
            })
            .filter(|w| *w > 0)
            .unwrap_or(DEFAULT_PAGE_WIDTH)
            .clamp(200, 6000);
        // Same bucketing as the chapter request, so the `<img>` the shell was
        // given and the raster it asks for land on the same cache entry.
        let width = bucket_width(width);
        let rendered = self
            .cached_page(page, width)
            .map_err(|e| CoreError::msg(e.to_string()))?;
        self.prefetch_around(page, width);
        Ok(Resource {
            href: href.to_string(),
            media_type: rendered.format.media_type().to_string(),
            data: rendered.data,
        })
    }
}

/// The raster's intrinsic size for one page, used as the `<img>` `width`/
/// `height` attributes so a page lays out before its bytes arrive.
/// (The parent's CSS decides the *displayed* size.)
fn page_display_size(doc: &PdfDoc, page: usize, width: u32) -> (u32, u32) {
    let (w, h) = doc.page_size(page - 1);
    let height = if w > 0.0 {
        (width as f64 * (h as f64 / w as f64)).round() as u32
    } else {
        width
    };
    (width, height.max(1))
}

/// `page/{n:04}` — the spine href shape for page `n` (1-based), the identity
/// that progress, the TOC and the shelf all use.
fn page_href(page: usize) -> String {
    format!("page/{page:04}")
}

/// Parse a page reference: `page/0007`, `page/0007.webp`, `/page/0007.png`,
/// `page/0007?w=1200` → `7`. The extension and query are the raster request's,
/// not the identity's.
fn parse_page_ref(href: &str) -> Option<usize> {
    let rest = href
        .split(['?', '#'])
        .next()
        .unwrap_or(href)
        .trim_start_matches('/')
        .strip_prefix("page/")?;
    let digits = strip_image_extension(rest.trim_end_matches('/'));
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

fn strip_image_extension(name: &str) -> &str {
    for ext in [
        ".png", ".PNG", ".jpg", ".JPG", ".jpeg", ".JPEG", ".webp", ".WEBP",
    ] {
        if let Some(stem) = name.strip_suffix(ext) {
            return stem;
        }
    }
    name
}

/// Per-page TOC label: the nearest outline entry at or before the page.
fn page_titles(outline: &[OutlineNode], pages: usize) -> Vec<Option<String>> {
    let mut titles: Vec<Option<String>> = vec![None; pages];
    let mut flat: Vec<(usize, String)> = Vec::new();
    flatten_outline(outline, &mut flat);
    flat.sort_by_key(|(page, _)| *page);
    for (page, title) in flat {
        if page < pages {
            titles[page] = Some(title);
        }
    }
    let mut current: Option<String> = None;
    for slot in titles.iter_mut() {
        if slot.is_some() {
            current = slot.clone();
        } else {
            *slot = current.clone();
        }
    }
    titles
}

fn flatten_outline(nodes: &[OutlineNode], out: &mut Vec<(usize, String)>) {
    for node in nodes {
        if let Some(page) = node.page {
            if !node.title.trim().is_empty() {
                out.push((page, node.title.clone()));
            }
        }
        flatten_outline(&node.children, out);
    }
}

fn outline_to_toc(nodes: &[OutlineNode]) -> Vec<TocNode> {
    nodes
        .iter()
        .filter_map(|node| {
            let children = outline_to_toc(&node.children);
            let label = node.title.trim().to_string();
            let href = node.page.map(|page| page_href(page + 1));
            if label.is_empty() && href.is_none() && children.is_empty() {
                return None;
            }
            Some(TocNode {
                label,
                href,
                children,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    // Synthetic PDFs live with the other test helpers in `lib.rs` so the suite
    // needs no committed binary fixture.
    use crate::tests::{temp_dir, write_cjk_unembedded_pdf, write_latin_pdf};

    #[test]
    fn page_href_roundtrip() {
        assert_eq!(page_href(1), "page/0001");
        assert_eq!(page_href(1234), "page/1234");
        assert_eq!(parse_page_ref("page/0001"), Some(1));
        assert_eq!(parse_page_ref("/page/1234"), Some(1234));
        // Resource URLs arrive with the raster's extension and a width query.
        assert_eq!(parse_page_ref("page/0007.png"), Some(7));
        assert_eq!(parse_page_ref("page/0007.jpg"), Some(7));
        assert_eq!(parse_page_ref("page/0007.webp"), Some(7));
        assert_eq!(parse_page_ref("page/0007.webp?w=1200"), Some(7));
        assert_eq!(parse_page_ref("../page/0007.webp"), None);
        assert_eq!(parse_page_ref("page/"), None);
        assert_eq!(parse_page_ref("EPUB/ch1.xhtml"), None);
        assert_eq!(parse_page_ref("page/000x"), None);
    }

    #[test]
    fn page_titles_follow_the_last_outline_entry() {
        let outline = vec![
            OutlineNode {
                title: "第一章".into(),
                page: Some(0),
                children: vec![],
            },
            OutlineNode {
                title: "第二章".into(),
                page: Some(3),
                children: vec![],
            },
        ];
        let titles = page_titles(&outline, 6);
        assert_eq!(titles[0].as_deref(), Some("第一章"));
        assert_eq!(titles[2].as_deref(), Some("第一章"));
        assert_eq!(titles[3].as_deref(), Some("第二章"));
        assert_eq!(titles[5].as_deref(), Some("第二章"));
    }

    #[test]
    fn page_titles_are_empty_without_an_outline() {
        assert_eq!(page_titles(&[], 3), vec![None, None, None]);
    }

    #[test]
    fn toc_nodes_point_at_page_hrefs() {
        let outline = vec![OutlineNode {
            title: "封面".into(),
            page: Some(0),
            children: vec![OutlineNode {
                title: "序".into(),
                page: Some(2),
                children: vec![],
            }],
        }];
        let toc = outline_to_toc(&outline);
        assert_eq!(toc.len(), 1);
        assert_eq!(toc[0].label, "封面");
        assert_eq!(toc[0].href.as_deref(), Some("page/0001"));
        assert_eq!(toc[0].children[0].href.as_deref(), Some("page/0003"));
    }

    /// The adapter contract end to end: open, one spine unit per page, page
    /// HTML carries the page image URL, and fetching that URL yields an image
    /// (twice — the second hit comes from the cache).
    #[test]
    fn book_trait_serves_pages_as_chapters_and_resources() {
        let dir = temp_dir("book-trait");
        let path = dir.join("latin.pdf");
        write_latin_pdf(&path);

        let book = PdfOpener.open(&path).expect("open via opener");
        assert_eq!(book.format_id(), "pdf");

        let spine = book.spine();
        assert_eq!(spine.len(), 1);
        assert_eq!(spine[0].href, "page/0001");
        assert_eq!(spine[0].media_type, PDF_PAGE_MEDIA_TYPE);

        let metadata = book.metadata();
        assert_eq!(metadata.title, "Spike Sample");
        assert_eq!(metadata.authors, vec!["Tester".to_string()]);
        assert!(metadata.identifiers.is_empty(), "no id: key for PDFs");

        let html = book
            .chapter_html(&spine[0].href, "http://icedreader.localhost/book/x/")
            .expect("chapter html");
        assert!(
            html.contains("src=\"http://icedreader.localhost/book/x/page/0001.webp?w="),
            "page image URL must go through the app protocol: {html}"
        );
        // The page document is ours, but it stays free of reading skin: no
        // colours, fonts or max-width — the parent owns layout. `width`/
        // `height` are the raster's intrinsic size, not styling.
        assert!(!html.contains("style="), "{html}");
        assert!(!html.contains("style>"), "{html}");
        assert!(html.contains("id=\"iced-reader-pdf-page\""));
        assert!(html.contains("data-page=\"1\""));

        let res = book.resource("page/0001.webp?w=400").expect("page raster");
        assert_eq!(res.media_type, "image/webp");
        assert_eq!(&res.data[..4], b"RIFF", "WebP container");
        assert_eq!(&res.data[8..12], b"WEBP", "WebP payload");
        let again = book
            .resource("/page/0001.webp?w=400")
            .expect("cached raster");
        assert_eq!(again.data.len(), res.data.len());

        assert!(book.resource("page/9999.webp").is_err(), "out of range");
        assert!(book.chapter_html("EPUB/ch1.xhtml", "http://x/").is_err());
    }

    #[test]
    fn requested_widths_share_cache_buckets() {
        // Resizing the window must not re-rasterise the same page: nearby
        // requests land on one bucket.
        assert_eq!(bucket_width(700), 768);
        assert_eq!(bucket_width(720), 768);
        assert_eq!(bucket_width(768), 768);
        assert_eq!(bucket_width(769), 896);
        // Degenerate requests still produce a usable width.
        assert_eq!(bucket_width(1), WIDTH_STEP);
        assert_eq!(bucket_width(99_999), 6000);
    }

    /// The shell lays out one continuous strip of pages from these, so they
    /// must exist for every spine unit and carry the real aspect ratio.
    #[test]
    fn page_sizes_cover_every_page_with_the_real_aspect() {
        let dir = temp_dir("book-sizes");
        let path = dir.join("latin.pdf");
        write_latin_pdf(&path);
        let book = PdfOpener.open(&path).expect("open via opener");

        let sizes = book.page_sizes();
        assert_eq!(sizes.len(), book.spine().len(), "one size per page");
        // The synthetic page is a 300×200 MediaBox.
        let (w, h) = sizes[0];
        assert!(
            (w / h - 1.5).abs() < 0.01,
            "aspect should follow the page box, got {w}×{h}"
        );
    }

    /// `chapter_html` is a fallback for callers other than the shell; it must
    /// still be one correct page with a reserved box.
    #[test]
    fn chapter_html_emits_one_page_with_its_box() {
        let dir = temp_dir("book-chapter");
        let path = dir.join("latin.pdf");
        write_latin_pdf(&path);
        let book = PdfOpener.open(&path).expect("open via opener");

        let html = book
            .chapter_html("page/0001", "http://x/")
            .expect("chapter html");
        assert_eq!(html.matches("<img ").count(), 1, "{html}");
        assert!(html.contains("data-page=\"1\""), "{html}");
        assert!(html.contains("width=\""), "{html}");
        assert!(book.chapter_html("page/0002", "http://x/").is_err());

        // A raster request is bucketed so window resizes reuse the cache: two
        // nearby widths must land on the same bucket and so on one cache entry.
        let bucketed = bucket_width(900);
        let res = book
            .resource("page/0001.webp?w=900")
            .expect("bucketed raster");
        assert_eq!(&res.data[..4], b"RIFF");
        assert_eq!(bucket_width(bucketed - 10), bucketed);
        let same = book
            .resource(&format!("page/0001.webp?w={}", bucketed - 10))
            .expect("same bucket");
        assert_eq!(same.data.len(), res.data.len());
    }

    /// A PDF whose only visible text uses a non-embedded CJK font is the one
    /// case the reader cannot draw — the adapter still opens it, and the
    /// import-time check reports the font.
    #[test]
    fn unrenderable_visible_text_is_reported_by_the_risk_check() {
        let dir = temp_dir("book-risk");
        let path = dir.join("cjk.pdf");
        write_cjk_unembedded_pdf(&path);

        let doc = PdfDoc::open(&path).unwrap();
        assert_eq!(doc.visible_text_risk(), vec!["SimSun".to_string()]);
    }

    /// Measures what the prefetcher buys: after one page request, its
    /// neighbours are already rasterised, so the next page turn is a cache hit.
    ///
    /// `cargo test -p iced-reader-pdf -- --ignored --nocapture prefetch_pays_off`
    #[test]
    #[ignore = "needs a real multi-page PDF next to the repo"]
    fn prefetch_pays_off() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let Ok(read) = std::fs::read_dir(&repo_root) else {
            return;
        };
        let mut paths: Vec<std::path::PathBuf> = read
            .filter_map(|item| item.ok())
            .map(|item| item.path())
            .filter(|path| {
                path.is_file()
                    && path
                        .extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
            })
            .collect();
        paths.sort();

        for path in paths {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let book = PdfBook::open(&path).expect("open");
            let width = bucket_width(1200);

            // Go through `resource` — that is the path the browser takes, and
            // the one that queues the neighbours (at the *requested* width).
            let href = |page: usize| format!("page/{page:04}.webp?w={width}");
            let t0 = std::time::Instant::now();
            let _ = book.resource(&href(10)).expect("page 10");
            let cold = t0.elapsed().as_secs_f64() * 1000.0;

            // Give the worker time to render the neighbours it was asked for.
            std::thread::sleep(std::time::Duration::from_millis(2500));

            let t1 = std::time::Instant::now();
            let _ = book.resource(&href(11)).expect("page 11");
            let warm = t1.elapsed().as_secs_f64() * 1000.0;

            println!("{name}: cold page {cold:.1} ms | neighbour after prefetch {warm:.1} ms");
            assert!(
                warm * 4.0 < cold.max(4.0),
                "a prefetched neighbour ({warm:.1} ms) should be far cheaper than a cold page ({cold:.1} ms)"
            );
        }
    }

    /// Runs against the real, uncommitted sample PDFs when they sit next to the
    /// repository (same idea as `formats-epub`'s `user_epubs_if_present`): the
    /// adapter must hold up on real books, not only on synthetic ones.
    ///
    /// Ignored by default because opening two dozen-MB PDFs (twice each) takes
    /// ~70 s: `cargo test -p iced-reader-pdf -- --ignored real_samples`.
    #[test]
    #[ignore = "reads the real sample PDFs next to the repo; ~70 s"]
    fn real_samples_if_present() {
        let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let samples = [
            // Scanned pages + invisible OCR text layer, no outline.
            "經濟漩渦.pdf",
            // Real text with embedded subset fonts, 148 outline entries.
            "Windows Everywhere - Paul Thurrott.pdf",
            // Pure scan: no text layer at all.
            "语文开窍(带目录), 1版 - 李德身 - 1987 - — - 24434886851d32d6996a96125cfd1aeb (z-library.sk, 1lib.sk, z-lib.sk).pdf",
        ];
        for name in samples {
            let path = repo_root.join(name);
            if !path.is_file() {
                continue;
            }
            let book = PdfOpener
                .open(&path)
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            let doc = PdfDoc::open(&path).unwrap();
            let pages = doc.page_count();
            let spine = book.spine();
            assert_eq!(spine.len(), pages, "{name}: one spine unit per page");
            assert_eq!(spine[0].href, "page/0001");
            assert_eq!(spine[pages - 1].href, format!("page/{pages:04}"));

            // Every outline entry must resolve to a page href of this book.
            fn check(nodes: &[TocNode], pages: usize, name: &str) {
                for node in nodes {
                    if let Some(href) = &node.href {
                        let page = parse_page_ref(href)
                            .unwrap_or_else(|| panic!("{name}: bad toc href {href}"));
                        assert!(page >= 1 && page <= pages, "{name}: {href} out of range");
                    }
                    check(&node.children, pages, name);
                }
            }
            check(&book.toc(), pages, name);

            // Page 1 must render to a real image through the resource path.
            let res = book
                .resource("page/0001.webp?w=400")
                .unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(&res.data[..4], b"RIFF", "{name}");
            assert!(res.data.len() > 1000, "{name}: {}", res.data.len());
            // 經濟漩渦's non-embedded fonts only back an invisible OCR layer.
            assert!(
                doc.visible_text_risk().is_empty(),
                "{name}: unexpected visible-text risk"
            );
        }
    }
}
