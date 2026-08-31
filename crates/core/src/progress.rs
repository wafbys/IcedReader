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
        self.entries.get(key)
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

/// Stable key: EPUB identifier if present, otherwise a normalized filesystem path.
pub fn progress_key(path: &Path, identifiers: &[String]) -> String {
    if let Some(id) = identifiers
        .iter()
        .map(|s| s.trim())
        .find(|s| !s.is_empty())
    {
        return format!("id:{id}");
    }
    let canon = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    format!("path:{}", canon.to_string_lossy().to_lowercase())
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
        let key = progress_key(Path::new("C:/books/a.epub"), &["urn:isbn:1".into()]);
        assert_eq!(key, "id:urn:isbn:1");
    }
}
