//! Word-note expansion for EPUBs whose inline notes live in
//! `data-wr-footernote` attributes on empty spans. This is the layout used by
//! WeRead-exported Chinese classics (e.g. 《资治通鉴全本注译》, 179k notes):
//! the note text sits in an attribute and the span itself is empty, so the
//! notes are invisible unless the reader runs script — which we never do in
//! chapter iframes. We expand them on the Rust side instead:
//!
//! ```html
//! <p>…弃疑<span class="reader js_readerFooterNote" data-wr-footernote="弃疑：…"></span>，…</p>
//! ```
//! becomes (with `doc_base` = the rewritten absolute URL of this document,
//! e.g. `http://icedreader.localhost/book/{id}/OEBPS/Text/x.xhtml`):
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
//! as an ordinary annotated EPUB (东周列国志). Both links are absolute
//! same-document URLs (`doc_base#…`) because the chapter is displayed via
//! `srcDoc`, where bare `#fragment` hrefs cannot be routed by the reader; the
//! pair lets the reader jump either way. Note items avoid column breaks
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
    if !contains_ci(html, NOTE_ATTR) {
        return html.to_string();
    }
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
        // Copy everything before this container verbatim.
        out.push_str(&html[pos..open]);
        if is_self_closing(html, open_end) {
            out.push_str(&html[open..open_end]);
            pos = open_end;
            continue;
        }
        out.push_str(&html[open..open_end]);
        // Unclosed container swallows the rest of the document; bail verbatim.
        let Some(close) = find_close_tag(html, open_end, tag) else {
            out.push_str(&html[open_end..]);
            return out;
        };
        let body = &html[open_end..close];
        let (converted, notes) = convert_paragraph(body, &mut seq, doc_base);
        out.push_str(&converted);
        let close_end = tag_end(html, close).unwrap_or(close + tag.len() + 3);
        out.push_str(&html[close..close_end]);
        if !notes.is_empty() {
            out.push_str(&note_block(&notes, doc_base));
        }
        pos = close_end;
    }
    out.push_str(&html[pos..]);
    out
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

struct Note {
    /// Numbering inside the paragraph, starting at 1.
    label: usize,
    /// File-wide counter; also the anchor id suffix.
    seq: u64,
    text: String,
}

/// Turn a paragraph body into marker-bearing text plus its collected notes.
fn convert_paragraph(body: &str, seq: &mut u64, doc_base: &str) -> (String, Vec<Note>) {
    let mut out = String::with_capacity(body.len() + 64);
    let mut notes: Vec<Note> = Vec::new();
    let mut pos = 0usize;
    while pos < body.len() {
        let Some(start) = find_open_tag(body, pos, "span") else {
            out.push_str(&body[pos..]);
            break;
        };
        let Some(tag_end) = tag_end(body, start) else {
            out.push_str(&body[pos..]);
            break;
        };
        out.push_str(&body[pos..start]);
        let tag = &body[start..tag_end];
        if !contains_ci(tag, NOTE_ATTR) || is_self_closing(body, tag_end) {
            out.push_str(tag);
            pos = tag_end;
            continue;
        }
        let Some(text) = attr_value(tag, NOTE_ATTR) else {
            out.push_str(tag);
            pos = tag_end;
            continue;
        };
        // Locate the matching </span>, tolerating nested spans.
        let (inner_end, span_end) = match span_close(body, tag_end) {
            Some((a, b)) => (a, b),
            None => {
                out.push_str(tag);
                pos = tag_end;
                continue;
            }
        };
        let label = notes.len() + 1;
        *seq += 1;
        notes.push(Note {
            label,
            seq: *seq,
            text,
        });
        let inner = &body[tag_end..inner_end];
        let note_seq = notes.last().unwrap().seq;
        let marker_id = format!("{MARKER_ID_PREFIX}{note_seq}");
        let note_href = format!("{doc_base}#wr-note-{note_seq}");
        let _ = write!(
            out,
            r##"<a id="{marker_id}" class="{MARKER_CLASS}" data-label="{label}" data-note="{}" href="{}">"##,
            escape_attr(&notes.last().unwrap().text),
            escape_attr(&note_href)
        );
        // Keep whatever the original span contained (normally nothing).
        out.push_str(inner);
        out.push_str("</a>");
        pos = span_end;
    }
    (out, notes)
}

/// Markup for the trailing note list of one paragraph. Each item carries two
/// back links: the leading `[n]` label (always visible) and a textless
/// trailing link that only shows when the item itself is split across a page
/// break (see `TAIL_CLASS`). Both target the in-text marker.
fn note_block(notes: &[Note], doc_base: &str) -> String {
    let mut out = String::with_capacity(64 + notes.len() * 96);
    out.push_str(r#"<div class="wr-notes">"#);
    for note in notes {
        let back_href = format!("{doc_base}#{MARKER_ID_PREFIX}{}", note.seq);
        let _ = write!(
            out,
            r#"<p class="wr-note-item" id="wr-note-{}"><a class="{BACK_CLASS}" href="{}" title="返回正文">[{}]</a>{}"#,
            note.seq,
            escape_attr(&back_href),
            note.label,
            escape_text(&note.text)
        );
        let _ = write!(
            out,
            r#"<a class="{BACK_CLASS} {TAIL_CLASS}" href="{}"></a></p>"#,
            escape_attr(&back_href)
        );
    }
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
fn attr_value(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut search = 0usize;
    while search < lower.len() {
        let rel = lower[search..].find(name)?;
        let at = search + rel;
        let after = &lower[at + name.len()..];
        let after_trim = after.trim_start();
        if after_trim.starts_with('=') {
            let after_eq = after_trim[1..].trim_start();
            let mut quote: Option<char> = None;
            let mut end = 0usize;
            for (i, c) in after_eq.char_indices() {
                if i == 0 && (c == '"' || c == '\'') {
                    quote = Some(c);
                    continue;
                }
                match quote {
                    Some(q) if c == q => {
                        end = i;
                        break;
                    }
                    Some(_) => {}
                    None if c.is_ascii_whitespace() || c == '>' => {
                        end = i;
                        break;
                    }
                    None => {}
                }
            }
            let raw = &after_eq[..end];
            let raw = match quote {
                Some(q) if raw.starts_with(q) && raw.len() >= 1 => &raw[q.len_utf8()..],
                _ => raw,
            };
            return Some(decode_entities(raw));
        }
        search = at + name.len();
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
        assert_eq!(out.matches("<div class=\"wr-notes\">").count(), 2, "{out}");
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
        assert!(out.contains(r#"<a id="wr-note-back-1" class="wr-note" data-label="1""#), "{out}");
        assert!(out.contains(">原文</a>"), "{out}");
        assert!(out.contains("释义"), "{out}");
    }

    #[test]
    fn unclosed_paragraph_is_left_verbatim() {
        let html = r#"<p>开头<span data-wr-footernote="n"></span>没有闭合"#;
        assert_eq!(expand_word_notes(html, DOC_BASE), html);
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
}
