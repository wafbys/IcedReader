use std::fs;
use std::path::{Path, PathBuf};

use iced_reader_core::{progress_key, Book, BookOpener, Locator, ProgressStore};
use iced_reader_epub::EpubOpener;
use serde::Serialize;

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
    pub open_error: Option<String>,
}

pub fn list_library(progress: &ProgressStore) -> Result<Vec<LibraryEntry>, String> {
    let dir = portable::library_dir().map_err(|e| e.to_string())?;
    Ok(list_library_in(&dir, progress))
}

pub fn list_library_in(dir: &Path, progress: &ProgressStore) -> Vec<LibraryEntry> {
    let mut entries: Vec<LibraryEntry> = read_epub_paths(dir)
        .into_iter()
        .map(|path| describe_book(&path, dir, progress))
        .collect();
    entries.sort_by(|a, b| match (b.updated_at, a.updated_at) {
        (Some(x), Some(y)) => x.cmp(&y).then_with(|| a.title.cmp(&b.title)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => a.title.cmp(&b.title),
    });
    entries
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

fn describe_book(path: &Path, library: &Path, progress: &ProgressStore) -> LibraryEntry {
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
        return LibraryEntry {
            path: path.to_string_lossy().into_owned(),
            file_name,
            title: fallback_title,
            authors: Vec::new(),
            progress_key: String::new(),
            chapter_index: None,
            chapter_count: None,
            chapter_title: None,
            fraction: None,
            updated_at: None,
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
            let rec = progress.get(&key);
            let (chapter_index, chapter_count, chapter_title, fraction) = match rec {
                Some(r) => chapter_progress(book.as_ref(), &r.locator),
                None => (None, Some(book.spine().len() as u32), None, None),
            };
            LibraryEntry {
                path: path.to_string_lossy().into_owned(),
                file_name,
                title,
                authors: meta.authors,
                progress_key: key,
                chapter_index,
                chapter_count,
                chapter_title,
                fraction,
                updated_at: rec.map(|r| r.updated_at),
                has_cover: meta.cover_href.is_some(),
                open_error: None,
            }
        }
        Err(err) => LibraryEntry {
            path: path.to_string_lossy().into_owned(),
            file_name,
            title: fallback_title,
            authors: Vec::new(),
            progress_key: String::new(),
            chapter_index: None,
            chapter_count: None,
            chapter_title: None,
            fraction: None,
            updated_at: None,
            has_cover: false,
            open_error: Some(err.to_string()),
        },
    }
}

fn chapter_progress(
    book: &dyn Book,
    locator: &Locator,
) -> (Option<u32>, Option<u32>, Option<String>, Option<f64>) {
    let spine = book.spine();
    let count = spine.len() as u32;
    let idx = spine.iter().position(|item| hrefs_match(&item.href, &locator.href));
    match idx {
        Some(i) => (
            Some(i as u32),
            Some(count),
            spine[i].title.clone(),
            Some(locator.fraction.clamp(0.0, 1.0)),
        ),
        None => (None, Some(count), None, Some(locator.fraction.clamp(0.0, 1.0))),
    }
}

fn hrefs_match(a: &str, b: &str) -> bool {
    let (file_a, frag_a) = split_href(a);
    let (file_b, frag_b) = split_href(b);
    let file_a = file_a.trim_start_matches('/');
    let file_b = file_b.trim_start_matches('/');
    if !file_a.eq_ignore_ascii_case(file_b) {
        return false;
    }
    match (frag_a, frag_b) {
        (None, None) => true,
        (Some(x), Some(y)) => x == y,
        _ => frag_a.is_none() || frag_b.is_none(),
    }
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
    }
}
