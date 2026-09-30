//! 划线档案的 Tauri 侧入口。
//!
//! 解析与写回都在 `iced_reader_core::notes`（它和元数据块共用同一个伴生 md）；
//! 这里只做转发，方便命令层继续写 `notes::upsert(...)` 这样的调用，并保留
//! 「这台机器上，书的档案文件叫什么路径」这一个事实（`notes_path_for`），
//! 以及**落档**（首次进入书架时写 `originalBookFile` / `md5`，只写一次）。

pub use iced_reader_core::notes::{notes_of, split_excerpt, update_pos, upsert, NoteEntry};

use iced_reader_core::{read_meta_file, write_meta_file};

/// 书伴生 md 的路径（`三体.epub` → `<library>/三体.epub.md`）。
pub fn notes_path_for(dir: &std::path::Path, file_name: &str) -> std::path::PathBuf {
    dir.join(format!("{file_name}.md"))
}

/// 落档：给一本书的伴生 md 写上「首次进入书架时的库内文件名」与「当时那个文件的
/// MD5」。两者**只写一次，此后永不改** —— 书后来改名了，`originalBookFile` 仍记
/// 着它进来时叫什么；文件被换掉了，`md5` 也不动，正好是「这不是当初那个文件」的
/// 证据。这就是这个字段存在的全部意义（没有任何逻辑读它们）。
///
/// 幂等：两个字段都在就直接返回（连文件都不重算 MD5）。写入只动元数据块 ——
/// `write_meta_file` 走 `split_meta` / `join_meta`，划线块与你写的笔记逐字保留。
///
/// 调用点（都在与其它 md 写入同一串行的命令路径里，不新增并发写者）：
/// 导入到书库 / 打开一本书、以及首次保存元数据（用**改名之前**的文件名）。
/// 返回是否真的写了。
pub fn stamp_import_identity(
    book_path: &std::path::Path,
    file_name: &str,
) -> crate::error::Result<bool> {
    let dir = crate::portable::library_dir()?;
    let md = notes_path_for(&dir, file_name);
    let mut meta = read_meta_file(&md).unwrap_or_default();
    if meta.original_book_file.is_some() && meta.md5.is_some() {
        return Ok(false);
    }
    if meta.original_book_file.is_none() {
        meta.original_book_file = Some(file_name.to_string());
    }
    if meta.md5.is_none() {
        meta.md5 = Some(crate::book_signals::file_md5(book_path)?);
    }
    write_meta_file(&md, &meta)?;
    Ok(true)
}
