use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{CoreError, Locator};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressRecord {
    pub locator: Locator,
    pub updated_at: i64,
}

#[derive(Debug, Default)]
pub struct ProgressStore {
    path: Option<PathBuf>,
    entries: HashMap<String, ProgressRecord>,
}

impl ProgressStore {
    pub fn in_memory() -> Self {
        Self::default()
    }

    pub fn open(path: PathBuf) -> Result<Self, CoreError> {
        let entries = if path.exists() {
            let bytes = fs::read(&path).map_err(|e| CoreError::msg(e.to_string()))?;
            serde_json::from_slice(&bytes).unwrap_or_default()
        } else {
            HashMap::new()
        };
        Ok(Self {
            path: Some(path),
            entries,
        })
    }

    pub fn get(&self, key: &str) -> Option<&ProgressRecord> {
        if !key.starts_with("lib:") {
            return self.entries.get(key);
        }
        // One book may be reached through several `lib:` keys (numbered copy
        // and plain name are the same book). Return the newest record among
        // the aliases instead of a possibly stale exact-key twin.
        let Some(stem) = lib_book_stem(key) else {
            return self.entries.get(key);
        };
        self.entries
            .iter()
            .filter(|(k, _)| lib_book_stem(k).as_deref() == Some(stem.as_str()))
            .max_by_key(|(_, rec)| rec.updated_at)
            .map(|(_, rec)| rec)
    }

    pub fn set(&mut self, key: String, locator: Locator) -> Result<(), CoreError> {
        let fraction = locator.fraction.clamp(0.0, 1.0);
        // Writing through one alias writes the book: drop any other alias
        // records so `get` never returns a stale twin and the JSON does not
        // accumulate drifting copies of the same book.
        if key.starts_with("lib:") {
            if let Some(stem) = lib_book_stem(&key) {
                self.entries.retain(|k, _| {
                    !(k.starts_with("lib:")
                        && *k != key
                        && lib_book_stem(k).as_deref() == Some(stem.as_str()))
                });
            }
        }
        self.entries.insert(
            key,
            ProgressRecord {
                locator: Locator { fraction, ..locator },
                updated_at: unix_now(),
            },
        );
        self.persist()
    }

    /// Remove one book's record: the exact key plus any `lib:` stem aliases
    /// that [`same_book`] treats as the same book. Returns whether anything was
    /// removed.
    pub fn remove(&mut self, key: &str) -> Result<bool, CoreError> {
        if !self.entries.keys().any(|k| same_book(k, key)) {
            return Ok(false);
        }
        self.entries.retain(|k, _| !same_book(k, key));
        self.persist()?;
        Ok(true)
    }

    fn persist(&self) -> Result<(), CoreError> {
        let Some(path) = &self.path else {
            return Ok(());
        };
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| CoreError::msg(e.to_string()))?;
        }
        let tmp = path.with_extension("json.tmp");
        let data = serde_json::to_vec_pretty(&self.entries)
            .map_err(|e| CoreError::msg(e.to_string()))?;
        fs::write(&tmp, data).map_err(|e| CoreError::msg(e.to_string()))?;
        fs::rename(&tmp, path).map_err(|e| CoreError::msg(e.to_string()))?;
        Ok(())
    }
}

/// Stable key so a moved portable folder still finds progress.
/// Prefer EPUB identifier; else a path relative to the portable library.
pub fn progress_key(path: &Path, identifiers: &[String], library_dir: Option<&Path>) -> String {
    if let Some(id) = identifiers
        .iter()
        .map(|s| s.trim())
        .find(|s| !s.is_empty())
    {
        return format!("id:{id}");
    }
    if let Some(lib) = library_dir {
        if let Some(rel) = relative_to(path, lib) {
            return format!("lib:{rel}");
        }
    }
    let canon = normalize_path(path);
    format!(
        "path:{}",
        canon.to_string_lossy().replace('\\', "/").to_lowercase()
    )
}

fn normalize_path(path: &Path) -> PathBuf {
    let canon = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let s = canon.to_string_lossy();
    PathBuf::from(s.strip_prefix(r"\\?\").unwrap_or(&s))
}

fn relative_to(path: &Path, root: &Path) -> Option<String> {
    let path = normalize_path(path);
    let root = normalize_path(root);
    path.strip_prefix(&root).ok().map(|rel| {
        rel.to_string_lossy()
            .replace('\\', "/")
            .to_lowercase()
    })
}

/// Equal keys, or two `lib:` keys sharing a stem (numbered copies and the
/// plain name are one book), address the same book.
pub(crate) fn same_book(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    if !(a.starts_with("lib:") && b.starts_with("lib:")) {
        return false;
    }
    match (lib_book_stem(a), lib_book_stem(b)) {
        (Some(x), Some(y)) => x == y,
        _ => false,
    }
}

/// `lib:书名-17.epub` and `lib:书名.epub` share a stem; the copy suffix is a
/// trailing `-` followed by digits. A plain trailing digit is part of the
/// title (`lib:三体3.epub` ≠ `lib:三体.epub`) and a fully numeric title
/// (`lib:1984.epub`) must survive so its `1984-2` copy still matches.
fn lib_book_stem(key: &str) -> Option<String> {
    let rest = key.strip_prefix("lib:")?;
    let mut stem = rest.strip_suffix(".epub").unwrap_or(rest);
    loop {
        let Some(hyph) = stem.rfind('-') else {
            break;
        };
        let (head, tail) = stem.split_at(hyph);
        if tail.len() > 1 && tail[1..].chars().all(|c| c.is_ascii_digit()) {
            stem = head;
        } else {
            break;
        }
    }
    let normalized = stem.replace('\\', "/").to_lowercase();
    (!normalized.is_empty()).then_some(normalized)
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_file_store() {
        let dir = std::env::temp_dir().join("icedreader-progress-test");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("progress.json");
        let _ = fs::remove_file(&path);

        let mut store = ProgressStore::open(path.clone()).unwrap();
        store
            .set(
                "id:urn:uuid:test".into(),
                Locator {
                    href: "/EPUB/ch2.xhtml".into(),
                    fraction: 0.37,
                    cfi: None,
                },
            )
            .unwrap();

        let reloaded = ProgressStore::open(path).unwrap();
        let rec = reloaded.get("id:urn:uuid:test").unwrap();
        assert_eq!(rec.locator.href, "/EPUB/ch2.xhtml");
        assert!((rec.locator.fraction - 0.37).abs() < 1e-9);
    }

    #[test]
    fn prefers_identifier() {
        let key = progress_key(Path::new("C:/books/a.epub"), &["urn:isbn:1".into()], None);
        assert_eq!(key, "id:urn:isbn:1");
    }

    #[test]
    fn library_relative_key() {
        let dir = std::env::temp_dir().join("icedreader-lib-key");
        let lib = dir.join("library");
        fs::create_dir_all(&lib).unwrap();
        let book = lib.join("Foo.epub");
        fs::write(&book, b"x").unwrap();
        let key = progress_key(&book, &[], Some(&lib));
        assert_eq!(key, "lib:foo.epub");
    }

    #[test]
    fn remove_clears_exact_key_and_lib_aliases() {
        let loc = |f: f64| Locator {
            href: "/OPS/chapter1.html".into(),
            fraction: f,
            cfi: None,
        };
        let mut store = ProgressStore::in_memory();
        store.set("lib:foo.epub".into(), loc(0.1)).unwrap();
        store.set("lib:foo-2.epub".into(), loc(0.2)).unwrap();
        store.set("lib:bar.epub".into(), loc(0.3)).unwrap();
        store.set("id:urn:uuid:other".into(), loc(0.4)).unwrap();

        assert!(store.remove("lib:foo.epub").unwrap());
        assert!(store.get("lib:foo.epub").is_none());
        assert!(store.get("lib:foo-2.epub").is_none());
        assert_eq!(store.get("lib:bar.epub").unwrap().locator.fraction, 0.3);
        assert_eq!(
            store.get("id:urn:uuid:other").unwrap().locator.fraction,
            0.4
        );
        assert!(!store.remove("lib:foo.epub").unwrap());
    }

    #[test]
    fn remove_id_key_only_touches_that_id() {
        let loc = Locator {
            href: "/OPS/chapter1.html".into(),
            fraction: 0.5,
            cfi: None,
        };
        let mut store = ProgressStore::in_memory();
        store.set("id:a".into(), loc.clone()).unwrap();
        store.set("id:b".into(), loc).unwrap();
        assert!(store.remove("id:a").unwrap());
        assert!(store.get("id:a").is_none());
        assert!(store.get("id:b").is_some());
    }

    #[test]
    fn lib_numbered_copies_share_progress() {
        let dir = std::env::temp_dir().join("icedreader-progress-alias");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("progress.json");
        let _ = fs::remove_file(&path);

        let mut store = ProgressStore::open(path).unwrap();
        store
            .set(
                "lib:新西游记++共两册-14.epub".into(),
                Locator {
                    href: "/OPS/chapter3.html".into(),
                    fraction: 0.4,
                    cfi: None,
                },
            )
            .unwrap();

        let rec = store
            .get("lib:新西游记++共两册.epub")
            .expect("alias to numbered copy");
        assert_eq!(rec.locator.href, "/OPS/chapter3.html");
        assert!((rec.locator.fraction - 0.4).abs() < 1e-9);
    }

    #[test]
    fn trailing_digit_is_part_of_title_not_a_copy_suffix() {
        let loc = |f: f64| Locator {
            href: "/OPS/chapter1.html".into(),
            fraction: f,
            cfi: None,
        };
        let mut store = ProgressStore::in_memory();
        store.set("lib:三体.epub".into(), loc(0.1)).unwrap();
        store.set("lib:三体3.epub".into(), loc(0.2)).unwrap();
        // Plain trailing digits belong to the title: two different books.
        assert_eq!(store.get("lib:三体.epub").unwrap().locator.fraction, 0.1);
        assert_eq!(store.get("lib:三体3.epub").unwrap().locator.fraction, 0.2);
        store.remove("lib:三体.epub").unwrap();
        assert!(store.get("lib:三体3.epub").is_some(), "other title kept");
    }

    #[test]
    fn numeric_title_copy_still_aliases() {
        let loc = Locator {
            href: "/OPS/chapter1.html".into(),
            fraction: 0.6,
            cfi: None,
        };
        let mut store = ProgressStore::in_memory();
        store.set("lib:1984.epub".into(), loc).unwrap();
        let rec = store
            .get("lib:1984-2.epub")
            .expect("numeric title copy aliases the book");
        assert!((rec.locator.fraction - 0.6).abs() < 1e-9);
    }

    #[test]
    fn writing_an_alias_drops_stale_twin_records() {
        let loc = |f: f64| Locator {
            href: "/OPS/chapter1.html".into(),
            fraction: f,
            cfi: None,
        };
        let mut store = ProgressStore::in_memory();
        store.set("lib:书名.epub".into(), loc(0.1)).unwrap();
        store.set("lib:书名-2.epub".into(), loc(0.9)).unwrap();
        // Only one alias record survives; both reads see the newest value.
        assert_eq!(
            store.get("lib:书名.epub").unwrap().locator.fraction,
            0.9
        );
        assert_eq!(
            store.get("lib:书名-2.epub").unwrap().locator.fraction,
            0.9
        );
        assert_eq!(store.entries.len(), 1, "alias records merged");
    }
}
