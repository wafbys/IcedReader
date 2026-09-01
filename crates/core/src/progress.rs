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
        if let Some(rec) = self.entries.get(key) {
            return Some(rec);
        }
        let Some(stem) = lib_book_stem(key) else {
            return None;
        };
        self.entries
            .iter()
            .filter(|(k, _)| lib_book_stem(k).as_deref() == Some(stem.as_str()))
            .max_by_key(|(_, rec)| rec.updated_at)
            .map(|(_, rec)| rec)
    }

    pub fn set(&mut self, key: String, locator: Locator) -> Result<(), CoreError> {
        let fraction = locator.fraction.clamp(0.0, 1.0);
        self.entries.insert(
            key,
            ProgressRecord {
                locator: Locator { fraction, ..locator },
                updated_at: unix_now(),
            },
        );
        self.persist()
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

/// `lib:新西游记++共两册-17.epub` and `lib:新西游记++共两册.epub` share a stem.
fn lib_book_stem(key: &str) -> Option<String> {
    let rest = key.strip_prefix("lib:")?;
    let rest = rest.strip_suffix(".epub").unwrap_or(rest);
    let trimmed = rest.trim_end_matches(|c: char| c.is_ascii_digit());
    let trimmed = trimmed.trim_end_matches('-');
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.replace('\\', "/").to_lowercase())
    }
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
}
