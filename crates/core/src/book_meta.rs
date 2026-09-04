//! User-editable per-book metadata, persisted as a companion Markdown file
//! next to the epub in `data/library/` (`<stem>.md`, e.g. `三体.epub` ↔
//! `三体.md`).
//!
//! The md file is **maintained by the program, not hand-edited by the user**
//! (the UI panel is the editing surface). Format:
//!
//! ```markdown
//! <!-- icedreader-meta
//! bookFile: 140亿年宇宙演化全史.epub
//! originalTitle: 140亿年宇宙演化全史…
//! title: 140亿年宇宙演化全史
//! subtitle:
//! volume:
//! author: [美] 尼尔·德格拉斯·泰森、[美] 唐纳德·戈德史密斯
//! translator: 阳曦
//! year: 2019
//! publisher: 北京联合出版公司
//! isbn: 9787559632487
//! displayTitle:
//! -->
//! ```
//!
//! - `bookFile` / `originalTitle`: captured on first save (the file name and
//!   the title the program first saw, before any user edit).
//! - `title` / `subtitle` / `volume` / `author` / `translator` / `year` /
//!   `publisher` / `isbn`: structured fields edited in the panel (md v2).
//! - `displayTitle`: the title the user confirmed for display. Empty means
//!   "derive it"; when non-empty it wins over everything else (never silently
//!   overwritten by auto-generation).
//!
//! Display-title join template (AGENTS): `书名 [ _ 副标题] [ - 卷册]
//! [ - 作者] [ - 译者] [ - 出版年份] [ - 出版社] [ - ISBN…]`. Auto-generated
//! separators are ASCII only — `" _ "` appears **only** between 书名 and
//! 副标题; every later segment (卷册 and the bibliographic data) is joined
//! with `" - "`. Empty segments are skipped (never an empty segment between
//! two separators); 书名 is required — without it nothing is derived.
//! Full-width characters inside the original `dc:title` are preserved as-is
//! (they belong to the book title).

use std::fs;
use std::io;
use std::path::Path;

/// Marker that opens the metadata comment block in the md file.
pub const META_OPEN: &str = "<!-- icedreader-meta";
/// Separates 书名 from 副标题 in the derived display title (the **only**
/// place `_` is used; see the join template in the module doc).
pub const TITLE_JOIN_SEP: &str = " _ ";

/// 卷册 and the bibliographic fields (`作者 - 译者 - 出版年份 - 出版社 -
/// ISBN`). User-confirmed 2026-09-04.
pub const FIELD_SEP: &str = " - ";

/// ASCII label prepended when a non-empty ISBN value does not already start
/// with `ISBN` (so the join reads `… - ISBN 978-7-…`, never `- ISBN-…`).
pub const ISBN_LABEL: &str = "ISBN ";

/// Label prepended when a non-empty translator value does not already start
/// with 译者, so the join reads `… - 译者 阳曦` (ASCII space, never a
/// full-width colon).
pub const TRANSLATOR_LABEL: &str = "译者 ";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BookMeta {
    /// File name of the epub this md belongs to (captured when the md is first written).
    pub book_file: Option<String>,
    /// Title the program first saw for this book, before any user edit.
    pub original_title: Option<String>,
    /// 主书名 (main title; required for the join).
    pub title: String,
    /// 副标题 (subtitle).
    pub subtitle: String,
    /// 卷册 (volume).
    pub volume: String,
    /// 作者 (author, single line; multiple names joined with 、 by the panel).
    pub author: String,
    /// 译者 (translator, single line).
    pub translator: String,
    /// 出版年份 (year of publication).
    pub year: String,
    /// 出版社 (publisher).
    pub publisher: String,
    /// ISBN（号码本身；拼接时自动补 ASCII 前缀，见 [`ISBN_LABEL`]）。
    pub isbn: String,
    /// User-confirmed display title; empty = derive from the fields.
    pub display_title: String,
}

impl BookMeta {
    pub fn is_empty(&self) -> bool {
        self.title.trim().is_empty()
            && self.subtitle.trim().is_empty()
            && self.volume.trim().is_empty()
            && self.author.trim().is_empty()
            && self.translator.trim().is_empty()
            && self.year.trim().is_empty()
            && self.publisher.trim().is_empty()
            && self.isbn.trim().is_empty()
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

/// Render the display-title per the join template (module doc):
/// `书名 [ _ 副标题] [ - 卷册] [ - 作者] [ - 译者] [ - 出版年份]
/// [ - 出版社] [ - ISBN…]`. Empty segments are skipped entirely — no empty
/// segment between two separators ever appears. 书名 is required: with no
/// title the function returns `""` and the resolution chain falls back to
/// the base title. An ISBN value not already starting with `ISBN` gets the
/// ASCII [`ISBN_LABEL`] prefix; a translator value not already starting with
/// 译者 gets [`TRANSLATOR_LABEL`] — both so the segments read e.g.
/// `… - 译者 阳曦 - … - ISBN 978-7-…`.
pub fn join_title(
    title: &str,
    subtitle: &str,
    volume: &str,
    author: &str,
    translator: &str,
    year: &str,
    publisher: &str,
    isbn: &str,
) -> String {
    let title = title.trim();
    if title.is_empty() {
        return String::new();
    }
    // The one `_` slot: between 书名 and 副标题 only.
    let mut head = title.to_string();
    let subtitle = subtitle.trim();
    if !subtitle.is_empty() {
        head.push_str(TITLE_JOIN_SEP);
        head.push_str(subtitle);
    }
    // Everything after 副标题 joins with ` - `.
    let mut parts = Vec::with_capacity(7);
    parts.push(head);
    for p in [volume, author] {
        let p = p.trim();
        if !p.is_empty() {
            parts.push(p.to_string());
        }
    }
    let translator = translator.trim();
    if !translator.is_empty() {
        parts.push(with_label(translator, TRANSLATOR_LABEL));
    }
    for p in [year, publisher] {
        let p = p.trim();
        if !p.is_empty() {
            parts.push(p.to_string());
        }
    }
    let isbn = isbn.trim();
    if !isbn.is_empty() {
        parts.push(with_label(isbn, ISBN_LABEL));
    }
    parts.join(FIELD_SEP)
}

/// Prepend `label` (trimmed for the prefix check) unless `value` already
/// starts with it, case-insensitively. ASCII labels keep the join ASCII-only.
fn with_label(value: &str, label: &str) -> String {
    let prefix = label.trim();
    match value.get(..prefix.len()) {
        Some(head) if head.eq_ignore_ascii_case(prefix) => value.to_string(),
        _ => format!("{label}{value}"),
    }
}

/// Display-title resolution chain (single source of truth, mirrored in
/// AGENTS.md): user-confirmed `displayTitle` → derived join of the edited
/// fields → whatever the book previously resolved to (`dc:title` or the file
/// name fallback, passed in as `base`).
pub fn resolved_title(overlay: Option<&BookMeta>, base: &str) -> String {
    match overlay {
        Some(m) if !m.display_title.trim().is_empty() => m.display_title.trim().to_string(),
        Some(m) => {
            let joined = join_title(
                &m.title,
                &m.subtitle,
                &m.volume,
                &m.author,
                &m.translator,
                &m.year,
                &m.publisher,
                &m.isbn,
            );
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
            "author" => meta.author = value,
            "translator" => meta.translator = value,
            "year" => meta.year = value,
            "publisher" => meta.publisher = value,
            "isbn" => meta.isbn = value,
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
    write_field(&mut out, "author", &meta.author);
    write_field(&mut out, "translator", &meta.translator);
    write_field(&mut out, "year", &meta.year);
    write_field(&mut out, "publisher", &meta.publisher);
    write_field(&mut out, "isbn", &meta.isbn);
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
    fn join_uses_underscore_only_between_title_and_subtitle() {
        // 书名 required: with no title nothing is derived.
        assert_eq!(join_title("", "黑暗森林", "", "", "", "", "", ""), "");
        assert_eq!(join_title("   ", "", "", "", "", "", "", ""), "");

        // Just a title.
        assert_eq!(join_title("三体", "", "", "", "", "", "", ""), "三体");

        // The single ` _ ` slot: 书名 ↔ 副标题.
        assert_eq!(join_title("三体", "黑暗森林", "", "", "", "", "", ""), "三体 _ 黑暗森林");

        // 卷册 and later bibliographic fields join with ` - ` (no ` _ ` there).
        assert_eq!(join_title("三体", "", "第二部", "", "", "", "", ""), "三体 - 第二部");
        assert_eq!(
            join_title("三体", "黑暗森林", "第二部", "", "", "", "", ""),
            "三体 _ 黑暗森林 - 第二部"
        );

        // 译者 sits between 作者 and 出版年份, and gets an ASCII label.
        assert_eq!(
            join_title("三体", "", "", "刘慈欣", "阳曦", "2008", "", ""),
            "三体 - 刘慈欣 - 译者 阳曦 - 2008"
        );
        // A value already labelled 译者 is kept as-is.
        assert_eq!(
            join_title("三体", "", "", "", "译者: 阳曦", "", "", ""),
            "三体 - 译者: 阳曦"
        );
        // No author, only translator → no doubled separator.
        assert_eq!(
            join_title("三体", "", "", "", "阳曦", "", "", ""),
            "三体 - 译者 阳曦"
        );

        // Full template with a missing publisher in the middle: no empty
        // segment, no doubled separator.
        assert_eq!(
            join_title("三体", "黑暗森林", "第二部", "刘慈欣", "阳曦", "2008", "", "978-7-5366-9293-0"),
            "三体 _ 黑暗森林 - 第二部 - 刘慈欣 - 译者 阳曦 - 2008 - ISBN 978-7-5366-9293-0"
        );

        // A value that already begins with ISBN (any case) is kept as-is.
        assert_eq!(join_title("三体", "", "", "", "", "", "", "isbn 978-7-1"), "三体 - isbn 978-7-1");
        assert_eq!(
            join_title("三体", "", "", "", "", "", "", "ISBN-13 978-7-1"),
            "三体 - ISBN-13 978-7-1"
        );

        // Multi-author and full-width author input pass through untouched
        // (the label/symbols the program emits stay ASCII).
        assert_eq!(
            join_title("三体", "", "", "刘慈欣、王晋康", "", "二〇〇八", "", ""),
            "三体 - 刘慈欣、王晋康 - 二〇〇八"
        );
        assert_eq!(
            join_title(" The Lord of the Rings ", " The Two Towers ", "", "", "", "", "", ""),
            "The Lord of the Rings _ The Two Towers"
        );
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

        // Bibliographic fields participate in the derived title.
        let full = BookMeta {
            title: "三体".into(),
            subtitle: "黑暗森林".into(),
            author: "刘慈欣".into(),
            year: "2008".into(),
            ..Default::default()
        };
        assert_eq!(resolved_title(Some(&full), base), "三体 _ 黑暗森林 - 刘慈欣 - 2008");

        // Title empty (everything else set) → falls through to base.
        let no_title = BookMeta {
            volume: "第二部".into(),
            author: "刘慈欣".into(),
            ..Default::default()
        };
        assert_eq!(resolved_title(Some(&no_title), base), base);

        // Completely empty overlay falls through to base.
        assert_eq!(resolved_title(Some(&BookMeta::default()), base), base);
    }

    #[test]
    fn parse_roundtrip_and_tolerance() {
        let meta = BookMeta {
            book_file: Some("三体.epub".into()),
            original_title: Some("三体".into()),
            title: "三体".into(),
            subtitle: "黑暗森林".into(),
            volume: "第二部".into(),
            author: "刘慈欣".into(),
            translator: "阳曦".into(),
            year: "2008".into(),
            publisher: "重庆出版社".into(),
            isbn: "978-7-5366-9293-0".into(),
            display_title: String::new(),
        };
        let text = format_meta(&meta);
        let back = parse_meta(&text).expect("parse own output");
        assert_eq!(back, meta);
        // The v2 keys are on disk.
        for key in ["translator", "author", "year", "publisher", "isbn", "displayTitle"] {
            assert!(text.contains(&format!("{key}: ")), "missing {key} in {text}");
        }

        // A v1 md (no v2 keys) parses with empty v2 fields — no data loss.
        let v1 = parse_meta(
            "<!-- icedreader-meta\ntitle: 三体\nvolume: 第二部\ndisplayTitle:\n-->",
        )
        .unwrap();
        assert_eq!(v1.title, "三体");
        assert_eq!(v1.volume, "第二部");
        assert_eq!(v1.author, "");
        assert_eq!(v1.translator, "");
        assert_eq!(v1.publisher, "");
        assert_eq!(v1.isbn, "");

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
            author: "刘慈欣".into(),
            translator: "阳曦".into(),
            year: "2008".into(),
            publisher: "重庆出版社".into(),
            isbn: "978-7-5366-9293-0".into(),
            display_title: String::new(),
        };
        write_meta_file(&path, &meta).unwrap();
        assert_eq!(read_meta_file(&path), Some(meta.clone()));

        // Missing file → None.
        assert_eq!(read_meta_file(&dir.join("nope.md")), None);
    }
}
