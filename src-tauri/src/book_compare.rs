//! 同书对照：判关系 → 选轴 → 给倾向。
//!
//! The *relative* layer. A book's own 优/良/中 and its 依据 stay purely
//! self-contained (`book_signals::grade`); this module only ever runs when the
//! shelf has already judged two or more files to be the same work, and it
//! answers the one question a per-book badge cannot: **which copy should I
//! keep?**
//!
//! Rules that keep it honest:
//! - Never a weighted total. Each axis is judged on its own and named in the
//!   conclusion, so the user can audit the lean (`docs/ideas/book-compare.md`).
//! - Near-misses are ties, and ties are shown rather than swallowed.
//! - Differences are split into 正文内容 / 书内装置 / 打包噪声; only the last
//!   group's leftovers are judged as defects — cover art is packaging, not a
//!   merit, but not a fault either.
//! - Nothing here deletes or merges. The panel states a lean; removal stays a
//!   user-confirmed action on the shelf.
//!
//! Design and the measured evidence: `docs/ideas/book-compare.md`.

use serde::Serialize;

use crate::book_signals::{self, BookSignals};

/// What these files are to each other. Order matters: the first relationship
/// that holds for every pair wins (see [`classify`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RelationKind {
    /// Every chapter's text is byte-identical: one typesetting source, packed
    /// twice. The only real differences left are packaging.
    SameTypesetting,
    /// Same 回目, but the text actually differs — a re-edited or re-typeset
    /// edition. Which one is "better" is not decidable from counts.
    SameEdition,
    /// One 回目 sequence is a proper subset of the other: a full version next
    /// to an abridged/selected one (or a volume next to its collection).
    Contained,
    /// Disjoint 回目 sets: different volumes of one set.
    Partition,
    /// 回目 don't line up. Either not the same work, or the grouping rule was
    /// fooled — worth showing as such rather than inventing a comparison.
    Unrelated,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AxisGroup {
    /// The text itself.
    Content,
    /// What the book carries besides the text: notes, plates.
    Apparatus,
    /// How the file was packed: covers, leftovers, size.
    Packaging,
    /// Provenance: identifier, author.
    Provenance,
}

/// Outcome of one axis. `Presented` is a measured number that must not be
/// scored (e.g. text length — an abridged edition is not "worse" for being
/// shorter), and `Incomparable` means the inputs lacked the measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Verdict {
    Tie,
    Winner,
    Shared,
    Incomparable,
    Presented,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CellMark {
    Best,
    Worst,
    Tie,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Cell {
    /// Human-readable value: "39 张", "2.7 MB", "有".
    pub display: String,
    /// Raw value for sorting / bars. `None` when the axis carries no number.
    pub num: Option<f64>,
    pub mark: CellMark,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Axis {
    pub key: &'static str,
    pub label: &'static str,
    pub group: AxisGroup,
    /// How the number was measured — shown under the row.
    pub note: Option<String>,
    pub cells: Vec<Cell>,
    pub verdict: Verdict,
    /// Indices that won this axis. Empty unless `verdict` is `Winner`/`Shared`.
    pub winners: Vec<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Column {
    pub file_name: String,
    pub title: String,
    /// The book's own 优/良/中 (independent of this comparison).
    pub quality: Option<String>,
    pub size_bytes: u64,
}

/// Per-chapter difference between exactly two copies, for the strip view.
/// Only produced when both have the same 回目 and the same chapter count.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterDiff {
    /// 回目 of each unit (may contain empty strings for untitled units).
    pub labels: Vec<String>,
    /// `chars[b] - chars[a]`; positive means the second copy has more text.
    pub deltas: Vec<i64>,
    /// Chapter text is byte-identical after whitespace normalisation.
    pub identical: Vec<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Comparison {
    pub kind: RelationKind,
    /// One sentence saying which judgement produced `kind`.
    pub kind_note: String,
    pub columns: Vec<Column>,
    pub axes: Vec<Axis>,
    pub chapter_diff: Option<ChapterDiff>,
    /// The copy this comparison leans towards, when exactly one copy wins and
    /// no other copy wins anything. Returned as an index rather than written
    /// into a sentence, because two copies can carry the *same* title and only
    /// the UI knows which label disambiguates them.
    pub lean: Option<usize>,
    /// Why that copy won: one line per axis it took.
    pub lean_reason: Vec<String>,
    /// Everything else worth saying: the relation, ties, splits, caveats.
    pub conclusion: Vec<String>,
}

/// One book's inputs. `signals` is the same cache the shelf grades from.
#[derive(Debug, Clone)]
pub struct CompareInput {
    pub file_name: String,
    pub title: String,
    pub quality: Option<String>,
    pub size_bytes: u64,
    pub signals: BookSignals,
}

// ---------------------------------------------------------------- relations

fn toc_key(s: &BookSignals) -> Vec<String> {
    s.headings
        .iter()
        .map(|h| h.trim().to_string())
        .filter(|h| !h.is_empty())
        .collect()
}

fn same_typesetting(a: &BookSignals, b: &BookSignals) -> bool {
    !a.chapter_shas.is_empty() && a.chapter_shas == b.chapter_shas
}

/// Sorted multiset containment: every heading of `a` appears in `b`.
fn subset_of(a: &[String], b: &[String]) -> bool {
    let mut b_sorted: Vec<&String> = b.iter().collect();
    b_sorted.sort();
    a.iter().all(|h| {
        b_sorted
            .binary_search_by(|probe| probe.as_str().cmp(h.as_str()))
            .is_ok()
    })
}

fn disjoint(a: &[String], b: &[String]) -> bool {
    !a.iter().any(|h| b.contains(h))
}

/// Classify the whole set. Written for the common 2-copy case and degrading
/// sensibly for more: a relationship is only claimed when it holds for *every*
/// pair.
///
/// Note on reachability: today's shelf grouping only pairs books with equal
/// chapter-text fingerprints or equal 回目, so `SameTypesetting` / `SameEdition`
/// are what production actually produces. `Contained` / `Partition` /
/// `Unrelated` are here because the command takes arbitrary names (a future
/// grouping rule, or a user picking two files) and guessing "these are the same
/// book" from partial agreement would be worse than saying so.
fn classify(sigs: &[&BookSignals]) -> (RelationKind, String) {
    let tocs: Vec<Vec<String>> = sigs.iter().map(|s| toc_key(s)).collect();

    let all_typeset = sigs
        .iter()
        .enumerate()
        .all(|(i, s)| sigs.iter().skip(i + 1).all(|o| same_typesetting(s, o)));
    if all_typeset {
        let packaging = sigs
            .iter()
            .any(|s| s.img_files != sigs[0].img_files || s.img_bytes != sigs[0].img_bytes);
        let note = if packaging {
            "逐章正文完全一致，但打包不同（图集/封面有差）——同一排版源的两次打包"
        } else {
            "逐章正文完全一致——同一排版源的两次打包"
        };
        return (RelationKind::SameTypesetting, note.to_string());
    }

    if tocs.windows(2).all(|w| w[0] == w[1]) {
        return (
            RelationKind::SameEdition,
            "回目序列一致，但正文有真实差异——同书异版".to_string(),
        );
    }

    let pair_overlaps_without_nesting =
        |a: &Vec<String>, b: &Vec<String>| !disjoint(a, b) && !subset_of(a, b) && !subset_of(b, a);
    let any_unrelated = tocs.iter().enumerate().any(|(i, a)| {
        tocs.iter()
            .skip(i + 1)
            .any(|b| pair_overlaps_without_nesting(a, b))
    });

    if !any_unrelated {
        if tocs.iter().enumerate().any(|(i, a)| {
            tocs.iter()
                .skip(i + 1)
                .any(|b| subset_of(a, b) || subset_of(b, a))
        }) {
            return (
                RelationKind::Contained,
                "一本的回目是另一本的子集——完整版与删节/选编（或分卷与合集）".to_string(),
            );
        }
        // Disjoint headings only mean "different volumes of one set" when a set
        // is actually present. Two books can be disjoint and unrelated.
        if tocs.len() >= 3
            && tocs
                .iter()
                .enumerate()
                .all(|(i, a)| tocs.iter().skip(i + 1).all(|b| disjoint(a, b)))
        {
            return (
                RelationKind::Partition,
                "回目互不相交——同一套书的不同分卷".to_string(),
            );
        }
    }

    (
        RelationKind::Unrelated,
        "回目对不上——要么不是同一本，要么同书分组被误判".to_string(),
    )
}

// ------------------------------------------------------------------- judge

#[derive(Debug, Clone, Copy, PartialEq)]
enum Direction {
    Higher,
    Lower,
    /// Measured and shown, deliberately not scored.
    Presented,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Tolerance {
    Exact,
    Absolute(f64),
    Fraction(f64),
}

impl Tolerance {
    fn at(&self, best: f64) -> f64 {
        match *self {
            Tolerance::Exact => 0.0,
            Tolerance::Absolute(a) => a,
            Tolerance::Fraction(f) => f * best.abs(),
        }
    }
}

/// Build one axis from already-extracted values. `values.len()` must equal the
/// number of columns; missing inputs are handled by the caller (skip the axis).
// A compact builder called from ten fixed sites; a spec struct would only add
// boilerplate without naming anything the call sites do not already show.
#[allow(clippy::too_many_arguments)]
fn axis(
    key: &'static str,
    label: &'static str,
    group: AxisGroup,
    note: Option<&str>,
    values: &[f64],
    displays: Vec<String>,
    direction: Direction,
    tolerance: Tolerance,
) -> Axis {
    let note = note.map(str::to_string);

    if direction == Direction::Presented {
        return Axis {
            key,
            label,
            group,
            note,
            cells: values
                .iter()
                .zip(displays)
                .map(|(v, display)| Cell {
                    display,
                    num: Some(*v),
                    mark: CellMark::Tie,
                })
                .collect(),
            verdict: Verdict::Presented,
            winners: Vec::new(),
        };
    }

    let best = match direction {
        Direction::Higher => values.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
        _ => values.iter().cloned().fold(f64::INFINITY, f64::min),
    };
    let tol = tolerance.at(best);
    // "Close enough" counts as a tie, so 46 vs 44 stops reading as a verdict.
    let winners: Vec<usize> = values
        .iter()
        .enumerate()
        .filter(|(_, v)| (**v - best).abs() <= tol)
        .map(|(i, _)| i)
        .collect();

    let (verdict, best_set) = if winners.len() == values.len() {
        (Verdict::Tie, Vec::new())
    } else if winners.len() == 1 {
        (Verdict::Winner, winners.clone())
    } else {
        (Verdict::Shared, winners.clone())
    };

    let worst = match direction {
        Direction::Higher => values.iter().cloned().fold(f64::INFINITY, f64::min),
        _ => values.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
    };
    let worst_set: Vec<usize> = values
        .iter()
        .enumerate()
        .filter(|(_, v)| (**v - worst).abs() <= tol)
        .map(|(i, _)| i)
        .collect();

    Axis {
        key,
        label,
        group,
        note,
        cells: values
            .iter()
            .enumerate()
            .zip(displays)
            .map(|((i, _), display)| Cell {
                display,
                num: Some(values[i]),
                mark: match verdict {
                    Verdict::Tie => CellMark::Tie,
                    _ if best_set.contains(&i) => CellMark::Best,
                    _ if worst_set.contains(&i) => CellMark::Worst,
                    _ => CellMark::Tie,
                },
            })
            .collect(),
        verdict,
        winners: best_set,
    }
}

// -------------------------------------------------------------- formatting

fn fmt_bytes(b: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    if b as f64 >= MB {
        format!("{:.1} MB", b as f64 / MB)
    } else {
        format!("{} KB", (b as f64 / 1024.0).round() as u64)
    }
}

fn fmt_count(n: f64) -> String {
    if n >= 10_000.0 {
        format!("{:.1} 万", n / 10_000.0)
    } else {
        format!("{}", n as i64)
    }
}

fn id_label(q: book_signals::IdQuality) -> &'static str {
    use book_signals::IdQuality::*;
    match q {
        Isbn => "ISBN",
        Asin => "商店编号",
        Other => "内部编号",
        RandomUuid => "随机 UUID",
        None => "无",
    }
}

/// Bytes of images the text does **not** reference but a stylesheet does (cover
/// art). `image_stats` splits every image three ways, so this is the remainder.
fn cover_bytes(s: &BookSignals) -> u64 {
    s.img_bytes
        .saturating_sub(s.img_referenced_bytes)
        .saturating_sub(s.img_orphan_bytes)
}

/// True when the image reference breakdown is present **and trustworthy**: a
/// pre-kind-3 cache has no breakdown at all, and a truncated scan (too many /
/// too-large images) leaves the referenced/orphan split partial. Either way the
/// per-copy numbers must not be scored — a missing measurement would otherwise
/// read as zero and "win".
fn has_breakdown(s: &BookSignals) -> bool {
    s.analysis_kind >= book_signals::REFERENCED_IMAGE_KIND
        && !s.img_truncated
        && !s.img_refs_truncated
}

/// Apparatus weight: 词注 if the book has them, else 上标注文.
fn note_weight(s: &BookSignals) -> u64 {
    if s.word_notes > 0 {
        s.word_notes
    } else if s.analysis_kind >= 2 {
        s.sup_count
    } else {
        0
    }
}

// ----------------------------------------------------------------- compare

/// Compare two or more copies of one work. Callers must have established that
/// they are copies (the shelf's 同书 grouping); this does not re-litigate that.
pub fn compare(inputs: &[CompareInput]) -> crate::error::Result<Comparison> {
    if inputs.len() < 2 {
        return Err("至少需要两本才能对照".into());
    }
    let sigs: Vec<&BookSignals> = inputs.iter().map(|i| &i.signals).collect();
    let (kind, kind_note) = classify(&sigs);

    let mut axes = Vec::new();
    content_axes(&mut axes, inputs, kind);
    apparatus_axes(&mut axes, inputs);
    packaging_axes(&mut axes, inputs);
    provenance_axes(&mut axes, inputs);

    let chapter_diff = if inputs.len() == 2 {
        chapter_diff(sigs[0], sigs[1])
    } else {
        None
    };

    let columns = inputs
        .iter()
        .map(|i| Column {
            file_name: i.file_name.clone(),
            title: i.title.clone(),
            quality: i.quality.clone(),
            size_bytes: i.size_bytes,
        })
        .collect();

    let (lean, lean_reason, conclusion) = conclude(inputs, &axes, kind, &kind_note);

    Ok(Comparison {
        kind,
        kind_note,
        columns,
        axes,
        chapter_diff,
        lean,
        lean_reason,
        conclusion,
    })
}

fn content_axes(axes: &mut Vec<Axis>, inputs: &[CompareInput], kind: RelationKind) {
    if kind == RelationKind::SameTypesetting {
        // The whole content group collapses to one honest sentence.
        axes.push(Axis {
            key: "text",
            label: "正文",
            group: AxisGroup::Content,
            note: Some("逐章去标签、去空白后比对".to_string()),
            cells: inputs
                .iter()
                .map(|_| Cell {
                    display: "逐字一致".to_string(),
                    num: None,
                    mark: CellMark::Tie,
                })
                .collect(),
            verdict: Verdict::Tie,
            winners: Vec::new(),
        });
    } else {
        // An abridged edition is shorter, not worse — measure, do not score.
        axes.push(axis(
            "text",
            "正文字数",
            AxisGroup::Content,
            Some("去空白后的正文字符数（长短不等于优劣）"),
            &inputs
                .iter()
                .map(|i| i.signals.chars as f64)
                .collect::<Vec<_>>(),
            inputs
                .iter()
                .map(|i| fmt_count(i.signals.chars as f64))
                .collect(),
            Direction::Presented,
            Tolerance::Exact,
        ));
    }

    let mojibake: Vec<f64> = inputs.iter().map(|i| i.signals.mojibake as f64).collect();
    if mojibake.iter().any(|v| *v > 0.0) {
        axes.push(axis(
            "mojibake",
            "乱码",
            AxisGroup::Content,
            Some("正文里的替换符 � 数量，越少越好"),
            &mojibake,
            mojibake.iter().map(|v| fmt_count(*v)).collect(),
            Direction::Lower,
            Tolerance::Exact,
        ));
    }

    let missing: Vec<f64> = inputs
        .iter()
        .map(|i| i.signals.missing_chars as f64)
        .collect();
    if missing.iter().any(|v| *v > 0.0) {
        axes.push(axis(
            "missing_chars",
            "缺字占位",
            AxisGroup::Content,
            Some("正文里的 □ 数量，越少越好"),
            &missing,
            missing.iter().map(|v| fmt_count(*v)).collect(),
            Direction::Lower,
            Tolerance::Exact,
        ));
    }
}

fn apparatus_axes(axes: &mut Vec<Axis>, inputs: &[CompareInput]) {
    let notes: Vec<f64> = inputs
        .iter()
        .map(|i| note_weight(&i.signals) as f64)
        .collect();
    axes.push(axis(
        "notes",
        "词注 / 注文",
        AxisGroup::Apparatus,
        Some("词注条数；没有词注时用上标注文条数（±5% 以内算平）"),
        &notes,
        notes.iter().map(|v| fmt_count(*v)).collect(),
        Direction::Higher,
        // Absolute counts do not scale: 1000 notes vs 1010 is a tie, but so
        // must be 20000 vs 20200.
        Tolerance::Fraction(0.05),
    ));

    let plates: Vec<f64> = inputs
        .iter()
        .map(|i| book_signals::plate_counts(&i.signals).1 as f64)
        .collect();
    axes.push(axis(
        "plates",
        "正文插图",
        AxisGroup::Apparatus,
        Some("正文引用且 ≥20KB 的图（封面与包内未引用的图不计）"),
        &plates,
        plates.iter().map(|v| format!("{} 张", *v as u64)).collect(),
        Direction::Higher,
        Tolerance::Absolute(2.0),
    ));
}

/// An "incomparable" packaging axis: one copy's measurement is missing (old
/// cache) or partial (scan truncated). Show what the measured copies have,
/// "—" for the rest, and mark every cell `Unknown` so nothing reads as a
/// verdict. Built from partial data, never scored.
fn incomparable_axis(
    key: &'static str,
    label: &'static str,
    note: &str,
    inputs: &[CompareInput],
    display: impl Fn(&BookSignals) -> String,
) -> Axis {
    Axis {
        key,
        label,
        group: AxisGroup::Packaging,
        note: Some(note.to_string()),
        cells: inputs
            .iter()
            .map(|i| {
                let text = if has_breakdown(&i.signals) {
                    display(&i.signals)
                } else {
                    "—".to_string()
                };
                Cell {
                    display: text,
                    num: None,
                    mark: CellMark::Unknown,
                }
            })
            .collect(),
        verdict: Verdict::Incomparable,
        winners: Vec::new(),
    }
}

fn packaging_axes(axes: &mut Vec<Axis>, inputs: &[CompareInput]) {
    // A copy whose image scan was truncated or whose cache predates the
    // reference breakdown has partial / zeroed orphan & cover numbers; scoring
    // those would let a missing measurement "win". Only judge when every copy
    // is measured, else say "can't compare" (silent when none is).
    let measured = inputs.iter().filter(|i| has_breakdown(&i.signals)).count();

    if measured == inputs.len() {
        let orphan_bytes: Vec<f64> = inputs
            .iter()
            .map(|i| i.signals.img_orphan_bytes as f64)
            .collect();
        axes.push(axis(
            "orphan_images",
            "包内残留图",
            AxisGroup::Packaging,
            Some("没有任何文档引用的图（换掉的旧封面、推广图）——不是长处"),
            &orphan_bytes,
            inputs
                .iter()
                .map(|i| {
                    format!(
                        "{} 张 · {}",
                        i.signals.img_orphan,
                        fmt_bytes(i.signals.img_orphan_bytes)
                    )
                })
                .collect(),
            Direction::Lower,
            // Under 64KB is packaging noise; above it is a real difference.
            Tolerance::Absolute(64.0 * 1024.0),
        ));
    } else if measured > 0 {
        axes.push(incomparable_axis(
            "orphan_images",
            "包内残留图",
            "没有任何文档引用的图（换掉的旧封面、推广图）——不是长处；有一本的图像测量不可用（旧版缓存或图像过多被截断）",
            inputs,
            |s| format!("{} 张 · {}", s.img_orphan, fmt_bytes(s.img_orphan_bytes)),
        ));
    }

    // Cover art is a packaging choice, not a merit — show it, never score it.
    if measured == inputs.len() {
        let covers: Vec<f64> = inputs
            .iter()
            .map(|i| cover_bytes(&i.signals) as f64)
            .collect();
        axes.push(axis(
            "cover_bytes",
            "封面 / 装帧图",
            AxisGroup::Packaging,
            Some("只被样式表引用的图；大小是打包选择，不判优劣"),
            &covers,
            covers.iter().map(|v| fmt_bytes(*v as u64)).collect(),
            Direction::Presented,
            Tolerance::Exact,
        ));
    } else if measured > 0 {
        axes.push(incomparable_axis(
            "cover_bytes",
            "封面 / 装帧图",
            "只被样式表引用的图；有一本的图像测量不可用（旧版缓存或图像过多被截断），无法比较",
            inputs,
            |s| fmt_bytes(cover_bytes(s)),
        ));
    }

    let sizes: Vec<f64> = inputs.iter().map(|i| i.size_bytes as f64).collect();
    axes.push(axis(
        "payload",
        "文件体积",
        AxisGroup::Packaging,
        Some("整个 epub 的字节数，用来解释体积差从哪来"),
        &sizes,
        sizes.iter().map(|v| fmt_bytes(*v as u64)).collect(),
        Direction::Presented,
        Tolerance::Exact,
    ));
}

fn provenance_axes(axes: &mut Vec<Axis>, inputs: &[CompareInput]) {
    let ids: Vec<f64> = inputs
        .iter()
        .map(|i| i.signals.id_quality.rank() as f64)
        .collect();
    if ids.iter().any(|v| *v != ids[0]) {
        axes.push(axis(
            "identifier",
            "标识符",
            AxisGroup::Provenance,
            Some("ISBN > 商店编号 > 内部编号 > 随机 UUID > 无"),
            &ids,
            inputs
                .iter()
                .map(|i| id_label(i.signals.id_quality).to_string())
                .collect(),
            Direction::Higher,
            Tolerance::Exact,
        ));
    }

    let creators: Vec<f64> = inputs
        .iter()
        .map(|i| if i.signals.has_creator { 1.0 } else { 0.0 })
        .collect();
    if creators.iter().any(|v| *v != creators[0]) {
        axes.push(axis(
            "creator",
            "作者 / 整理者",
            AxisGroup::Provenance,
            None,
            &creators,
            creators
                .iter()
                .map(|v| if *v > 0.0 { "有" } else { "无" }.to_string())
                .collect(),
            Direction::Higher,
            Tolerance::Exact,
        ));
    }
}

/// Per-chapter table for the strip view. Needs the same 回目 and the same number
/// of units; anything else and the alignment would be a guess.
///
/// `chapter_shas` is per **unique file** (a repeated file is hashed once) while
/// `chapter_chars` is per **spine unit**. They only line up when every spine
/// unit is its own file; a TOC-as-chapters book slices one file into many
/// units, so zipping the two would mislabel the strip (F: 天津往事). Refuse
/// rather than show a wrong alignment.
fn chapter_diff(a: &BookSignals, b: &BookSignals) -> Option<ChapterDiff> {
    if a.chapter_shas.len() != b.chapter_shas.len()
        || a.chapter_chars.len() != b.chapter_chars.len()
        || a.chapter_shas.len() != a.chapter_chars.len()
        || a.chapter_shas.is_empty()
        || toc_key(a) != toc_key(b)
    {
        return None;
    }
    Some(ChapterDiff {
        labels: a.headings.clone(),
        deltas: a
            .chapter_chars
            .iter()
            .zip(&b.chapter_chars)
            .map(|(x, y)| *y as i64 - *x as i64)
            .collect(),
        identical: a
            .chapter_shas
            .iter()
            .zip(&b.chapter_shas)
            .map(|(x, y)| x == y)
            .collect(),
    })
}

/// Returns `(lean, lean_reason, notes)`. The lean is only stated when exactly
/// one copy wins and no other copy wins anything — a split is reported as a
/// split, never averaged away.
fn conclude(
    inputs: &[CompareInput],
    axes: &[Axis],
    kind: RelationKind,
    kind_note: &str,
) -> (Option<usize>, Vec<String>, Vec<String>) {
    let mut notes = vec![kind_note.to_string()];
    let mut lean = None;
    let mut lean_reason = Vec::new();

    let judged: Vec<&Axis> = axes
        .iter()
        .filter(|a| matches!(a.verdict, Verdict::Winner | Verdict::Shared))
        .collect();

    if judged.is_empty() {
        notes.push("所有可判的轴都打平，没有足以分出高下的差异。".to_string());
    } else {
        let mut wins: Vec<Vec<&Axis>> = vec![Vec::new(); inputs.len()];
        for a in &judged {
            for i in &a.winners {
                wins[*i].push(a);
            }
        }
        let winners: Vec<usize> = (0..inputs.len()).filter(|i| !wins[*i].is_empty()).collect();

        if winners.len() == 1 {
            let i = winners[0];
            lean = Some(i);
            lean_reason = wins[i]
                .iter()
                .map(|a| {
                    let others: Vec<String> = a
                        .cells
                        .iter()
                        .enumerate()
                        .filter(|(j, _)| *j != i)
                        .map(|(_, c)| c.display.clone())
                        .collect();
                    format!(
                        "{} {}（另 {}）",
                        a.label,
                        a.cells[i].display,
                        others.join(" / ")
                    )
                })
                .collect();
        } else {
            // Identified by file name: two copies may share a title.
            let mut parts = Vec::new();
            for i in &winners {
                let named: Vec<String> = wins[*i].iter().map(|a| a.label.to_string()).collect();
                parts.push(format!("「{}」{}", inputs[*i].file_name, named.join("、")));
            }
            notes.push(format!("各有长短，没有单一赢家：{}。", parts.join("；")));
        }
    }

    // This caveat is the point of the comparison, so it must survive the
    // "everything else ties" case — which is exactly when it matters most.
    if kind == RelationKind::SameEdition {
        notes.push("两本正文确实不同，留哪本取决于你想要哪个版本。".to_string());
    }

    (lean, lean_reason, notes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::book_signals::{IdQuality, ANALYSIS_KIND};

    fn signals(shas: &[&str], headings: &[&str], chapter_chars: &[u64], chars: u64) -> BookSignals {
        BookSignals {
            rev: "r".into(),
            chars,
            chapter_shas: shas.iter().map(|s| s.to_string()).collect(),
            chapter_chars: chapter_chars.to_vec(),
            chapter_chars_kind: book_signals::CHAPTER_CHARS_PER_SPINE,
            fingerprint: "fp".into(),
            mojibake: 0,
            br_count: 0,
            empty_p: 0,
            img_count: 0,
            headings: headings.iter().map(|s| s.to_string()).collect(),
            id_quality: IdQuality::Other,
            has_creator: true,
            img_files: 0,
            img_bytes: 0,
            img_truncated: false,
            img_substantial: 0,
            img_referenced: 0,
            img_referenced_substantial: 0,
            img_referenced_bytes: 0,
            img_css_only: 0,
            img_orphan: 0,
            img_orphan_bytes: 0,
            img_refs_truncated: false,
            word_notes: 0,
            missing_chars: 0,
            sup_count: 0,
            analysis_kind: ANALYSIS_KIND,
            pdf: None,
        }
    }

    fn input(name: &str, size: u64, signals: BookSignals) -> CompareInput {
        CompareInput {
            file_name: name.to_string(),
            title: name.to_string(),
            quality: Some("优".into()),
            size_bytes: size,
            signals,
        }
    }

    fn axis_named<'a>(c: &'a Comparison, key: &str) -> &'a Axis {
        c.axes.iter().find(|a| a.key == key).expect(key)
    }

    /// The measured sample: identical text, one copy carrying 2 leftover images
    /// worth ~1.7 MB. The panel must point at the smaller one.
    #[test]
    fn leftover_images_decide_between_two_identical_packings() {
        let mut clean = signals(&["a", "b"], &["第一回", "第二回"], &[100, 200], 300);
        clean.img_bytes = 14_432_056;
        clean.img_files = 58;
        clean.img_referenced = 53;
        clean.img_referenced_substantial = 39;
        clean.img_referenced_bytes = 13_415_656;
        clean.img_css_only = 1;
        clean.img_orphan = 4;
        clean.img_orphan_bytes = 1_016_400;

        let mut padded = clean.clone();
        padded.img_bytes = 19_788_216;
        padded.img_files = 60;
        padded.img_orphan = 6;
        padded.img_orphan_bytes = 2_774_595;

        let c = compare(&[
            input("27.3 M.epub", 28_705_228, clean),
            input("34.2 M.epub", 34_054_305, padded),
        ])
        .unwrap();

        assert_eq!(c.kind, RelationKind::SameTypesetting);
        assert!(c.kind_note.contains("打包不同"), "{}", c.kind_note);
        assert_eq!(axis_named(&c, "text").verdict, Verdict::Tie);
        // Same plates on both sides — content is not what differs.
        assert_eq!(axis_named(&c, "plates").verdict, Verdict::Tie);
        let orphan = axis_named(&c, "orphan_images");
        assert_eq!(orphan.verdict, Verdict::Winner);
        assert_eq!(orphan.winners, vec![0]);
        assert_eq!(c.lean, Some(0));
        assert!(
            c.lean_reason.iter().any(|r| r.contains("包内残留图")),
            "{:?}",
            c.lean_reason
        );
    }

    /// 46 vs 44 plates is a real difference but not a verdict-worthy one.
    #[test]
    fn near_miss_on_plates_reads_as_a_tie() {
        let mut a = signals(&["a"], &["一"], &[10], 10);
        a.img_referenced = 44;
        a.img_referenced_substantial = 44;
        let mut b = a.clone();
        b.img_referenced = 46;
        b.img_referenced_substantial = 46;
        let c = compare(&[input("a.epub", 1, a), input("b.epub", 1, b)]).unwrap();
        assert_eq!(axis_named(&c, "plates").verdict, Verdict::Tie);
        assert_eq!(axis_named(&c, "plates").cells[0].mark, CellMark::Tie);
    }

    /// Same 回目 but genuinely different text: say so, and don't pretend the
    /// longer one is better.
    #[test]
    fn re_edited_edition_is_flagged_but_text_length_is_not_scored() {
        let a = signals(&["a", "b"], &["第一回", "第二回"], &[100, 200], 300);
        let b = signals(&["a", "b2"], &["第一回", "第二回"], &[100, 260], 360);
        let c = compare(&[input("a.epub", 1, a), input("b.epub", 1, b)]).unwrap();
        assert_eq!(c.kind, RelationKind::SameEdition);
        assert_eq!(axis_named(&c, "text").verdict, Verdict::Presented);
        assert!(c
            .conclusion
            .iter()
            .any(|l| l.contains("取决于你想要哪个版本")));
        let strip = c.chapter_diff.expect("same 回目 aligns");
        assert_eq!(strip.identical, vec![true, false]);
        assert_eq!(strip.deltas, vec![0, 60]);
    }

    /// TOC-as-chapters books reuse one file across many spine units, so
    /// `chapter_shas` (per file) is shorter than `chapter_chars` (per unit).
    /// The strip must be omitted instead of zipping misaligned arrays.
    #[test]
    fn chapter_strip_is_omitted_when_one_file_covers_many_spine_units() {
        // One file sliced into three units: shas = 1, chars = 3.
        let a = signals(&["a"], &["一"], &[10, 20, 30], 60);
        let b = signals(&["b"], &["一"], &[10, 20, 35], 65);
        let c = compare(&[input("a.epub", 1, a), input("b.epub", 1, b)]).unwrap();
        assert_eq!(c.kind, RelationKind::SameEdition);
        assert!(
            c.chapter_diff.is_none(),
            "per-file shas cannot align with per-unit chars"
        );
    }

    /// One 回目 set inside the other = full vs abridged.
    #[test]
    fn subset_of_headings_is_contained() {
        let a = signals(&["a", "b", "c"], &["一", "二", "三"], &[1, 1, 1], 3);
        let b = signals(&["a", "c"], &["一", "三"], &[1, 1], 2);
        let c = compare(&[input("full.epub", 1, a), input("short.epub", 1, b)]).unwrap();
        assert_eq!(c.kind, RelationKind::Contained);
    }

    /// A book with no reference breakdown (pre-kind-3 cache) must not be
    /// reported as having zero plates.
    #[test]
    fn old_cache_falls_back_to_the_manifest_count() {
        let mut a = signals(&["a"], &["一"], &[10], 10);
        a.analysis_kind = book_signals::REFERENCED_IMAGE_KIND - 1;
        a.img_files = 20;
        a.img_substantial = 12;
        let mut b = a.clone();
        b.img_files = 10;
        b.img_substantial = 3;
        let c = compare(&[input("a.epub", 1, a), input("b.epub", 1, b)]).unwrap();
        let plates = axis_named(&c, "plates");
        assert_eq!(plates.cells[0].display, "12 张");
        assert_eq!(plates.cells[1].display, "3 张");
        assert_eq!(plates.verdict, Verdict::Winner);
        // No breakdown ⇒ the cover axis is not invented.
        assert!(c.axes.iter().all(|a| a.key != "cover_bytes"));
    }

    /// Identical books everywhere: no lean, and the panel says so.
    #[test]
    fn fully_identical_copies_have_no_lean() {
        let a = signals(&["a"], &["一"], &[10], 10);
        let c = compare(&[input("a.epub", 1, a.clone()), input("b.epub", 1, a)]).unwrap();
        assert_eq!(c.kind, RelationKind::SameTypesetting);
        assert_eq!(c.lean, None);
        assert!(
            c.conclusion.iter().any(|l| l.contains("打平")),
            "{:?}",
            c.conclusion
        );
    }

    #[test]
    fn a_split_reports_both_sides_instead_of_averaging() {
        let mut a = signals(&["a"], &["一"], &[10], 10);
        a.mojibake = 0;
        a.img_orphan_bytes = 0;
        let mut b = signals(&["a"], &["一"], &[10], 10);
        b.mojibake = 5; // b is worse on 乱码
        b.img_orphan_bytes = 0;
        // a has fewer plates; b is dirtier — each wins one axis.
        a.img_referenced_substantial = 3;
        b.img_referenced_substantial = 50;
        let c = compare(&[input("a.epub", 1, a), input("b.epub", 1, b)]).unwrap();
        assert_eq!(c.lean, None);
        assert!(
            c.conclusion.iter().any(|l| l.contains("各有长短")),
            "{:?}",
            c.conclusion
        );
    }

    #[test]
    fn one_copy_is_not_a_comparison() {
        let a = signals(&["a"], &["一"], &[10], 10);
        assert!(compare(&[input("a.epub", 1, a)]).is_err());
    }

    /// Mixed cache generations: one copy has the image breakdown, the other
    /// predates it. The axis must say "can't compare" rather than print a zero
    /// that reads as "no cover art".
    #[test]
    fn mixed_cache_generations_report_an_incomparable_axis() {
        let a = signals(&["a"], &["一"], &[10], 10);
        let mut b = a.clone();
        b.analysis_kind = book_signals::REFERENCED_IMAGE_KIND - 1;
        let c = compare(&[input("a.epub", 1, a), input("b.epub", 1, b)]).unwrap();
        let cover = axis_named(&c, "cover_bytes");
        assert_eq!(cover.verdict, Verdict::Incomparable);
        assert_eq!(cover.cells[1].display, "—");
        assert_eq!(cover.cells[1].mark, CellMark::Unknown);
        assert!(cover.winners.is_empty());
    }

    /// Real-library smoke test: every pair of epubs on this machine, printed
    /// when the two look like the same work.
    /// `cargo test -p iced-reader --lib -- --ignored --nocapture real_pairs`
    #[test]
    #[ignore = "reads the real library epubs next to the repo / in target/"]
    fn real_pairs_on_this_machine() {
        use iced_reader_core::BookOpener;
        use iced_reader_epub::EpubOpener;

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let dirs = [
            root.clone(),
            root.join("target/debug/data/library"),
            root.join("target/release/data/library"),
        ];
        let mut paths: Vec<std::path::PathBuf> = Vec::new();
        for dir in &dirs {
            let Ok(read) = std::fs::read_dir(dir) else {
                continue;
            };
            paths.extend(read.filter_map(|i| i.ok()).map(|i| i.path()).filter(|p| {
                p.is_file()
                    && p.extension()
                        .and_then(|e| e.to_str())
                        .is_some_and(|e| e.eq_ignore_ascii_case("epub"))
            }));
        }
        paths.sort();
        paths.dedup();

        let mut books: Vec<CompareInput> = Vec::new();
        for path in paths {
            let file_name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let Ok(book) = EpubOpener.open(&path) else {
                continue;
            };
            let meta = book.metadata();
            let images = iced_reader_epub::image_stats(&path).unwrap_or_default();
            let Ok(signals) = book_signals::analyze_book(
                book.as_ref(),
                &meta.identifiers,
                !meta.authors.is_empty(),
                "diagnostic",
                images,
            ) else {
                continue;
            };
            books.push(CompareInput {
                file_name: file_name.clone(),
                title: file_name,
                quality: None,
                size_bytes: std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
                signals,
            });
        }

        let mut related = 0usize;
        for i in 0..books.len() {
            for j in (i + 1)..books.len() {
                let pair = vec![books[i].clone(), books[j].clone()];
                let Ok(c) = compare(&pair) else {
                    continue;
                };
                if c.kind == RelationKind::Unrelated {
                    continue;
                }
                related += 1;
                println!("=== {}  vs  {}", books[i].file_name, books[j].file_name);
                println!("  关系：{:?} — {}", c.kind, c.kind_note);
                for line in &c.conclusion {
                    println!("  结论：{line}");
                }
                for a in &c.axes {
                    let cells: Vec<String> = a.cells.iter().map(|x| x.display.clone()).collect();
                    println!(
                        "  [{:?}] {} = {}  → {:?}",
                        a.group,
                        a.label,
                        cells.join(" | "),
                        a.verdict
                    );
                }
                if let Some(d) = &c.chapter_diff {
                    println!(
                        "  章级对照：{} 个单元，其中 {} 个正文不同",
                        d.labels.len(),
                        d.identical.iter().filter(|x| !**x).count()
                    );
                }
            }
        }
        println!("—— 共 {related} 组非「无关」配对");
    }
}
