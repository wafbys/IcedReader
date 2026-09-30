//! 划线档案的 Tauri 侧入口。
//!
//! 解析与写回都在 `iced_reader_core::notes`（它和元数据块共用同一个伴生 md）；
//! 这里只做转发，方便命令层继续写 `notes::upsert(...)` 这样的调用，并保留
//! 「这台机器上，书的档案文件叫什么路径」这一个事实（`notes_path_for`）。

pub use iced_reader_core::notes::{notes_of, update_pos, upsert, NoteEntry};

/// 书伴生 md 的路径（`三体.epub` → `<library>/三体.epub.md`）。
pub fn notes_path_for(dir: &std::path::Path, file_name: &str) -> std::path::PathBuf {
    dir.join(format!("{file_name}.md"))
}
