//! User-editable per-book metadata, persisted as a companion Markdown file
//! next to the epub in `data/library/` (`<stem>.md`, e.g. `三体.epub` ↔
//! `三体.md`).
//!
//! The md file is **maintained by the program, not hand-edited by the user**
//! (the UI panel is the editing surface). Format:
//!
//! ```markdown
//! <!-- icedreader-meta
//! bookFile: 三体.epub
//! originalTitle: 三体
//! title: 三体
//! subtitle:
//! volume: 死神永生
//! displayTitle: 三体 _ 死神永生
//! -->
//! ```
//!
//! - `bookFile` / `originalTitle`: captured on first save (the file name and
//!   the title the program first saw, before any user edit).
//! - `title` / `subtitle` / `volume`: structured fields edited in the panel.
//! - `displayTitle`: the title the user confirmed for display. Empty means
//!   "derive it"; when non-empty it wins over everything else (never silently
//!   overwritten by auto-generation).
//!
//! Symbols: auto-generated separators are ASCII only — `" _ "` joins
//! title/subtitle/volume. Full-width characters inside the original
//! `dc:title` are preserved as-is (they belong to the book title).

use std::fs;
use std::io;
use std::path::Path;

/// Marker that opens the metadata comment block in the md file.
pub const META_OPEN: &str = "<!-- icedreader-meta";
/// Joins title / subtitle / volume into the derived display title.
pub const TITLE_JOIN_SEP: &str = " _ ";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BookMeta {
    /// File name of the epub this md belongs to (captured when the md is first written).
    pub book_file: Option<String>,
    /// Title the program first saw for this book, before any user edit.
    pub original_title: Option<String>,
    /// 主书名 (main title).
    pub title: String,
    /// 副标题 (subtitle).
    pub subtitle: String,
    /// 卷册 (volume).
    pub volume: String,
    /// User-confirmed display title; empty = derive from the fields.
    pub display_title: String,
}

impl BookMeta {
    pub fn is_empty(&self) -> bool {
        self.title.trim().is_empty()
            && self.subtitle.trim().is_empty()
            && self.volume.trim().is_empty()
            && self.display_title.trim().is_empty()
    }
}

/// Collapse runs of whitespace (incl. U+3000 full-width space and NBSP) into
/// single ASCII spaces and trim the ends. Only touch generated/edited fields —
/// never rewrite the original `dc:title` with this.
pub fn clean_title(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_space = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            if !out.is_empty() {
                pending_space = true;
            }
        } else {
            if pending_space && !out.is_empty() {
                out.push(' ');
            }
            pending_space = false;
            out.push(ch);
        }
    }
    out
}

/// Join non-empty fields with `" _ "` (ASCII only, never full-width).
pub fn join_title(title: &str, subtitle: &str, volume: &str) -> String {
    let mut parts = Vec::with_capacity(3);
    for p in [title, subtitle, volume] {
        let p = p.trim();
        if !p.is_empty() {
            parts.push(p);
        }
    }
    parts.join(TITLE_JOIN_SEP)
}

/// Display-title resolution chain (single source of truth, mirrored in
/// AGENTS.md): user-confirmed `displayTitle` → derived join of the edited
/// fields → whatever the book previously resolved to (`dc:title` or the file
/// name fallback, passed in as `base`).
pub fn resolved_title(overlay: Option<&BookMeta>, base: &str) -> String {
    match overlay {
        Some(m) if !m.display_title.trim().is_empty() => m.display_title.trim().to_string(),
        Some(m) => {
            let joined = join_title(&m.title, &m.subtitle, &m.volume);
            if joined.is_empty() {
                base.to_string()
            } else {
                joined
            }
        }
        None => base.to_string(),
    }
}

/// Parse the metadata block out of an md file's text. Returns `None` when the
/// marker is missing or malformed enough to lack an end — the caller treats
/// that as "no companion metadata". Unknown keys and broken lines are skipped,
/// so a hand-edited file degrades gracefully instead of failing.
pub fn parse_meta(text: &str) -> Option<BookMeta> {
    let start = text.find(META_OPEN)?;
    let after = &text[start + META_OPEN.len()..];
    let end = after.find("-->")?;
    let body = &after[..end];
    let mut meta = BookMeta::default();
    for line in body.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim().to_string();
        match key.trim() {
            "bookFile" => meta.book_file = Some(value),
            "originalTitle" => meta.original_title = Some(value),
            "title" => meta.title = value,
            "subtitle" => meta.subtitle = value,
            "volume" => meta.volume = value,
            "displayTitle" => meta.display_title = value,
            _ => {}
        }
    }
    Some(meta)
}

/// Read and parse one companion md file. `None` on any read/parse failure.
pub fn read_meta_file(path: &Path) -> Option<BookMeta> {
    let text = fs::read_to_string(path).ok()?;
    parse_meta(&text)
}

fn write_field(out: &mut String, key: &str, value: &str) {
    out.push_str(key);
    out.push_str(": ");
    out.push_str(value);
    out.push('\n');
}

/// Serialize a [`BookMeta`] to the companion md text.
pub fn format_meta(meta: &BookMeta) -> String {
    let mut out = String::from("<!-- icedreader-meta\n");
    if let Some(book_file) = &meta.book_file {
        write_field(&mut out, "bookFile", book_file);
    }
    if let Some(original_title) = &meta.original_title {
        write_field(&mut out, "originalTitle", original_title);
    }
    write_field(&mut out, "title", &meta.title);
    write_field(&mut out, "subtitle", &meta.subtitle);
    write_field(&mut out, "volume", &meta.volume);
    write_field(&mut out, "displayTitle", &meta.display_title);
    out.push_str("-->\n");
    out
}

/// Atomically write the companion md (tmp + rename), creating parents if needed.
pub fn write_meta_file(path: &Path, meta: &BookMeta) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("md.tmp");
    fs::write(&tmp, format_meta(meta))?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_collapses_fullwidth_and_runs() {
        assert_eq!(clean_title("  三体\u{3000}\u{3000}黑暗森林  "), "三体 黑暗森林");
        assert_eq!(clean_title("A\u{00a0}B"), "A B");
        assert_eq!(clean_title("  spaced   out  "), "spaced out");
        assert_eq!(clean_title("   "), "");
        // CJK ideographic space is already whitespace; NBSP handled too.
        assert_eq!(clean_title("\u{3000}书名\u{3000}"), "书名");
    }

    #[test]
    fn join_skips_empty_fields_and_uses_ascii_sep() {
        assert_eq!(join_title("三体", "", ""), "三体");
        assert_eq!(join_title("三体", "黑暗森林", ""), "三体 _ 黑暗森林");
        assert_eq!(join_title("三体", "", "第二部"), "三体 _ 第二部");
        assert_eq!(join_title("三体", "黑暗森林", "第二部"), "三体 _ 黑暗森林 _ 第二部");
        assert_eq!(join_title("", "", ""), "");
        assert_eq!(join_title(" The Lord of the Rings ", " The Two Towers ", ""), "The Lord of the Rings _ The Two Towers");
    }

    #[test]
    fn resolution_chain_prefers_user_confirmed_title() {
        let base = "dc:title 原样";
        // No overlay → untouched base.
        assert_eq!(resolved_title(None, base), base);

        // Overlay with a hand-confirmed display title wins.
        let hand = BookMeta {
            display_title: "手改显示名".into(),
            title: "字段主书名".into(),
            ..Default::default()
        };
        assert_eq!(resolved_title(Some(&hand), base), "手改显示名");

        // Edited fields derive a title when displayTitle is empty.
        let fields = BookMeta {
            title: "字段主书名".into(),
            subtitle: "副".into(),
            ..Default::default()
        };
        assert_eq!(resolved_title(Some(&fields), base), "字段主书名 _ 副");

        // Completely empty overlay falls through to base.
        assert_eq!(resolved_title(Some(&BookMeta::default()), base), base);
    }

    #[test]
    fn parse_roundtrip_and_tolerance() {
        let meta = BookMeta {
            book_file: Some("三体.epub".into()),
            original_title: Some("三体".into()),
            title: "三体".into(),
            subtitle: String::new(),
            volume: "死神永生".into(),
            display_title: "三体 _ 死神永生".into(),
        };
        let text = format_meta(&meta);
        let back = parse_meta(&text).expect("parse own output");
        assert_eq!(back, meta);

        // Values may contain colons; the first one separates key from value.
        let with_colon = parse_meta("<!-- icedreader-meta\ntitle: 书名：副标题\n-->\n").unwrap();
        assert_eq!(with_colon.title, "书名：副标题");

        // Missing marker / unterminated block → None.
        assert!(parse_meta("# just markdown").is_none());
        assert!(parse_meta("<!-- icedreader-meta\ntitle: 没闭合").is_none());

        // Broken lines are skipped, known keys still parse.
        let messy = parse_meta(
            "前言\n<!-- icedreader-meta\ngarbage line without colon\nsubtitle: 能读到\ntitle\n: 坏行\nvolume: 第二卷\n-->\n后记",
        )
        .unwrap();
        assert_eq!(messy.subtitle, "能读到");
        assert_eq!(messy.volume, "第二卷");
        assert_eq!(messy.title, "");

        // Unknown keys ignored (forward compatible).
        let extra = parse_meta("<!-- icedreader-meta\nfutureKey: x\ntitle: 书\n-->\n").unwrap();
        assert_eq!(extra.title, "书");
    }

    #[test]
    fn read_write_roundtrip_on_disk() {
        let dir = std::env::temp_dir().join("icedreader-book-meta-test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("三体.md");
        let meta = BookMeta {
            book_file: Some("三体.epub".into()),
            original_title: Some("三体".into()),
            title: "三体".into(),
            subtitle: "黑暗森林".into(),
            volume: String::new(),
            display_title: String::new(),
        };
        write_meta_file(&path, &meta).unwrap();
        assert_eq!(read_meta_file(&path), Some(meta.clone()));

        // Missing file → None.
        assert_eq!(read_meta_file(&dir.join("nope.md")), None);
    }
}
