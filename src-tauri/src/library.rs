use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use iced_reader_core::{
    clean_title, progress_key, read_meta_file, resolved_title, BookOpener, Locator, ProgressStore,
};
use iced_reader_epub::EpubOpener;
use serde::Serialize;

use crate::book_signals;
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
    pub open_error: Option<String>,
    /// 优/良/中 from the cached first-import book signals (rev valid only).
    pub quality: Option<String>,
    /// Measured facts and merits behind the grade (shown in the shelf tooltip).
    pub quality_plus: Vec<String>,
    /// What held the book back — defects / missing provenance (empty on 优).
    pub quality_minus: Vec<String>,
    /// File names of other library books judged the same book (hint only).
    pub duplicates: Vec<String>,
}

/// Un-cached shelf listing used by the in-crate tests below.
#[cfg(test)]
pub fn list_library_in(dir: &Path, progress: &ProgressStore) -> Vec<LibraryEntry> {
    let mut cache = LibraryMetaCache::default();
    list_library_cached(dir, progress, &mut cache)
}

/// Book-shelf listing without re-opening every epub: file-bound metadata
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
        self.books.insert(path.to_path_buf(), (rev, profile.clone()));
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
}

impl BookProfile {
    pub fn chapter_count(&self) -> Option<u32> {
        (!self.chapter_hrefs.is_empty()).then(|| self.chapter_hrefs.len() as u32)
    }
}

pub fn list_library_cached(
    dir: &Path,
    progress: &ProgressStore,
    cache: &mut LibraryMetaCache,
) -> Vec<LibraryEntry> {
    let entries: Vec<LibraryEntry> = read_epub_paths(dir)
        .into_iter()
        .map(|path| {
            let mut entry = entry_from(&path, &cache.profile(&path, dir), progress);
            // Companion md overlays the file-bound title (displayTitle → joined
            // fields → dc:title/file name). Read per listing so a metadata edit
            // shows up immediately without touching the epub-rev profile cache.
            if let Ok(meta_path) = meta_path_for(dir, &entry.file_name) {
                if let Some(meta) = read_meta_file(&meta_path) {
                    entry.title = resolved_title(Some(&meta), &entry.title);
                }
            }
            entry
        })
        .collect();
    enrich_and_sort(entries)
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
fn enrich_and_sort(mut entries: Vec<LibraryEntry>) -> Vec<LibraryEntry> {
    let all = book_signals::read_all();
    for e in &mut entries {
        if e.open_error.is_some() {
            continue;
        }
        let Some(sig) = all.get(&e.file_name) else {
            continue;
        };
        if sig.rev != e.cover_rev {
            continue; // file changed since the cached analysis; keep unknown
        }
        let g = book_signals::grade(sig);
        e.quality = Some(g.label.to_string());
        e.quality_plus = g.plus;
        e.quality_minus = g.minus;
    }

    // Same-typesetting groups (equal chapter-text fingerprint).
    let valid: Vec<(usize, &book_signals::BookSignals)> = entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let s = all.get(&e.file_name)?;
            (s.rev == e.cover_rev).then_some((i, s))
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

    entries.sort_by(|a, b| {
        let q = |e: &LibraryEntry| quality_rank(e.quality.as_deref());
        match (b.updated_at, a.updated_at) {
            (Some(x), Some(y)) => x
                .cmp(&y)
                .then_with(|| q(b).cmp(&q(a)))
                .then_with(|| a.title.cmp(&b.title)),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => q(b)
                .cmp(&q(a))
                .then_with(|| a.title.cmp(&b.title)),
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
        self.covers.insert(file_name.to_string(), (rev, media, data));
    }

    /// Drop one book's cover after deletion.
    pub fn remove(&mut self, file_name: &str) {
        self.covers.remove(file_name);
    }
}

pub fn cover_bytes(path: &Path) -> Result<(String, Vec<u8>), String> {
    let book = EpubOpener
        .open(path)
        .map_err(|e| e.to_string())?;
    let href = book
        .metadata()
        .cover_href
        .ok_or_else(|| "no cover".to_string())?;
    let res = book.resource(&href).map_err(|e| e.to_string())?;
    if res.data.is_empty() {
        return Err("empty cover".into());
    }
    Ok((res.media_type, res.data))
}

/// Cover bytes for one request, served from the in-process cache whenever the
/// file revision is unchanged.
pub fn cover_bytes_cached(
    path: &Path,
    file_name: &str,
    cache: &mut CoverCache,
) -> Result<(String, Vec<u8>), String> {
    let rev = file_rev(path);
    if let Some((media, data)) = cache.get(file_name, &rev) {
        return Ok((media.to_string(), data.to_vec()));
    }
    let (media, data) = cover_bytes(path)?;
    cache.insert(file_name, rev, media.clone(), data.clone());
    Ok((media, data))
}

pub fn library_cover_path(file_name: &str) -> Result<PathBuf, String> {
    let as_path = Path::new(file_name);
    if file_name.is_empty()
        || as_path.components().any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err("invalid cover name".into());
    }
    let dir = portable::library_dir().map_err(|e| e.to_string())?;
    let path = dir.join(file_name);
    if !path.is_file() {
        return Err("book not in library".into());
    }
    Ok(path)
}

/// Companion metadata path for a library epub (`三体.epub` → `三体.md`). Only
/// a plain file name inside `dir` is accepted (no separators / `..`), mirroring
/// [`delete_book_from`] and [`library_cover_path`].
pub fn meta_path_for(dir: &Path, file_name: &str) -> Result<PathBuf, String> {
    let as_path = Path::new(file_name);
    if file_name.is_empty()
        || as_path.components().any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err("invalid book file name".into());
    }
    Ok(dir.join(as_path).with_extension("md"))
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
        if matches!(
            ch,
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
        ) || c < 0x20
        {
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

/// Pick a stem that does not collide with any existing library file, using
/// the same `-2`, `-3`… numbering the importer's aliases use (a `-N` copy and
/// the plain name are one book, AGENTS 进度键). Case-insensitive, like NTFS.
/// `preferred` is already clean (see [`clean_file_stem`]).
pub fn unique_stem(dir: &Path, preferred: &str) -> String {
    let Ok(read) = fs::read_dir(dir) else {
        return preferred.to_string();
    };
    let taken: std::collections::HashSet<String> = read
        .filter_map(|item| item.ok())
        .filter(|item| item.path().is_file())
        .filter_map(|item| item.file_name().to_str().map(|n| n.to_lowercase()))
        .filter(|name| name.ends_with(".epub") || name.ends_with(".md"))
        .collect();
    let preferred_lower = format!("{preferred}.epub").to_lowercase();
    if !taken.contains(&preferred_lower)
        && !taken.contains(&format!("{preferred}.md").to_lowercase())
    {
        return preferred.to_string();
    }
    let mut n = 2;
    loop {
        let candidate = format!("{preferred}-{n}");
        let candidate_lower = format!("{candidate}.epub").to_lowercase();
        if !taken.contains(&candidate_lower)
            && !taken.contains(&format!("{candidate}.md").to_lowercase())
        {
            return candidate;
        }
        n += 1;
    }
}

/// Rename a library book's epub to `new_stem` (already clean + unique) and
/// drop its old companion md — the caller writes the md under the new name
/// right after, so moving the old md first would only add a second failing
/// rename point. Returns the new file name. Missing old md is fine; unrelated
/// files are untouched. A rename that fails after the epub moved leaves a
/// half-renamed state (epub new name, no md) — extremely unlikely, reported
/// as an error so the shelf reload reflects reality.
pub fn rename_book_files(
    dir: &Path,
    old_file_name: &str,
    new_stem: &str,
) -> Result<String, String> {
    let as_path = Path::new(old_file_name);
    if old_file_name.is_empty()
        || as_path.components().any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err("invalid book file name".into());
    }
    let epub_old = dir.join(old_file_name);
    if !epub_old.is_file() {
        return Err("book not in library".into());
    }
    let epub_new = dir.join(format!("{new_stem}.epub"));
    if epub_new.is_file() {
        return Err(format!("target already exists: {new_stem}.epub"));
    }
    fs::rename(&epub_old, &epub_new).map_err(|e| e.to_string())?;
    let md_old = meta_path_for(dir, old_file_name)?;
    if md_old.is_file() {
        // Best-effort: the new md is written right after this returns.
        let _ = fs::remove_file(&md_old);
    }
    Ok(format!("{new_stem}.epub"))
}

/// Delete one library book file. Only a plain file name inside `dir` is
/// accepted (no separators / `..`), mirroring `library_cover_path`. The caller
/// is responsible for clearing the book's progress/annotation records.
pub fn delete_book_from(dir: &Path, file_name: &str) -> Result<PathBuf, String> {
    let as_path = Path::new(file_name);
    if file_name.is_empty()
        || as_path.components().any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err("invalid book file name".into());
    }
    let path = dir.join(file_name);
    if !path.is_file() {
        return Err("book not in library".into());
    }
    fs::remove_file(&path).map_err(|e| e.to_string())?;
    // The companion md (user metadata) dies with the book; missing is fine.
    let _ = fs::remove_file(meta_path_for(dir, file_name)?);
    Ok(path)
}

fn read_epub_paths(dir: &Path) -> Vec<PathBuf> {
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = read
        .filter_map(|item| item.ok())
        .map(|item| item.path())
        .filter(|path| {
            path.is_file()
                && path
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("epub"))
        })
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

/// Slow path: open the epub once and extract everything bound to the file
/// content (no progress). Route calls through [`LibraryMetaCache`] so that
/// unchanged books are not re-opened on every shelf refresh.
fn profile_book(path: &Path, library: &Path) -> BookProfile {
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "book.epub".into());
    let fallback_title = file_name
        .strip_suffix(".epub")
        .or_else(|| file_name.strip_suffix(".EPUB"))
        .unwrap_or(&file_name)
        .to_string();

    let opener = EpubOpener;
    if !opener.can_open(path) {
        // Defensive: readers only list *.epub, but keep the same fallback-key
        // rule so any unreadable entry still maps to its `lib:` records.
        let key = progress_key(path, &[], Some(library));
        return BookProfile {
            file_name: file_name.clone(),
            title: fallback_title,
            authors: Vec::new(),
            progress_key: key,
            chapter_hrefs: Vec::new(),
            chapter_titles: Vec::new(),
            has_cover: false,
            open_error: Some("不是 EPUB".into()),
        };
    }

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
                has_cover: meta.cover_href.is_some(),
                open_error: None,
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
        open_error: profile.open_error.clone(),
        quality: None,
        quality_plus: Vec::new(),
        quality_minus: Vec::new(),
        duplicates: Vec::new(),
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
        assert_eq!(unique_stem(&root, "三体 - 刘慈欣"), "三体 - 刘慈欣");

        fs::write(root.join("三体 - 刘慈欣.epub"), b"a").unwrap();
        fs::write(root.join("三体 - 刘慈欣-2.md"), b"b").unwrap();
        fs::write(root.join("OTHER.EPUB"), b"c").unwrap();
        // .epub collision → -2; -2.md collision → -3 (case-insensitive).
        assert_eq!(unique_stem(&root, "三体 - 刘慈欣"), "三体 - 刘慈欣-3");
        assert_eq!(unique_stem(&root, "other"), "other-2");
        assert_eq!(unique_stem(&root, "三体 - 刘慈欣-2"), "三体 - 刘慈欣-2-2");
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
        assert!(!root.join("sample.md").exists(), "companion md must be deleted with the book");
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

        let book = EpubOpener.open(&dest).unwrap();
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
        let first = list_library_cached(&root, &store, &mut cache);
        assert_eq!(first.len(), 1);
        assert!(first[0].updated_at.is_none());

        // Same file, second listing: cached profile, no re-open.
        let again = list_library_cached(&root, &store, &mut cache);
        assert_eq!(again.len(), 1);
        assert_eq!(again[0].title, first[0].title);

        // Progress still shows through the cached profile.
        let book = EpubOpener.open(&dest).unwrap();
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
        let listed = list_library_cached(&root, &store, &mut cache);
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
        assert!(profile.open_error.is_some(), "broken file must be listed as unreadable");
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
        cache.insert("a.epub", "rev1".into(), "image/jpeg".into(), b"one".to_vec());
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
