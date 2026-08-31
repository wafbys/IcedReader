use std::fs;
use std::io::Read;
use std::path::Path;
use std::time::UNIX_EPOCH;

use iced_reader_core::{
    apply_custom_fonts, rewrite_css_font_families, sniff_font, FontFile, FontSlot, FontUrls,
    SettingsStore,
};

use crate::portable;
use crate::protocol;

const MAX_FONT_BYTES: u64 = 64 * 1024 * 1024;

pub fn urls(store: &SettingsStore) -> FontUrls {
    FontUrls {
        serif: slot_url(store, FontSlot::Serif),
        sans: slot_url(store, FontSlot::Sans),
        mono: slot_url(store, FontSlot::Mono),
        cjk: slot_url(store, FontSlot::Cjk),
    }
}

fn slot_url(store: &SettingsStore, slot: FontSlot) -> String {
    let mut url = protocol::font_url(slot.as_str());
    let Some(file) = store.font_file(slot) else {
        return url;
    };
    let path = store.fonts_dir().join(&file.file);
    if let Ok(meta) = fs::metadata(&path) {
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        url.push_str(&format!("?r={}-{}", meta.len(), mtime));
    }
    url
}

pub fn apply_html_if_active(html: String, settings: &SettingsStore) -> String {
    if settings.custom_fonts_active() {
        apply_custom_fonts(&html, &urls(settings))
    } else {
        html
    }
}

pub fn apply_css_if_active(css: Vec<u8>, settings: &SettingsStore) -> Vec<u8> {
    if !settings.custom_fonts_active() {
        return css;
    }
    match std::str::from_utf8(&css) {
        Ok(s) => rewrite_css_font_families(s).into_bytes(),
        Err(_) => css,
    }
}

pub fn copy_into_slot(slot: FontSlot, src: &Path) -> Result<FontFile, String> {
    portable::ensure_layout().map_err(|e| e.to_string())?;
    let fonts_dir = portable::fonts_dir().map_err(|e| e.to_string())?;

    let meta = fs::metadata(src).map_err(|e| e.to_string())?;
    if !meta.is_file() {
        return Err("不是字体文件".into());
    }
    if meta.len() > MAX_FONT_BYTES {
        return Err("字体文件过大（超过 64MB）".into());
    }

    let mut header = [0u8; 4];
    fs::File::open(src)
        .and_then(|mut f| f.read_exact(&mut header))
        .map_err(|e| e.to_string())?;
    let kind = sniff_font(&header).ok_or_else(|| "无法识别的字体文件".to_string())?;

    let dest_name = format!("{}.{ext}", slot.as_str(), ext = kind.extension());
    let dest = fonts_dir.join(&dest_name);
    let tmp = fonts_dir.join(format!(".{}.upload", slot.as_str()));
    fs::copy(src, &tmp).map_err(|e| e.to_string())?;
    remove_slot_files(&fonts_dir, slot);
    if let Err(err) = fs::rename(&tmp, &dest) {
        let _ = fs::remove_file(&tmp);
        return Err(err.to_string());
    }

    let original_name = src
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| dest_name.clone());

    Ok(FontFile {
        file: dest_name,
        original_name,
    })
}

pub fn delete_slot_files(slot: FontSlot) {
    if let Ok(dir) = portable::fonts_dir() {
        remove_slot_files(&dir, slot);
    }
}

pub fn remove_slot_files(dir: &Path, slot: FontSlot) {
    for ext in ["ttf", "otf", "woff", "woff2", "ttc"] {
        let path = dir.join(format!("{}.{ext}", slot.as_str()));
        let _ = fs::remove_file(path);
    }
}

pub fn read_slot_font(store: &SettingsStore, slot: FontSlot) -> Option<(Vec<u8>, &'static str)> {
    let file = store.font_file(slot)?;
    let path = store.fonts_dir().join(&file.file);
    let data = fs::read(&path).ok()?;
    let mime = sniff_font(&data)?.mime();
    Some((data, mime))
}
