//! Word-note expansion for EPUBs whose inline notes carry no visible text of
//! their own, so the notes are invisible unless the reader runs script — which
//! we never do in chapter iframes. Two layouts are recognised:
//!
//! 1. **WeRead** (`data-wr-footernote` on an empty span). The note text sits in
//!    the attribute; the layout used by WeRead-exported Chinese classics
//!    (e.g. 《资治通鉴全本注译》, 179k notes):
//!
//! ```html
//! <p>…弃疑<span class="reader js_readerFooterNote" data-wr-footernote="弃疑：…"></span>，…</p>
//! ```
//!
//! 2. **duokan / 读客** (`zy-footnote` on the noteref `<img>`, note text in an
//!    `<aside epub:type="footnote" id="footnote-N">` at the end of the file).
//!    The inline icon is an 11px image, so a reader can neither read the note
//!    nor tell how many notes a paragraph has:
//!
//! ```html
//! <p>…同居<a epub:type="noteref" href="#footnote-3-53"><img src="…/image_001.png"
//!    alt="伦敦时尚艺术区。——笔者注" zy-footnote="伦敦时尚艺术区。——笔者注"
//!    class="epub-footnote"/></a>。</p>
//! <aside epub:type="footnote" id="footnote-3-53"><ol class="duokan-footnote-content">
//!    <li class="duokan-footnote-item">伦敦时尚艺术区。——笔者注</li></ol></aside>
//! ```
//!
//! Both are rewritten into the same reading shape (with `doc_base` = the
//! rewritten absolute URL of this document, e.g.
//! `http://icedreader.localhost/book/{id}/OEBPS/Text/x.xhtml`):
//! ```html
//! <p>…弃疑<a id="wr-note-back-3" class="wr-note" data-label="1" data-note="弃疑：…" href="http://…/x.xhtml#wr-note-3"></a>，…</p>
//! <div class="wr-notes"><p class="wr-note-item" id="wr-note-3"><a class="wr-note-back" href="http://…/x.xhtml#wr-note-back-3" title="返回正文">[1]</a>弃疑：…</p></div>
//! ```
//!
//! The marker `<a>` carries no text of its own (the visible number is drawn
//! via CSS `::after`), so the chapter's text-node sequence only gains the
//! trailing note blocks — deterministically on every render, keeping
//! highlight anchoring stable. The full note text rides in the marker's
//! `data-note` (no `title`, so the browser never shows a competing native
//! tooltip); the parent page paints a dark hover bubble from it. Clicking
//! jumps to the full note block, and the label at the start of each note
//! (`[n]`) is itself the back link to its marker — the same `[n]`↔note shape
//! as an ordinary annotated EPUB (东周列国志). The links are absolute
//! same-document URLs (`doc_base#…`) because the chapter is displayed via
//! `srcDoc`, where bare `#fragment` hrefs cannot be routed by the reader, so
//! form 2's original `#footnote-N` marker hrefs are rewritten too; the pair
//! lets the reader jump either way. Note items avoid column breaks
//! (`break-inside: avoid`) so they stay whole like a printed footnote; an
//! item too tall for one column still splits, and each item then carries a
//! textless trailing back link (`a.wr-note-back.wr-note-back-tail`, visible
//! only through a `wr-note-cross` class the parent page adds when a split
//! actually happened) so the continuation page can jump back too.

/// Blocks we treat as note-hosting containers. Paragraph text and headings
/// (e.g. an annotated volume title in a WeRead-export book) both carry word
/// notes; notes inside other containers are left untouched.
const CONTAINER_TAGS: [&str; 7] = ["p", "h1", "h2", "h3", "h4", "h5", "h6"];
const NOTE_ATTR: &str = "data-wr-footernote";
/// Form 2 (duokan / 读客) marker: the note text rides on the noteref icon, and
/// the readable copy lives in an `<aside epub:type="footnote" id="…">`.
const DUOKAN_ATTR: &str = "zy-footnote";
const DUOKAN_ASIDE: &str = "aside";
const DUOKAN_ID_ATTR: &str = "id";
const DUOKAN_HREF_ATTR: &str = "href";
const MARKER_CLASS: &str = "wr-note";
/// id on the in-text marker; the note block's 返回 link targets it.
const MARKER_ID_PREFIX: &str = "wr-note-back-";
/// class of the note block's back-to-text link.
const BACK_CLASS: &str = "wr-note-back";
/// extra class of the trailing back link inside each note item. It is empty
/// and invisible by default; when a column break actually splits an item
/// (very long note under a large font / short viewport), the parent page adds
/// `wr-note-cross` to the item and the trailing link becomes visible, so the
/// reader sitting on the continuation page can still jump back to the marker.
const TAIL_CLASS: &str = "wr-note-back-tail";

use std::fmt::Write as _;

/// `doc_base` is the rewritten absolute URL of the document this HTML slice
/// belongs to (`resource_base + file`); generated note links point into that
/// same document so the front end can route them as same-file anchors.
pub fn expand_word_notes(html: &str, doc_base: &str) -> String {
    if !contains_ci(html, NOTE_ATTR) && !contains_ci(html, DUOKAN_ATTR) {
        return html.to_string();
    }
    // Form 2's note text lives in `<aside>` blocks elsewhere in the file; read
    // them first so each inline icon can re-emit its note right where it sits.
    let mut footnotes = collect_duokan_footnotes(html);
    let mut out = String::with_capacity(html.len() + 512);
    let mut seq: u64 = 0;
    let mut pos = 0usize;
    while pos < html.len() {
        let Some((open, tag)) = next_container(html, pos) else {
            break;
        };
        let Some(open_end) = tag_end(html, open) else {
            break;
        };
        // Copy everything before this container verbatim, tracking where every
        // aside lands in the output so consumed ones can be cut at the end.
        push_tracked(&mut out, &html[pos..open], pos, &mut footnotes);
        if is_self_closing(html, open_end) {
            out.push_str(&html[open..open_end]);
            pos = open_end;
            continue;
        }
        out.push_str(&html[open..open_end]);
        // Explicit `</tag>`, else HTML5 implied close: next p/h1–h6 (or EOF).
        // Do not abort the rest of the document on a missing `</p>`.
        let (body_end, after, copy_close) = match find_close_tag(html, open_end, tag) {
            Some(close) => {
                let close_end = tag_end(html, close).unwrap_or(close + tag.len() + 3);
                (close, close_end, true)
            }
            None if tag == "p" => match next_container(html, open_end) {
                Some((at, _)) => (at, at, false),
                None => (html.len(), html.len(), false),
            },
            None => {
                out.push_str(&html[open_end..]);
                return out;
            }
        };
        let converted = convert_paragraph(
            &html[open_end..body_end],
            &mut seq,
            doc_base,
            // Note text: form 2's `<aside>` first (the icon's own copy can be
            // truncated), then either layout's inline attribute.
            &mut |note: &NoteMarker| {
                note.key
                    .as_deref()
                    .and_then(|key| footnote_text(&mut footnotes, key))
                    .or_else(|| note.attr_text.clone().filter(|t| !t.is_empty()))
            },
        );
        out.push_str(&converted.text);
        if copy_close {
            out.push_str(&html[body_end..after]);
        }
        // Each note's block sits right after the paragraph carrying its marker.
        for block in &converted.blocks {
            out.push_str(block);
        }
        pos = after;
    }
    let tail = html[pos..].to_string();
    push_tracked(&mut out, &tail, pos, &mut footnotes);
    // Drop the asides whose text was re-emitted beside their markers; an aside
    // no marker ever pointed at stays where the book put it.
    drop_used_asides(&out, &footnotes)
}

/// `html[start..start + slice.len()]` into `out`, keeping each aside's span
/// pointed at the same bytes *in the output* so later edits cannot shift them.
fn push_tracked(out: &mut String, slice: &str, start: usize, footnotes: &mut [DuokanNote]) {
    let shift = out.len() as isize - start as isize;
    for note in footnotes.iter_mut() {
        if note.start >= start && note.start < start + slice.len() {
            note.start = (note.start as isize + shift) as usize;
            note.end = (note.end as isize + shift) as usize;
        }
    }
    out.push_str(slice);
}

/// One note found inside a paragraph (either layout), in document order.
struct NoteMarker {
    /// In-text marker: `seq` is file-wide (and the `id` suffix), `label` is the
    /// number shown in the paragraph.
    seq: u64,
    label: usize,
    /// Form 2 only: fragment of the noteref href, decoded, pairing the icon
    /// with its `<aside>` text.
    key: Option<String>,
    /// Form 1's note text, or form 2's icon attribute as a fallback when no
    /// aside matches the key.
    attr_text: Option<String>,
    /// The `wr-notes` block emitted right after the paragraph, filled in once
    /// the note text is resolved.
    block: Option<String>,
}

struct Converted {
    text: String,
    /// Note blocks for this paragraph, in document order.
    blocks: Vec<String>,
}

struct DuokanNote {
    /// Decoded, lower-cased id, plus span so consumed asides can be dropped.
    key: String,
    text: String,
    start: usize,
    end: usize,
    used: bool,
}

/// Every `<aside epub:type="footnote" id="…">` in the file, with the plain text
/// of its body. Non-footnote asides are ignored.
fn collect_duokan_footnotes(html: &str) -> Vec<DuokanNote> {
    let mut notes = Vec::new();
    let mut pos = 0usize;
    while pos < html.len() {
        let Some(open) = find_open_tag(html, pos, DUOKAN_ASIDE) else {
            break;
        };
        let Some(open_end) = tag_end(html, open) else {
            break;
        };
        let Some(close) = find_close_tag(html, open_end, DUOKAN_ASIDE) else {
            break;
        };
        let close_end = tag_end(html, close).unwrap_or(close + DUOKAN_ASIDE.len() + 3);
        if let Some((key, text)) = aside_note(&html[open..open_end], &html[open_end..close]) {
            notes.push(DuokanNote {
                key,
                text,
                start: open,
                end: close_end,
                used: false,
            });
        }
        pos = close_end.max(open_end);
    }
    notes
}

/// `(id, note text)` of one `<aside>` opening tag + its inner HTML, or `None`
/// when the element is not a footnote aside.
fn aside_note(open_tag: &str, inner: &str) -> Option<(String, String)> {
    let epub_type = attr_value(open_tag, "epub:type")?;
    if !epub_type.eq_ignore_ascii_case("footnote") {
        return None;
    }
    let id = attr_value(open_tag, DUOKAN_ID_ATTR)?;
    let text = note_text(inner);
    if text.is_empty() {
        return None;
    }
    Some((normalize_note_id(&id), text))
}

/// The aside's text, marking it consumed so its original copy is dropped.
fn footnote_text(footnotes: &mut [DuokanNote], key: &str) -> Option<String> {
    let note = footnotes.iter_mut().find(|n| n.key == key)?;
    note.used = true;
    Some(note.text.clone())
}

/// Remove the asides whose text was re-emitted at their markers, given their
/// spans in `html` (output coordinates). Rebuilt from scratch — removals only
/// ever cut bytes out, so the recorded offsets stay valid.
fn drop_used_asides(html: &str, footnotes: &[DuokanNote]) -> String {
    let mut cuts: Vec<(usize, usize)> = footnotes
        .iter()
        .filter(|n| n.used)
        .map(|n| (n.start, n.end))
        .collect();
    if cuts.is_empty() {
        return html.to_string();
    }
    cuts.sort_unstable();
    let mut out = String::with_capacity(html.len());
    let mut pos = 0usize;
    for (start, end) in cuts {
        if start < pos {
            continue;
        }
        out.push_str(&html[pos..start]);
        pos = end;
    }
    out.push_str(&html[pos..]);
    out
}

/// Plain note text out of an aside's inner HTML: tags dropped, entities
/// decoded. `duokan-footnote-item` list decoration (a bullet/number the
/// browser would draw) is stripped along with the tags.
fn note_text(inner: &str) -> String {
    let mut out = String::with_capacity(inner.len());
    let mut pos = 0usize;
    while pos < inner.len() {
        match inner[pos..].find('<') {
            Some(rel) => {
                let at = pos + rel;
                out.push_str(&inner[pos..at]);
                let Some(end) = tag_end(inner, at) else {
                    break;
                };
                pos = end;
            }
            None => {
                out.push_str(&inner[pos..]);
                break;
            }
        }
    }
    decode_entities(out.trim()).trim().to_string()
}

/// Percent-decoded, percent-encoding-insensitive note key: TOC hrefs and
/// `id` attributes are written by different tools, so `#%E6%B3%A8-1` and
/// `id="注-1"` must still pair up.
fn normalize_note_id(id: &str) -> String {
    percent_decode(id.trim()).to_lowercase()
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0usize;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(a), Some(b)) = (from_hex(bytes[i + 1]), from_hex(bytes[i + 2])) {
                out.push((a << 4) | b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn from_hex(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Fragment (`#…`) of a noteref href, decoded, or `None` for an href that
/// points elsewhere.
fn href_fragment(href: &str) -> Option<String> {
    let hash = href.find('#')?;
    let frag = &href[hash + 1..];
    (!frag.is_empty()).then(|| normalize_note_id(frag))
}

/// Next note-hosting container opening tag at/after `from`.
/// Returns (index of `<`, lower-cased tag name).
fn next_container(html: &str, from: usize) -> Option<(usize, &'static str)> {
    let mut pos = from;
    while pos < html.len() {
        if html[pos..].starts_with("<!--") {
            let after = html[pos + 4..]
                .find("-->")
                .map(|r| pos + 4 + r + 3)
                .unwrap_or(html.len());
            pos = after;
            continue;
        }
        let at = find_byte(html, pos, b'<')?;
        debug_assert!(at <= html.len(), "at={at} len={} pos={pos}", html.len());
        if html.get(at..).is_none_or(|rest| rest.starts_with("<!--")) {
            let Some(rest) = html.get(at..) else {
                return None;
            };
            if rest.starts_with("<!--") {
                let after = html[at + 4..]
                    .find("-->")
                    .map(|r| at + 4 + r + 3)
                    .unwrap_or(html.len());
                pos = after;
                continue;
            }
        }
        if html[at..].starts_with("</") {
            pos = at + 2;
            continue;
        }
        let name = tag_name(&html[at + 1..]).1;
        let matched = CONTAINER_TAGS.iter().find(|c| **c == name).copied();
        if let Some(tag) = matched {
            let bytes = html.as_bytes();
            if boundary_at(bytes, at + 1 + name.len()) {
                return Some((at, tag));
            }
        }
        pos = at + 1;
    }
    None
}

fn find_byte(html: &str, from: usize, needle: u8) -> Option<usize> {
    html.as_bytes()[from..]
        .iter()
        .position(|&b| b == needle)
        .map(|r| from + r)
}

/// One paragraph body in, the same markup with note markers plus the notes
/// found in it out. Both layouts are walked in a single document-order pass, so
/// labels number the paragraph and `seq` numbers the file exactly as before.
/// `resolve` supplies a note's text (form 2 looks it up in its `<aside>`).
fn convert_paragraph(
    body: &str,
    seq: &mut u64,
    doc_base: &str,
    resolve: &mut dyn FnMut(&NoteMarker) -> Option<String>,
) -> Converted {
    let mut out = String::with_capacity(body.len() + 64);
    let mut notes: Vec<NoteMarker> = Vec::new();
    let mut cursor = 0usize;
    let mut pos = 0usize;
    while pos < body.len() {
        let Some(at) = find_byte(body, pos, b'<') else {
            break;
        };
        if body[at..].starts_with("<!--") {
            pos = body[at + 4..]
                .find("-->")
                .map(|r| at + 4 + r + 3)
                .unwrap_or(body.len());
            continue;
        }
        let Some(end) = tag_end(body, at) else {
            break;
        };
        let is_close = body[at..].starts_with("</");
        let name = if is_close {
            tag_name(&body[at + 2..]).1
        } else {
            tag_name(&body[at + 1..]).1
        };
        // Form 1: an empty span carrying the note text in an attribute.
        if name == "span" && !is_close && contains_ci(&body[at..end], NOTE_ATTR) {
            if let Some(text) = attr_value(&body[at..end], NOTE_ATTR) {
                let span_end = if is_self_closing(body, end) {
                    Some((end, end))
                } else {
                    span_close(body, end)
                };
                if let Some((inner_end, after)) = span_end {
                    *seq += 1;
                    let note = NoteMarker {
                        seq: *seq,
                        label: notes.len() + 1,
                        key: None,
                        attr_text: Some(text),
                        block: None,
                    };
                    let mut marker = String::new();
                    if emit_note(
                        &mut marker,
                        &mut notes,
                        note,
                        doc_base,
                        // Whatever the original span held (normally nothing)
                        // stays inside the marker.
                        &body[end..inner_end],
                        resolve,
                    ) {
                        out.push_str(&body[cursor..at]);
                        out.push_str(&marker);
                        cursor = after;
                        pos = after;
                    } else {
                        // No readable note text: keep the span as it was.
                        *seq -= 1;
                        pos = at + 1;
                    }
                    continue;
                }
            }
        }
        // Form 2: the duokan note icon inside its noteref anchor.
        if name == "a" && !is_close && contains_ci(&body[at..end], DUOKAN_HREF_ATTR) {
            if let Some((icon, icon_end, key)) = duokan_icon(body, at, end) {
                *seq += 1;
                let note = NoteMarker {
                    seq: *seq,
                    label: notes.len() + 1,
                    key: Some(key),
                    attr_text: attr_value(&body[icon..icon_end], DUOKAN_ATTR)
                        .filter(|t| !t.is_empty())
                        .or_else(|| {
                            attr_value(&body[icon..icon_end], "alt").filter(|t| !t.is_empty())
                        }),
                    block: None,
                };
                let mut marker = String::new();
                if emit_note(&mut marker, &mut notes, note, doc_base, "", resolve) {
                    // The marker replaces the icon inside the anchor, so it
                    // keeps the icon's exact position in the sentence (the
                    // anchor itself stays: its `#footnote-N` href never leaves
                    // the document, and the reader routes it like any other
                    // in-book anchor).
                    out.push_str(&body[cursor..icon]);
                    out.push_str(&marker);
                    cursor = icon_end;
                    pos = icon_end;
                } else {
                    // No readable text anywhere: keep the icon as it was, and
                    // hand the sequence number back.
                    *seq -= 1;
                    pos = at + 1;
                }
                continue;
            }
        }
        // Nothing to expand here: step over just this `<` and let the next
        // iteration carry on copying from `cursor`.
        pos = at + 1;
    }
    out.push_str(&body[cursor..]);
    Converted {
        text: out,
        blocks: notes.iter().filter_map(|n| n.block.clone()).collect(),
    }
}

/// The duokan note icon inside the anchor starting at `anchor`: `(icon start,
/// icon end, note key)`. The key is the decoded `#fragment` of the anchor's
/// href, which pairs the icon with its `<aside>`.
fn duokan_icon(body: &str, anchor: usize, anchor_end: usize) -> Option<(usize, usize, String)> {
    let href = attr_value(&body[anchor..anchor_end], DUOKAN_HREF_ATTR)?;
    let key = href_fragment(&href)?;
    let mut pos = anchor_end;
    while let Some(at) = find_open_tag(body, pos, "img") {
        // An icon past the anchor's own `</a>` is not this anchor's icon.
        if contains_ci(&body[anchor_end..at], "</a") {
            return None;
        }
        let end = tag_end(body, at)?;
        if contains_ci(&body[at..end], DUOKAN_ATTR) {
            return Some((at, end, key));
        }
        pos = end;
    }
    None
}

/// Write the in-text marker for one note — no text of its own (the number is
/// drawn from `data-label` by CSS), the note text in `data-note` for the hover
/// bubble — around `inner`, and queue its `wr-notes` block. Returns false, and
/// writes nothing, when the note has no readable text anywhere.
fn emit_note(
    out: &mut String,
    notes: &mut Vec<NoteMarker>,
    mut note: NoteMarker,
    doc_base: &str,
    inner: &str,
    resolve: &mut dyn FnMut(&NoteMarker) -> Option<String>,
) -> bool {
    let Some(text) = resolve(&note) else {
        return false;
    };
    let href = format!("{doc_base}#wr-note-{}", note.seq);
    let _ = write!(
        out,
        r##"<a id="{MARKER_ID_PREFIX}{}" class="{MARKER_CLASS}" data-label="{}" data-note="{}" href="{}">"##,
        note.seq,
        note.label,
        escape_attr(&text),
        escape_attr(&href)
    );
    out.push_str(inner);
    out.push_str("</a>");
    note.block = Some(note_block(&note, &text, doc_base));
    notes.push(note);
    true
}

/// Markup for the note list under the paragraph holding one marker. The item
/// carries two back links: the leading `[n]` label (always visible) and a
/// textless trailing link that only shows when a page break really split the
/// item (see `TAIL_CLASS`). Both target the in-text marker.
fn note_block(note: &NoteMarker, text: &str, doc_base: &str) -> String {
    let back_href = format!("{doc_base}#{MARKER_ID_PREFIX}{}", note.seq);
    let mut out = String::with_capacity(128 + text.len());
    out.push_str(r#"<div class="wr-notes">"#);
    let _ = write!(
        out,
        r#"<p class="wr-note-item" id="wr-note-{}"><a class="{BACK_CLASS}" href="{}" title="返回正文">[{}]</a>{}"#,
        note.seq,
        escape_attr(&back_href),
        note.label,
        escape_text(text)
    );
    let _ = write!(
        out,
        r#"<a class="{BACK_CLASS} {TAIL_CLASS}" href="{}"></a></p>"#,
        escape_attr(&back_href)
    );
    out.push_str("</div>");
    out
}

/// Index of `<name` (case-insensitive, tag-name boundary) at/after `from`,
/// skipping HTML comments. `None` when absent.
fn find_open_tag(html: &str, from: usize, name: &str) -> Option<usize> {
    let mut pos = from;
    while pos < html.len() {
        if html[pos..].starts_with("<!--") {
            let after = html[pos + 4..]
                .find("-->")
                .map(|r| pos + 4 + r + 3)
                .unwrap_or(html.len());
            pos = after;
            continue;
        }
        let rel = find_ci(&html[pos..], name)?;
        let at = pos + rel;
        let bytes = html.as_bytes();
        // Byte-level lookaround only: `at±k` may land inside a multi-byte
        // char when the tag name appears in Chinese prose.
        if at > 0
            && bytes.get(at - 1) == Some(&b'<')
            && boundary_at(bytes, at + name.len())
        {
            return Some(at - 1);
        }
        pos = at + name.len();
    }
    None
}

/// True when the byte right after a tag name is whitespace, `>`, or `/`.
fn boundary_at(bytes: &[u8], at: usize) -> bool {
    match bytes.get(at) {
        None => true,
        Some(&c) => c.is_ascii_whitespace() || c == b'>' || c == b'/',
    }
}

/// Index of the first `</name` (case-insensitive, boundary) at/after `from`.
fn find_close_tag(html: &str, from: usize, name: &str) -> Option<usize> {
    let mut pos = from;
    while pos < html.len() {
        let rel = find_ci(&html[pos..], name)?;
        let at = pos + rel;
        let bytes = html.as_bytes();
        if let Some(b) = at.checked_sub(2) {
            if bytes.get(b) == Some(&b'<')
                && bytes.get(b + 1) == Some(&b'/')
                && boundary_at(bytes, at + name.len())
            {
                return Some(b);
            }
        }
        pos = at + name.len();
    }
    None
}


/// Index just past the `>` of the tag starting at `tag_start` (quote-aware).
fn tag_end(html: &str, tag_start: usize) -> Option<usize> {
    let bytes = html.as_bytes();
    let mut i = tag_start;
    let mut quote: u8 = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' | b'\'' => {
                if quote == 0 {
                    quote = bytes[i];
                } else if quote == bytes[i] {
                    quote = 0;
                }
            }
            b'>' if quote == 0 => return Some(i + 1),
            _ => {}
        }
        i += 1;
    }
    None
}

/// True when the tag ending at `tag_end` is self-closing (`/>`).
fn is_self_closing(html: &str, tag_end: usize) -> bool {
    let bytes = html.as_bytes();
    if tag_end < 2 {
        return false;
    }
    let mut i = tag_end - 2;
    while i > 0 && bytes[i].is_ascii_whitespace() {
        i -= 1;
    }
    bytes[i] == b'/'
}

/// Given the open `<span …>` end, return (index of `</span>`'s `<`, index
/// just past its `>`), tolerating nested spans. `None` when unclosed.
fn span_close(html: &str, open_end: usize) -> Option<(usize, usize)> {
    let mut depth = 1usize;
    let mut pos = open_end;
    while pos < html.len() {
        let rel = match find_ci(&html[pos..], "<") {
            Some(r) => r,
            None => return None,
        };
        let at = pos + rel;
        if html[at..].starts_with("<!--") {
            pos = html[at + 4..]
                .find("-->")
                .map(|r| at + 4 + r + 3)
                .unwrap_or(html.len());
            continue;
        }
        if html[at..].starts_with("</") {
            let rest = &html[at + 2..];
            let (close_len, close_name) = tag_name(rest);
            if close_name.eq_ignore_ascii_case("span") {
                depth -= 1;
                if depth == 0 {
                    let end = tag_end(html, at).unwrap_or(at + 2 + close_len + 1);
                    return Some((at, end));
                }
            }
            pos = tag_end(html, at).unwrap_or(at + 2);
            continue;
        }
        let rest = &html[at + 1..];
        let (_len, open_name) = tag_name(rest);
        if open_name.eq_ignore_ascii_case("span") && !is_self_closing(html, tag_end(html, at)?) {
            depth += 1;
        }
        pos = tag_end(html, at)?;
    }
    None
}

/// First tag name after an opening `<` or `</`: (byte len, lower-cased name).
fn tag_name(rest: &str) -> (usize, String) {
    let mut end = 0usize;
    for c in rest.chars() {
        if c.is_ascii_alphanumeric() {
            end += c.len_utf8();
        } else {
            break;
        }
    }
    (end, rest[..end].to_ascii_lowercase())
}

/// Decoded value of attribute `name` inside a single tag, or `None`.
/// The name is matched case-insensitively; the value is sliced from the
/// original tag so Latin letters in the note text keep their case.
fn attr_value(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let needle = name.to_ascii_lowercase();
    let mut search = 0usize;
    while search < lower.len() {
        let rel = lower[search..].find(&needle)?;
        let at = search + rel;
        let after_name = at + needle.len();
        let rest = &lower[after_name..];
        let ws = rest.len() - rest.trim_start().len();
        let eq_at = after_name + ws;
        if !lower[eq_at..].starts_with('=') {
            search = after_name;
            continue;
        }
        let after_eq = eq_at + 1;
        let rest2 = &lower[after_eq..];
        let ws2 = rest2.len() - rest2.trim_start().len();
        let val_at = after_eq + ws2;
        let bytes = lower.as_bytes();
        let (quote, content_start) = match bytes.get(val_at) {
            Some(q @ (b'"' | b'\'')) => (Some(*q), val_at + 1),
            _ => (None, val_at),
        };
        let mut end = content_start;
        while end < bytes.len() {
            let c = bytes[end];
            match quote {
                Some(q) if c == q => break,
                None if c.is_ascii_whitespace() || c == b'>' => break,
                _ => end += 1,
            }
        }
        return Some(decode_entities(&tag[content_start..end]));
    }
    None
}

/// Minimal HTML entity decoding for attribute values we copy into output.
fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    s.replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// Escape text for an attribute value (double-quoted).
fn escape_attr(s: &str) -> String {
    escape_common(s)
        .replace('"', "&quot;")
}

/// Escape text for an element text node.
fn escape_text(s: &str) -> String {
    escape_common(s)
}

fn escape_common(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(c),
        }
    }
    out
}

fn contains_ci(hay: &str, needle: &str) -> bool {
    hay.len() >= needle.len()
        && hay
            .as_bytes()
            .windows(needle.len())
            .any(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
}

fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    hay.as_bytes()
        .windows(needle.len())
        .position(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC_BASE: &str = "http://icedreader.localhost/book/t/OEBPS/Text/c.xhtml";

    #[test]
    fn leaves_html_without_notes_untouched() {
        let html = r#"<html><body><p>你好，世界。</p></body></html>"#;
        assert_eq!(expand_word_notes(html, DOC_BASE), html);
    }

    #[test]
    fn expands_empty_note_span_into_marker_and_block() {
        let html = r#"<p>知莫大于弃疑<span class="reader js_readerFooterNote" data-wr-footernote="弃疑：抛弃不明的谋划。"></span>，行莫大于无过</p>"#;
        let out = expand_word_notes(html, DOC_BASE);
        assert!(
            out.contains(r##"<a id="wr-note-back-1" class="wr-note" data-label="1" data-note="弃疑：抛弃不明的谋划。" href="http://icedreader.localhost/book/t/OEBPS/Text/c.xhtml#wr-note-1"></a>"##),
            "{out}"
        );
        assert!(out.contains(r#"<div class="wr-notes">"#), "{out}");
        assert!(out.contains(r#"<p class="wr-note-item" id="wr-note-1">"#), "{out}");
        assert!(out.contains("弃疑：抛弃不明的谋划。"), "{out}");
        // the block's note label [1] doubles as the back link to the marker
        assert!(
            out.contains(r##"<a class="wr-note-back" href="http://icedreader.localhost/book/t/OEBPS/Text/c.xhtml#wr-note-back-1" title="返回正文">[1]</a>弃疑：抛弃不明的谋划。"##),
            "{out}"
        );
        // …and each item ends with a textless trailing back link (visible
        // only when a column break actually splits the item).
        assert!(
            out.contains(r##"弃疑：抛弃不明的谋划。<a class="wr-note-back wr-note-back-tail" href="http://icedreader.localhost/book/t/OEBPS/Text/c.xhtml#wr-note-back-1"></a></p>"##),
            "{out}"
        );
        assert!(out.contains("</div>"), "{out}");
        // marker goes inside the paragraph; block right after it
        let p_end = out.find("</p>").unwrap();
        let div = out.find("<div class=\"wr-notes\">").unwrap();
        assert!(div > p_end, "{out}");
        // original span gone
        assert!(!out.contains("data-wr-footernote"), "{out}");
        assert!(!out.contains("js_readerFooterNote"), "{out}");
    }

    #[test]
    fn paragraph_numbering_resets_and_seq_grows() {
        let html = r#"<p>a<span data-wr-footernote="n1"></span>b<span data-wr-footernote="n2"></span></p><p>c<span data-wr-footernote="n3"></span></p>"#;
        let out = expand_word_notes(html, DOC_BASE);
        assert!(
            out.contains(r##"id="wr-note-back-1" class="wr-note" data-label="1" data-note="n1" href="http://icedreader.localhost/book/t/OEBPS/Text/c.xhtml#wr-note-1""##),
            "{out}"
        );
        assert!(
            out.contains(r##"id="wr-note-back-2" class="wr-note" data-label="2" data-note="n2" href="http://icedreader.localhost/book/t/OEBPS/Text/c.xhtml#wr-note-2""##),
            "{out}"
        );
        assert!(
            out.contains(r##"id="wr-note-back-3" class="wr-note" data-label="1" data-note="n3" href="http://icedreader.localhost/book/t/OEBPS/Text/c.xhtml#wr-note-3""##),
            "{out}"
        );
        assert!(
            out.contains(r##"id="wr-note-3"><a class="wr-note-back" href="http://icedreader.localhost/book/t/OEBPS/Text/c.xhtml#wr-note-back-3" title="返回正文">[1]</a>n3"##),
            "{out}"
        );
        // One `wr-notes` block per note, so [1] and [2] of the same paragraph
        // do not share a block id.
        assert_eq!(out.matches("<div class=\"wr-notes\">").count(), 3, "{out}");
        assert_eq!(out.matches("id=\"wr-note-").count(), 6, "{out}");
    }

    #[test]
    fn decodes_entities_in_note_text() {
        let html = r#"<p>句<span data-wr-footernote="曰“a&amp;b”，通&#39;c&#39;。"></span></p>"#;
        let out = expand_word_notes(html, DOC_BASE);
        assert!(out.contains("曰“a&amp;b”，通'c'。"), "{out}");
        assert!(out.contains(r##"data-note="曰“a&amp;b”，通'c'。"##), "{out}");
        assert!(out.contains("c.xhtml#wr-note-1"), "{out}");
    }

    #[test]
    fn notes_in_headings_expand() {
        // Volume-title annotations sit inside <h3>, not <p>.
        let html = r#"<h3 class="secondTitle">威烈王<span data-wr-footernote="威烈王：名午。"></span></h3><p>正文<span data-wr-footernote="正文注。"></span></p>"#;
        let out = expand_word_notes(html, DOC_BASE);
        assert!(
            out.contains(r##"<a id="wr-note-back-1" class="wr-note" data-label="1" data-note="威烈王：名午。" href="http://icedreader.localhost/book/t/OEBPS/Text/c.xhtml#wr-note-1"></a>"##),
            "{out}"
        );
        assert!(
            out.contains(r##"<a id="wr-note-back-2" class="wr-note" data-label="1" data-note="正文注。" href="http://icedreader.localhost/book/t/OEBPS/Text/c.xhtml#wr-note-2"></a>"##),
            "{out}"
        );
        assert_eq!(out.matches("<div class=\"wr-notes\">").count(), 2, "{out}");
        assert!(!out.contains("data-wr-footernote"), "{out}");
    }

    #[test]
    fn notes_outside_paragraphs_stay_unexpanded() {
        let html = r#"<div><span data-wr-footernote="kept"></span></div>"#;
        let out = expand_word_notes(html, DOC_BASE);
        assert!(out.contains("data-wr-footernote=\"kept\""), "{out}");
    }

    #[test]
    fn keeps_inner_content_of_non_empty_span() {
        let html = r#"<p>语<span data-wr-footernote="释义">原文</span>尾</p>"#;
        let out = expand_word_notes(html, DOC_BASE);
        assert!(out.contains(r##"<a id="wr-note-back-1" class="wr-note" data-label="1""##), "{out}");
        assert!(out.contains(">原文</a>"), "{out}");
        assert!(out.contains("释义"), "{out}");
    }

    #[test]
    fn unclosed_paragraph_still_expands_to_eof() {
        let html = r#"<p>开头<span data-wr-footernote="n"></span>没有闭合"#;
        let out = expand_word_notes(html, DOC_BASE);
        assert!(out.contains("data-note=\"n\""), "{out}");
        assert!(out.contains("class=\"wr-note\""), "{out}");
    }

    #[test]
    fn implied_p_close_before_heading_continues() {
        let html = r#"<p>上<span data-wr-footernote="See Rome."></span><h1>标<span data-wr-footernote="B"></span></h1>"#;
        let out = expand_word_notes(html, DOC_BASE);
        assert!(out.contains("data-note=\"See Rome.\""), "{out}");
        assert!(out.contains("data-note=\"B\""), "{out}");
        assert_eq!(out.matches("<div class=\"wr-notes\">").count(), 2, "{out}");
    }

    #[test]
    fn self_closing_note_span_expands() {
        let html = r#"<p>x<span data-wr-footernote="n" /></p>"#;
        let out = expand_word_notes(html, DOC_BASE);
        assert!(out.contains("data-note=\"n\""), "{out}");
        assert!(!out.contains("data-wr-footernote"), "{out}");
    }

    #[test]
    fn uppercase_markup_is_handled() {
        let html = r#"<P>大<span DATA-WR-FOOTERNOTE="注文"></SPAN>。</P>"#;
        let out = expand_word_notes(html, DOC_BASE);
        assert!(
            out.contains(r##"<a id="wr-note-back-1" class="wr-note" data-label="1" data-note="注文" href="http://icedreader.localhost/book/t/OEBPS/Text/c.xhtml#wr-note-1"></a>"##),
            "{out}"
        );
    }

    /// One `<p>` + `<aside>` pair as 读客 / calibre books write it. The icon and
    /// the aside carry the same text; only the aside's copy survives.
    const DUOKAN_ONE_NOTE: &str = concat!(
        r##"<p class="calibre7"><span class="calibre10">在梅利莎位于切尔西<a epub:type="noteref" href="#footnote-3-53"> <img src="http://x/EPUB/images/image_001.png" alt="伦敦时尚艺术区。——笔者注" zy-footnote="伦敦时尚艺术区。——笔者注" class="epub-footnote"/></a>的公寓里同居。</span></p>"##,
        "\n\t",
        r##"<aside epub:type="footnote" id="footnote-3-53"><ol class="duokan-footnote-content"><li class="duokan-footnote-item">伦敦时尚艺术区。——笔者注</li></ol></aside>"##,
    );

    #[test]
    fn duokan_icon_becomes_a_word_note_marker() {
        let out = expand_word_notes(DUOKAN_ONE_NOTE, DOC_BASE);
        // Icon + its noteref anchor are gone; a textless marker sits in place.
        assert!(
            out.contains(r##"<a id="wr-note-back-1" class="wr-note" data-label="1" data-note="伦敦时尚艺术区。——笔者注" href="http://icedreader.localhost/book/t/OEBPS/Text/c.xhtml#wr-note-1"></a>"##),
            "{out}"
        );
        assert!(!out.contains("zy-footnote"), "{out}");
        assert!(!out.contains("<img"), "{out}");
        // The note is readable, with the `[1]` label as its back link.
        assert!(
            out.contains(r##"<p class="wr-note-item" id="wr-note-1"><a class="wr-note-back" href="http://icedreader.localhost/book/t/OEBPS/Text/c.xhtml#wr-note-back-1" title="返回正文">[1]</a>伦敦时尚艺术区。——笔者注"##),
            "{out}"
        );
        // …and the aside it came from is not left behind as plain text.
        assert!(!out.contains("<aside"), "{out}");
        assert!(!out.contains("duokan-footnote"), "{out}");
        assert_eq!(out.matches("伦敦时尚艺术区").count(), 2, "{out}"); // marker + item
    }

    #[test]
    fn duokan_numbering_per_paragraph_matches_we_read_rules() {
        let html = concat!(
            r##"<p>甲<a href="#fn-a"><img zy-footnote="注甲"/></a>乙<a href="#fn-b"><img zy-footnote="注乙"/></a></p>"##,
            r##"<p>丙<a href="#fn-c"><img zy-footnote="注丙"/></a></p>"##,
            r##"<aside epub:type="footnote" id="fn-a">注甲</aside>"##,
            r##"<aside epub:type="footnote" id="fn-b">注乙</aside>"##,
            r##"<aside epub:type="footnote" id="fn-c">注丙</aside>"##,
        );
        let out = expand_word_notes(html, DOC_BASE);
        assert!(out.contains(r##"data-label="1" data-note="注甲""##), "{out}");
        assert!(out.contains(r##"data-label="2" data-note="注乙""##), "{out}");
        assert!(out.contains(r##"data-label="1" data-note="注丙""##), "{out}");
        // File-wide seq keeps growing across paragraphs, ids stay unique.
        assert!(out.contains(r##"id="wr-note-back-3""##), "{out}");
        assert!(out.contains(r##"<p class="wr-note-item" id="wr-note-3">"##), "{out}");
        // One block per note (a paragraph with two notes gets two).
        assert_eq!(out.matches("<div class=\"wr-notes\">").count(), 3, "{out}");
        assert_eq!(out.matches("<aside").count(), 0, "{out}");
    }

    #[test]
    fn duokan_aside_supplies_text_even_without_an_icon() {
        let html = concat!(
            r##"<p>正文<a href="#fn-1"><img zy-footnote=""/></a></p>"##,
            r##"<aside epub:type="footnote" id="fn-1">完整注文，比图标属性长。</aside>"##,
        );
        let out = expand_word_notes(html, DOC_BASE);
        assert!(out.contains(r##"data-note="完整注文，比图标属性长。""##), "{out}");
    }

    #[test]
    fn duokan_falls_back_to_the_icon_text_without_an_aside() {
        // The aside went missing in the export, but the icon still has the note.
        let html = r##"<p>正文<a href="#gone"><img alt="图标里的注" zy-footnote="图标里的注"/></a>尾</p>"##;
        let out = expand_word_notes(html, DOC_BASE);
        assert!(out.contains(r##"data-note="图标里的注""##), "{out}");
        assert!(out.contains(">图标里的注<"), "{out}");
    }

    #[test]
    fn non_footnote_aside_is_left_alone() {
        let html = concat!(
            r##"<p>正文<a href="#fn-1"><img zy-footnote="注"/></a></p>"##,
            r##"<aside id="sidebar">侧栏文字</aside>"##,
            r##"<aside epub:type="footnote" id="fn-1">注</aside>"##,
        );
        let out = expand_word_notes(html, DOC_BASE);
        assert!(out.contains(r##"<aside id="sidebar">侧栏文字</aside>"##), "{out}");
        assert!(!out.contains(r##"id="fn-1""##), "{out}");
    }

    #[test]
    fn percent_encoded_note_id_still_pairs() {
        let html = concat!(
            r##"<p>正文<a href="#%E6%B3%A8-1"><img zy-footnote="短的"/></a></p>"##,
            r##"<aside epub:type="footnote" id="注-1">百分号编码配对的注文。</aside>"##,
        );
        let out = expand_word_notes(html, DOC_BASE);
        assert!(out.contains(r##"data-note="百分号编码配对的注文。""##), "{out}");
    }

    #[test]
    fn duokan_icon_outside_a_noteref_is_kept() {
        // No href fragment on the anchor: nothing says which note this is, so
        // the image keeps its place instead of turning into a dead marker.
        let html = r##"<p>正文<a href="other.xhtml"><img src="i.png" zy-footnote="注"/></a></p>"##;
        let out = expand_word_notes(html, DOC_BASE);
        assert!(out.contains(r##"<img src="i.png" zy-footnote="注"/>"##), "{out}");
        assert!(!out.contains("wr-note"), "{out}");
    }

    #[test]
    fn empty_note_text_leaves_the_markup_untouched() {
        // An attribute (or an aside) with no text would leave a marker nobody
        // can open, so both layouts keep their source markup instead.
        let we_read = r##"<p>知<span data-wr-footernote=""></span>行</p>"##;
        assert_eq!(expand_word_notes(we_read, DOC_BASE), we_read);
        let duokan = r##"<p>知<a href="#fn-1"><img zy-footnote=""/></a>行</p>"##;
        assert_eq!(expand_word_notes(duokan, DOC_BASE), duokan);
    }

    #[test]
    fn duokan_markers_in_headings_expand() {
        let html = concat!(
            r##"<h1>标题<a epub:type="noteref" href="#fn-h"><img zy-footnote="标题注"/></a></h1>"##,
            r##"<aside epub:type="footnote" id="fn-h">标题注</aside>"##,
        );
        let out = expand_word_notes(html, DOC_BASE);
        assert!(out.contains(r##"data-note="标题注""##), "{out}");
        assert!(!out.contains("<img"), "{out}");
    }
}
