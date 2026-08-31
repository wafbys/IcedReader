use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::fonts::sniff_font;
use crate::CoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FontSlot {
    Serif,
    Sans,
    Mono,
    Cjk,
}

impl FontSlot {
    pub const ALL: [FontSlot; 4] = [Self::Serif, Self::Sans, Self::Mono, Self::Cjk];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Serif => "serif",
            Self::Sans => "sans",
            Self::Mono => "mono",
            Self::Cjk => "cjk",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "serif" => Some(Self::Serif),
            "sans" => Some(Self::Sans),
            "mono" => Some(Self::Mono),
            "cjk" => Some(Self::Cjk),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FontFile {
    pub file: String,
    pub original_name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FontSlots {
    pub serif: Option<FontFile>,
    pub sans: Option<FontFile>,
    pub mono: Option<FontFile>,
    pub cjk: Option<FontFile>,
}

impl FontSlots {
    pub fn get(&self, slot: FontSlot) -> Option<&FontFile> {
        match slot {
            FontSlot::Serif => self.serif.as_ref(),
            FontSlot::Sans => self.sans.as_ref(),
            FontSlot::Mono => self.mono.as_ref(),
            FontSlot::Cjk => self.cjk.as_ref(),
        }
    }

    pub fn set(&mut self, slot: FontSlot, file: Option<FontFile>) {
        match slot {
            FontSlot::Serif => self.serif = file,
            FontSlot::Sans => self.sans = file,
            FontSlot::Mono => self.mono = file,
            FontSlot::Cjk => self.cjk = file,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReaderSettings {
    #[serde(default = "default_use_original")]
    pub use_original_fonts: bool,
    #[serde(default)]
    pub fonts: FontSlots,
}

fn default_use_original() -> bool {
    true
}

impl Default for ReaderSettings {
    fn default() -> Self {
        Self {
            use_original_fonts: true,
            fonts: FontSlots::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FontSettingsView {
    pub use_original_fonts: bool,
    pub fonts: FontSlots,
    pub missing_slots: Vec<FontSlot>,
    pub custom_fonts_active: bool,
}

#[derive(Debug, Default)]
pub struct SettingsStore {
    path: Option<PathBuf>,
    fonts_dir: PathBuf,
    data: ReaderSettings,
}

impl SettingsStore {
    pub fn in_memory(fonts_dir: PathBuf) -> Self {
        Self {
            path: None,
            fonts_dir,
            data: ReaderSettings::default(),
        }
    }

    pub fn open(path: PathBuf, fonts_dir: PathBuf) -> Result<Self, CoreError> {
        let data = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Ok(Self {
            path: Some(path),
            fonts_dir,
            data,
        })
    }

    pub fn fonts_dir(&self) -> &Path {
        &self.fonts_dir
    }

    pub fn font_file(&self, slot: FontSlot) -> Option<&FontFile> {
        self.data.fonts.get(slot)
    }

    pub fn view(&self) -> FontSettingsView {
        let missing = self.missing_slots();
        FontSettingsView {
            use_original_fonts: self.data.use_original_fonts,
            fonts: self.data.fonts.clone(),
            custom_fonts_active: !self.data.use_original_fonts && missing.is_empty(),
            missing_slots: missing,
        }
    }

    pub fn custom_fonts_active(&self) -> bool {
        !self.data.use_original_fonts && self.missing_slots().is_empty()
    }

    pub fn missing_slots(&self) -> Vec<FontSlot> {
        FontSlot::ALL
            .iter()
            .copied()
            .filter(|slot| !self.slot_ready(*slot))
            .collect()
    }

    pub fn slot_ready(&self, slot: FontSlot) -> bool {
        let Some(file) = self.data.fonts.get(slot) else {
            return false;
        };
        let path = self.fonts_dir.join(&file.file);
        let mut header = [0u8; 4];
        let Ok(mut f) = fs::File::open(&path) else {
            return false;
        };
        f.read_exact(&mut header).is_ok() && sniff_font(&header).is_some()
    }

    pub fn set_use_original_fonts(&mut self, value: bool) -> Result<(), CoreError> {
        self.data.use_original_fonts = value;
        self.persist()
    }

    pub fn set_font(&mut self, slot: FontSlot, file: FontFile) -> Result<(), CoreError> {
        self.data.fonts.set(slot, Some(file));
        self.persist()
    }

    pub fn clear_font(&mut self, slot: FontSlot) -> Result<(), CoreError> {
        self.data.fonts.set(slot, None);
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
        let data =
            serde_json::to_vec_pretty(&self.data).map_err(|e| CoreError::msg(e.to_string()))?;
        fs::write(&tmp, data).map_err(|e| CoreError::msg(e.to_string()))?;
        fs::rename(&tmp, path).map_err(|e| CoreError::msg(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ttf_bytes() -> Vec<u8> {
        let mut b = b"\0\x01\0\0".to_vec();
        b.extend_from_slice(&[0u8; 16]);
        b
    }

    fn write_slot(dir: &Path, slot: FontSlot) {
        fs::write(dir.join(format!("{}.ttf", slot.as_str())), ttf_bytes()).unwrap();
    }

    #[test]
    fn default_uses_original_fonts() {
        let dir = std::env::temp_dir().join("icedreader-settings-default");
        let _ = fs::create_dir_all(&dir);
        let store = SettingsStore::in_memory(dir);
        assert!(store.view().use_original_fonts);
        assert!(!store.custom_fonts_active());
        assert_eq!(store.missing_slots().len(), 4);
    }

    #[test]
    fn custom_active_only_when_toggle_off_and_all_slots_valid() {
        let root = std::env::temp_dir().join("icedreader-settings-complete");
        let _ = fs::remove_dir_all(&root);
        let fonts = root.join("fonts");
        fs::create_dir_all(&fonts).unwrap();
        let path = root.join("settings.json");
        let mut store = SettingsStore::open(path, fonts.clone()).unwrap();
        store.set_use_original_fonts(false).unwrap();
        assert!(!store.custom_fonts_active());

        for slot in FontSlot::ALL {
            write_slot(&fonts, slot);
            store
                .set_font(
                    slot,
                    FontFile {
                        file: format!("{}.ttf", slot.as_str()),
                        original_name: format!("{}.ttf", slot.as_str()),
                    },
                )
                .unwrap();
        }
        assert!(store.custom_fonts_active());
        assert!(store.missing_slots().is_empty());

        fs::remove_file(fonts.join("cjk.ttf")).unwrap();
        assert!(!store.custom_fonts_active());
        assert_eq!(store.missing_slots(), vec![FontSlot::Cjk]);
    }

    #[test]
    fn roundtrip_persists_intent_with_incomplete_fonts() {
        let root = std::env::temp_dir().join("icedreader-settings-roundtrip");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("settings.json");
        let fonts = root.join("fonts");
        fs::create_dir_all(&fonts).unwrap();
        let mut store = SettingsStore::open(path.clone(), fonts.clone()).unwrap();
        store.set_use_original_fonts(false).unwrap();
        store
            .set_font(
                FontSlot::Serif,
                FontFile {
                    file: "serif.ttf".into(),
                    original_name: "MySerif.ttf".into(),
                },
            )
            .unwrap();

        let reloaded = SettingsStore::open(path, fonts).unwrap();
        let view = reloaded.view();
        assert!(!view.use_original_fonts);
        assert!(!view.custom_fonts_active);
        assert_eq!(
            view.fonts.serif.as_ref().map(|f| f.original_name.as_str()),
            Some("MySerif.ttf")
        );
    }
}
