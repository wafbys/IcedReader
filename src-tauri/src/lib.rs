mod fonts;
mod book_compare;
mod book_meta;
mod book_signals;
mod library;
mod notes;
mod openers;
mod portable;
mod protocol;
mod window_state;

use std::collections::HashMap;
use std::fs;
use std::sync::{Arc, Mutex};

use iced_reader_core::{
    book_extension, clean_person_list, clean_title, collect_publisher_fonts, progress_key,
    read_meta_file, resolved_title, write_meta_file, AnnotationStore, Book, BookMeta,
    ChapterView, FontSettingsView, FontSlot, Highlight, Locator, Metadata, ProgressStore,
    SettingsStore, SpineItem, TocNode, COLOR_GREEN, COLOR_YELLOW, PDF_FORMAT,
};
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
    /// Books sit behind an `Arc` so a command can take one **out of the map and
    /// drop the lock** before doing slow work. That matters for PDF:
    /// rasterising a page is synchronous and a heavy page can take 0.7–1.7 s,
    /// and holding the map lock across it would stall every other book
    /// operation.
    pub books: Mutex<HashMap<String, Arc<dyn Book>>>,
    pub progress: Mutex<ProgressStore>,
    pub settings: Mutex<SettingsStore>,
    pub annotations: Mutex<AnnotationStore>,
    /// Per-file-revision shelf metadata (avoids re-opening big epubs on every shelf refresh).
    pub library_meta: Mutex<library::LibraryMetaCache>,
    /// Per-file-revision cover bytes (avoids re-opening the archive per cover
    /// request). Behind an `Arc` so a cover miss can warm it on a worker thread
    /// while the protocol request answers with a placeholder immediately.
    pub covers: Arc<Mutex<library::CoverCache>>,
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
    /// Per-chapter raw visible-text char counts (spine order) + implicit total.
    /// Whole-book position weights for notes.md 全书% and 按位置跳转.
    /// PDFs weigh every page equally (`1` per page) — a page is the unit.
    #[serde(rename = "chapterChars")]
    pub chapter_chars: Vec<u64>,
    /// Format-specific notices for the reader. PDFs whose **visible** text uses
    /// fonts the renderer cannot supply land here (see
    /// `iced_reader_pdf::PdfDoc::visible_text_risk`); empty for EPUB.
    pub warnings: Vec<String>,
    /// Per-spine-unit size in the format's own units, for **fixed-layout**
    /// formats (PDF): the shell builds one continuous strip of page
    /// placeholders from it, so scroll length and page jumps are right before
    /// any image arrives. Empty for reflowable formats (EPUB).
    #[serde(rename = "pageSizes")]
    pub page_sizes: Vec<(f32, f32)>,
}

#[tauri::command]
async fn open_book(path: String, state: tauri::State<'_, AppState>) -> Result<OpenedBook, String> {
    let source = std::path::Path::new(&path);
    let opener = openers::opener_for(source)
        .ok_or_else(|| format!("unsupported file: {path}"))?;
    let is_pdf = opener.format_id() == PDF_FORMAT;
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
    // Computed **in the background**: a whole-document analysis is not cheap
    // (a 952-page PDF spends ~1 s even with optimized dependencies, and the
    // reader would show a blank window for that long). The shelf picks the
    // grade up from `book-signals.json` on its next listing; this open's
    // `warnings` come from an already-cached analysis when there is one.
    let rev = library::file_rev(&imported);
    let file_name = imported
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let cached_signals = book_signals::read_all().get(&file_name).cloned();
    if !file_name.is_empty() {
        let need = match &cached_signals {
            None => true,
            Some(s) => {
                s.rev != rev
                    || s.analysis_kind != book_signals::ANALYSIS_KIND
                    || (is_pdf && s.pdf.is_none())
                    || (!is_pdf && (s.chapter_chars.is_empty() || s.chapter_chars_kind != book_signals::CHAPTER_CHARS_PER_SPINE))
            }
        };
        if need {
            book_signals::analyze_in_background(imported.clone(), file_name.clone(), rev.clone());
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

    let spine = book.spine();
    // PDFs: one page is one spine unit, so every page weighs the same and the
    // whole-book % / 按位置跳转 work on page numbers without any text metrics.
    let chapter_chars = if is_pdf {
        vec![1u64; spine.len()]
    } else {
        book_signals::read_all()
            .get(&file_name)
            .map(|s| s.chapter_chars.clone())
            .unwrap_or_default()
    };
    let warnings = if is_pdf {
        pdf_warnings(&file_name, &rev, cached_signals.as_ref())
    } else {
        Vec::new()
    };

    let opened = OpenedBook {
        id: Uuid::new_v4().to_string(),
        format: book.format_id().to_string(),
        path: imported.to_string_lossy().into_owned(),
        progress_key: key,
        progress,
        metadata,
        toc: book.toc(),
        spine,
        chapter_chars,
        warnings,
        page_sizes: book.page_sizes(),
    };
    state
        .books
        .lock()
        .map_err(|e| e.to_string())?
        .insert(opened.id.clone(), Arc::from(book));
    Ok(opened)
}

/// Notices for a PDF the renderer cannot draw completely: visible text whose
/// font is neither embedded nor one of the standard 14. Scanned books (with or
/// without an invisible OCR layer) are **not** affected, which is why the check
/// looks at text rendering modes instead of just at the font list.
///
/// Reads the **cached** analysis instead of opening the document again: the
/// census costs ~1 s on a big book, and opening a book must not wait for it
/// (the background analysis fills the cache; the next open shows the banner).
fn pdf_warnings(
    file_name: &str,
    rev: &str,
    cached: Option<&book_signals::BookSignals>,
) -> Vec<String> {
    let fonts = cached
        .filter(|s| s.rev == rev)
        .and_then(|s| s.pdf.as_ref())
        .map(|pdf| pdf.unresolved_visible_fonts.clone())
        .unwrap_or_default();
    if fonts.is_empty() {
        return Vec::new();
    }
    let _ = file_name;
    vec![format!(
        "此 PDF 的正文使用了未嵌入的字体（{}），这些页可能显示不全。",
        fonts.join("、")
    )]
}
fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 本地 ISO 时间（notes.md 注释块机器字段）。
fn local_iso(secs: i64) -> String {
    use chrono::{DateTime, Local};
    DateTime::from_timestamp(secs, 0)
        .map(|d| d.with_timezone(&Local).format("%Y-%m-%dT%H:%M:%S%:z").to_string())
        .unwrap_or_default()
}

/// 本地人类可读时间（引用行「划于 …」「已删于 …」）。
fn local_human(secs: i64) -> String {
    use chrono::{DateTime, Local};
    DateTime::from_timestamp(secs, 0)
        .map(|d| d.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_default()
}

/// 划线 href → spine 下标（文件 + 可选 #fragment 双匹配；摊平目录里同一
/// 文件多锚点靠 fragment 精确，回退到仅文件）。
fn spine_index_for(spine: &[SpineItem], href: &str) -> Option<usize> {
    fn key(h: &str) -> (String, String) {
        let (file, frag) = h
            .split_once('#')
            .map(|(a, b)| (a, Some(b)))
            .unwrap_or((h, None));
        let file = file
            .split('?')
            .next()
            .unwrap_or(file)
            .trim()
            .trim_start_matches('/')
            .to_lowercase();
        (file, frag.map(|f| f.to_lowercase()).unwrap_or_default())
    }
    let (file, frag) = key(href);
    spine
        .iter()
        .position(|s| {
            let (sf, sfrag) = key(&s.href);
            sf == file && sfrag == frag
        })
        .or_else(|| spine.iter().position(|s| key(&s.href).0 == file))
}

/// 读某本书的 notes.md（不存在/无档案返回空串）。
fn read_notes_text(file_name: &str) -> String {
    let Ok(dir) = portable::library_dir() else {
        return String::new();
    };
    match notes::notes_path_for(&dir, file_name) {
        Ok(path) => fs::read_to_string(path).unwrap_or_default(),
        Err(_) => String::new(),
    }
}

/// 写某本书的 notes.md；空内容 = 移除档案文件。
fn write_notes_text(file_name: &str, text: &str) -> Result<(), String> {
    let dir = portable::library_dir().map_err(|e| e.to_string())?;
    let path = notes::notes_path_for(&dir, file_name)?;
    if text.trim().is_empty() {
        if path.is_file() {
            fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("notes.md.tmp");
    fs::write(&tmp, text).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())?;
    Ok(())
}

/// notes.md 里一条划线的用户笔记（读回供悬停/列表）。
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct NoteView {
    id: String,
    note: String,
}

#[tauri::command]
fn list_annotations(key: String, state: tauri::State<'_, AppState>) -> Result<Vec<Highlight>, String> {
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
    color: String,
    pos: f64,
    state: tauri::State<'_, AppState>,
) -> Result<Highlight, String> {
    // 颜色规范化：只认 yellow/green，其余归默认黄（存储 key 即 ::highlight 名）。
    let color = if color == COLOR_GREEN {
        COLOR_GREEN.to_string()
    } else {
        COLOR_YELLOW.to_string()
    };
    let highlight = Highlight {
        id: Uuid::new_v4().to_string(),
        href,
        start_text,
        start_offset,
        end_text,
        end_offset,
        text,
        color,
        pos: pos.clamp(0.0, 1.0),
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

/// 删除划线：正文记录移除；notes.md 里该条若存在（写过备注）则打删除时间
/// 留痕、用户笔记保留。纯划线（从未写备注）删除后档案无痕。
#[tauri::command]
fn delete_annotation(
    file_name: String,
    key: String,
    id: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let text = read_notes_text(&file_name);
    if !text.is_empty() {
        let now = unix_now();
        if let Some(updated) = notes::mark_deleted(&text, &id, &local_iso(now), &local_human(now))
        {
            write_notes_text(&file_name, &updated)?;
        }
    }
    state
        .annotations
        .lock()
        .map_err(|e| e.to_string())?
        .remove(&key, &id)
        .map_err(|e| e.to_string())?;
    Ok(())
}

/// 写/改一条划线的备注（notes.md 用户区）。空串 = 撤掉该备注：移除程序
/// 保护区，用户区文字转普通文本保留（外部编辑器写的不丢）。
#[tauri::command]
fn save_note(
    file_name: String,
    book_id: String,
    key: String,
    id: String,
    note: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    // 划线必须在当前书里；章归属需要打开的书（spine 标题）。
    let rec = state
        .annotations
        .lock()
        .map_err(|e| e.to_string())?
        .list(&key)
        .into_iter()
        .find(|h| h.id == id)
        .ok_or_else(|| "划线不存在".to_string())?;
    let (section_title, created_iso, excerpt) = {
        // Clone the `Arc` out and drop the map lock before asking the book for
        // its spine (an EPUB spine read touches the archive).
        let book = {
            let books = state.books.lock().map_err(|e| e.to_string())?;
            books
                .get(&book_id)
                .ok_or_else(|| "book not open".to_string())?
                .clone()
        };
        let spine = book.spine();
        let idx = spine_index_for(&spine, &rec.href).ok_or_else(|| "无法定位划线章节".to_string())?;
        let total = spine.len();
        let title = spine[idx]
            .title
            .clone()
            .unwrap_or_default()
            .trim()
            .to_string();
        let title_show = if title.is_empty() {
            rec.href.clone()
        } else {
            title
        };
        let section = format!("## 第 {} 章 · {}（{}/{}）", idx + 1, title_show, idx + 1, total);
        let created_iso = local_iso(rec.created_at);
        let created_human = local_human(rec.created_at);
        let excerpt = format!(
            "> 【{}】{}（{} · 划于 {}）",
            notes::color_label(&rec.color),
            rec.text,
            notes::pos_label(rec.pos),
            created_human
        );
        (section, created_iso, excerpt)
    };

    let text = read_notes_text(&file_name);
    if note.trim().is_empty() {
        if !text.is_empty() {
            if let Some(updated) = notes::remove_note(&text, &id) {
                write_notes_text(&file_name, &updated)?;
            }
        }
        return Ok(());
    }
    let comment_lines = vec![
        notes::NOTE_OPEN.to_string(),
        format!("id: {id}"),
        format!("color: {}", rec.color),
        format!("created: {created_iso}"),
        "deleted:".to_string(),
        format!("posPct: {}", (rec.pos.clamp(0.0, 1.0) * 100.0).round() as u32),
        notes::NOTE_CLOSE.to_string(),
    ];
    let entry = notes::NoteEntry {
        id,
        section_title,
        comment_lines,
        excerpt,
        note,
    };
    let updated = notes::upsert(&text, &entry);
    write_notes_text(&file_name, &updated)
}

/// 读出整本 notes.md 的用户笔记（id → 笔记），供悬停浮层与划线列表。
#[tauri::command]
fn read_notes(file_name: String) -> Result<Vec<NoteView>, String> {
    let text = read_notes_text(&file_name);
    Ok(notes::notes_of(&text)
        .into_iter()
        .map(|(id, note)| NoteView { id, note })
        .collect())
}

#[tauri::command]
async fn get_chapter(id: String, href: String, state: tauri::State<'_, AppState>) -> Result<ChapterView, String> {
    // Take the book out of the map and drop the lock before laying out: for a
    // PDF this call rasterises a page (up to ~1.7 s on a heavy first page).
    let book = {
        let books = state.books.lock().map_err(|e| e.to_string())?;
        books
            .get(&id)
            .ok_or_else(|| "book not open".to_string())?
            .clone()
    };
    let is_pdf = book.format_id() == PDF_FORMAT;
    let base = protocol::resource_base(&id);
    let html = book
        .chapter_html(&href, &base)
        .map_err(|e| e.to_string())?;
    // A PDF page document is our own and carries no CSS, so the publisher
    // font report comes back empty either way.
    let publisher_fonts = collect_publisher_fonts(&html, &base, &href, |res_href| {
        load_book_text(book.as_ref(), res_href)
    });
    // Custom reader fonts never touch a PDF page: the glyphs are already
    // rasterised into the image.
    if is_pdf {
        return Ok(ChapterView {
            html,
            publisher_fonts,
        });
    }
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
    state: tauri::State<'_, AppState>,
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
fn get_font_settings(state: tauri::State<'_, AppState>) -> Result<FontSettingsView, String> {
    let settings = state.settings.lock().map_err(|e| e.to_string())?;
    Ok(settings.view())
}

#[tauri::command]
fn set_use_original_fonts(
    use_original_fonts: bool,
    state: tauri::State<'_, AppState>,
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
    state: tauri::State<'_, AppState>,
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
    state: tauri::State<'_, AppState>,
) -> Result<FontSettingsView, String> {
    let slot = FontSlot::parse(&slot).ok_or_else(|| "未知字体槽位".to_string())?;
    let file = fonts::copy_into_slot(slot, std::path::Path::new(&path))?;
    let mut settings = state.settings.lock().map_err(|e| e.to_string())?;
    settings.set_font(slot, file).map_err(|e| e.to_string())?;
    Ok(settings.view())
}

#[tauri::command]
fn clear_font(slot: String, state: tauri::State<'_, AppState>) -> Result<FontSettingsView, String> {
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
async fn list_library(state: tauri::State<'_, AppState>) -> Result<Vec<library::LibraryEntry>, String> {
    let dir = portable::library_dir().map_err(|e| e.to_string())?;
    let progress = {
        let store = state.progress.lock().map_err(|e| e.to_string())?;
        store.snapshot()
    };
    let mut cache = state.library_meta.lock().map_err(|e| e.to_string())?;
    // Read the signals cache once per listing (never recomputed here).
    let signals = book_signals::read_all();
    let entries = library::list_library_cached(&dir, &progress, &mut cache, &signals);
    drop(cache);
    // Warm covers for this shelf **off** the listing path: a PDF cover costs
    // real CPU, and a protocol request must never do it (see
    // `serve_library_cover`). With this the first cover request usually hits and
    // the shelf shows real covers instead of the placeholder.
    for entry in &entries {
        if entry.has_cover {
            library::warm_cover_in_background(
                dir.join(&entry.file_name),
                entry.file_name.clone(),
                std::sync::Arc::clone(&state.covers),
            );
        }
    }
    Ok(entries)
}

/// 同书对照: compare two or more shelf copies of one work and say which to
/// keep. On-demand only — never part of the shelf listing.
///
/// A copy whose signals were never computed (imported but never opened) is
/// analyzed right here rather than silently dropped: a comparison missing one
/// of the books would be worse than a couple of seconds of waiting.
#[tauri::command]
fn compare_books(
    file_names: Vec<String>,
    state: tauri::State<'_, AppState>,
) -> Result<book_compare::Comparison, String> {
    if file_names.len() < 2 {
        return Err("至少需要两本同书才能对照".into());
    }
    let dir = portable::library_dir().map_err(|e| e.to_string())?;

    // Reuse the shelf listing so the panel shows exactly the titles and grades
    // the shelf shows (companion md included).
    let progress = {
        let store = state.progress.lock().map_err(|e| e.to_string())?;
        store.snapshot()
    };
    let mut cache = state
        .library_meta
        .lock()
        .map_err(|e| e.to_string())?;
    let entries =
        library::list_library_cached(&dir, &progress, &mut cache, &book_signals::read_all());
    drop(cache);

    let mut inputs = Vec::with_capacity(file_names.len());
    for file_name in &file_names {
        // Same guard as `meta_path_for` / `library_cover_path`: a plain file
        // name inside the library, never a path.
        if file_name.is_empty()
            || std::path::Path::new(file_name)
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err("invalid book file name".into());
        }
        if file_name.to_ascii_lowercase().ends_with(".pdf") {
            return Err("PDF 暂不参与同书对照".into());
        }
        let Some(entry) = entries.iter().find(|e| &e.file_name == file_name) else {
            return Err(format!("不在书库中：{file_name}"));
        };
        let path = dir.join(file_name);
        if !path.is_file() {
            return Err(format!("不在书库中：{file_name}"));
        }
        if entry.open_error.is_some() {
            return Err(format!("打不开：{file_name}"));
        }

        // Cached signals only count for the file revision they were computed
        // from — the same rule the shelf grades by.
        let rev = library::file_rev(&path);
        let signals = book_signals::read_all()
            .get(file_name)
            .filter(|s| s.rev == rev && s.analysis_kind == book_signals::ANALYSIS_KIND)
            .cloned()
            .or_else(|| book_signals::analyze_path_now(&path, file_name, &rev))
            .ok_or_else(|| format!("无法分析：{file_name}"))?;

        inputs.push(book_compare::CompareInput {
            file_name: file_name.clone(),
            title: entry.title.clone(),
            quality: entry.quality.clone(),
            size_bytes: std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
            signals,
        });
    }

    book_compare::compare(&inputs)
}

/// Open one book's editable metadata (the companion md) for the 编辑元数据
/// panel. Reads the md per call — never part of the list hot path.
#[tauri::command]
fn get_book_meta(
    file_name: String,
    state: tauri::State<'_, AppState>,
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

/// Re-read the book's own metadata and rebuild the 编辑元数据 form from it
/// (清空手填、填充原书字段)。The panel only fills the form with the result —
/// saving stays an explicit separate action by the user.
#[tauri::command]
async fn reread_book_meta(
    file_name: String,
    state: tauri::State<'_, AppState>,
) -> Result<book_meta::BookMetaView, String> {
    let dir = portable::library_dir().map_err(|e| e.to_string())?;
    let md_path = library::meta_path_for(&dir, &file_name)?;
    let path = dir.join(&file_name);
    if !path.is_file() {
        return Err("book not in library".into());
    }
    let existing = read_meta_file(&md_path);
    let profile = {
        let mut cache = state.library_meta.lock().map_err(|e| e.to_string())?;
        cache.profile(&path, &dir)
    };
    let original_title = existing
        .and_then(|m| m.original_title)
        .unwrap_or_else(|| profile.title.clone());
    let book = openers::open_any(&path).map_err(|e| e.to_string())?;
    let metadata = book.metadata();
    Ok(book_meta::reread_view_for(&profile, &original_title, &metadata))
}

/// Save one book's metadata to its companion md. Creates the md on first save
/// (freezing bookFile / originalTitle), overwrites it afterwards. The md is
/// program-maintained — the UI panel is the only editing surface.
#[tauri::command]
async fn set_book_meta(
    file_name: String,
    fields: book_meta::BookMetaFields,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let dir = portable::library_dir().map_err(|e| e.to_string())?;
    let md_path = library::meta_path_for(&dir, &file_name)?;
    let path = dir.join(&file_name);
    if !path.is_file() {
        return Err("book not in library".into());
    }

    let existing = read_meta_file(&md_path);
    // File-bound profile (dc:title base + progress key). Opening an epub only
    // happens on a cache miss; saving metadata is low-frequency, so fine.
    let profile = state
        .library_meta
        .lock()
        .map_err(|e| e.to_string())?
        .profile(&path, &dir);
    let original_title = existing
        .as_ref()
        .and_then(|m| m.original_title.clone())
        .unwrap_or_else(|| profile.title.clone());

    // The display title the user will see after this save (hand-confirmed
    // displayTitle wins, else the field join). The library file is renamed to
    // its cleaned form so the on-disk name matches what the shelf shows.
    let staged = BookMeta {
        title: clean_title(&fields.title),
        subtitle: clean_title(&fields.subtitle),
        volume: clean_title(&fields.volume),
        author: clean_person_list(&fields.author),
        translator: clean_person_list(&fields.translator),
        year: clean_title(&fields.year),
        publisher: clean_title(&fields.publisher),
        isbn: clean_title(&fields.isbn),
        display_title: clean_title(&fields.display_title),
        book_file: None,
        original_title: None,
    };
    let display_title = resolved_title(Some(&staged), &profile.title);
    let old_stem = library::epub_stem(&file_name);
    let desired_stem = library::clean_file_stem(&display_title);
    let md_name = format!("{old_stem}.md");
    let notes_name = format!("{old_stem}.notes.md");
    let target_stem = library::unique_stem_ignoring(
        &dir,
        &desired_stem,
        &[file_name.as_str(), md_name.as_str(), notes_name.as_str()],
    );
    let needs_rename = !old_stem.eq_ignore_ascii_case(&target_stem);

    let (final_file_name, final_md_path) = if needs_rename {
        let new_name = library::rename_book_files(&dir, &file_name, &target_stem)?;
        // `id:` / `path:` progress keys survive a rename untouched; only the
        // `lib:` key (which embeds the file name) must be carried over, along
        // with the highlights and cached quality signals under the old name.
        if profile.progress_key.starts_with("lib:") {
            // The key embeds the file name *and its format*: renaming a PDF
            // must not carry the EPUB of the same title along.
            let extension = book_extension(&file_name).unwrap_or("epub");
            let new_lib_key = format!("lib:{}.{}", target_stem.to_lowercase(), extension);
            state
                .progress
                .lock()
                .map_err(|e| e.to_string())?
                .rename_key(&profile.progress_key, &new_lib_key)
                .map_err(|e| e.to_string())?;
            state
                .annotations
                .lock()
                .map_err(|e| e.to_string())?
                .rename_book(&profile.progress_key, &new_lib_key)
                .map_err(|e| e.to_string())?;
        }
        book_signals::rename_key(&file_name, &new_name);
        // Drop cached shelf metadata/cover bytes for the old path/name so the
        // next listing/profile call rebuilds them under the new identity.
        if let Ok(mut cache) = state.library_meta.lock() {
            cache.remove(&path);
        }
        if let Ok(mut cache) = state.covers.lock() {
            cache.remove(&file_name);
        }
        let final_md = library::meta_path_for(&dir, &new_name)?;
        (new_name, final_md)
    } else {
        (file_name.clone(), md_path)
    };

    let meta = BookMeta {
        book_file: existing
            .as_ref()
            .and_then(|m| m.book_file.clone())
            .or_else(|| Some(final_file_name.clone())),
        original_title: Some(original_title),
        title: clean_title(&fields.title),
        subtitle: clean_title(&fields.subtitle),
        volume: clean_title(&fields.volume),
        author: clean_person_list(&fields.author),
        translator: clean_person_list(&fields.translator),
        year: clean_title(&fields.year),
        publisher: clean_title(&fields.publisher),
        isbn: clean_title(&fields.isbn),
        display_title: clean_title(&fields.display_title),
    };
    write_meta_file(&final_md_path, &meta).map_err(|e| e.to_string())
}

/// Remove a library book: its file first, then the progress and
/// annotation records keyed to it. Callers must confirm with the user first.
#[tauri::command]
fn delete_book(
    file_name: String,
    progress_key: String,
    state: tauri::State<'_, AppState>,
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
fn close_book(id: String, state: tauri::State<'_, AppState>) -> Result<(), String> {
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
            covers: Arc::new(Mutex::new(library::CoverCache::default())),
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
            compare_books,
            get_book_meta,
            reread_book_meta,
            set_book_meta,
            delete_book,
            resource_origin,
            pending_book,
            get_chapter,
            save_progress,
            list_annotations,
            add_annotation,
            delete_annotation,
            save_note,
            read_notes,
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
