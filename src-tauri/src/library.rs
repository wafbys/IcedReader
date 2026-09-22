use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use iced_reader_core::{
    book_extension, book_stem, clean_title, progress_key, read_meta_file, resolved_title, Locator,
    ProgressStore, PDF_FORMAT,
};
use serde::Serialize;

use crate::book_signals;
use crate::openers;
use crate::portable;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryEntry {
    pub path: String,
    pub file_name: String,
    pub title: String,
    pub authors: Vec<String>,
    pub progress_key: String,
    pub chapter_index: Option<u32>,
    pub chapter_count: Option<u32>,
    pub chapter_title: Option<String>,
    pub fraction: Option<f64>,
    pub updated_at: Option<i64>,
    pub has_cover: bool,
    /// Length + mtime so the cover URL changes when the same filename is replaced.
    pub cover_rev: String,
    /// Book file size in bytes (0 when it cannot be stat'ed). Shown in the shelf
    /// tooltip and the 编辑元数据 panel; the same length also rides inside
    /// `cover_rev`, but that string is a cache key, not something to display.
    pub size_bytes: u64,
    pub open_error: Option<String>,
    /// 优/良/中 from the cached first-import book signals (rev valid only).
    pub quality: Option<String>,
    /// Measured facts and merits behind the grade (shown in the shelf tooltip).
    pub quality_plus: Vec<String>,
    /// What held the book back — defects / missing provenance (empty on 优).
    pub quality_minus: Vec<String>,
    /// File names of other library books judged the same book (hint only).
    pub duplicates: Vec<String>,
    /// Live OPF identifier class (not sent to the UI). Replaces a stale cached
    /// `idQuality` so a 10-digit Kindle id no longer keeps a 优 badge.
    #[serde(skip)]
    pub id_quality: book_signals::IdQuality,
}

/// Un-cached shelf listing used by the in-crate tests below.
#[cfg(test)]
pub fn list_library_in(dir: &Path, progress: &ProgressStore) -> Vec<LibraryEntry> {
    let mut cache = LibraryMetaCache::default();
    list_library_cached(dir, progress, &mut cache, &book_signals::read_all())
}

/// Shelf listing with a caller-supplied signals cache (tests: grading glue
/// without writing into the real portable `data/` directory).
#[cfg(test)]
pub fn list_library_with_signals(
    dir: &Path,
    progress: &ProgressStore,
    signals: &HashMap<String, book_signals::BookSignals>,
) -> Vec<LibraryEntry> {
    let mut cache = LibraryMetaCache::default();
    list_library_cached(dir, progress, &mut cache, signals)
}

/// Book-shelf listing without re-opening every book: file-bound metadata
/// (title/authors/spine…) is cached per file revision, so only the progress
/// fields are re-read from the (in-memory) store on each call. Opening and
/// flattening the TOC of a big book (资治通鉴: ~1.4 s) then only happens once
/// per changed file instead of on every shelf refresh.
#[derive(Default)]
pub struct LibraryMetaCache {
    books: HashMap<PathBuf, (String, BookProfile)>,
}

impl LibraryMetaCache {
    pub fn profile(&mut self, path: &Path, library_dir: &Path) -> BookProfile {
        let rev = file_rev(path);
        if let Some((cached_rev, profile)) = self.books.get(path) {
            if cached_rev == &rev {
                return profile.clone();
            }
        }
        let profile = profile_book(path, library_dir);
        self.books
            .insert(path.to_path_buf(), (rev, profile.clone()));
        profile
    }

    /// Drop one book after deletion (keeps the map from accumulating dead entries).
    pub fn remove(&mut self, path: &Path) {
        self.books.remove(path);
    }
}

/// File-bound shelf metadata; reusable across listing calls while the file is
/// unchanged (see [`LibraryMetaCache`]).
#[derive(Debug, Clone)]
pub struct BookProfile {
    pub file_name: String,
    pub title: String,
    pub authors: Vec<String>,
    pub progress_key: String,
    /// Reading-order hrefs (flattened TOC/spine). Empty when the book fails to open.
    pub chapter_hrefs: Vec<String>,
    pub chapter_titles: Vec<Option<String>>,
    pub has_cover: bool,
    pub open_error: Option<String>,
    pub id_quality: book_signals::IdQuality,
}

impl BookProfile {
    pub fn chapter_count(&self) -> Option<u32> {
        (!self.chapter_hrefs.is_empty()).then_some(self.chapter_hrefs.len() as u32)
    }
}

pub fn list_library_cached(
    dir: &Path,
    progress: &ProgressStore,
    cache: &mut LibraryMetaCache,
    signals: &HashMap<String, book_signals::BookSignals>,
) -> Vec<LibraryEntry> {
    let entries: Vec<LibraryEntry> = read_book_paths(dir)
        .into_iter()
        .map(|path| {
            let mut entry = entry_from(&path, &cache.profile(&path, dir), progress);
            // Companion md overlays the file-bound title (displayTitle → joined
            // fields → dc:title/file name). Read per listing so a metadata edit
            // shows up immediately without touching the epub-rev profile cache.
            if let Ok(meta_path) = meta_path_for(dir, &entry.file_name) {
                if let Some(meta) = read_meta_file(&meta_path) {
                    entry.title = resolved_title(Some(&meta), &entry.title);
                    if !meta.author.trim().is_empty() {
                        entry.authors = meta
                            .author
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                }
            }
            entry
        })
        .collect();
    enrich_and_sort(entries, signals)
}

fn quality_rank(quality: Option<&str>) -> u8 {
    match quality {
        Some("优") => 3,
        Some("良") => 2,
        Some("中") => 1,
        _ => 0,
    }
}

/// Attach cached quality grades (rev-valid only), hint duplicate books
/// (same-typesetting repack, or a same-edition different repack), then sort:
/// recently read first, then grade, then title.
///
/// The signals cache is passed in (read once per listing by the caller) so
/// grading is testable without touching the real portable `data/` directory.
fn enrich_and_sort(
    mut entries: Vec<LibraryEntry>,
    all: &HashMap<String, book_signals::BookSignals>,
) -> Vec<LibraryEntry> {
    for e in entries.iter_mut() {
        if e.open_error.is_some() {
            continue;
        }
        let Some(sig) = all.get(&e.file_name) else {
            continue;
        };
        if sig.rev != e.cover_rev {
            continue; // file changed since the cached analysis; keep unknown
        }
        // Re-classify from the live OPF so an old cache that stored a 10-digit
        // Kindle id as Isbn does not keep granting 优 until the book is reopened.
        // Two formats, two questions: an EPUB is graded on text cleanliness and
        // apparatus, a PDF on what the reader can do with it (text layer, font
        // embedding, outline) — see `book_signals::grade_pdf`.
        let g = if sig.pdf.is_some() {
            book_signals::grade_pdf(sig)
        } else {
            book_signals::grade_with_id(sig, e.id_quality)
                .with_filename_isbn(e.id_quality, &e.file_name)
        };
        e.quality = Some(g.label.to_string());
        e.quality_plus = g.plus;
        e.quality_minus = g.minus;
    }

    // Same-typesetting groups (equal chapter-text fingerprint). An empty
    // fingerprint means "not computed" (or a PDF, whose signals carry their own
    // coarse one) and must never group unrelated books together.
    let valid: Vec<(usize, &book_signals::BookSignals)> = entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let s = all.get(&e.file_name)?;
            (s.rev == e.cover_rev && !s.fingerprint.is_empty()).then_some((i, s))
        })
        .collect();
    let mut by_fp: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, s) in &valid {
        by_fp.entry(s.fingerprint.as_str()).or_default().push(*i);
    }
    for idxs in by_fp.values().filter(|v| v.len() > 1) {
        for i in idxs {
            let others: Vec<String> = idxs
                .iter()
                .filter(|j| *j != i)
                .map(|j| entries[*j].file_name.clone())
                .collect();
            for other in others {
                if !entries[*i].duplicates.contains(&other) {
                    entries[*i].duplicates.push(other);
                }
            }
        }
    }
    // Same-edition hint across different fingerprints: identical heading
    // sequence and near-equal total length (repacks that moved files around).
    for a in 0..valid.len() {
        for b in (a + 1)..valid.len() {
            let (ia, sa) = valid[a];
            let (ib, sb) = valid[b];
            if sa.fingerprint == sb.fingerprint {
                continue;
            }
            if sa.headings != sb.headings {
                continue;
            }
            if sa.chars == 0 || sb.chars == 0 {
                continue;
            }
            let ratio = (sa.chars.max(sb.chars) - sa.chars.min(sb.chars)) as f64
                / sa.chars.max(sb.chars) as f64;
            if ratio > 0.02 {
                continue;
            }
            let name_a = entries[ia].file_name.clone();
            let name_b = entries[ib].file_name.clone();
            if !entries[ia].duplicates.contains(&name_b) {
                entries[ia].duplicates.push(name_b);
            }
            if !entries[ib].duplicates.contains(&name_a) {
                entries[ib].duplicates.push(name_a);
            }
        }
    }
    // De-duplicate hints regardless of which pass added them.
    for e in &mut entries {
        let mut seen: Vec<String> = Vec::with_capacity(e.duplicates.len());
        for d in e.duplicates.drain(..) {
            if !seen.contains(&d) {
                seen.push(d);
            }
        }
        e.duplicates = seen;
    }

    // How the copies compare is deliberately *not* folded in here: a book's
    // 依据 must describe the book itself, or it changes whenever an unrelated
    // copy appears on the shelf. The relative verdicts live in the 同书对照
    // panel (`docs/ideas/book-compare.md`).

    entries.sort_by(|a, b| {
        let q = |e: &LibraryEntry| quality_rank(e.quality.as_deref());
        match (b.updated_at, a.updated_at) {
            (Some(x), Some(y)) => x
                .cmp(&y)
                .then_with(|| q(b).cmp(&q(a)))
                .then_with(|| a.title.cmp(&b.title)),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => q(b).cmp(&q(a)).then_with(|| a.title.cmp(&b.title)),
        }
    });
    entries
}

/// Cover bytes cache keyed by file name; every hit is validated against the
/// file revision, so a replaced epub re-reads its cover exactly once and an
/// unchanged one is served from memory instead of re-opening the whole
/// archive on every shelf visit.
#[derive(Default)]
pub struct CoverCache {
    /// file_name → (rev, media type, bytes)
    covers: HashMap<String, (String, String, Vec<u8>)>,
}

/// Keep memory bounded: the biggest sample covers are several MB each, so a
/// modest cap stays cheap while covering realistic shelf sizes.
const COVER_CACHE_MAX: usize = 32;

impl CoverCache {
    pub fn get(&self, file_name: &str, rev: &str) -> Option<(&str, &[u8])> {
        self.covers
            .get(file_name)
            .filter(|(cached_rev, _, _)| cached_rev == rev)
            .map(|(_, media, data)| (media.as_str(), data.as_slice()))
    }

    pub fn insert(&mut self, file_name: &str, rev: String, media: String, data: Vec<u8>) {
        if self.covers.len() >= COVER_CACHE_MAX {
            self.covers.clear();
        }
        self.covers
            .insert(file_name.to_string(), (rev, media, data));
    }

    /// Drop one book's cover after deletion.
    pub fn remove(&mut self, file_name: &str) {
        self.covers.remove(file_name);
    }
}

/// Cover bytes for one library book: EPUBs hand over their declared cover
/// resource, PDFs render page 1 (a PDF has no cover image to point at).
/// A 2×2 paper-coloured PNG served **instead of rendering** a cover inside a
/// protocol request.
///
/// Rendering a PDF cover costs real CPU — 450 ms in release and ~9 s in an
/// unoptimised dev build (measured on the sample books) — and a protocol
/// request is a synchronous callback: doing that work there made the whole
/// shelf crawl (the pre-PDF shelf only copied bytes out of a zip). A miss
/// therefore answers with this placeholder and warms the cache on a worker
/// thread; the shelf notices it by `naturalWidth <= 2` and retries.
pub const COVER_PLACEHOLDER_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00, 0x02, 0x08, 0x06, 0x00, 0x00, 0x00, 0x72, 0xb6, 0x0d,
    0x24, 0x00, 0x00, 0x00, 0x01, 0x73, 0x52, 0x47, 0x42, 0x00, 0xae, 0xce, 0x1c, 0xe9, 0x00, 0x00,
    0x00, 0x04, 0x67, 0x41, 0x4d, 0x41, 0x00, 0x00, 0xb1, 0x8f, 0x0b, 0xfc, 0x61, 0x05, 0x00, 0x00,
    0x00, 0x09, 0x70, 0x48, 0x59, 0x73, 0x00, 0x00, 0x0e, 0xc3, 0x00, 0x00, 0x0e, 0xc3, 0x01, 0xc7,
    0x6f, 0xa8, 0x64, 0x00, 0x00, 0x00, 0x11, 0x49, 0x44, 0x41, 0x54, 0x18, 0x57, 0x63, 0xf8, 0xf6,
    0xf1, 0xf9, 0x7f, 0x10, 0x66, 0x80, 0x31, 0x00, 0x88, 0xc4, 0x0f, 0x35, 0xbe, 0xd8, 0x5b, 0xf0,
    0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

/// Warm one book's cover **on a worker thread**. Called by the shelf scan (so
/// the first cover request usually hits) and by a request that missed. Cheap to
/// call repeatedly: it skips anything already cached for the file revision.
pub fn warm_cover_in_background(
    path: PathBuf,
    file_name: String,
    cache: std::sync::Arc<std::sync::Mutex<CoverCache>>,
) {
    std::thread::spawn(move || {
        let rev = file_rev(&path);
        // Render **outside** the lock: a PDF cover costs hundreds of ms and
        // holding the cache lock across it would block every other cover
        // request. Check, render, then insert under the lock (a duplicate
        // render from a racing warm is cheaper than the stall).
        {
            let Ok(cache) = cache.lock() else {
                return;
            };
            if cache.get(&file_name, &rev).is_some() {
                return;
            }
        }
        let Ok((media, data)) = cover_bytes(&path) else {
            return;
        };
        let Ok(mut cache) = cache.lock() else {
            return;
        };
        if cache.get(&file_name, &rev).is_none() {
            cache.insert(&file_name, rev, media, data);
        }
    });
}

pub fn cover_bytes(path: &Path) -> crate::error::Result<(String, Vec<u8>)> {
    let opener = openers::opener_for(path).ok_or_else(|| "不支持的格式".to_string())?;
    if opener.format_id() == PDF_FORMAT {
        return Ok(iced_reader_pdf::cover(path, PDF_COVER_WIDTH)?);
    }
    let book = opener.open(path)?;
    let href = book
        .metadata()
        .cover_href
        .ok_or_else(|| "no cover".to_string())?;
    let res = book.resource(&href)?;
    if res.data.is_empty() {
        return Err("empty cover".into());
    }
    Ok((res.media_type, res.data))
}

/// Shelf cover width for PDFs; the shelf shows ~200 px thumbnails and the
/// in-process cover cache keys on the file revision, so one render per book.
const PDF_COVER_WIDTH: u32 = 400;

pub fn library_cover_path(file_name: &str) -> crate::error::Result<PathBuf> {
    let as_path = Path::new(file_name);
    if file_name.is_empty()
        || as_path
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err("invalid cover name".into());
    }
    let dir = portable::library_dir()?;
    let path = dir.join(file_name);
    if !path.is_file() {
        return Err("book not in library".into());
    }
    Ok(path)
}

/// Companion metadata path for a library book (`三体.epub` → `三体.md`). Only
/// a plain file name inside `dir` is accepted (no separators / `..`), mirroring
/// [`delete_book_from`] and [`library_cover_path`].
pub fn meta_path_for(dir: &Path, file_name: &str) -> crate::error::Result<PathBuf> {
    let as_path = Path::new(file_name);
    if file_name.is_empty()
        || as_path
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err("invalid book file name".into());
    }
    Ok(dir.join(as_path).with_extension("md"))
}

/// Companion notes archive `<stem>.notes.md` (划线+备注档案，删除留痕)。
/// Same name guard as [`meta_path_for`]: plain file name only, and the
/// extension swap keeps `<stem>.epub → <stem>.notes.md`.
pub fn notes_path_for(dir: &Path, file_name: &str) -> crate::error::Result<PathBuf> {
    let as_path = Path::new(file_name);
    if file_name.is_empty()
        || as_path
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err("invalid book file name".into());
    }
    Ok(dir.join(as_path).with_extension("notes.md"))
}

/// Turn a display title into a usable file stem for the library directory:
/// fold whitespace (via [`iced_reader_core::clean_title`]), replace Windows-
/// forbidden characters (`<>:"/\|?*`) and control chars with spaces, trim
/// trailing dots/spaces, cap the length, and never return empty.
/// Full-width characters are kept intact (they are legal in file names).
pub fn clean_file_stem(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        let c = ch as u32;
        if matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*') || c < 0x20 {
            out.push(' ');
        } else {
            out.push(ch);
        }
    }
    let collapsed = clean_title(&out);
    let trimmed = collapsed.trim_end_matches([' ', '.']);
    const MAX_STEM_CHARS: usize = 180;
    let mut stem: String = trimmed.chars().take(MAX_STEM_CHARS).collect();
    stem = stem.trim_end_matches([' ', '.']).to_string();
    if stem.is_empty() {
        stem = "未命名".into();
    }
    stem
}

/// Pick a stem that does not collide with any existing library file.
/// Collision copies use ` (2)`, ` (3)`… — not `-N`, which `lib:` progress
/// keys treat as the same book. Case-insensitive, like NTFS.
/// `preferred` is already clean (see [`clean_file_stem`]); `ignore` file names
/// (the book being renamed) do not count as taken, so a re-save does not bump
/// `三体 (2)` to `(3)`.
pub fn unique_stem_ignoring(dir: &Path, preferred: &str, ignore: &[&str]) -> String {
    let Ok(read) = fs::read_dir(dir) else {
        return preferred.to_string();
    };
    let ignore_lower: std::collections::HashSet<String> =
        ignore.iter().map(|n| n.to_lowercase()).collect();
    let taken: std::collections::HashSet<String> = read
        .filter_map(|item| item.ok())
        .filter(|item| item.path().is_file())
        .filter_map(|item| item.file_name().to_str().map(|n| n.to_lowercase()))
        .filter(|name| !ignore_lower.contains(name))
        .filter(|name| name.ends_with(".md") || book_extension(name).is_some())
        .collect();
    if !stem_taken(&taken, preferred) {
        return preferred.to_string();
    }
    let mut n = 2;
    loop {
        let candidate = format!("{preferred} ({n})");
        if !stem_taken(&taken, &candidate) {
            return candidate;
        }
        n += 1;
    }
}

/// A stem counts as taken when any format's book file, companion md or notes
/// archive already uses it.
fn stem_taken(taken: &std::collections::HashSet<String>, stem: &str) -> bool {
    let s = stem.to_lowercase();
    taken.contains(&format!("{s}.md"))
        || taken.contains(&format!("{s}.notes.md"))
        || iced_reader_core::BOOK_EXTENSIONS
            .iter()
            .any(|ext| taken.contains(&format!("{s}.{ext}")))
}

/// File stem of a library book name (`Foo.EPUB` → `Foo`, `Foo.pdf` → `Foo`).
pub fn epub_stem(file_name: &str) -> &str {
    book_stem(file_name)
}

/// Rename a library book's file to `new_stem` (already clean + unique), keeping
/// the format's extension, and drop its old companion md — the caller writes
/// the md under the new name right after, so moving the old md first would only
/// add a second failing rename point. Returns the new file name. Missing old md
/// is fine; unrelated files are untouched. A rename that fails after the book
/// file moved leaves a half-renamed state (book under the new name, no md) —
/// extremely unlikely, reported as an error so the shelf reload reflects
/// reality.
pub fn rename_book_files(
    dir: &Path,
    old_file_name: &str,
    new_stem: &str,
) -> crate::error::Result<String> {
    let as_path = Path::new(old_file_name);
    if old_file_name.is_empty()
        || as_path
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err("invalid book file name".into());
    }
    let book_old = dir.join(old_file_name);
    if !book_old.is_file() {
        return Err("book not in library".into());
    }
    // Keep the format: renaming `三体.pdf` yields `新名.pdf`, never `.epub`.
    let extension = book_extension(old_file_name).unwrap_or("epub");
    let new_name = format!("{new_stem}.{extension}");
    if old_file_name.eq_ignore_ascii_case(&new_name) {
        return Ok(old_file_name.to_string());
    }
    let book_new = dir.join(&new_name);
    if book_new.is_file() {
        return Err(format!("target already exists: {new_name}").into());
    }
    fs::rename(&book_old, &book_new)?;
    let md_old = meta_path_for(dir, old_file_name)?;
    if md_old.is_file() {
        // Best-effort: the new md is written right after this returns.
        let _ = fs::remove_file(&md_old);
    }
    // The notes archive travels with the stem (rename, not drop — it holds
    // the user's notes). Best-effort like the md above.
    let notes_old = notes_path_for(dir, old_file_name)?;
    if notes_old.is_file() {
        if let Ok(notes_new) = notes_path_for(dir, &new_name) {
            let _ = fs::rename(&notes_old, &notes_new);
        }
    }
    Ok(new_name)
}

/// Delete one library book file. Only a plain file name inside `dir` is
/// accepted (no separators / `..`), mirroring `library_cover_path`. The caller
/// is responsible for clearing the book's progress/annotation records.
pub fn delete_book_from(dir: &Path, file_name: &str) -> crate::error::Result<PathBuf> {
    let as_path = Path::new(file_name);
    if file_name.is_empty()
        || as_path
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err("invalid book file name".into());
    }
    let path = dir.join(file_name);
    if !path.is_file() {
        return Err("book not in library".into());
    }
    fs::remove_file(&path)?;
    // The companion md (user metadata) dies with the book; missing is fine.
    let _ = fs::remove_file(meta_path_for(dir, file_name)?);
    // The notes archive (划线+备注) dies with the book too.
    let _ = fs::remove_file(notes_path_for(dir, file_name)?);
    Ok(path)
}

/// Book files in the shelf directory (`*.epub`, `*.pdf`), sorted by name.
/// One level only: the shelf never walks sub-directories.
fn read_book_paths(dir: &Path) -> Vec<PathBuf> {
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = read
        .filter_map(|item| item.ok())
        .map(|item| item.path())
        .filter(|path| path.is_file() && openers::is_supported(path))
        .collect();
    paths.sort();
    paths
}

pub(crate) fn file_rev(path: &Path) -> String {
    let Ok(meta) = fs::metadata(path) else {
        return String::new();
    };
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{}-{}", meta.len(), mtime)
}

/// Book file size in bytes, for display (shelf tooltip / 编辑元数据). A missing
/// or unreadable file is 0 — the shelf still lists the entry so the user can
/// delete the broken book instead of the row vanishing.
fn file_size(path: &Path) -> u64 {
    fs::metadata(path).map(|meta| meta.len()).unwrap_or(0)
}

/// Slow path: open the book once and extract everything bound to the file
/// content (no progress). Route calls through [`LibraryMetaCache`] so that
/// unchanged books are not re-opened on every shelf refresh. Works for every
/// format the opener registry knows.
fn profile_book(path: &Path, library: &Path) -> BookProfile {
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "book".into());
    let fallback_title = book_stem(&file_name).to_string();

    let Some(opener) = openers::opener_for(path) else {
        // Defensive: the shelf only lists supported files, but keep the same
        // fallback-key rule so any unreadable entry still maps to its `lib:`
        // records.
        let key = progress_key(path, &[], Some(library));
        return BookProfile {
            file_name: file_name.clone(),
            title: fallback_title,
            authors: Vec::new(),
            progress_key: key,
            chapter_hrefs: Vec::new(),
            chapter_titles: Vec::new(),
            has_cover: false,
            open_error: Some("不支持的格式".into()),
            id_quality: book_signals::IdQuality::None,
        };
    };
    let is_pdf = opener.format_id() == PDF_FORMAT;

    match opener.open(path) {
        Ok(book) => {
            let meta = book.metadata();
            let title = if meta.title.trim().is_empty() || meta.title == "Untitled" {
                fallback_title
            } else {
                meta.title
            };
            let key = progress_key(path, &meta.identifiers, Some(library));
            let spine = book.spine();
            BookProfile {
                file_name: file_name.clone(),
                title,
                authors: meta.authors,
                progress_key: key,
                chapter_hrefs: spine.iter().map(|s| s.href.clone()).collect(),
                chapter_titles: spine.iter().map(|s| s.title.clone()).collect(),
                // A PDF has no cover resource; page 1 stands in for it, so it
                // always has one as long as it opened at all.
                has_cover: is_pdf || meta.cover_href.is_some(),
                open_error: None,
                // Quality signals are EPUB semantics (chapter text, headings,
                // embedded images); a PDF simply has none yet.
                id_quality: if is_pdf {
                    book_signals::IdQuality::None
                } else {
                    book_signals::best_id_quality(&meta.identifiers)
                },
            }
        }
        Err(err) => {
            // A book that used to open fine can later become unreadable (a
            // same-named replacement with a broken file). Keep deriving the
            // `lib:` progress key from the file name so the shelf still shows
            // its progress and 删除 clears the old records — AGENTS: 坏书也
            // 能删，同名重新导入进度从零。
            let key = progress_key(path, &[], Some(library));
            BookProfile {
                file_name: file_name.clone(),
                title: fallback_title,
                authors: Vec::new(),
                progress_key: key,
                chapter_hrefs: Vec::new(),
                chapter_titles: Vec::new(),
                has_cover: false,
                open_error: Some(err.to_string()),
                id_quality: book_signals::IdQuality::None,
            }
        }
    }
}

/// Combine a file-bound profile with the current progress record.
fn entry_from(path: &Path, profile: &BookProfile, progress: &ProgressStore) -> LibraryEntry {
    let rec = if profile.progress_key.is_empty() {
        None
    } else {
        progress.get(&profile.progress_key)
    };
    let (chapter_index, chapter_title) = match rec {
        Some(r) => locate_chapter(profile, &r.locator),
        None => (None, None),
    };
    LibraryEntry {
        path: path.to_string_lossy().into_owned(),
        file_name: profile.file_name.clone(),
        title: profile.title.clone(),
        authors: profile.authors.clone(),
        progress_key: profile.progress_key.clone(),
        chapter_index,
        chapter_count: profile.chapter_count(),
        chapter_title,
        fraction: rec.map(|r| r.locator.fraction.clamp(0.0, 1.0)),
        updated_at: rec.map(|r| r.updated_at),
        has_cover: profile.has_cover,
        cover_rev: file_rev(path),
        size_bytes: file_size(path),
        open_error: profile.open_error.clone(),
        quality: None,
        quality_plus: Vec::new(),
        quality_minus: Vec::new(),
        duplicates: Vec::new(),
        id_quality: profile.id_quality,
    }
}

fn locate_chapter(profile: &BookProfile, locator: &Locator) -> (Option<u32>, Option<String>) {
    let idx = profile
        .chapter_hrefs
        .iter()
        .position(|href| hrefs_match(href, &locator.href, true))
        .or_else(|| {
            profile
                .chapter_hrefs
                .iter()
                .position(|href| hrefs_match(href, &locator.href, false))
        });
    match idx {
        Some(i) => (
            Some(i as u32),
            profile.chapter_titles.get(i).cloned().flatten(),
        ),
        None => (None, None),
    }
}

fn hrefs_match(a: &str, b: &str, keep_fragment: bool) -> bool {
    let (file_a, frag_a) = split_href(a);
    let (file_b, frag_b) = split_href(b);
    let file_a = file_a.trim_start_matches('/');
    let file_b = file_b.trim_start_matches('/');
    if !file_a.eq_ignore_ascii_case(file_b) {
        return false;
    }
    if !keep_fragment {
        return true;
    }
    frag_a == frag_b
}

fn split_href(href: &str) -> (&str, Option<&str>) {
    let href = href.split('?').next().unwrap_or(href);
    match href.split_once('#') {
        Some((file, frag)) if !frag.is_empty() => (file, Some(frag)),
        Some((file, _)) => (file, None),
        None => (href, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced_reader_core::ProgressStore;

    #[test]
    fn clean_file_stem_sanitizes_and_caps() {
        // Windows-forbidden and control chars → spaces, whitespace folded.
        assert_eq!(clean_file_stem("三体：黑暗森林"), "三体：黑暗森林");
        assert_eq!(clean_file_stem("A:B"), "A B");
        assert_eq!(clean_file_stem("a/b\\c|d?e*f<g>h\"i"), "a b c d e f g h i");
        assert_eq!(clean_file_stem("  三体\u{3000}  二 "), "三体 二");
        // Trailing dots/spaces are illegal at the end of a Windows name.
        assert_eq!(clean_file_stem("书名..."), "书名");
        assert_eq!(clean_file_stem("书名. "), "书名");
        // Full-width characters survive untouched.
        assert_eq!(clean_file_stem("（未读·探索家）"), "（未读·探索家）");
        // Empty input never yields an empty stem.
        assert_eq!(clean_file_stem("   "), "未命名");
        assert_eq!(clean_file_stem("///"), "未命名");
        // Overlong stems are capped at 180 chars without panicking mid-char.
        let long = "书".repeat(400);
        assert_eq!(clean_file_stem(&long).chars().count(), 180);
    }

    #[test]
    fn unique_stem_avoids_existing_epub_and_md() {
        let root = std::env::temp_dir().join("icedreader-unique-stem");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        // Nothing taken → the preferred stem wins.
        assert_eq!(
            unique_stem_ignoring(&root, "三体 - 刘慈欣", &[]),
            "三体 - 刘慈欣"
        );

        fs::write(root.join("三体 - 刘慈欣.epub"), b"a").unwrap();
        fs::write(root.join("三体 - 刘慈欣-2.md"), b"b").unwrap();
        fs::write(root.join("OTHER.EPUB"), b"c").unwrap();
        // .epub collision → (2); existing (2) → (3). `-N` is not used (lib: aliases).
        assert_eq!(
            unique_stem_ignoring(&root, "三体 - 刘慈欣", &[]),
            "三体 - 刘慈欣 (2)"
        );
        assert_eq!(unique_stem_ignoring(&root, "other", &[]), "other (2)");
        fs::write(root.join("三体 - 刘慈欣 (2).epub"), b"d").unwrap();
        assert_eq!(
            unique_stem_ignoring(&root, "三体 - 刘慈欣", &[]),
            "三体 - 刘慈欣 (3)"
        );
        assert_eq!(
            unique_stem_ignoring(&root, "三体 - 刘慈欣", &["三体 - 刘慈欣.epub"]),
            "三体 - 刘慈欣"
        );
    }

    #[test]
    fn lists_a_pdf_with_page_units_and_a_lib_key() {
        let root = std::env::temp_dir().join("icedreader-library-pdf");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        write_tiny_pdf(&root.join("經濟漩渦.pdf"));

        let entries = list_library_in(&root, &ProgressStore::in_memory());
        assert_eq!(entries.len(), 1, "{entries:?}");
        let entry = &entries[0];
        assert_eq!(entry.file_name, "經濟漩渦.pdf");
        assert_eq!(entry.title, "经济漩涡：一个样本");
        // One page = one spine unit, so the shelf counts pages.
        assert_eq!(entry.chapter_count, Some(1));
        assert_eq!(entry.progress_key, "lib:經濟漩渦.pdf");
        assert!(entry.open_error.is_none());
        // No cover resource inside a PDF; page 1 stands in, so it has one.
        assert!(entry.has_cover);
        // The badge comes from the cached PDF signals; this test lists a temp
        // directory without populating that cache, so it stays ungraded here
        // (grading itself is covered by `book_signals::pdf_grades_follow_the_text_layer`).
        assert!(entry.quality.is_none());
    }

    #[test]
    fn a_pdf_and_an_epub_of_one_title_are_two_books() {
        let root = std::env::temp_dir().join("icedreader-library-two-formats");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        write_tiny_pdf(&root.join("同名书.pdf"));
        fs::copy(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures/sample.epub"),
            root.join("同名书.epub"),
        )
        .unwrap();

        let entries = list_library_in(&root, &ProgressStore::in_memory());
        assert_eq!(entries.len(), 2);
        let keys: Vec<&str> = entries.iter().map(|e| e.progress_key.as_str()).collect();
        assert!(keys.contains(&"lib:同名书.pdf"), "{keys:?}");
        // The EPUB keeps its own key (an `id:` one when it has an identifier),
        // never the PDF's `lib:` key.
        assert!(keys.iter().any(|k| !k.ends_with("同名书.pdf")), "{keys:?}");
    }

    #[test]
    fn shelf_badges_a_pdf_from_its_signals() {
        use iced_reader_pdf::{PdfQuality, TextLayer};

        /// An all-defaults signal set (the struct has no `Default` impl).
        fn empty(rev: String) -> book_signals::BookSignals {
            book_signals::BookSignals {
                rev,
                chars: 0,
                chapter_shas: Vec::new(),
                chapter_chars: Vec::new(),
                chapter_chars_kind: book_signals::CHAPTER_CHARS_PER_SPINE,
                fingerprint: "pdf-fp".into(),
                mojibake: 0,
                br_count: 0,
                empty_p: 0,
                img_count: 0,
                headings: Vec::new(),
                id_quality: book_signals::IdQuality::None,
                has_creator: false,
                img_files: 0,
                img_bytes: 0,
                img_truncated: false,
                img_substantial: 0,
                img_referenced: 0,
                img_referenced_substantial: 0,
                img_referenced_bytes: 0,
                img_css_only: 0,
                img_orphan: 0,
                img_orphan_bytes: 0,
                img_refs_truncated: false,
                word_notes: 0,
                missing_chars: 0,
                sup_count: 0,
                analysis_kind: book_signals::ANALYSIS_KIND,
                pdf: None,
            }
        }

        let root = std::env::temp_dir().join("icedreader-library-pdf-badge");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("带角标的书.pdf");
        write_tiny_pdf(&path);
        let rev = file_rev(&path);

        // A born-digital file: text + embedded fonts + outline.
        let mut signals = HashMap::new();
        signals.insert(
            "带角标的书.pdf".to_string(),
            book_signals::BookSignals {
                pdf: Some(PdfQuality {
                    pages: 300,
                    outline_entries: 42,
                    text_layer: TextLayer::MachineReadable,
                    sampled_pages: 24,
                    text_pages: 24,
                    fonts: 4,
                    embedded_fonts: 4,
                    unresolved_visible_fonts: Vec::new(),
                    has_title: true,
                    has_author: true,
                    encrypted: false,
                }),
                ..empty(rev.clone())
            },
        );

        let entries = list_library_with_signals(&root, &ProgressStore::in_memory(), &signals);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].quality.as_deref(), Some("优"));
        assert!(entries[0]
            .quality_plus
            .iter()
            .any(|r| r.contains("正文可提取文字")));
        assert!(entries[0]
            .quality_plus
            .iter()
            .any(|r| r.contains("含书签目录（42 条）")));
        assert!(entries[0].quality_minus.is_empty());

        // A scan with an OCR layer is 良; a plain scan is 中.
        for (layer, expected) in [(TextLayer::OcrLayer, "良"), (TextLayer::ScanOnly, "中")] {
            let mut scan_signals = signals.clone();
            if let Some(sig) = scan_signals.get_mut("带角标的书.pdf") {
                if let Some(pdf) = sig.pdf.as_mut() {
                    pdf.text_layer = layer;
                }
            }
            let listed =
                list_library_with_signals(&root, &ProgressStore::in_memory(), &scan_signals);
            assert_eq!(listed[0].quality.as_deref(), Some(expected), "{layer:?}");
        }

        // No cached signals at all ⇒ no badge (the shelf must not guess).
        let ungraded =
            list_library_with_signals(&root, &ProgressStore::in_memory(), &HashMap::new());
        assert!(ungraded[0].quality.is_none());
    }

    /// Minimal one-page PDF with a correct xref table (the shelf tests must not
    /// depend on any committed binary sample).
    fn write_tiny_pdf(path: &Path) {
        let content = "BT /F1 24 Tf 20 100 Td (Hello) Tj ET";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 200] \
             /Resources << /Font << /F1 4 0 R >> >> /Contents 5 0 R >>"
                .to_string(),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
            format!(
                "<< /Length {} >>\nstream\n{}\nendstream",
                content.len(),
                content
            ),
            // UTF-16BE title: 经济漩涡：一个样本
            "<< /Title <FEFF7ECF6D4E6F296DA1FF1A4E004E2A6837672C> >>".to_string(),
        ];
        let mut pdf = String::from("%PDF-1.4\n");
        let mut offsets = Vec::new();
        for (index, body) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.push_str(&format!("{} 0 obj\n{}\nendobj\n", index + 1, body));
        }
        let xref_at = pdf.len();
        pdf.push_str(&format!(
            "xref\n0 {}\n0000000000 65535 f \n",
            objects.len() + 1
        ));
        for offset in &offsets {
            pdf.push_str(&format!("{offset:010} 00000 n \n"));
        }
        pdf.push_str(&format!(
            "trailer\n<< /Size {} /Root 1 0 R /Info 6 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
            objects.len() + 1
        ));
        fs::write(path, pdf.into_bytes()).unwrap();
    }

    #[test]
    fn rename_book_files_moves_epub_and_drops_old_md() {
        let root = std::env::temp_dir().join("icedreader-library-rename");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let sample = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures/sample.epub");
        fs::copy(&sample, root.join("旧名.epub")).unwrap();
        fs::write(root.join("旧名.md"), b"<!-- icedreader-meta\n-->").unwrap();
        fs::write(root.join("无关.txt"), b"x").unwrap();

        let new_name = rename_book_files(&root, "旧名.epub", "新名 - 作者").unwrap();
        assert_eq!(new_name, "新名 - 作者.epub");
        assert!(root.join("新名 - 作者.epub").is_file());
        assert!(!root.join("旧名.epub").exists());
        assert!(!root.join("旧名.md").exists(), "old companion md removed");
        assert!(root.join("无关.txt").is_file());

        // Refuses non-plain names and missing files.
        assert!(rename_book_files(&root, "../x.epub", "y").is_err());
        assert!(rename_book_files(&root, "没有.epub", "y").is_err());
        // Refuses an occupied target.
        fs::write(root.join("占位.epub"), b"z").unwrap();
        assert!(rename_book_files(&root, "新名 - 作者.epub", "占位").is_err());
    }

    #[test]
    fn delete_book_removes_file_and_refuses_escapes() {
        let root = std::env::temp_dir().join("icedreader-library-delete");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let sample = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures/sample.epub");
        fs::copy(&sample, root.join("sample.epub")).unwrap();

        let deleted = delete_book_from(&root, "sample.epub").unwrap();
        assert!(!deleted.exists());
        let entries = list_library_in(&root, &ProgressStore::in_memory());
        assert!(entries.is_empty());

        assert!(delete_book_from(&root, "../sample.epub").is_err());
        assert!(delete_book_from(&root, "sub/sample.epub").is_err());
        assert!(delete_book_from(&root, "missing.epub").is_err());
        assert!(delete_book_from(&root, "").is_err());
    }

    #[test]
    fn lists_sample_epub_from_temp_library() {
        let root = std::env::temp_dir().join("icedreader-library-list");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let sample = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures/sample.epub");
        fs::copy(&sample, root.join("sample.epub")).unwrap();
        fs::write(root.join("notes.txt"), b"skip").unwrap();

        let entries = list_library_in(&root, &ProgressStore::in_memory());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].file_name, "sample.epub");
        assert!(!entries[0].title.is_empty());
        assert!(entries[0].open_error.is_none());
        assert!(entries[0].chapter_count.unwrap_or(0) >= 1);
        assert!(entries[0].updated_at.is_none());
        assert!(!entries[0].cover_rev.is_empty());
    }

    #[test]
    fn delete_book_removes_companion_md() {
        let root = std::env::temp_dir().join("icedreader-library-delete-md");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let sample = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures/sample.epub");
        fs::copy(&sample, root.join("sample.epub")).unwrap();
        fs::write(root.join("sample.md"), b"<!-- icedreader-meta\n-->").unwrap();
        fs::write(root.join("other.md"), b"keep me").unwrap();

        delete_book_from(&root, "sample.epub").unwrap();
        assert!(!root.join("sample.epub").exists());
        assert!(
            !root.join("sample.md").exists(),
            "companion md must be deleted with the book"
        );
        assert!(root.join("other.md").exists(), "unrelated md files stay");
        assert!(meta_path_for(&root, "../x.epub").is_err());
        assert_eq!(
            meta_path_for(&root, "三体.epub").unwrap(),
            root.join("三体.md")
        );
    }

    #[test]
    fn lists_saved_progress_for_sample() {
        let root = std::env::temp_dir().join("icedreader-library-progress");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let sample = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures/sample.epub");
        let dest = root.join("sample.epub");
        fs::copy(&sample, &dest).unwrap();

        let book = openers::open_any(&dest).unwrap();
        let href = book.spine()[0].href.clone();
        let key = progress_key(&dest, &book.metadata().identifiers, Some(&root));
        let mut store = ProgressStore::in_memory();
        store
            .set(
                key,
                Locator {
                    href,
                    fraction: 0.5,
                    cfi: None,
                },
            )
            .unwrap();

        let entries = list_library_in(&root, &store);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].chapter_index, Some(0));
        assert!((entries[0].fraction.unwrap() - 0.5).abs() < 1e-9);
        assert!(entries[0].updated_at.is_some());
    }

    #[test]
    fn meta_cache_skips_reopening_unchanged_book_and_tracks_progress() {
        let root = std::env::temp_dir().join("icedreader-library-meta-cache");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let sample = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures/sample.epub");
        let dest = root.join("sample.epub");
        fs::copy(&sample, &dest).unwrap();

        let mut store = ProgressStore::in_memory();
        let mut cache = LibraryMetaCache::default();
        let first = list_library_cached(&root, &store, &mut cache, &HashMap::new());
        assert_eq!(first.len(), 1);
        assert!(first[0].updated_at.is_none());

        // Same file, second listing: cached profile, no re-open.
        let again = list_library_cached(&root, &store, &mut cache, &HashMap::new());
        assert_eq!(again.len(), 1);
        assert_eq!(again[0].title, first[0].title);

        // Progress still shows through the cached profile.
        let book = openers::open_any(&dest).unwrap();
        let href = book.spine()[0].href.clone();
        let key = progress_key(&dest, &book.metadata().identifiers, Some(&root));
        store
            .set(
                key,
                Locator {
                    href,
                    fraction: 0.25,
                    cfi: None,
                },
            )
            .unwrap();
        let listed = list_library_cached(&root, &store, &mut cache, &HashMap::new());
        assert!((listed[0].fraction.unwrap() - 0.25).abs() < 1e-9);
    }

    #[test]
    fn unreadable_book_keeps_lib_key_for_cleanup() {
        let root = std::env::temp_dir().join("icedreader-library-broken");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("broken.epub");
        fs::write(&path, b"definitely not a zip").unwrap();

        // A book that previously opened fine became unreadable after a
        // same-named replacement: profile must still derive the lib: key so
        // the shelf shows old progress and 删除 clears it.
        let profile = profile_book(&path, &root);
        assert!(
            profile.open_error.is_some(),
            "broken file must be listed as unreadable"
        );
        assert_eq!(profile.progress_key, "lib:broken.epub");
        assert_eq!(profile.chapter_count(), None);

        let mut store = ProgressStore::in_memory();
        store
            .set(
                profile.progress_key.clone(),
                Locator {
                    href: "/OPS/chapter2.html".into(),
                    fraction: 0.4,
                    cfi: None,
                },
            )
            .unwrap();
        let entry = entry_from(&path, &profile, &store);
        assert!(entry.open_error.is_some());
        assert!((entry.fraction.unwrap() - 0.4).abs() < 1e-9);
        // delete_book clears records by this key; empty keys used to no-op.
        assert!(store.remove(&profile.progress_key).unwrap());
    }

    #[test]
    fn cover_cache_keyed_by_file_revision() {
        let mut cache = CoverCache::default();
        cache.insert(
            "a.epub",
            "rev1".into(),
            "image/jpeg".into(),
            b"one".to_vec(),
        );
        assert_eq!(
            cache.get("a.epub", "rev1"),
            Some(("image/jpeg", b"one".as_slice()))
        );
        // A replaced file (new revision) must miss and be re-read.
        assert_eq!(cache.get("a.epub", "rev2"), None);
        cache.remove("a.epub");
        assert_eq!(cache.get("a.epub", "rev1"), None);
    }
}
