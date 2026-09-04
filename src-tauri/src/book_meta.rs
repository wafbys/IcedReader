//! View models + pure assembly for the 编辑元数据 panel. The companion md
//! lives next to the epub in `data/library/`; the file format, parsing and the
//! display-title resolution rules live in `iced_reader_core::book_meta`.

use iced_reader_core::{clean_person_list, clean_title, join_title, resolved_title, BookMeta};
use serde::{Deserialize, Serialize};

use crate::book_signals::{self, IdQuality};
use crate::library::BookProfile;

/// 面板里多名原书作者预填的连接符。书名中不出现中文标点（程序生成部分禁
/// 则），所以用 ASCII 逗号+空格，不用顿号。
const AUTHOR_LIST_JOIN: &str = ", ";

/// Panel payload: whatever the user typed in the inputs.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookMetaFields {
    pub title: String,
    pub subtitle: String,
    pub volume: String,
    pub author: String,
    pub translator: String,
    pub year: String,
    pub publisher: String,
    pub isbn: String,
    pub display_title: String,
}

/// What the panel shows when it opens for one book.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BookMetaView {
    pub file_name: String,
    /// Read-only: the title the program first saw (before any user edit).
    pub original_title: String,
    /// 主书名 — prefilled with the companion value or a cleaned current title.
    pub title: String,
    pub subtitle: String,
    pub volume: String,
    /// 作者 — prefilled with the companion value or the book's dc:creator
    /// (multiple names joined with 、), so the join has an author to show.
    pub author: String,
    /// 译者 — no dc source in the epub metadata yet; starts from the md value.
    pub translator: String,
    pub year: String,
    pub publisher: String,
    pub isbn: String,
    /// 手改框初值：md 里用户确认过的 displayTitle；空 = 未确认，保存后由
    /// 字段拼接接管（区别 `display_title` 的“当前裁决结果”——那个永远非空）。
    pub confirmed_title: String,
    /// The currently displayed title (resolution result) — the user may edit it.
    pub display_title: String,
    /// What a “regenerate” would produce from the current fields right now.
    pub suggested_title: String,
}

pub fn view_for(profile: &BookProfile, overlay: Option<&BookMeta>) -> BookMetaView {
    let base = profile.title.trim();    let original_title = overlay
        .and_then(|m| m.original_title.clone())
        .unwrap_or_else(|| base.to_string());
    let title = overlay
        .and_then(|m| (!m.title.trim().is_empty()).then(|| m.title.clone()))
        .unwrap_or_else(|| clean_title(base));
    let subtitle = overlay
        .map(|m| m.subtitle.clone())
        .unwrap_or_default();
    let volume = overlay.map(|m| m.volume.clone()).unwrap_or_default();
    // Prefill the author from the book's own dc:creator when the companion md
    // has none — the shelf card already shows it, so the join should too
    // (edit/clear it in the panel to override).
    let author = overlay
        .and_then(|m| (!m.author.trim().is_empty()).then(|| m.author.clone()))
        .unwrap_or_else(|| clean_person_list(&profile.authors.join(AUTHOR_LIST_JOIN)));
    let year = overlay.map(|m| m.year.clone()).unwrap_or_default();
    let publisher = overlay.map(|m| m.publisher.clone()).unwrap_or_default();
    let isbn = overlay.map(|m| m.isbn.clone()).unwrap_or_default();
    let translator = overlay
        .map(|m| m.translator.clone())
        .unwrap_or_default();
    let confirmed_title = overlay
        .map(|m| m.display_title.clone())
        .unwrap_or_default();
    let display_title = resolved_title(overlay, base);
    let suggested_title = join_title(
        &title, &subtitle, &volume, &author, &translator, &year, &publisher, &isbn,
    );
    BookMetaView {
        file_name: profile.file_name.clone(),
        original_title,
        title,
        subtitle,
        volume,
        author,
        translator,
        year,
        publisher,
        isbn,
        confirmed_title,
        display_title,
        suggested_title,
    }
}

/// Rebuild the panel from the book's own epub metadata (重新读取原书元数据):
/// clears everything user-entered and fills title/authors/publisher/ISBN
/// freshly from the file. subtitle/volume/translator/year have no usable OPF
/// source yet, so they come back empty. `originalTitle` is untouched — it
/// froze on the first save (or equals the current base title for a
/// never-saved book). Whether to save stays the user's call.
pub fn reread_view_for(
    profile: &BookProfile,
    original_title: &str,
    meta: &iced_reader_core::Metadata,
) -> BookMetaView {
    let base = profile.title.trim();
    let dc = meta.title.trim();
    let title_base = if dc.is_empty() || dc == "Untitled" {
        base
    } else {
        dc
    };
    let title = clean_title(title_base);
    let author = clean_person_list(&meta.authors.join(AUTHOR_LIST_JOIN));
    let publisher = meta.publisher.clone().unwrap_or_default();
    let isbn = extract_isbn(&meta.identifiers);
    let suggested_title = join_title(&title, "", "", &author, "", "", &publisher, &isbn);
    BookMetaView {
        file_name: profile.file_name.clone(),
        original_title: original_title.to_string(),
        title,
        subtitle: String::new(),
        volume: String::new(),
        author,
        translator: String::new(),
        year: String::new(),
        publisher,
        isbn,
        confirmed_title: String::new(),
        display_title: title_base.to_string(),
        suggested_title,
    }
}

/// First ISBN-looking identifier, stripped of its urn:/label prefix
/// (`urn:isbn:978-7-…` → `978-7-…`). Empty when none looks like an ISBN.
pub fn extract_isbn(identifiers: &[String]) -> String {
    identifiers
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .find(|s| book_signals::classify_identifier(s) == IdQuality::Isbn)
        .map(normalize_isbn)
        .unwrap_or_default()
}

/// Cut everything before the first ASCII digit (`urn:isbn:` / `isbn:` /
/// stray labels) so the stored value is the bare number, keeping its dashes.
fn normalize_isbn(s: &str) -> String {
    match s.find(|c: char| c.is_ascii_digit()) {
        Some(i) => s[i..].trim().to_string(),
        None => s.trim().to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(title: &str) -> BookProfile {
        BookProfile {
            file_name: "三体.epub".into(),
            title: title.into(),
            authors: Vec::new(),
            progress_key: "lib:三体.epub".into(),
            chapter_hrefs: Vec::new(),
            chapter_titles: Vec::new(),
            has_cover: false,
            open_error: None,
        }
    }

    fn profile_with_authors(title: &str, authors: Vec<&str>) -> BookProfile {
        let mut p = profile(title);
        p.authors = authors.into_iter().map(|s| s.to_string()).collect();
        p
    }

    #[test]
    fn no_overlay_prefills_cleaned_current_title() {
        let view = view_for(&profile("  三体  "), None);
        // The panel always deals in trimmed values (both the prefilled field
        // and the display box); originalTitle keeps the first-seen name.
        assert_eq!(view.original_title, "三体");
        assert_eq!(view.title, "三体");
        assert_eq!(view.display_title, "三体");
        assert_eq!(view.suggested_title, "三体");
        // No companion md yet → nothing is user-confirmed → empty hand-edit box.
        assert_eq!(view.confirmed_title, "");
        assert_eq!(view.file_name, "三体.epub");
        // Bibliographic fields start empty (no dc source in this profile).
        assert_eq!(view.author, "");
        assert_eq!(view.translator, "");
        assert_eq!(view.year, "");
        assert_eq!(view.publisher, "");
        assert_eq!(view.isbn, "");
    }

    #[test]
    fn author_prefills_from_dc_creator() {
        let view = view_for(&profile_with_authors("三体", vec!["刘慈欣"]), None);
        assert_eq!(view.author, "刘慈欣");
        // The suggested join shows the full template shape.
        assert_eq!(view.suggested_title, "三体 - 刘慈欣");

        let multi = view_for(
            &profile_with_authors("三体", vec!["刘慈欣", "王晋康"]),
            None,
        );
        assert_eq!(multi.author, "刘慈欣, 王晋康");
        assert_eq!(multi.suggested_title, "三体 - 刘慈欣, 王晋康");
    }

    #[test]
    fn overlay_fields_shape_the_view() {
        let meta = BookMeta {
            original_title: Some("首发时的脏名".into()),
            title: "三体".into(),
            subtitle: "黑暗森林".into(),
            volume: "第二部".into(),
            author: "刘慈欣".into(),
            translator: "阳曦".into(),
            year: "2008".into(),
            publisher: "重庆出版社".into(),
            isbn: "978-7-5366-9293-0".into(),
            display_title: String::new(),
            book_file: None,
        };
        let view = view_for(&profile("原 dc:title"), Some(&meta));
        // originalTitle freezes the first-seen value, not the current dc:title.
        assert_eq!(view.original_title, "首发时的脏名");
        assert_eq!(view.title, "三体");
        assert_eq!(
            view.suggested_title,
            "三体 _ 黑暗森林 - 第二部 - 刘慈欣 - 译者 阳曦 - 2008 - 重庆出版社 - ISBN 978-7-5366-9293-0"
        );
        assert_eq!(view.display_title, view.suggested_title);
        // md.displayTitle empty → still derived, hand-edit box stays empty.
        assert_eq!(view.confirmed_title, "");
    }

    #[test]
    fn extract_isbn_prefers_isbn_and_strips_prefix() {
        assert_eq!(extract_isbn(&[]), "");
        assert_eq!(
            extract_isbn(&[
                "urn:uuid:xxx".into(),
                "urn:isbn:978-7-5366-9293-0".into(),
            ]),
            "978-7-5366-9293-0"
        );
        assert_eq!(extract_isbn(&["isbn:9781234567890".into()]), "9781234567890");
        assert_eq!(extract_isbn(&[" 978-7-1 ".into()]), "978-7-1");
        assert_eq!(extract_isbn(&["amazon:XXXXXX".into()]), "");
    }

    #[test]
    fn reread_view_clears_edits_and_fills_from_epub_meta() {
        use iced_reader_core::Metadata;
        let profile = profile_with_authors("旧 profile 名", vec!["旧作者"]);
        let meta = Metadata {
            title: "  原书dc:书名  ".into(),
            authors: vec!["原书作者甲".into(), "原书作者乙".into()],
            language: None,
            publisher: Some("原书出版社".into()),
            identifiers: vec!["urn:isbn:978-7-1".into()],
            description: None,
            cover_href: None,
        };
        let view = reread_view_for(&profile, "定格的原书名", &meta);
        assert_eq!(view.original_title, "定格的原书名");
        assert_eq!(view.title, "原书dc:书名");
        assert_eq!(view.author, "原书作者甲, 原书作者乙");
        assert_eq!(view.publisher, "原书出版社");
        assert_eq!(view.isbn, "978-7-1");
        // User-edited fields come back empty; nothing is confirmed.
        for empty in [&view.subtitle, &view.volume, &view.translator, &view.year, &view.confirmed_title] {
            assert!(empty.is_empty(), "expected empty field");
        }
        assert_eq!(
            view.suggested_title,
            "原书dc:书名 - 原书作者甲, 原书作者乙 - 原书出版社 - ISBN 978-7-1"
        );
    }

    #[test]
    fn hand_confirmed_display_title_wins() {
        let meta = BookMeta {
            title: "三体".into(),
            subtitle: "黑暗森林".into(),
            author: "刘慈欣".into(),
            display_title: "三体：黑暗森林（用户手写）".into(),
            ..Default::default()
        };
        let view = view_for(&profile("旧名"), Some(&meta));
        assert_eq!(view.display_title, "三体：黑暗森林（用户手写）");
        // The confirmed (hand-edited) value feeds the hand-edit box on reopen…
        assert_eq!(view.confirmed_title, "三体：黑暗森林（用户手写）");
        // …and the regenerate suggestion still follows the join template.
        assert_eq!(view.suggested_title, "三体 _ 黑暗森林 - 刘慈欣");
    }
}
