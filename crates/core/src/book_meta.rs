//! User-editable per-book metadata, persisted as a companion Markdown file
//! next to the book in `data/library/` (full file name + `.md`, e.g.
//! `三体.epub` ↔ `三体.epub.md`, `三体.pdf` ↔ `三体.pdf.md`).
//!
//! That one file carries **everything the reader keeps beside a book**: the
//! metadata block below and the highlight archive (see `src-tauri/src/notes.rs`).
//! The file is a real Markdown document the user may open and edit; the program
//! only owns the marked regions and rewrites them in place — everything else is
//! preserved verbatim (`split_meta` / `join_meta`).
//!
//! Format:
//!
//! ```markdown
//! <!-- icedreader-meta
//! originalBookFile: 140亿年宇宙演化全史.epub
//! md5: 2b0e1f…（32 hex）
//! title: 140亿年宇宙演化全史
//! subtitle:
//! volume:
//! author: [美] 尼尔·德格拉斯·泰森, [美] 唐纳德·戈德史密斯
//! translator: 阳曦
//! year: 2019
//! publisher: 北京联合出版公司
//! isbn: 9787559632487
//! -->
//!
//! ## 第 1 章 · 开头
//!
//! <!-- icedreader-note
//! id: …
//! -->
//! > 【重点】摘录一
//! > （全书 34% · 划于 …）
//!
//! 用户的自由笔记。
//! ```
//!
//! # 每个键的定义（一义一键，不许两名一义）
//!
//! | 键 | 唯一含义 | 谁写·何时 | 读者 | 空值 |
//! | --- | --- | --- | --- | --- |
//! | `originalBookFile` | 这本书**首次进入书架时**的库内文件名；此后永不改（书改名了它也记着原来的名字） | 首次落档：`src-tauri` 的 `notes::stamp_import_identity`（导入 / 首次打开 / 首次保存 / 书架首列，谁先到谁写） | 无（留痕） | 字段缺失 = 还没落过档 |
//! | `md5` | **首次落档时那个书文件字节的 MD5**（32 位小写 hex）。本阅读器从不重写书文件，所以它 = 「这条 md 认的是哪个文件」 | 同 `originalBookFile`，同一次写入 | 无（留痕；将来用于「文件被换过」的校验） | 缺失 = 还没落过档 |
//! | `title` | 主书名。**拼接的第一段，也是唯一必填字段**（面板留空禁保存；`join_title` 遇空返回 `""`，名字回退到原书名/文件名） | 只有 `set_book_meta`（编辑元数据面板） | `join_title` → 显示名 → 库内文件名；面板回显 | 空 = 不拼入（**不是**「未知」） |
//! | `subtitle` | 副标题。**唯一使用 `" _ "` 的那一段** | 同上 | 同上 | 空 = 不拼入 |
//! | `volume` | 卷册（「第二部」这类） | 同上 | 同上 | 空 = 不拼入 |
//! | `author` | 作者；多人用 ASCII `, ` 连接（顿号 U+3001 折成 `, `） | 同上 | 同上 | 空 = 不拼入 |
//! | `translator` | 译者；多人同 `, `；拼接时自动补结尾「译」 | 同上 | 同上 | 空 = 不拼入 |
//! | `year` | 出版年份，纯文本不校验 | 同上 | 同上 | 空 = 不拼入 |
//! | `publisher` | 出版社 | 同上 | 同上 | 空 = 不拼入 |
//! | `isbn` | ISBN 号码本体；拼接时自动补 ASCII `ISBN ` 前缀，不校验位数 | 同上 | 同上 | 空 = 不拼入 |
//!
//! 绑定关系（字段不参与）：**书 ↔ md 的唯一绑定是文件名**（`X.epub` ↔
//! `X.epub.md`）。显示名 = 库内文件名 = 字段拼接结果，三者恒等。
//!
//! 没有 `originalTitle`（2026-09-30 删）：`originalBookFile` 记着「它来时叫什么」，
//! 而原书当前书名随时可由面板的「重新读取原书元数据」从文件里取回，冻结一份
//! 反而会与文件不一致。也没有手填显示名：名字永远是字段拼接。
//!
//! 块是**按结构体重写**的（`format_meta` 固定键序）：认不出的键读时忽略，但旧版本
//! 一旦再保存就会把它丢掉 —— 加字段时要记住。
//!
//! 拼接模板：`书名 [ _ 副标题] [ - 卷册] [ - 作者] [ - 译者 译] [ - 出版年份]
//! [ - 出版社] [ - ISBN…]`。程序生成的符号一律 ASCII —— `" _ "` **只**出现在书名
//! 与副标题之间，其后各段一律 `" - "`；空段整体跳过（不会出现两分隔符夹空段）；
//! 书名必填。原书 `dc:title` 自带的全角字符原样保留（它们是书名的一部分）。


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

/// Suffix appended when a non-empty translator value does not already end with
/// 译, so the join reads `… - 阳曦 译` (ASCII space, never a full-width colon).
/// The value keeps whatever the user typed; only the closing 译 is added.
pub const TRANSLATOR_SUFFIX: &str = " 译";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BookMeta {
    /// 这本书**首次进入书架时**的库内文件名（留痕，永不更新）。只由
    /// `src-tauri` 的落档路径写；`set_book_meta` 原样透传。
    pub original_book_file: Option<String>,
    /// 首次落档时那个书文件字节的 MD5（32 位小写十六进制；留痕，永不更新）。
    pub md5: Option<String>,
    /// 主书名 (main title; required for the join).
    pub title: String,
    /// 副标题 (subtitle).
    pub subtitle: String,
    /// 卷册 (volume).
    pub volume: String,
    /// 作者 (author, single line; multiple names joined with `, `).
    pub author: String,
    /// 译者 (translator, single line).
    pub translator: String,
    /// 出版年份 (year of publication).
    pub year: String,
    /// 出版社 (publisher).
    pub publisher: String,
    /// ISBN（号码本身；拼接时自动补 ASCII 前缀，见 [`ISBN_LABEL`]）。
    pub isbn: String,
}

impl BookMeta {
    /// 面板那 8 个字段是否全空（`originalBookFile` / `md5` 是程序留痕，不计）。
    pub fn is_empty(&self) -> bool {
        self.title.trim().is_empty()
            && self.subtitle.trim().is_empty()
            && self.volume.trim().is_empty()
            && self.author.trim().is_empty()
            && self.translator.trim().is_empty()
            && self.year.trim().is_empty()
            && self.publisher.trim().is_empty()
            && self.isbn.trim().is_empty()
    }
}

/// Normalize a person-list field (author/translator): fold 、 (U+3001) to
/// ASCII `, ` then [`clean_title`]. The rendered book title must not carry
/// Chinese punctuation the program produced, and these fields feed the title.
pub fn clean_person_list(s: &str) -> String {
    clean_title(&s.replace('\u{3001}', ", "))
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
/// `书名 [ _ 副标题] [ - 卷册] [ - 作者] [ - 译者 译] [ - 出版年份]
/// [ - 出版社] [ - ISBN…]`. Empty segments are skipped entirely — no empty
/// segment between two separators ever appears. 书名 is required: with no
/// title the function returns `""` and the resolution chain falls back to
/// the base title. An ISBN value not already starting with `ISBN` gets the
/// ASCII [`ISBN_LABEL`] prefix; a translator value not already ending with
/// 译 gets [`TRANSLATOR_SUFFIX`] — so the segments read e.g.
/// `… - 阳曦 译 - … - ISBN 978-7-…`.
// Eight metadata fields, each optional and independent; bundling them into a
// struct would only move the same positional arguments to every call site.
#[allow(clippy::too_many_arguments)]
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
    let volume = clean_title(volume);
    if !volume.is_empty() {
        parts.push(volume);
    }
    let author = clean_person_list(author);
    if !author.is_empty() {
        parts.push(author);
    }
    let translator = clean_person_list(translator);
    if !translator.is_empty() {
        parts.push(with_suffix(&translator, TRANSLATOR_SUFFIX));
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

/// Append `suffix` (trimmed for the check) unless `value` already ends with
/// it, case-insensitively. Used for the translator: the name keeps whatever the
/// user typed and only the closing ` 译` is added (`阳曦` → `阳曦 译`).
fn with_suffix(value: &str, suffix: &str) -> String {
    let tail = suffix.trim();
    if value.to_lowercase().ends_with(&tail.to_lowercase()) {
        value.to_string()
    } else {
        format!("{value}{suffix}")
    }
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

/// Display-title resolution chain (single source of truth):
/// derived join of the edited fields → whatever the book previously resolved
/// to (`dc:title` or the file name fallback, passed in as `base`).
///
/// There is deliberately no user-confirmed override: the shelf name, the
/// reader chrome and the file on disk must always be the same string, so the
/// fields are the only way to change it.
pub fn resolved_title(overlay: Option<&BookMeta>, base: &str) -> String {
    match overlay {
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
            "originalBookFile" => meta.original_book_file = Some(value),
            // 旧 md 的 `bookFile` 是它能给的**最早**名字（首次保存时），当作
            // `originalBookFile` 读入；读不到真正的导入名也没必要编一个。
            "bookFile" => {
                if meta.original_book_file.is_none() {
                    meta.original_book_file = Some(value);
                }
            }
            "md5" => meta.md5 = Some(value),
            // `originalTitle` 已删（2026-09-30），旧值直接忽略。
            "title" => meta.title = value,
            "subtitle" => meta.subtitle = value,
            "volume" => meta.volume = value,
            "author" => meta.author = value,
            "translator" => meta.translator = value,
            "year" => meta.year = value,
            "publisher" => meta.publisher = value,
            "isbn" => meta.isbn = value,
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

/// Serialize a [`BookMeta`] into its md comment block (no trailing blank line;
/// use [`join_meta`] to place it above the file body). Key order is fixed; the
/// block is rewritten whole, so keys added by a newer build are dropped by an
/// older one on its next save.
pub fn format_meta(meta: &BookMeta) -> String {
    let mut out = String::from("<!-- icedreader-meta\n");
    if let Some(original_book_file) = &meta.original_book_file {
        write_field(&mut out, "originalBookFile", original_book_file);
    }
    if let Some(md5) = &meta.md5 {
        write_field(&mut out, "md5", md5);
    }
    write_field(&mut out, "title", &meta.title);
    write_field(&mut out, "subtitle", &meta.subtitle);
    write_field(&mut out, "volume", &meta.volume);
    write_field(&mut out, "author", &meta.author);
    write_field(&mut out, "translator", &meta.translator);
    write_field(&mut out, "year", &meta.year);
    write_field(&mut out, "publisher", &meta.publisher);
    write_field(&mut out, "isbn", &meta.isbn);
    out.push_str("-->\n");
    out
}

/// Split a companion md into its metadata block and everything else. The block
/// is `<!-- icedreader-meta … -->` plus the blank run that follows it; `body` is
/// the rest of the file **verbatim** — that is where the highlight archive and
/// the user's own prose live. A file without the marker yields `("", text)`.
///
/// Every writer (metadata save, highlight upsert) keeps the other half byte for
/// byte; only the marked region is rewritten.
pub fn split_meta(text: &str) -> (String, String) {
    let Some(start) = text.find(META_OPEN) else {
        return (String::new(), text.to_string());
    };
    let after = &text[start + META_OPEN.len()..];
    let Some(end) = after.find("-->") else {
        // Unterminated block: treat the whole file as user content rather than
        // truncating it (the reader parses no metadata in this case either).
        return (String::new(), text.to_string());
    };
    let block_end = start + META_OPEN.len() + end + "-->".len();
    let mut body_start = block_end;
    // Swallow the newline that ends the `-->` line, then any blank lines, so a
    // rewritten block does not accumulate whitespace on every save.
    let bytes = text.as_bytes();
    while body_start < bytes.len() && (bytes[body_start] == b'\n' || bytes[body_start] == b'\r') {
        body_start += 1;
    }
    while body_start < bytes.len()
        && (bytes[body_start] == b'\n' || bytes[body_start] == b'\r' || bytes[body_start] == b' ')
    {
        body_start += 1;
    }
    let mut block = text[..block_end].to_string();
    if !block.ends_with('\n') {
        block.push('\n');
    }
    (block, text[body_start..].to_string())
}

/// Put a metadata block back on top of a file body. An empty `block` leaves the
/// body untouched (a book with no edited metadata keeps a pure highlight file).
pub fn join_meta(block: &str, body: &str) -> String {
    if block.trim().is_empty() {
        return body.to_string();
    }
    let mut out = block.to_string();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    if !body.trim().is_empty() {
        out.push('\n');
        out.push_str(body);
    }
    out
}

/// Atomically write the companion md (tmp + rename), creating parents if needed.
/// The file body (highlight archive + user prose) is preserved verbatim; only
/// the metadata block is replaced.
pub fn write_meta_file(path: &Path, meta: &BookMeta) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let existing = fs::read_to_string(path).unwrap_or_default();
    let (_, body) = split_meta(&existing);
    let tmp = path.with_extension("md.tmp");
    fs::write(&tmp, join_meta(&format_meta(meta), &body))?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_collapses_fullwidth_and_runs() {
        assert_eq!(
            clean_title("  三体\u{3000}\u{3000}黑暗森林  "),
            "三体 黑暗森林"
        );
        assert_eq!(clean_title("A\u{00a0}B"), "A B");
        assert_eq!(clean_title("  spaced   out  "), "spaced out");
        assert_eq!(clean_title("   "), "");
        // CJK ideographic space is already whitespace; NBSP handled too.
        assert_eq!(clean_title("\u{3000}书名\u{3000}"), "书名");
    }

    #[test]
    fn clean_person_list_folds_dunhao_to_ascii() {
        assert_eq!(clean_person_list("刘慈欣、王晋康"), "刘慈欣, 王晋康");
        assert_eq!(clean_person_list("A、B"), "A, B");
        assert_eq!(clean_person_list("单人"), "单人");
        assert_eq!(clean_person_list(""), "");
        // Full-width comma is left alone; only 、 is a list separator here.
        assert_eq!(clean_person_list("a，b、c"), "a，b, c");
    }

    #[test]
    fn join_uses_underscore_only_between_title_and_subtitle() {
        // 书名 required: with no title nothing is derived.
        assert_eq!(join_title("", "黑暗森林", "", "", "", "", "", ""), "");
        assert_eq!(join_title("   ", "", "", "", "", "", "", ""), "");

        // Just a title.
        assert_eq!(join_title("三体", "", "", "", "", "", "", ""), "三体");

        // The single ` _ ` slot: 书名 ↔ 副标题.
        assert_eq!(
            join_title("三体", "黑暗森林", "", "", "", "", "", ""),
            "三体 _ 黑暗森林"
        );

        // 卷册 and later bibliographic fields join with ` - ` (no ` _ ` there).
        assert_eq!(
            join_title("三体", "", "第二部", "", "", "", "", ""),
            "三体 - 第二部"
        );
        assert_eq!(
            join_title("三体", "黑暗森林", "第二部", "", "", "", "", ""),
            "三体 _ 黑暗森林 - 第二部"
        );

        // 译者 sits between 作者 and 出版年份, and gets a closing 译.
        assert_eq!(
            join_title("三体", "", "", "刘慈欣", "阳曦", "2008", "", ""),
            "三体 - 刘慈欣 - 阳曦 译 - 2008"
        );
        // A value that already ends with 译 is kept as-is (no doubled 译).
        assert_eq!(
            join_title("三体", "", "", "", "译者: 阳曦", "", "", ""),
            "三体 - 译者: 阳曦 译"
        );
        // Program-added 译 precedes anything the user wrote themselves, rather
        // than trying to punctuate a list of names.
        assert_eq!(
            join_title("三体", "", "", "", "阳曦、李明", "", "", ""),
            "三体 - 阳曦, 李明 译"
        );
        // No author, only translator → no doubled separator.
        assert_eq!(
            join_title("三体", "", "", "", "阳曦", "", "", ""),
            "三体 - 阳曦 译"
        );

        // Full template with a missing publisher in the middle: no empty
        // segment, no doubled separator.
        assert_eq!(
            join_title(
                "三体",
                "黑暗森林",
                "第二部",
                "刘慈欣",
                "阳曦",
                "2008",
                "",
                "978-7-5366-9293-0"
            ),
            "三体 _ 黑暗森林 - 第二部 - 刘慈欣 - 阳曦 译 - 2008 - ISBN 978-7-5366-9293-0"
        );

        // A value that already begins with ISBN (any case) is kept as-is.
        assert_eq!(
            join_title("三体", "", "", "", "", "", "", "isbn 978-7-1"),
            "三体 - isbn 978-7-1"
        );
        assert_eq!(
            join_title("三体", "", "", "", "", "", "", "ISBN-13 978-7-1"),
            "三体 - ISBN-13 978-7-1"
        );

        // Multi-author input: 、 folds to ASCII `, ` so the rendered title
        // carries no Chinese punctuation the program produced; full-width
        // author content otherwise passes through.
        assert_eq!(
            join_title("三体", "", "", "刘慈欣、王晋康", "", "二〇〇八", "", ""),
            "三体 - 刘慈欣, 王晋康 - 二〇〇八"
        );
        assert_eq!(
            join_title("三体", "", "", "译者", "阳曦", "", "", ""),
            "三体 - 译者 - 阳曦 译"
        );
        assert_eq!(
            join_title(
                " The Lord of the Rings ",
                " The Two Towers ",
                "",
                "",
                "",
                "",
                "",
                ""
            ),
            "The Lord of the Rings _ The Two Towers"
        );
    }

    #[test]
    fn resolution_chain_derives_from_fields() {
        let base = "dc:title 原样";
        // No overlay → untouched base.
        assert_eq!(resolved_title(None, base), base);

        // Edited fields derive the title; there is no hand-written override.
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
        assert_eq!(
            resolved_title(Some(&full), base),
            "三体 _ 黑暗森林 - 刘慈欣 - 2008"
        );

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
            original_book_file: Some("三体.epub".into()),
            md5: Some("2b0e1f4a5c6d7e8f90a1b2c3d4e5f607".into()),
            title: "三体".into(),
            subtitle: "黑暗森林".into(),
            volume: "第二部".into(),
            author: "刘慈欣".into(),
            translator: "阳曦".into(),
            year: "2008".into(),
            publisher: "重庆出版社".into(),
            isbn: "978-7-5366-9293-0".into(),
        };
        let text = format_meta(&meta);
        let back = parse_meta(&text).expect("parse own output");
        assert_eq!(back, meta);
        // The v2 keys are on disk.
        for key in ["translator", "author", "year", "publisher", "isbn"] {
            assert!(
                text.contains(&format!("{key}: ")),
                "missing {key} in {text}"
            );
        }
        // 程序留痕的两个键也在，且用新键名。
        for key in ["originalBookFile: ", "md5: "] {
            assert!(text.contains(key), "missing {key} in {text}");
        }
        // 已删的键不再出现。
        for gone in ["displayTitle", "originalTitle", "bookFile:"] {
            assert!(!text.contains(gone), "{gone} must be gone from {text}");
        }

        // A v1 md (no v2 keys) parses with empty v2 fields — no data loss.
        let v1 = parse_meta("<!-- icedreader-meta\ntitle: 三体\nvolume: 第二部\n-->").unwrap();
        assert_eq!(v1.title, "三体");
        assert_eq!(v1.volume, "第二部");
        assert_eq!(v1.author, "");
        assert_eq!(v1.translator, "");
        assert_eq!(v1.publisher, "");
        assert_eq!(v1.isbn, "");

        // A stale displayTitle line is simply ignored (unknown key).
        let stale =
            parse_meta("<!-- icedreader-meta\ntitle: 三体\ndisplayTitle: 手写名\n-->\n").unwrap();
        assert_eq!(stale.title, "三体");
        assert_eq!(resolved_title(Some(&stale), "base"), "三体");

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

    /// 旧 md 的 `bookFile` 当作 `originalBookFile` 读入（那是它能给的最早名字）；
    /// `originalTitle` 直接忽略；写回去只有新键名。
    #[test]
    fn legacy_keys_are_adopted_or_ignored() {
        let legacy = parse_meta(
            "<!-- icedreader-meta\nbookFile: 旧名.epub\noriginalTitle: 旧书名\ntitle: 书\n-->\n",
        )
        .unwrap();
        assert_eq!(legacy.original_book_file.as_deref(), Some("旧名.epub"));
        assert_eq!(legacy.md5, None);
        assert_eq!(legacy.title, "书");
        // 两个键都在时以新键为准。
        let both = parse_meta(
            "<!-- icedreader-meta\noriginalBookFile: 新名.epub\nbookFile: 旧名.epub\ntitle: 书\n-->\n",
        )
        .unwrap();
        assert_eq!(both.original_book_file.as_deref(), Some("新名.epub"));
        let out = format_meta(&legacy);
        assert!(out.contains("originalBookFile: 旧名.epub"), "{out}");
        assert!(!out.contains("\nbookFile:"), "{out}");
        assert!(!out.contains("originalTitle"), "{out}");
    }

    #[test]
    fn read_write_roundtrip_on_disk() {
        let dir = std::env::temp_dir().join("icedreader-book-meta-test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("三体.epub.md");
        let meta = BookMeta {
            original_book_file: Some("三体.epub".into()),
            md5: Some("2b0e1f4a5c6d7e8f90a1b2c3d4e5f607".into()),
            title: "三体".into(),
            subtitle: "黑暗森林".into(),
            volume: String::new(),
            author: "刘慈欣".into(),
            translator: "阳曦".into(),
            year: "2008".into(),
            publisher: "重庆出版社".into(),
            isbn: "978-7-5366-9293-0".into(),
        };
        write_meta_file(&path, &meta).unwrap();
        assert_eq!(read_meta_file(&path), Some(meta.clone()));

        // Missing file → None.
        assert_eq!(read_meta_file(&dir.join("nope.md")), None);
    }

    #[test]
    fn split_meta_keeps_the_body_verbatim() {
        let text = format_meta(&BookMeta {
            title: "三体".into(),
            ..Default::default()
        }) + "\n## 第 1 章 · 开头\n\n笔记一行。\n";
        let (block, body) = split_meta(&text);
        assert!(block.starts_with("<!-- icedreader-meta"));
        assert!(block.ends_with("-->\n"));
        assert_eq!(body, "## 第 1 章 · 开头\n\n笔记一行。\n");

        // No marker → everything is user content.
        let (block, body) = split_meta("纯笔记，没有元数据块\n");
        assert!(block.is_empty());
        assert_eq!(body, "纯笔记，没有元数据块\n");

        // Unterminated block → never truncate the file.
        let (block, body) = split_meta("<!-- icedreader-meta\ntitle: 三体\n");
        assert!(block.is_empty());
        assert_eq!(body, "<!-- icedreader-meta\ntitle: 三体\n");

        // The file's own leading prose stays in the body when the block sits
        // above it (the block is always written first by the program).
        let (_, body) = split_meta("<!-- icedreader-meta\ntitle: 书\n-->\n\n前置散文\n");
        assert_eq!(body, "前置散文\n");
    }

    #[test]
    fn join_meta_leaves_the_body_alone() {
        let block = format_meta(&BookMeta {
            title: "三体".into(),
            ..Default::default()
        });
        let body = "## 第 1 章 · 开头\n\n我的笔记。\n";
        let joined = join_meta(&block, body);
        assert!(joined.starts_with("<!-- icedreader-meta"));
        assert!(joined.ends_with(body));
        // Round trip: split gets the same body back, byte for byte.
        assert_eq!(split_meta(&joined).1, body);

        // No metadata block → body untouched (a pure highlight file).
        assert_eq!(join_meta("", body), body);
        assert_eq!(join_meta("   \n", body), body);

        // Empty body → just the block, no trailing blank line.
        let only_block = join_meta(&block, "");
        assert!(only_block.ends_with("-->\n"));
        assert_eq!(split_meta(&only_block).1, "");
    }

    /// The whole point of the split: rewriting the metadata block must not
    /// touch one byte of the highlight archive below it, and vice versa.
    #[test]
    fn metadata_rewrite_preserves_the_notes_body() {
        let dir = std::env::temp_dir().join("icedreader-meta-body-test");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("三体.epub.md");

        let body = "## 第 1 章 · 开头\n\n<!-- icedreader-note\nid: abc\n-->\n> 【重点】摘录（全书 34% · 划于 2026-09-05 14:30）\n\n用户写的笔记。\n";
        fs::write(&path, join_meta(&format_meta(&BookMeta::default()), body)).unwrap();

        let meta = BookMeta {
            original_book_file: Some("三体.epub".into()),
            md5: Some("2b0e1f4a5c6d7e8f90a1b2c3d4e5f607".into()),
            title: "三体".into(),
            author: "刘慈欣".into(),
            ..Default::default()
        };
        write_meta_file(&path, &meta).unwrap();

        let text = fs::read_to_string(&path).unwrap();
        assert_eq!(split_meta(&text).1, body);
        assert_eq!(read_meta_file(&path), Some(meta));
    }
}
