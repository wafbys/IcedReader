mod protocol;

use std::collections::HashMap;
use std::sync::Mutex;

use iced_reader_core::{
    progress_key, Book, BookOpener, Locator, Metadata, ProgressStore, SpineItem, TocNode,
};
use iced_reader_epub::EpubOpener;
use serde::Serialize;
use tauri::Manager;
use uuid::Uuid;

pub struct AppState {
    pub books: Mutex<HashMap<String, Box<dyn Book>>>,
    pub progress: Mutex<ProgressStore>,
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
    if !opener.can_open(std::path::Path::new(&path)) {
        return Err(format!("unsupported file: {path}"));
    }
    let book = opener
        .open(std::path::Path::new(&path))
        .map_err(|e| e.to_string())?;
    let metadata = book.metadata();
    let key = progress_key(std::path::Path::new(&path), &metadata.identifiers);
    let progress = state
        .progress
        .lock()
        .ok()
        .and_then(|store| store.get(&key).map(|r| r.locator.clone()));
    let opened = OpenedBook {
        id: Uuid::new_v4().to_string(),
        format: book.format_id().to_string(),
        path: path.clone(),
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

#[tauri::command]
fn get_chapter(id: String, href: String, state: tauri::State<AppState>) -> Result<String, String> {
    let books = state.books.lock().map_err(|e| e.to_string())?;
    let book = books.get(&id).ok_or_else(|| "book not open".to_string())?;
    book.chapter_html(&href, &protocol::resource_base(&id))
        .map_err(|e| e.to_string())
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
    #[cfg(any(windows, target_os = "android"))]
    {
        "http://icedreader.localhost".into()
    }
    #[cfg(not(any(windows, target_os = "android")))]
    {
        "icedreader://localhost".into()
    }
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(AppState {
            books: Mutex::new(HashMap::new()),
            progress: Mutex::new(ProgressStore::in_memory()),
        })
        .setup(|app| {
            if let Ok(dir) = app.path().app_data_dir() {
                let _ = std::fs::create_dir_all(&dir);
                if let Ok(store) = ProgressStore::open(dir.join("progress.json")) {
                    if let Ok(mut slot) = app.state::<AppState>().progress.lock() {
                        *slot = store;
                    }
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            open_book,
            close_book,
            resource_origin,
            pending_book,
            get_chapter,
            save_progress
        ])
        .register_uri_scheme_protocol("icedreader", protocol::handle)
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
