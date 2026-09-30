//! 书伴生 md 的划线档案。全书只有 **一个** 伴生 md（`三体.epub.md`），
//! 顶部是 `<!-- icedreader-meta -->` 元数据块，其下是这里管的划线区。
//!
//! 每次划线都落一条（程序保护区 + 摘抄行），写过备注再往用户区加字。
//!
//! 程序保护区：`<!-- icedreader-note` 注释块 + **紧跟其后的摘抄行**（`> `
//! 引用，一段一条）。块里的机器字段是**划线记录的完整存储**（`id` / `color` /
//! `created` / `deleted` / `posPct`，渲染高亮必需的坐标
//! `href` / `startText` / `startOffset` / `endText` / `endOffset`，摘录
//! `text`（空白折成一行的全文）与它的段落切点 `paras`）。`paras` 是 `text`
//! 里段落之间那个空格的字节下标，所以摘抄行在**任何一次重写**（位置回填、改
//! 备注）之后都能从记录原样摊回多条引用，而不是写一次就不能动。摘抄行的最后
//! 一条固定是 `> （全书 N% · 划于 …）`，删除时各条加删除线、末条附（已删于 …）。
//! 其后到下一个保护区 / `##` 章标题 = 用户笔记区。重写只动保护区，
//! 用户区逐字保留（阅读器备注框与外部 md 软件写的是同一处）。按「注释块后
//! 连续的 `> ` 行」认摘抄行。缺 `id:` 的块与手写无块段落原样保留。
//!
//! 删除：有备注则保护区记 `deleted:`、摘抄行加删除线并附（已删于 …）；
//! 纯划线（从未写备注）删除即整条移除、md 无痕（若用户区被外部写过，
//! 转成普通文本保留；因此变空的章标题一并清掉）。同一处再划 = 新 id。
//!
//! 时间/位置等人类文本由调用方格式化后经 [`NoteEntry`] 传入。
//!
//! **元数据块是别人的地盘**：下列公开入口用 [`crate::split_meta`] /
//! [`crate::join_meta`] 把它原样切开、原样拼回，只在正文里增删划线 ——
//! 改划线碰不到元数据，改元数据也碰不到划线。

use crate::annotations::{Highlight, StoredHighlight};
use crate::book_meta::{join_meta, split_meta};

pub const NOTE_OPEN: &str = "<!-- icedreader-note";
pub const NOTE_CLOSE: &str = "-->";

/// 颜色 → md 摘抄行里的语义标签（opinionated：黄=重点、绿=摘抄）。
pub fn color_label(color: &str) -> &'static str {
    match color {
        "green" => "摘抄",
        _ => "重点",
    }
}

/// `pos` 0–1 → 「全书 N%」；未知（权重还没算出来就划了线）→「位置未知」。
pub fn pos_label(pos: Option<f64>) -> String {
    match pos {
        Some(pos) => {
            let pct = (pos.clamp(0.0, 1.0) * 100.0).round() as u32;
            format!("全书 {pct}%")
        }
        None => "位置未知".into(),
    }
}

/// `pos` 0–1 → 整数百分比字符串（伴生 md 注释块的机器字段 `posPct`）；
/// 未知则空串，不写一个假的 0。
pub fn pos_pct(pos: Option<f64>) -> String {
    match pos {
        Some(pos) => ((pos.clamp(0.0, 1.0) * 100.0).round() as u32).to_string(),
        None => String::new(),
    }
}

/// 一条划线的档案条目。机器字段（坐标、颜色、位置、时间、摘录）由
/// [`NoteEntry::highlight`] 生成到保护区注释块里；`comment_lines` 是已经排好
/// 的块（内部用）；`excerpt` 是紧跟其后的摘抄行（普通文本，程序生成：一段一条
/// `> ` 引用，末条是位置/划线时间）。
pub struct NoteEntry {
    /// 这条划线的完整记录（坐标 + 颜色 + 位置 + 时间 + 摘录原文 + 段落切点）。
    pub highlight: Highlight,
    /// 章标题行全文（`## 第 12 章 · …`），新条目按它分组归属。
    pub section_title: String,
    /// 用户笔记区文本（外部编辑器/备注框都可写）。
    pub note: String,
}

impl NoteEntry {
    /// 程序保护区的注释块各行（`<!-- icedreader-note` … `-->`）。机器字段
    /// 一律在这里，人读的摘抄行由 [`NoteSeg::excerpt`] 负责。
    fn comment_lines(&self) -> Vec<String> {
        let h = &self.highlight;
        let mut out = vec![NOTE_OPEN.to_string()];
        out.push(format!("id: {}", h.id));
        out.push(format!("color: {}", h.color));
        out.push(format!("created: {}", h.created_at));
        out.push(format!("deleted: {}", String::new()));
        out.push(format!("posPct: {}", pos_pct(h.pos)));
        out.push(format!("href: {}", h.href));
        out.push(format!("startText: {}", h.start_text));
        out.push(format!("startOffset: {}", h.start_offset));
        out.push(format!("endText: {}", h.end_text));
        out.push(format!("endOffset: {}", h.end_offset));
        out.push(format!("text: {}", single_line(&h.text)));
        out.push(format!(
            "paras: {}",
            h.paras
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(",")
        ));
        out.push(NOTE_CLOSE.to_string());
        out
    }
}

/// 摘录原文压成一行：块字段是逐行的，换行会切碎块。
fn single_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 原始选区文本（段落之间是换行）→ 记录用的折行全文 + 段落切点。
///
/// 段内空白折成一个空格、空段丢弃（选区的行尾缩进、源里的排版空白都不进记录）；
/// 切点是折行后的 `text` 里**段落之间那个空格**的字节下标，配合
/// [`excerpt_paragraphs`] 可原样摊回段落。空选区 → 空串 + 无切点。
pub fn split_excerpt(raw: &str) -> (String, Vec<usize>) {
    let mut text = String::new();
    let mut paras: Vec<usize> = Vec::new();
    for line in raw.split('\n') {
        let para = single_line(line);
        if para.is_empty() {
            continue;
        }
        if !text.is_empty() {
            paras.push(text.len());
            text.push(' ');
        }
        text.push_str(&para);
    }
    (text, paras)
}

/// 按段落切点把折成一行的摘录摊回段落。越界、非字符边界或落到原地的切点直接
/// 忽略（`text:` / `paras:` 被外部改过时保持宽容），不丢字。
fn excerpt_paragraphs(text: &str, paras: &[usize]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut start = 0usize;
    for &cut in paras {
        if cut <= start || cut >= text.len() || !text.is_char_boundary(cut) {
            continue;
        }
        out.push(text[start..cut].trim().to_string());
        start = cut + 1; // 跳过切点那个连接空格
    }
    out.push(text[start..].trim().to_string());
    out.retain(|p| !p.is_empty());
    out
}

pub struct NoteSeg {
    pub id: String,
    pub open_lines: Vec<String>,   // 注释块各行（含 <!-- 与 -->）
    pub excerpt: Option<Vec<String>>, // 摘抄行（普通文本，一段一条）
    pub note: Vec<String>,         // 用户区原文行
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

/// 结构解析：把文本切成 Note 段与 Text 段。注释块之后紧接的**连续 `> ` 行**
/// = 摘抄行（一段一条，最后一条是位置/划线时间）；其后到下一个锚点（注释块 /
/// `## `）之间的行是用户区。
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
            // 摘抄行：块结束后（允许空行）连续的 `> ` 引用行。
            let mut k = j;
            while k < n && lines[k].trim().is_empty() {
                k += 1;
            }
            let mut excerpt: Vec<String> = Vec::new();
            while k < n && lines[k].starts_with("> ") {
                excerpt.push(lines[k].clone());
                k += 1;
            }
            let excerpt = (!excerpt.is_empty()).then_some(excerpt);
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
                    out.extend(excerpt.iter().cloned());
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
    let excerpt = excerpt_for(entry);
    Seg::Note(NoteSeg {
        id: entry.highlight.id.clone(),
        open_lines: entry.comment_lines(),
        excerpt: (!excerpt.is_empty()).then_some(excerpt),
        note: note_lines,
    })
}

/// 人读的摘抄行：一段一条 `> ` 引用，`> 【重点|摘抄】` 只挂在头一条上；最后一条
/// 固定是 `> （全书 N% · 划于 …）`。由记录本身生成（段落按 `paras` 摊回），所以
/// 它永远和块里的机器字段一致。时间显示为**本机时区**（块里的 `created` 才是
/// 权威的 unix 秒）。
fn excerpt_for(entry: &NoteEntry) -> Vec<String> {
    let h = &entry.highlight;
    if h.text.trim().is_empty() {
        return Vec::new();
    }
    let paras = excerpt_paragraphs(&h.text, &h.paras);
    let mut out: Vec<String> = Vec::new();
    let label = color_label(&h.color);
    for (i, para) in paras.iter().enumerate() {
        if i == 0 {
            out.push(format!("> 【{label}】{para}"));
        } else {
            out.push(format!("> {para}"));
        }
    }
    if out.is_empty() {
        // `paras` 全被宽容掉（例如 text 被外部改小）：整段当一段，别丢字。
        out.push(format!("> 【{label}】{}", single_line(&h.text)));
    }
    let mut bits = vec![pos_label(h.pos)];
    if h.created_at > 0 {
        bits.push(format!("划于 {}", human_stamp(h.created_at)));
    }
    out.push(format!("> （{}）", bits.join(" · ")));
    out
}

/// 将一条划线写入档案：`id` 已存在 → 原位替换保护区与笔记区（UI 保存 =
/// 用户最新意图，覆盖用户区）；不存在 → 归入 `section_title` 章（没有则
/// 在文末建章），插到该章内容之后。返回新全文。
pub fn upsert(text: &str, entry: &NoteEntry) -> String {
    let (meta, body) = split_meta(text);
    let updated = upsert_body(&body, entry);
    join_meta(&meta, &updated)
}

fn upsert_body(text: &str, entry: &NoteEntry) -> String {
    let mut segs = parse(text);
    if let Some(pos) = segs
        .iter()
        .position(|s| s.id() == Some(entry.highlight.id.as_str()))
    {
        segs[pos] = note_from_entry(entry);
        return serialize(&segs);
    }
    let note_seg = note_from_entry(entry);
    if let Some(sec) = find_section(&segs, &entry.section_title) {
        // 插到该章内最后一条 Note 之后（保持追加顺序），下一个 `## ` 章标题
        // 或文尾为止；前面不是空行时补一个分隔空行。
        let mut insert_at = sec + 1;
        for (k, seg) in segs.iter().enumerate().skip(sec + 1) {
            match seg {
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
                [Seg::Text(vec![String::new()]), note_seg],
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
/// `deleted_at` 是删除时间（unix 秒）；人类可读的「已删于 …」由调用方在
/// 摘抄行里给出。
pub fn mark_deleted(text: &str, id: &str, deleted_at: i64) -> Option<String> {
    let (meta, body) = split_meta(text);
    let updated = mark_deleted_body(&body, id, deleted_at)?;
    Some(join_meta(&meta, &updated))
}

fn mark_deleted_body(text: &str, id: &str, deleted_at: i64) -> Option<String> {
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
                *line = format!("deleted: {deleted_at}");
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
        note.open_lines.insert(insert_at, format!("deleted: {deleted_at}"));
    }
    // 摘抄行（可能多行）逐条加删除线，删除时间落在最后一条。
    if let Some(lines) = &mut note.excerpt {
        if !lines.is_empty() && !lines.iter().any(|l| l.contains("已删于")) {
            let human = human_stamp(deleted_at);
            let last = lines.len() - 1;
            for (i, line) in lines.iter_mut().enumerate() {
                let body = line.strip_prefix("> ").unwrap_or(line).to_string();
                *line = if i == last {
                    format!("> ~~{body}~~（已删于 {human}）")
                } else {
                    format!("> ~~{body}~~")
                };
            }
        }
    }
    Some(serialize(&segs))
}

/// unix 秒 → `YYYY-MM-DD HH:MM`，**本机时区**。用于摘抄行与「已删于 …」这类
/// 留痕文字；块里的 `created` / `deleted` 才是权威值，这里只负责给人看。
fn human_stamp(secs: i64) -> String {
    use chrono::{DateTime, Local};
    if secs <= 0 {
        return "未知时间".into();
    }
    DateTime::from_timestamp(secs, 0)
        .map(|d| d.with_timezone(&Local).format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| "未知时间".into())
}

/// 移除该条的注释块与摘抄行（纯划线删除：整条不留痕）。用户笔记区若被外部
/// 编辑器写过则转成普通文本留在原位（绝不丢字）；因此变空的章标题一并清掉。
/// id 不在档案里返回 `None`。
pub fn remove_note(text: &str, id: &str) -> Option<String> {
    let (meta, body) = split_meta(text);
    let updated = remove_note_body(&body, id)?;
    Some(join_meta(&meta, &updated))
}

fn remove_note_body(text: &str, id: &str) -> Option<String> {
    let mut segs = parse(text);
    let pos = segs.iter().position(|s| s.id() == Some(id))?;
    let Seg::Note(note) = segs.remove(pos) else {
        return None;
    };
    let mut orphan = note.note;
    if orphan.iter().all(|l| is_blank_line(l)) {
        // 没有用户文字：直接移除，不留下空段。
        return Some(serialize(&prune_empty_sections(segs)));
    }
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
    Some(serialize(&prune_empty_sections(segs)))
}

/// 回填全书位置：只重写该条的注释块与摘抄行，用户笔记区逐字保留。
/// id 不在档案里返回 `None`。
pub fn update_pos(text: &str, id: &str, entry: &NoteEntry) -> Option<String> {
    let (meta, body) = split_meta(text);
    let updated = update_pos_body(&body, id, entry)?;
    Some(join_meta(&meta, &updated))
}

fn update_pos_body(text: &str, id: &str, entry: &NoteEntry) -> Option<String> {
    let mut segs = parse(text);
    let pos = segs.iter().position(|s| s.id() == Some(id))?;
    let Seg::Note(note) = &mut segs[pos] else {
        return None;
    };
    note.open_lines = entry.comment_lines();
    let excerpt = excerpt_for(entry);
    note.excerpt = (!excerpt.is_empty()).then_some(excerpt);
    Some(serialize(&segs))
}

/// 清掉因此变空的自动建章标题（标题下到下一个 `## ` 或文尾之间既无 Note 也
/// 无用户非空文本）。用户自己写的非空内容会让章保留。
fn prune_empty_sections(mut segs: Vec<Seg>) -> Vec<Seg> {
    let mut i = 0;
    while i < segs.len() {
        let is_heading = matches!(&segs[i], Seg::Text(lines)
            if lines.first().is_some_and(|l| l.starts_with("## ")));
        if !is_heading {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        let mut filled = false;
        while j < segs.len() {
            match &segs[j] {
                Seg::Note(_) => {
                    filled = true;
                    break;
                }
                Seg::Text(t) => {
                    if t.first().is_some_and(|l| l.starts_with("## ")) {
                        break;
                    }
                    if t.iter().any(|l| !is_blank_line(l)) {
                        filled = true;
                        break;
                    }
                    j += 1;
                }
            }
        }
        if filled {
            i += 1;
            continue;
        }
        segs.remove(i);
        // 吃掉标题后连着的光秃空行段。
        while i < segs.len() {
            let blank =
                matches!(&segs[i], Seg::Text(t) if t.iter().all(|l| is_blank_line(l)));
            if blank {
                segs.remove(i);
            } else {
                break;
            }
        }
    }
    segs
}

/// 读出档案里每条划线的用户笔记（id → 笔记文本），供悬停浮层与列表。
pub fn notes_of(text: &str) -> Vec<(String, String)> {
    let (_, body) = split_meta(text);
    notes_of_body(&body)
}

fn notes_of_body(text: &str) -> Vec<(String, String)> {
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

/// 把 md 里所有划线块读成记录（含所属章标题）。缺 `id` 或坐标不完整的块跳过
/// ——那是手写/损坏的块，写回时原样保留（见 [`parse`]）。
pub fn stored_highlights(text: &str) -> Vec<StoredHighlight> {
    let (_, body) = split_meta(text);
    let segs = parse(&body);
    let mut out = Vec::new();
    let mut section = String::new();
    for seg in &segs {
        match seg {
            Seg::Text(lines) => {
                if let Some(head) = lines.first() {
                    if head.starts_with("## ") {
                        section = head.clone();
                    }
                }
            }
            Seg::Note(note) => {
                // 已删除的条目留在档案里留痕（`deleted:` + 删除线），但它
                // 不再是划线：读记录时跳过，否则删掉的划线会被重新画出来。
                if is_deleted(note) {
                    continue;
                }
                if let Some(stored) = stored_from_seg(note, &section) {
                    out.push(stored);
                }
            }
        }
    }
    out
}

/// 该条是否已被标记删除（保护区里 `deleted:` 有值）。
fn is_deleted(seg: &NoteSeg) -> bool {
    seg.open_lines.iter().any(|l| {
        l.trim()
            .strip_prefix("deleted:")
            .is_some_and(|v| !v.trim().is_empty())
    })
}

/// 一个划线块 → 记录。缺 `id` 或某个坐标字段不完整时返回 `None`。
fn stored_from_seg(seg: &NoteSeg, section_title: &str) -> Option<StoredHighlight> {
    let field = |key: &str| -> Option<String> {
        seg.open_lines.iter().find_map(|line| {
            let (k, v) = line.trim().split_once(':')?;
            (k.trim() == key).then(|| v.trim().to_string())
        })
    };
    let num = |key: &str| field(key).and_then(|v| v.parse::<usize>().ok());
    let pos = field("posPct")
        .filter(|v| !v.is_empty())
        .and_then(|v| v.parse::<f64>().ok())
        .map(|pct| pct / 100.0);
    let paras: Vec<usize> = field("paras")
        .map(|v| {
            v.split(',')
                .filter_map(|p| p.trim().parse::<usize>().ok())
                .collect()
        })
        .unwrap_or_default();
    Some(StoredHighlight {
        highlight: Highlight {
            id: seg.id.clone(),
            href: field("href").unwrap_or_default(),
            start_text: num("startText")?,
            start_offset: num("startOffset")?,
            end_text: num("endText")?,
            end_offset: num("endOffset")?,
            text: field("text").unwrap_or_default(),
            paras,
            color: field("color").unwrap_or_else(|| "yellow".into()),
            pos,
            created_at: field("created")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(0),
        },
        section_title: section_title.to_string(),
        note: seg.note.join("\n").trim().to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::annotations::{Highlight, COLOR_GREEN, COLOR_YELLOW};

    /// 一条带坐标的划线记录（测试用：摘录「摘录一」、章内有位置 34%）。
    fn hl(id: &str, text: &str) -> Highlight {
        Highlight {
            id: id.into(),
            href: "/EPUB/ch1.xhtml".into(),
            start_text: 3,
            start_offset: 5,
            end_text: 4,
            end_offset: 1,
            text: text.into(),
            paras: Vec::new(),
            color: COLOR_YELLOW.into(),
            pos: Some(0.34),
            created_at: 1_756_900_000,
        }
    }

    fn entry(id: &str, note: &str) -> NoteEntry {
        NoteEntry {
            highlight: hl(id, "摘录一"),
            section_title: "## 第 1 章 · 开头".into(),
            note: note.into(),
        }
    }

    /// 段落摊开的可读形式（用 `|` 分隔，便于断言）。
    fn paras_of(text: &str, cuts: &[usize]) -> String {
        excerpt_paragraphs(text, cuts).join("|")
    }

    #[test]
    fn color_and_pos_labels() {
        assert_eq!(color_label("green"), "摘抄");
        assert_eq!(color_label("yellow"), "重点");
        assert_eq!(color_label("unknown"), "重点");
        assert_eq!(pos_label(Some(0.0)), "全书 0%");
        assert_eq!(pos_label(Some(0.342)), "全书 34%");
        assert_eq!(pos_label(Some(0.995)), "全书 100%");
        assert_eq!(pos_label(Some(1.5)), "全书 100%");
        assert_eq!(pos_label(None), "位置未知");
    }

    #[test]
    fn empty_file_upsert_creates_section_and_entry() {
        let out = upsert("", &entry("a", "第一条笔记"));
        // 机器字段（坐标在内）全在保护区，人读的摘抄行紧跟其后。
        for line in [
            "id: a",
            "color: yellow",
            "posPct: 34",
            "href: /EPUB/ch1.xhtml",
            "startText: 3",
            "startOffset: 5",
            "endText: 4",
            "endOffset: 1",
            "text: 摘录一",
        ] {
            assert!(out.contains(line), "missing {line} in {out}");
        }
        // 没有段落切点时字段留空（不写一个假的 0）。
        assert!(out.contains("paras: \n"), "empty paras must stay blank: {out}");
        assert!(out.starts_with("## 第 1 章 · 开头\n\n<!-- icedreader-note\n"));
        // 摘抄行是 md 引用块（以 `>` 开头，md 软件里看得舒服）。
        assert!(out.lines().any(|l| l.starts_with("> 【重点】")));
        // 单段摘录 = 一条引用行 + 一条位置/时间行。
        assert_eq!(out.lines().filter(|l| l.starts_with("> ")).count(), 2);
        assert_eq!(
            notes_of(&out),
            vec![("a".to_string(), "第一条笔记".to_string())]
        );
    }

    /// 折成一行的摘录 + 段落切点：切点必须正好落在段落之间那个空格上，
    /// 摊回来一字不差。
    #[test]
    fn split_excerpt_folds_to_one_line_and_keeps_the_cuts() {
        let raw = "第一段。\n  第二段，行尾有空格  \n\n第三段。";
        let (text, paras) = split_excerpt(raw);
        assert_eq!(text, "第一段。 第二段，行尾有空格 第三段。");
        assert_eq!(paras.len(), 2, "cuts: {paras:?}");
        for &cut in &paras {
            assert_eq!(&text[cut..cut + 1], " ", "切点必须落在连接空格上");
        }
        assert_eq!(paras_of(&text, &paras), "第一段。|第二段，行尾有空格|第三段。");
        // 空选区 → 空串 + 无切点。
        assert_eq!(split_excerpt("   \n \n"), (String::new(), Vec::new()));
    }

    /// 手改过的 `text:` / `paras:`：越界、非字符边界、重复的切点一律忽略，
    /// 一个字都不丢，也不 panic。
    #[test]
    fn excerpt_paragraphs_tolerates_broken_cuts() {
        let text = "甲乙 丙丁";
        assert_eq!(paras_of(text, &[6]), "甲乙|丙丁");
        assert_eq!(paras_of(text, &[999, 0, 6, 6]), "甲乙|丙丁");
        assert_eq!(paras_of(text, &[1]), "甲乙 丙丁", "非字符边界 → 整段");
        assert_eq!(paras_of("单段", &[]), "单段");
        assert_eq!(paras_of("", &[3]), "");
    }

    /// 多段摘抄：md 里一段一条 `> ` 引用（标签只在第一条），末条是位置/时间；
    /// 位置回填（整条重写）之后段落结构照旧；删除时每条都加删除线。
    #[test]
    fn multi_paragraph_excerpt_roundtrips_and_survives_a_rewrite() {
        let (text, paras) = split_excerpt("一段。\n二段。\n三段。");
        let mut h = hl("a", &text);
        h.paras = paras.clone();
        let v1 = upsert(
            "",
            &NoteEntry {
                highlight: h.clone(),
                section_title: "## 第 1 章 · 开头".into(),
                note: String::new(),
            },
        );
        let cut_field = format!(
            "paras: {}",
            paras
                .iter()
                .map(|p| p.to_string())
                .collect::<Vec<_>>()
                .join(",")
        );
        assert!(v1.contains(&cut_field), "块里要记下切点：{v1}");
        let quote: Vec<&str> = v1.lines().filter(|l| l.starts_with("> ")).collect();
        assert_eq!(quote.len(), 4, "quote: {quote:?}");
        assert_eq!(quote[0], "> 【重点】一段。");
        assert_eq!(quote[1], "> 二段。");
        assert_eq!(quote[2], "> 三段。");
        assert!(quote[3].starts_with("> （全书 34% · 划于 "));

        // 读回来一致（含切点）。
        assert_eq!(stored_highlights(&v1)[0].highlight, h);

        // 位置回填 = 整条重写：段落结构必须原样回来。
        let mut moved = h.clone();
        moved.pos = Some(0.42);
        let out = update_pos(
            &v1,
            "a",
            &NoteEntry {
                highlight: moved,
                section_title: String::new(),
                note: String::new(),
            },
        )
        .unwrap();
        let quote2: Vec<&str> = out.lines().filter(|l| l.starts_with("> ")).collect();
        assert_eq!(quote2.len(), 4, "quote after pos backfill: {quote2:?}");
        assert_eq!(quote2[1], "> 二段。");
        assert!(out.contains("> （全书 42% · 划于 "));

        // 删除留痕：每条引用都加删除线，时间落在末条。
        let del = mark_deleted(&out, "a", 1_756_987_200).unwrap();
        assert!(del.contains("> ~~【重点】一段。~~"));
        assert!(del.contains("> ~~二段。~~"));
        assert!(del.contains("> ~~三段。~~"));
        assert!(del.contains("已删于 "));
    }

    /// 记录进得去也出得来：坐标/颜色/位置/摘录在块里往返一致。
    #[test]
    fn stored_highlights_roundtrip() {
        let mut h = hl("a", "摘录一");
        h.color = COLOR_GREEN.into();
        h.pos = None;
        let text = upsert(
            "",
            &NoteEntry {
                highlight: h.clone(),
                section_title: "## 第 1 章 · 开头".into(),
                note: "备注".into(),
            },
        );
        let stored = stored_highlights(&text);
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].highlight, h);
        assert_eq!(stored[0].section_title, "## 第 1 章 · 开头");
        assert_eq!(stored[0].note, "备注");
        // 位置未知不被写成 0。
        assert_eq!(stored[0].highlight.pos, None);
    }

    #[test]
    fn upsert_existing_id_replaces_block_and_note_in_place() {
        let v1 = upsert("", &entry("a", "旧笔记"));
        let mut e2 = entry("a", "新笔记（外部编辑器改过样式，UI 保存覆盖）");
        e2.highlight.color = COLOR_GREEN.into();
        let out = upsert(&v1, &e2);
        assert!(out.contains("color: green"));
        assert!(out.contains("> 【摘抄】摘录一"));
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
        assert!(out.starts_with(preamble), "文件头必须原样保留");
        assert!(out.contains("## 第 2 章 · 另一章"));
        assert!(out.contains("笔记 b"));
        assert!(out.contains("笔记 a"));
    }

    /// 元数据块在文件顶部时，改划线不碰它一个字节。
    #[test]
    fn metadata_block_is_untouched_by_highlight_writes() {
        let meta = "<!-- icedreader-meta\ntitle: 三体\nauthor: 刘慈欣\n-->\n";
        let with_meta = format!("{meta}\n{}", upsert("", &entry("a", "笔记 a")));
        let out = upsert(&with_meta, &entry("b", "笔记 b"));
        assert!(out.starts_with(meta), "元数据块必须原样保留");
        assert!(out.contains("笔记 a") && out.contains("笔记 b"));
        let (block, _body) = split_meta(&out);
        assert_eq!(block, meta);
        // Both blocks come back, still filed under their chapter heading — the
        // archive is grouped by `## 第 N 章 · …`, and the metadata block above
        // never swallows that section.
        let stored = stored_highlights(&out);
        assert_eq!(stored.len(), 2);
        assert!(stored
            .iter()
            .all(|s| s.section_title == "## 第 1 章 · 开头"));
    }

    #[test]
    fn mark_deleted_keeps_note_and_annotates() {
        let v1 = upsert("", &entry("a", "用户笔记内容\n第二行"));
        // 2026-09-05T16:00:00Z 附近的秒数：只要是个正的 unix 秒即可。
        let out = mark_deleted(&v1, "a", 1_756_987_200).unwrap();
        assert!(out.contains("deleted: 1756987200"));
        assert!(out.contains("（已删于 "), "删除线要带删除时间");
        assert!(out.contains("> ~~【重点】摘录一"));
        assert!(out.contains("用户笔记内容\n第二行"));
        assert_eq!(
            notes_of(&out),
            vec![("a".to_string(), "用户笔记内容\n第二行".to_string())]
        );
        // 幂等：再次调用返回 None，内容不变。
        assert!(mark_deleted(&out, "a", 1).is_none());
        // 不在档案里的 id → None。
        assert!(mark_deleted(&v1, "missing", 1).is_none());
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
    fn plain_highlight_delete_leaves_no_trace() {
        // 没写备注的划线删除 = 整条移除；自动建的章标题也一并清掉。
        let v1 = upsert("", &entry("a", ""));
        assert!(v1.contains("## 第 1 章"));
        assert!(v1.contains("> 【重点】"));
        let out = remove_note(&v1, "a").unwrap();
        assert_eq!(out, "", "md 无痕（连章标题都不剩）");
    }

    #[test]
    fn plain_delete_keeps_section_with_other_entries() {
        let v1 = upsert("", &entry("a", ""));
        let v2 = upsert(&v1, &entry("b", ""));
        let out = remove_note(&v2, "a").unwrap();
        assert!(out.contains("## 第 1 章"), "还有 b，章标题保留");
        assert!(!out.contains("id: a"));
        assert!(out.contains("id: b"));
        assert_eq!(serialize(&parse(&out)), out);
    }

    #[test]
    fn clearing_note_keeps_block_and_excerpt() {
        let v1 = upsert("", &entry("a", "旧备注"));
        // 清空备注 = 用空用户区覆盖，保护区与摘抄行留下（划线还在）。
        let out = upsert(&v1, &entry("a", ""));
        assert!(out.contains("id: a"));
        assert!(out.contains("> 【重点】摘录一"));
        assert!(!out.contains("旧备注"));
        assert!(notes_of(&out).is_empty());
    }

    #[test]
    fn update_pos_rewrites_block_and_keeps_user_note() {
        let v1 = upsert("", &entry("a", "我的备注"));
        let mut moved = entry("a", "我的备注");
        moved.highlight.pos = Some(0.42);
        let out = update_pos(&v1, "a", &moved).unwrap();
        assert!(out.contains("posPct: 42"));
        assert!(out.contains("全书 42%"));
        assert!(out.contains("我的备注"), "用户区逐字保留");
        // 不存在的 id → None。
        assert!(update_pos(&v1, "missing", &moved).is_none());
    }

    #[test]
    fn orphan_handwritten_content_and_comment_without_id_survive() {
        let hand = "# 我的手动笔记\n\n完全手写的一段，没有注释块。\n";
        let v1 = upsert("", &entry("a", "a 的笔记"));
        let mixed = format!("{hand}{v1}");
        // 用户区里出现顶层 `## `（虽不鼓励）应被切断保护，内容仍在。
        let with_head = upsert(&mixed, &entry("b", "b 的笔记\n\n## 我自己小节\n\n正文内容"));
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
        assert!(parsed
            .iter()
            .any(|(id, n)| id == "b" && n == "笔记 b 第二行"));
        // 重解析再序列化应保持结构稳定（解析不引入漂移）。
        assert_eq!(serialize(&parse(&text)), text);
    }

    /// 缺 id 的手写块原样保留（读的时候跳过，写的时候不碰）。
    #[test]
    fn comment_block_without_id_is_left_alone() {
        let v1 = upsert("", &entry("a", "笔记 a"));
        let hand = format!("{v1}\n<!-- icedreader-note\ncolor: yellow\n-->\n");
        let out = upsert(&hand, &entry("b", "笔记 b"));
        assert!(out.contains("<!-- icedreader-note\ncolor: yellow\n-->"));
        assert_eq!(stored_highlights(&out).len(), 2);
    }
}
