mod fonts;
mod book_meta;
mod book_signals;
mod library;
mod portable;
mod protocol;
mod window_state;

use std::collections::HashMap;
use std::sync::Mutex;

use iced_reader_core::{
    clean_title, collect_publisher_fonts, progress_key, read_meta_file, resolved_title,
    write_meta_file, AnnotationStore, Book, BookMeta, BookOpener, ChapterView, FontSettingsView,
    FontSlot, Highlight, Locator, Metadata, ProgressStore, SettingsStore, SpineItem, TocNode,
};
use iced_reader_epub::EpubOpener;
use serde::Serialize;
use tauri::Manager;
use uuid::Uuid;

pub fn prepare_portable() {
    portable::prepare_webview_env();
}

/// 窗口标题：产品名 + 版本号 + 构建时 git 短 hash（无 git 环境时省略 hash）。
/// hash 由 build.rs 在编译期经 ICED_READER_GIT_HASH 注入，属「build 时」固化值。
fn window_title(base: &str) -> String {
    let version = env!("CARGO_PKG_VERSION");
    match option_env!("ICED_READER_GIT_HASH") {
        Some(hash) if !hash.is_empty() => format!("{base} {version} ({hash})"),
        _ => format!("{base} {version}"),
    }
}

pub struct AppState {
    pub books: Mutex<HashMap<String, Box<dyn Book>>>,
    pub progress: Mutex<ProgressStore>,
    pub settings: Mutex<SettingsStore>,
    pub annotations: Mutex<AnnotationStore>,
    /// Per-file-revision shelf metadata (avoids re-opening big epubs on every shelf refresh).
    pub library_meta: Mutex<library::LibraryMetaCache>,
    /// Per-file-revision cover bytes (avoids re-opening the archive per cover request).
    pub covers: Mutex<library::CoverCache>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenedBook {
    pub id: String,
    pub format: String,
    pub path: String,
    pub progress_key: String,
    pub progress: Option<Locator>,
    pub metadata: Metadata,
    pub toc: Vec<TocNode>,
    pub spine: Vec<SpineItem>,
}

#[tauri::command]
fn open_book(path: String, state: tauri::State<AppState>) -> Result<OpenedBook, String> {
    let opener = EpubOpener;
    let source = std::path::Path::new(&path);
    if !opener.can_open(source) {
        return Err(format!("unsupported file: {path}"));
    }
    let imported = portable::import_book(source).map_err(|e| e.to_string())?;
    let book = opener.open(&imported).map_err(|e| e.to_string())?;
    let mut metadata = book.metadata();
    let library = portable::library_dir().ok();
    let key = progress_key(
        &imported,
        &metadata.identifiers,
        library.as_deref(),
    );
    let progress = state
        .progress
        .lock()
        .ok()
        .and_then(|store| store.get(&key).map(|r| r.locator.clone()));

    // First-import book signals (fingerprint + quality), cached by file rev.
    // Only computed when the cache is missing/stale; list_library never
    // recomputes. UI shows a busy state while this runs (AGENTS: 导入时计算
    // 且要有界面反馈；同书只提示，质量分入书架排序与封面角标).
    let rev = library::file_rev(&imported);
    let file_name = imported
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !file_name.is_empty() {
        let need = book_signals::read_all()
            .get(&file_name)
            .map(|s| s.rev != rev)
            .unwrap_or(true);
        if need {
            let images = iced_reader_epub::image_stats(&imported).unwrap_or((0, 0, false));
            if let Ok(signals) = book_signals::analyze_book(
                book.as_ref(),
                &metadata.identifiers,
                !metadata.authors.is_empty(),
                &rev,
                images,
            ) {
                book_signals::write_one(&file_name, &signals);
            }
        }
    }

    // Companion md overlays the title everywhere (shelf, reader chrome):
    // displayTitle → joined fields → dc:title/file name. Same resolution the
    // shelf applies in library.rs, so both surfaces always agree.
    if let Ok(dir) = portable::library_dir() {
        if let Ok(meta_path) = library::meta_path_for(&dir, &file_name) {
            if let Some(meta) = read_meta_file(&meta_path) {
                metadata.title = resolved_title(Some(&meta), &metadata.title);
            }
        }
    }

    let opened = OpenedBook {
        id: Uuid::new_v4().to_string(),
        format: book.format_id().to_string(),
        path: imported.to_string_lossy().into_owned(),
        progress_key: key,
        progress,
        metadata,
        toc: book.toc(),
        spine: book.spine(),
    };
    state
        .books
        .lock()
        .map_err(|e| e.to_string())?
        .insert(opened.id.clone(), book);
    Ok(opened)
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[tauri::command]
fn list_annotations(key: String, state: tauri::State<AppState>) -> Result<Vec<Highlight>, String> {
    state
        .annotations
        .lock()
        .map_err(|e| e.to_string())
        .map(|store| store.list(&key))
}

#[tauri::command]
fn add_annotation(
    key: String,
    href: String,
    start_text: usize,
    start_offset: usize,
    end_text: usize,
    end_offset: usize,
    text: String,
    state: tauri::State<AppState>,
) -> Result<Highlight, String> {
    let highlight = Highlight {
        id: Uuid::new_v4().to_string(),
        href,
        start_text,
        start_offset,
        end_text,
        end_offset,
        text,
        created_at: unix_now(),
    };
    state
        .annotations
        .lock()
        .map_err(|e| e.to_string())?
        .add(key, highlight.clone())
        .map_err(|e| e.to_string())?;
    Ok(highlight)
}

#[tauri::command]
fn delete_annotation(key: String, id: String, state: tauri::State<AppState>) -> Result<(), String> {
    state
        .annotations
        .lock()
        .map_err(|e| e.to_string())?
        .remove(&key, &id)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn get_chapter(id: String, href: String, state: tauri::State<AppState>) -> Result<ChapterView, String> {
    let (html, publisher_fonts) = {
        let books = state.books.lock().map_err(|e| e.to_string())?;
        let book = books.get(&id).ok_or_else(|| "book not open".to_string())?;
        let base = protocol::resource_base(&id);
        let html = book
            .chapter_html(&href, &base)
            .map_err(|e| e.to_string())?;
        let publisher_fonts = collect_publisher_fonts(&html, &base, &href, |res_href| {
            load_book_text(book.as_ref(), res_href)
        });
        (html, publisher_fonts)
    };
    let settings = state.settings.lock().map_err(|e| e.to_string())?;
    Ok(ChapterView {
        html: fonts::apply_html_if_active(html, &settings),
        publisher_fonts,
    })
}

fn load_book_text(book: &dyn Book, href: &str) -> Option<String> {
    let trimmed = href
        .split(['#', '?'])
        .next()
        .unwrap_or(href)
        .trim()
        .trim_start_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    for candidate in [trimmed.to_string(), format!("/{trimmed}")] {
        if let Ok(res) = book.resource(&candidate) {
            if let Ok(text) = String::from_utf8(res.data) {
                return Some(text);
            }
        }
    }
    None
}

#[tauri::command]
fn save_progress(
    key: String,
    href: String,
    fraction: f64,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    state
        .progress
        .lock()
        .map_err(|e| e.to_string())?
        .set(
            key,
            Locator {
                href,
                fraction,
                cfi: None,
            },
        )
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn resource_origin() -> String {
    protocol::origin().into()
}

#[tauri::command]
fn get_font_settings(state: tauri::State<AppState>) -> Result<FontSettingsView, String> {
    let settings = state.settings.lock().map_err(|e| e.to_string())?;
    Ok(settings.view())
}

#[tauri::command]
fn set_use_original_fonts(
    use_original_fonts: bool,
    state: tauri::State<AppState>,
) -> Result<FontSettingsView, String> {
    let mut settings = state.settings.lock().map_err(|e| e.to_string())?;
    settings
        .set_use_original_fonts(use_original_fonts)
        .map_err(|e| e.to_string())?;
    Ok(settings.view())
}

#[tauri::command]
fn set_font_scale(
    font_scale: u32,
    state: tauri::State<AppState>,
) -> Result<FontSettingsView, String> {
    let mut settings = state.settings.lock().map_err(|e| e.to_string())?;
    settings
        .set_font_scale(font_scale)
        .map_err(|e| e.to_string())?;
    Ok(settings.view())
}

#[tauri::command]
fn install_font(
    slot: String,
    path: String,
    state: tauri::State<AppState>,
) -> Result<FontSettingsView, String> {
    let slot = FontSlot::parse(&slot).ok_or_else(|| "未知字体槽位".to_string())?;
    let file = fonts::copy_into_slot(slot, std::path::Path::new(&path))?;
    let mut settings = state.settings.lock().map_err(|e| e.to_string())?;
    settings.set_font(slot, file).map_err(|e| e.to_string())?;
    Ok(settings.view())
}

#[tauri::command]
fn clear_font(slot: String, state: tauri::State<AppState>) -> Result<FontSettingsView, String> {
    let slot = FontSlot::parse(&slot).ok_or_else(|| "未知字体槽位".to_string())?;
    let view = {
        let mut settings = state.settings.lock().map_err(|e| e.to_string())?;
        settings.clear_font(slot).map_err(|e| e.to_string())?;
        settings.view()
    };
    fonts::delete_slot_files(slot);
    Ok(view)
}

/// List the shelf. File-bound metadata is served from the per-revision cache
/// (the expensive open + flattened TOC happens once per changed file); only
/// the progress fields come from the live store.
#[tauri::command]
fn list_library(state: tauri::State<AppState>) -> Result<Vec<library::LibraryEntry>, String> {
    let dir = portable::library_dir().map_err(|e| e.to_string())?;
    let progress = state.progress.lock().map_err(|e| e.to_string())?;
    let mut cache = state.library_meta.lock().map_err(|e| e.to_string())?;
    Ok(library::list_library_cached(&dir, &progress, &mut cache))
}

/// Open one book's editable metadata (the companion md) for the 编辑元数据
/// panel. Reads the md per call — never part of the list hot path.
#[tauri::command]
fn get_book_meta(
    file_name: String,
    state: tauri::State<AppState>,
) -> Result<book_meta::BookMetaView, String> {
    let dir = portable::library_dir().map_err(|e| e.to_string())?;
    let md_path = library::meta_path_for(&dir, &file_name)?;
    let path = dir.join(&file_name);
    if !path.is_file() {
        return Err("book not in library".into());
    }
    let overlay = read_meta_file(&md_path);
    let profile = {
        let mut cache = state.library_meta.lock().map_err(|e| e.to_string())?;
        cache.profile(&path, &dir)
    };
    Ok(book_meta::view_for(&profile, overlay.as_ref()))
}

/// Save one book's metadata to its companion md. Creates the md on first save
/// (freezing bookFile / originalTitle), overwrites it afterwards. The md is
/// program-maintained — the UI panel is the only editing surface.
#[tauri::command]
fn set_book_meta(
    file_name: String,
    fields: book_meta::BookMetaFields,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    let dir = portable::library_dir().map_err(|e| e.to_string())?;
    let md_path = library::meta_path_for(&dir, &file_name)?;
    let path = dir.join(&file_name);
    if !path.is_file() {
        return Err("book not in library".into());
    }

    let existing = read_meta_file(&md_path);
    let original_title = existing
        .as_ref()
        .and_then(|m| m.original_title.clone())
        .unwrap_or_else(|| {
            state
                .library_meta
                .lock()
                .ok()
                .map(|mut cache| cache.profile(&path, &dir).title)
                .unwrap_or_else(|| file_name.trim_end_matches(".epub").to_string())
        });
    let meta = BookMeta {
        book_file: existing
            .as_ref()
            .and_then(|m| m.book_file.clone())
            .or_else(|| Some(file_name.clone())),
        original_title: Some(original_title),
        title: clean_title(&fields.title),
        subtitle: clean_title(&fields.subtitle),
        volume: clean_title(&fields.volume),
        display_title: clean_title(&fields.display_title),
    };
    write_meta_file(&md_path, &meta).map_err(|e| e.to_string())
}

/// Remove a library book: its epub file first, then the progress and
/// annotation records keyed to it. Callers must confirm with the user first.
#[tauri::command]
fn delete_book(
    file_name: String,
    progress_key: String,
    state: tauri::State<AppState>,
) -> Result<(), String> {
    let dir = portable::library_dir().map_err(|e| e.to_string())?;
    library::delete_book_from(&dir, &file_name)?;
    state
        .progress
        .lock()
        .map_err(|e| e.to_string())?
        .remove(&progress_key)
        .map_err(|e| e.to_string())?;
    state
        .annotations
        .lock()
        .map_err(|e| e.to_string())?
        .remove_book(&progress_key)
        .map_err(|e| e.to_string())?;
    // Drop any cached metadata / cover bytes for the removed file.
    let deleted = dir.join(&file_name);
    if let Ok(mut cache) = state.library_meta.lock() {
        cache.remove(&deleted);
    }
    if let Ok(mut cache) = state.covers.lock() {
        cache.remove(&file_name);
    }
    book_signals::remove(&file_name);
    Ok(())
}

#[tauri::command]
fn pending_book() -> Option<String> {
    std::env::var("ICED_READER_OPEN").ok().filter(|p| !p.is_empty())
}

#[tauri::command]
fn close_book(id: String, state: tauri::State<AppState>) -> Result<(), String> {
    state
        .books
        .lock()
        .map_err(|e| e.to_string())?
        .remove(&id);
    Ok(())
}

fn disable_browser_accelerators(window: &tauri::WebviewWindow) {
    let _ = window.with_webview(|webview| {
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings3;
        use windows::core::Interface;
        let controller = webview.controller();
        if let Ok(core) = unsafe { controller.CoreWebView2() } {
            if let Ok(settings) = unsafe { core.Settings() } {
                if let Ok(settings3) = settings.cast::<ICoreWebView2Settings3>() {
                    let _ = unsafe { settings3.SetAreBrowserAcceleratorKeysEnabled(false) };
                }
            }
        }
    });
}

pub fn run() {
    portable::prepare_webview_env();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            books: Mutex::new(HashMap::new()),
            progress: Mutex::new(ProgressStore::in_memory()),
            settings: Mutex::new(SettingsStore::in_memory(std::path::PathBuf::from("fonts"))),
            annotations: Mutex::new(AnnotationStore::in_memory()),
            library_meta: Mutex::new(library::LibraryMetaCache::default()),
            covers: Mutex::new(library::CoverCache::default()),
        })
        .setup(|app| {
            portable::ensure_layout().map_err(|e| e.to_string())?;
            if let Ok(file) = portable::progress_file() {
                if let Ok(store) = ProgressStore::open(file) {
                    if let Ok(mut slot) = app.state::<AppState>().progress.lock() {
                        *slot = store;
                    }
                }
            }
            if let (Ok(file), Ok(fonts_dir)) = (portable::settings_file(), portable::fonts_dir()) {
                if let Ok(store) = SettingsStore::open(file, fonts_dir) {
                    if let Ok(mut slot) = app.state::<AppState>().settings.lock() {
                        *slot = store;
                    }
                }
            }
            if let Ok(file) = portable::annotations_file() {
                if let Ok(store) = AnnotationStore::open(file) {
                    if let Ok(mut slot) = app.state::<AppState>().annotations.lock() {
                        *slot = store;
                    }
                }
            }
            let webview_dir = portable::webview_dir().map_err(|e| e.to_string())?;
            let conf = app
                .config()
                .app
                .windows
                .first()
                .cloned()
                .ok_or("missing window config")?;
            let window = tauri::WebviewWindowBuilder::from_config(app, &conf)?
                .data_directory(webview_dir)
                .build()?;
            let _ = window.set_title(&window_title(&conf.title));
            window_state::attach(&window);
            disable_browser_accelerators(&window);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            open_book,
            close_book,
            list_library,
            get_book_meta,
            set_book_meta,
            delete_book,
            resource_origin,
            pending_book,
            get_chapter,
            save_progress,
            list_annotations,
            add_annotation,
            delete_annotation,
            get_font_settings,
            set_use_original_fonts,
            set_font_scale,
            install_font,
            clear_font
        ])
        .register_uri_scheme_protocol("icedreader", protocol::handle)
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::window_title;

    #[test]
    fn title_always_contains_version() {
        let title = window_title("IcedReader");
        assert!(title.starts_with(&format!("IcedReader {}", env!("CARGO_PKG_VERSION"))));
    }

    #[test]
    fn title_has_hash_when_env_injected() {
        let title = window_title("IcedReader");
        match option_env!("ICED_READER_GIT_HASH") {
            Some(hash) if !hash.is_empty() => {
                assert_eq!(title, format!("IcedReader {} ({hash})", env!("CARGO_PKG_VERSION")));
            }
            _ => assert_eq!(title, format!("IcedReader {}", env!("CARGO_PKG_VERSION"))),
        }
    }
}
