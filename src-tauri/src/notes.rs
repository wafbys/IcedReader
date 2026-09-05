//! `<stem>.notes.md` — 划线与备注档案（AGENTS 约束③「划线生命周期与笔记」）。
//!
//! 程序保护区：`<!-- icedreader-note` 注释块 + **紧跟其后的摘抄行**（md
//! 引用块，`> ` 开头，md 软件里看得舒服）；其后的文本到下一个保护区/`##`
//! 章标题为用户的自由笔记区。程序靠「注释块后首个以 `> ` 开头的行」识别
//! 摘抄行，重写时只动注释块与摘抄行、用户区原样保留（A+B：阅读器内备注
//! 框与外部 md 软件编辑的是同一处）。宽容解析：缺 `id:` 的注释块与手写无
//! 块段落一律按原样文本保留，不丢不覆盖。
//!
//! 时间/位置等人类文本由调用方（Tauri 层）格式化后经 [`NoteEntry`] 传入，
//! 本模块只保证结构正确性：注释块 ↔ 摘抄行 ↔ 用户区的分界与顺序。

use crate::library;

pub const NOTE_OPEN: &str = "<!-- icedreader-note";
pub const NOTE_CLOSE: &str = "-->";

/// 颜色 → md 摘抄行里的语义标签（opinionated：黄=重点、绿=摘抄）。
pub fn color_label(color: &str) -> &'static str {
    match color {
        "green" => "摘抄",
        _ => "重点",
    }
}

/// `pos` 0–1 → 「全书 N%」。
pub fn pos_label(pos: f64) -> String {
    let pct = (pos.clamp(0.0, 1.0) * 100.0).round() as u32;
    format!("全书 {pct}%")
}

/// 一条划线的档案条目。`comment_lines` 是保护区注释块（首行
/// `<!-- icedreader-note`、末行 `-->`）；`excerpt` 是紧跟其后的摘抄行
/// （普通文本，程序生成，含标签/位置/划线时间）。
pub struct NoteEntry {
    pub id: String,
    /// 章标题行全文（`## 第 12 章 · …`），新条目按它分组归属。
    pub section_title: String,
    /// 程序保护区：注释块各行（含 `<!--` 与 `-->`）。
    pub comment_lines: Vec<String>,
    /// 摘抄行全文（以 `> ` 开头的引用块；`> 【重点】…（全书 N% · 划于 …）`）。
    pub excerpt: String,
    /// 用户笔记区文本（外部编辑器/备注框都可写）。
    pub note: String,
}

pub struct NoteSeg {
    pub id: String,
    pub open_lines: Vec<String>, // 注释块各行（含 <!-- 与 -->）
    pub excerpt: Option<String>, // 摘抄行（普通文本）
    pub note: Vec<String>,       // 用户区原文行
}

enum Seg {
    Note(NoteSeg),
    Text(Vec<String>),
}

impl Seg {
    fn id(&self) -> Option<&str> {
        match self {
            Seg::Note(n) => Some(&n.id),
            Seg::Text(_) => None,
        }
    }
}

fn split_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    text.lines().map(|l| l.to_string()).collect()
}

fn is_note_open(line: &str) -> bool {
    line.trim_start().starts_with(NOTE_OPEN)
}

fn is_section(line: &str) -> bool {
    line.starts_with("## ")
}

fn parse_fields(open_lines: &[String]) -> Option<String> {
    let mut id: Option<String> = None;
    for line in open_lines {
        let t = line.trim();
        if t.is_empty() || t.starts_with(NOTE_OPEN) || t.starts_with(NOTE_CLOSE) {
            continue;
        }
        if let Some((key, value)) = t.split_once(':') {
            if key.trim() == "id" {
                let v = value.trim();
                if !v.is_empty() {
                    id = Some(v.to_string());
                }
            }
        }
    }
    id
}

/// 结构解析：把文本切成 Note 段与 Text 段。注释块之后紧接的首个非空行 =
/// 摘抄行（普通文本）；其后到下一个锚点（注释块 / `## `）之间的行是用户区。
fn parse(text: &str) -> Vec<Seg> {
    let lines = split_lines(text);
    let n = lines.len();
    let mut segs: Vec<Seg> = Vec::new();
    let mut i = 0usize;
    while i < n {
        if is_note_open(&lines[i]) {
            // 注释块：到 `-->` 行（缺则到锚点/文尾，宽容）。
            let mut j = i;
            while j < n && !lines[j].trim_start().starts_with(NOTE_CLOSE) {
                j += 1;
            }
            if j < n {
                j += 1; // 包含 --> 行
            }
            let open_lines = lines[i..j].to_vec();
            let id = parse_fields(&open_lines);
            // 摘抄行：块结束后（允许空行）首个以 `> ` 开头的引用行。
            let mut k = j;
            while k < n && lines[k].trim().is_empty() {
                k += 1;
            }
            let excerpt = if k < n && lines[k].starts_with("> ") {
                let e = lines[k].clone();
                k += 1;
                Some(e)
            } else {
                None
            };
            // 用户区：直到下一个锚点或文尾。
            let mut m = k;
            while m < n && !is_note_open(&lines[m]) && !is_section(&lines[m]) {
                m += 1;
            }
            if let Some(id) = id {
                segs.push(Seg::Note(NoteSeg {
                    id,
                    open_lines,
                    excerpt,
                    note: lines[k..m].to_vec(),
                }));
            } else {
                // 缺 id 的注释块：整段（到下一锚点）当普通文本，原样保留。
                segs.push(Seg::Text(lines[i..m].to_vec()));
            }
            i = m;
        } else {
            let mut j = i;
            while j < n && !is_note_open(&lines[j]) {
                j += 1;
            }
            segs.push(Seg::Text(lines[i..j].to_vec()));
            i = j;
        }
    }
    segs
}

fn serialize(segs: &[Seg]) -> String {
    let mut out: Vec<String> = Vec::new();
    for seg in segs {
        match seg {
            Seg::Text(lines) => out.extend(lines.iter().cloned()),
            Seg::Note(note) => {
                out.extend(note.open_lines.iter().cloned());
                if let Some(excerpt) = &note.excerpt {
                    out.push(excerpt.clone());
                }
                out.extend(note.note.iter().cloned());
            }
        }
    }
    // 去掉收尾多余空行，保证恰好一个结尾换行（保持文件整洁）。
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    if out.is_empty() {
        String::new()
    } else {
        let mut s = out.join("\n");
        s.push('\n');
        s
    }
}

fn is_blank_line(line: &str) -> bool {
    line.trim().is_empty()
}

/// 章标题行（`## …`）所在的 Text 段下标（段首行即标题）。
fn find_section(segs: &[Seg], title: &str) -> Option<usize> {
    segs.iter().position(|s| match s {
        Seg::Text(lines) => lines.first().is_some_and(|l| l == title),
        Seg::Note(_) => false,
    })
}

fn note_from_entry(entry: &NoteEntry) -> Seg {
    // 新条目在摘抄行与笔记之间留一个空行，普通段落分开（用户区原样保留）。
    let mut note_lines = split_lines(&entry.note);
    if !note_lines.is_empty() && !is_blank_line(&note_lines[0]) {
        note_lines.insert(0, String::new());
    }
    let excerpt = if entry.excerpt.is_empty() {
        None
    } else {
        Some(entry.excerpt.clone())
    };
    Seg::Note(NoteSeg {
        id: entry.id.clone(),
        open_lines: entry.comment_lines.clone(),
        excerpt,
        note: note_lines,
    })
}

/// 将一条划线写入档案：`id` 已存在 → 原位替换保护区与笔记区（UI 保存 =
/// 用户最新意图，覆盖用户区）；不存在 → 归入 `section_title` 章（没有则
/// 在文末建章），插到该章内容之后。返回新全文。
pub fn upsert(text: &str, entry: &NoteEntry) -> String {
    let mut segs = parse(text);
    if let Some(pos) = segs.iter().position(|s| s.id() == Some(entry.id.as_str())) {
        segs[pos] = note_from_entry(entry);
        return serialize(&segs);
    }
    let note_seg = note_from_entry(entry);
    if let Some(sec) = find_section(&segs, &entry.section_title) {
        // 插到该章内最后一条 Note 之后（保持追加顺序），下一个 `## ` 章标题
        // 或文尾为止；前面不是空行时补一个分隔空行。
        let mut insert_at = sec + 1;
        for k in sec + 1..segs.len() {
            match &segs[k] {
                Seg::Note(_) => insert_at = k + 1,
                Seg::Text(lines) => {
                    if lines.first().is_some_and(|l| l.starts_with("## ")) {
                        break;
                    }
                }
            }
        }
        let prev_blank = match &segs[insert_at - 1] {
            Seg::Text(lines) => lines.last().is_some_and(|l| is_blank_line(l)),
            Seg::Note(n) => n.note.last().is_some_and(|l| is_blank_line(l)),
        };
        if prev_blank {
            segs.insert(insert_at, note_seg);
        } else {
            segs.splice(
                insert_at..insert_at,
                [Seg::Text(vec![String::new()]), note_seg].into_iter(),
            );
        }
        serialize(&segs)
    } else {
        // 无此章：文末追加（标题后空一行再放条目）。
        segs.push(Seg::Text(vec![entry.section_title.clone()]));
        segs.push(Seg::Text(vec![String::new()]));
        segs.push(note_seg);
        serialize(&segs)
    }
}

/// 划线删除：在档案里给该条打删除标记（`deleted:` + 摘抄行删除线），
/// 用户笔记区原样保留。该 id 不在档案里（纯划线从未写备注）返回 `None`。
pub fn mark_deleted(
    text: &str,
    id: &str,
    deleted_iso: &str,
    deleted_human: &str,
) -> Option<String> {
    let mut segs = parse(text);
    let pos = segs.iter().position(|s| s.id() == Some(id))?;
    let Seg::Note(note) = &mut segs[pos] else {
        return None;
    };
    if note.open_lines.iter().any(|l| {
        l.trim()
            .strip_prefix("deleted:")
            .is_some_and(|v| !v.trim().is_empty())
    }) {
        return None; // 已标记过（幂等保护）
    }
    // deleted: 行填时间。
    let mut touched = false;
    for line in note.open_lines.iter_mut() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("deleted:") {
            if rest.trim().is_empty() {
                *line = format!("deleted: {deleted_iso}");
                touched = true;
            }
        }
    }
    if !touched {
        // 宽容：缺 deleted: 行时补一行（保护区修复）。
        let insert_at = note
            .open_lines
            .iter()
            .position(|l| l.trim_start().starts_with(NOTE_CLOSE))
            .unwrap_or(note.open_lines.len());
        note.open_lines
            .insert(insert_at, format!("deleted: {deleted_iso}"));
    }
    // 摘抄行加删除线与删除说明（引用块内包 md 删除线语法）。
    if let Some(excerpt) = &note.excerpt {
        if !excerpt.contains("已删于") && excerpt.starts_with("> ") {
            let body = &excerpt[2..];
            note.excerpt = Some(format!("> ~~{body}~~（已删于 {deleted_human}）"));
        }
    }
    Some(serialize(&segs))
}

/// 撤掉备注：移除该条的注释块与摘抄行，用户笔记区转成普通文本留在原位
/// （外部编辑器写进的内容绝不丢）。id 不在档案里返回 `None`。
pub fn remove_note(text: &str, id: &str) -> Option<String> {
    let mut segs = parse(text);
    let pos = segs.iter().position(|s| s.id() == Some(id))?;
    let Seg::Note(note) = segs.remove(pos) else {
        return None;
    };
    let mut orphan = note.note;
    // 前面若不是空行，补一个让孤立的用户区与上方内容分开。
    let need_lead = match segs.get(pos.wrapping_sub(1)) {
        Some(Seg::Text(lines)) => !lines.last().is_some_and(|l| is_blank_line(l)),
        Some(Seg::Note(_)) => true,
        None => false,
    };
    if need_lead {
        orphan.insert(0, String::new());
    }
    segs.insert(pos, Seg::Text(orphan));
    Some(serialize(&segs))
}

/// 读出档案里每条划线的用户笔记（id → 笔记文本），供悬停浮层与列表。
pub fn notes_of(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for seg in parse(text) {
        if let Seg::Note(note) = seg {
            let joined: String = note.note.join("\n");
            let trimmed = joined.trim();
            if !trimmed.is_empty() {
                out.push((note.id, trimmed.to_string()));
            }
        }
    }
    out
}

/// notes 文件完整路径（库内 `<stem>.epub` → `<stem>.notes.md`）。
pub fn notes_path_for(
    dir: &std::path::Path,
    file_name: &str,
) -> Result<std::path::PathBuf, String> {
    library::notes_path_for(dir, file_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comment_lines(id: &str, color: &str) -> Vec<String> {
        vec![
            "<!-- icedreader-note".into(),
            format!("id: {id}"),
            format!("color: {color}"),
            "created: 2026-09-05T14:30:00+08:00".into(),
            "deleted:".into(),
            "posPct: 34".into(),
            "-->".into(),
        ]
    }

    fn entry(id: &str, note: &str) -> NoteEntry {
        NoteEntry {
            id: id.into(),
            section_title: "## 第 1 章 · 开头".into(),
            comment_lines: comment_lines(id, "yellow"),
            excerpt: "> 【重点】摘录一（全书 34% · 划于 2026-09-05 14:30）".into(),
            note: note.into(),
        }
    }

    #[test]
    fn color_and_pos_labels() {
        assert_eq!(color_label("green"), "摘抄");
        assert_eq!(color_label("yellow"), "重点");
        assert_eq!(color_label("unknown"), "重点");
        assert_eq!(pos_label(0.0), "全书 0%");
        assert_eq!(pos_label(0.342), "全书 34%");
        assert_eq!(pos_label(0.995), "全书 100%");
        assert_eq!(pos_label(1.5), "全书 100%");
    }

    #[test]
    fn empty_file_upsert_creates_section_and_entry() {
        let out = upsert("", &entry("a", "第一条笔记"));
        let expected = "## 第 1 章 · 开头\n\n<!-- icedreader-note\nid: a\ncolor: yellow\ncreated: 2026-09-05T14:30:00+08:00\ndeleted:\nposPct: 34\n-->\n> 【重点】摘录一（全书 34% · 划于 2026-09-05 14:30）\n\n第一条笔记\n";
        assert_eq!(out, expected);
        // 摘抄行是 md 引用块（以 `>` 开头，md 软件里看得舒服）。
        assert!(out.lines().any(|l| l.starts_with("> 【重点】")));
        assert_eq!(
            notes_of(&out),
            vec![("a".to_string(), "第一条笔记".to_string())]
        );
    }

    #[test]
    fn upsert_existing_id_replaces_block_and_note_in_place() {
        let v1 = upsert("", &entry("a", "旧笔记"));
        let mut e2 = entry("a", "新笔记（外部编辑器改过样式，UI 保存覆盖）");
        e2.comment_lines = comment_lines("a", "green");
        e2.excerpt = "> 【摘抄】摘录一（全书 34% · 划于 2026-09-05 14:30）".into();
        let out = upsert(&v1, &e2);
        assert!(out.contains("color: green"));
        assert!(out.contains("> 【摘抄】摘录一（全书 34% · 划于 2026-09-05 14:30）"));
        assert!(out.contains("新笔记（外部编辑器改过样式，UI 保存覆盖）"));
        assert!(!out.contains("旧笔记"));
        assert_eq!(
            notes_of(&out),
            vec![(
                "a".to_string(),
                "新笔记（外部编辑器改过样式，UI 保存覆盖）".to_string()
            )]
        );
    }

    #[test]
    fn unrelated_content_survives_upsert_byte_for_byte() {
        let preamble = "# 资治通鉴 划线笔记\n\n我在文件头写的东西。\n\n";
        let v1 = upsert("", &entry("a", "笔记 a"));
        let v1 = format!("{preamble}{v1}");
        let mut e2 = entry("b", "笔记 b");
        e2.section_title = "## 第 2 章 · 另一章".into();
        let out = upsert(&v1, &e2);
        assert!(out.starts_with(&preamble), "文件头必须原样保留");
        assert!(out.contains("## 第 2 章 · 另一章"));
        assert!(out.contains("笔记 b"));
        assert!(out.contains("笔记 a"));
    }

    #[test]
    fn mark_deleted_keeps_note_and_annotates() {
        let v1 = upsert("", &entry("a", "用户笔记内容\n第二行"));
        let out = mark_deleted(&v1, "a", "2026-09-05T16:00:00+08:00", "2026-09-05 16:00").unwrap();
        assert!(out.contains("deleted: 2026-09-05T16:00:00+08:00"));
        assert!(out.contains(
            "> ~~【重点】摘录一（全书 34% · 划于 2026-09-05 14:30）~~（已删于 2026-09-05 16:00）"
        ));
        assert!(out.contains("用户笔记内容\n第二行"));
        assert_eq!(
            notes_of(&out),
            vec![("a".to_string(), "用户笔记内容\n第二行".to_string())]
        );
        // 幂等：再次调用返回 None，内容不变。
        assert!(mark_deleted(&out, "a", "x", "x").is_none());
        // 不在档案里的 id → None。
        assert!(mark_deleted(&v1, "missing", "x", "x").is_none());
    }

    #[test]
    fn remove_note_keeps_user_text_as_free_content() {
        let v1 = upsert("", &entry("a", "我手写的笔记"));
        let out = remove_note(&v1, "a").unwrap();
        assert!(!out.contains("icedreader-note"));
        assert!(!out.contains("> 【重点】摘录一"));
        assert!(out.contains("我手写的笔记"), "用户文字不丢");
        assert!(notes_of(&out).is_empty());
    }

    #[test]
    fn orphan_handwritten_content_and_comment_without_id_survive() {
        let hand = "# 我的手动笔记\n\n完全手写的一段，没有注释块。\n";
        let v1 = upsert("", &entry("a", "a 的笔记"));
        let mixed = format!("{hand}{v1}");
        // 用户区里出现顶层 `## `（虽不鼓励）应被切断保护，内容仍在。
        let with_head = upsert(
            &mixed,
            &entry("b", "b 的笔记\n\n## 我自己小节\n\n正文内容"),
        );
        assert!(with_head.contains("## 我自己小节"));
        assert!(with_head.contains("我自己小节\n\n正文内容"));
        assert!(with_head.contains("b 的笔记"));
        assert!(with_head.contains("a 的笔记"));
    }

    #[test]
    fn roundtrip_preserves_notes_of() {
        let mut text = String::new();
        text = upsert(&text, &entry("a", "笔记 a"));
        text = upsert(&text, &entry("b", "笔记 b 第二行"));
        let parsed = notes_of(&text);
        assert_eq!(parsed.len(), 2);
        assert!(parsed.iter().any(|(id, n)| id == "a" && n == "笔记 a"));
        assert!(parsed.iter().any(|(id, n)| id == "b" && n == "笔记 b 第二行"));
        // 重解析再序列化应保持结构稳定（解析不引入漂移）。
        assert_eq!(serialize(&parse(&text)), text);
    }
}
