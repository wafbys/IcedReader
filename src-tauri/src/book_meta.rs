//! View models + pure assembly for the 编辑元数据 panel. The companion md
//! lives next to the epub in `data/library/`; the file format, parsing and the
//! display-title resolution rules live in `iced_reader_core::book_meta`.

use iced_reader_core::{clean_title, join_title, resolved_title, BookMeta};
use serde::{Deserialize, Serialize};

use crate::library::BookProfile;

/// Panel payload: whatever the user typed in the four inputs.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookMetaFields {
    pub title: String,
    pub subtitle: String,
    pub volume: String,
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
    /// 手改框初值：md 里用户确认过的 displayTitle；空 = 未确认，保存后由
    /// 字段拼接接管（区别 `display_title` 的“当前裁决结果”——那个永远非空）。
    pub confirmed_title: String,
    /// The currently displayed title (resolution result) — the user may edit it.
    pub display_title: String,
    /// What a “regenerate” would produce from the current fields right now.
    pub suggested_title: String,
}

pub fn view_for(profile: &BookProfile, overlay: Option<&BookMeta>) -> BookMetaView {
    let base = profile.title.trim();
    let original_title = overlay
        .and_then(|m| m.original_title.clone())
        .unwrap_or_else(|| base.to_string());
    let title = overlay
        .and_then(|m| (!m.title.trim().is_empty()).then(|| m.title.clone()))
        .unwrap_or_else(|| clean_title(base));
    let subtitle = overlay
        .map(|m| m.subtitle.clone())
        .unwrap_or_default();
    let volume = overlay.map(|m| m.volume.clone()).unwrap_or_default();
    let confirmed_title = overlay
        .map(|m| m.display_title.clone())
        .unwrap_or_default();
    let display_title = resolved_title(overlay, base);
    let suggested_title = join_title(&title, &subtitle, &volume);
    BookMetaView {
        file_name: profile.file_name.clone(),
        original_title,
        title,
        subtitle,
        volume,
        confirmed_title,
        display_title,
        suggested_title,
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
    }

    #[test]
    fn overlay_fields_shape_the_view() {
        let meta = BookMeta {
            original_title: Some("首发时的脏名".into()),
            title: "三体".into(),
            subtitle: "黑暗森林".into(),
            volume: String::new(),
            display_title: String::new(),
            book_file: None,
        };
        let view = view_for(&profile("原 dc:title"), Some(&meta));
        // originalTitle freezes the first-seen value, not the current dc:title.
        assert_eq!(view.original_title, "首发时的脏名");
        assert_eq!(view.title, "三体");
        assert_eq!(view.suggested_title, "三体 _ 黑暗森林");
        assert_eq!(view.display_title, "三体 _ 黑暗森林");
        // md.displayTitle empty → still derived, hand-edit box stays empty.
        assert_eq!(view.confirmed_title, "");
    }

    #[test]
    fn hand_confirmed_display_title_wins() {
        let meta = BookMeta {
            title: "三体".into(),
            subtitle: "黑暗森林".into(),
            volume: String::new(),
            display_title: "三体：黑暗森林（用户手写）".into(),
            book_file: None,
            original_title: None,
        };
        let view = view_for(&profile("旧名"), Some(&meta));
        assert_eq!(view.display_title, "三体：黑暗森林（用户手写）");
        // The confirmed (hand-edited) value feeds the hand-edit box on reopen…
        assert_eq!(view.confirmed_title, "三体：黑暗森林（用户手写）");
        // …and the regenerate suggestion still follows the ASCII-join rule.
        assert_eq!(view.suggested_title, "三体 _ 黑暗森林");
    }
}
