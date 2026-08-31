//! Custom fonts: CJK overlay, CSS rewrite, and `@font-face` injection.
//!
//! Applied only when the caller decides custom fonts are active (all four
//! slots present and 「使用原书字体」 is off). This crate does not inject
//! reader chrome.

use crate::settings::FontSlot;

pub const FAMILY_SERIF: &str = "IcedReaderSerif";
pub const FAMILY_SANS: &str = "IcedReaderSans";
pub const FAMILY_MONO: &str = "IcedReaderMono";

const STYLE_MARKER: &str = "data-icedreader-fonts";

/// Non-CJK codepoints so the serif/sans/mono files do not steal Han/kana/hangul.
pub const LATIN_UNICODE_RANGE: &str = "U+0000-10FF, U+1200-2E7F, U+A000-A95F, U+A980-ABFF, U+FB00-FB4F, U+FE20-FE2F";

/// Han, kana, hangul, CJK punctuation/fullwidth. No PUA, no Latin, no emoji.
pub const CJK_UNICODE_RANGE: &str = "\
U+1100-11FF, U+2E80-2FFF, U+3000-303F, U+3040-309F, U+30A0-30FF, \
U+3100-318F, U+3190-319F, U+31A0-31BF, U+31C0-31EF, U+31F0-31FF, \
U+3200-32FF, U+3300-33FF, U+3400-4DBF, U+4E00-9FFF, \
U+A960-A97F, U+AC00-D7AF, U+D7B0-D7FF, \
U+F900-FAFF, U+FE10-FE1F, U+FE30-FE4F, U+FF00-FFEF, \
U+1F200-1F2FF, \
U+20000-2A6DF, U+2A700-2B73F, U+2B740-2B81F, U+2B820-2CEAF, \
U+2CEB0-2EBEF, U+2EBF0-2EE5F, U+2F800-2FA1F, \
U+30000-3134F, U+31350-323AF";

#[derive(Debug, Clone)]
pub struct FontUrls {
    pub serif: String,
    pub sans: String,
    pub mono: String,
    pub cjk: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontKind {
    Ttf,
    Otf,
    Ttc,
    Woff,
    Woff2,
}

impl FontKind {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Ttf => "ttf",
            Self::Otf => "otf",
            Self::Ttc => "ttc",
            Self::Woff => "woff",
            Self::Woff2 => "woff2",
        }
    }

    pub fn mime(self) -> &'static str {
        match self {
            Self::Ttf => "font/ttf",
            Self::Otf => "font/otf",
            Self::Ttc => "font/collection",
            Self::Woff => "font/woff",
            Self::Woff2 => "font/woff2",
        }
    }
}

pub fn sniff_font(bytes: &[u8]) -> Option<FontKind> {
    if bytes.len() < 4 {
        return None;
    }
    match &bytes[0..4] {
        b"OTTO" => Some(FontKind::Otf),
        b"true" | [0x00, 0x01, 0x00, 0x00] => Some(FontKind::Ttf),
        b"ttcf" => Some(FontKind::Ttc),
        b"wOFF" => Some(FontKind::Woff),
        b"wOF2" => Some(FontKind::Woff2),
        _ => None,
    }
}

pub fn font_override_css(urls: &FontUrls) -> String {
    let mut css = String::new();
    for (family, url) in [
        (FAMILY_SERIF, urls.serif.as_str()),
        (FAMILY_SANS, urls.sans.as_str()),
        (FAMILY_MONO, urls.mono.as_str()),
    ] {
        css.push_str(&format!(
            "@font-face {{\n  font-family: \"{family}\";\n  src: url(\"{url}\");\n  font-display: swap;\n  unicode-range: {LATIN_UNICODE_RANGE};\n}}\n"
        ));
        css.push_str(&format!(
            "@font-face {{\n  font-family: \"{family}\";\n  src: url(\"{cjk}\");\n  font-display: swap;\n  unicode-range: {CJK_UNICODE_RANGE};\n}}\n",
            cjk = urls.cjk
        ));
    }
    css.push_str(&format!(
        "html {{ font-family: \"{FAMILY_SERIF}\"; }}\n\
code, kbd, samp, pre, tt, var {{ font-family: \"{FAMILY_MONO}\"; }}\n"
    ));
    css
}

pub fn apply_custom_fonts(html: &str, urls: &FontUrls) -> String {
    if html.contains(STYLE_MARKER) {
        return html.to_string();
    }
    let rewritten = rewrite_html_fonts(html);
    inject_font_style(&rewritten, &font_override_css(urls))
}

pub fn rewrite_html_fonts(html: &str) -> String {
    let lower = html.to_ascii_lowercase();
    let mut out = String::with_capacity(html.len());
    let mut i = 0;
    while i < html.len() {
        let tag = find_style_tag(&lower, i);
        let attr = find_style_attr(&lower, i);
        match (tag, attr) {
            (None, None) => {
                out.push_str(&html[i..]);
                break;
            }
            (Some(t), Some(a)) if a < t => i = write_style_attr(html, &mut out, i, a),
            (Some(t), _) => i = write_style_tag(html, &lower, &mut out, i, t),
            (None, Some(a)) => i = write_style_attr(html, &mut out, i, a),
        }
    }
    out
}

pub fn rewrite_css_font_families(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let lower = input.to_ascii_lowercase();
    let bytes = input.as_bytes();
    let n = bytes.len();
    let mut i = 0;
    while i < n {
        if starts_comment(bytes, i) {
            let end = skip_comment(bytes, i);
            out.push_str(&input[i..end]);
            i = end;
            continue;
        }
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let end = skip_string(bytes, i);
            out.push_str(&input[i..end]);
            i = end;
            continue;
        }
        if is_at_font_face(&lower, i) {
            let end = skip_brace_block(bytes, i);
            out.push_str(&input[i..end]);
            i = end;
            continue;
        }
        if is_font_family_at(&lower, i) {
            let prop_len = "font-family".len();
            out.push_str(&input[i..i + prop_len]);
            i += prop_len;
            let ws_start = i;
            while i < n && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            if i >= n || bytes[i] != b':' {
                out.push_str(&input[ws_start..i]);
                continue;
            }
            out.push_str(&input[ws_start..=i]);
            i += 1;
            let pad_start = i;
            while i < n && bytes[i].is_ascii_whitespace() {
                i += 1;
            }
            out.push_str(&input[pad_start..i]);
            let val_end = scan_declaration_value(bytes, i);
            out.push_str(&map_font_family_value(&input[i..val_end]));
            i = val_end;
            continue;
        }
        let ch = input[i..].chars().next().expect("index on char boundary");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

fn inject_font_style(html: &str, css: &str) -> String {
    let style = format!("<style type=\"text/css\" {STYLE_MARKER}=\"1\">\n{css}</style>");
    if let Some(gt) = find_open_tag_end(html, "head") {
        let at = gt + 1;
        let mut s = String::with_capacity(html.len() + style.len());
        s.push_str(&html[..at]);
        s.push_str(&style);
        s.push_str(&html[at..]);
        s
    } else if let Some(i) = find_ci(html, "<body") {
        let mut s = String::with_capacity(html.len() + style.len());
        s.push_str(&html[..i]);
        s.push_str(&style);
        s.push_str(&html[i..]);
        s
    } else {
        format!("{style}{html}")
    }
}

fn find_open_tag_end(html: &str, name: &str) -> Option<usize> {
    let lower = html.to_ascii_lowercase();
    let needle = format!("<{name}");
    let mut search = 0;
    while let Some(rel) = lower[search..].find(&needle) {
        let i = search + rel;
        let after = i + needle.len();
        let next = lower.as_bytes().get(after).copied().unwrap_or(b'>');
        if next == b'>' || next.is_ascii_whitespace() {
            return html[after..].find('>').map(|r| after + r);
        }
        search = after;
    }
    None
}

fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    hay.to_ascii_lowercase().find(&needle.to_ascii_lowercase())
}

fn find_style_tag(lower: &str, from: usize) -> Option<usize> {
    let mut search = from;
    while let Some(rel) = lower[search..].find("<style") {
        let i = search + rel;
        let after = i + "<style".len();
        let next = lower.as_bytes().get(after).copied().unwrap_or(b'>');
        if next == b'>' || next == b'/' || next.is_ascii_whitespace() {
            return Some(i);
        }
        search = after;
    }
    None
}

fn find_style_attr(lower: &str, from: usize) -> Option<usize> {
    let mut search = from;
    while let Some(rel) = lower[search..].find("style") {
        let i = search + rel;
        if i > 0 {
            let prev = lower.as_bytes()[i - 1];
            if !prev.is_ascii_whitespace() {
                search = i + 5;
                continue;
            }
        }
        let mut j = i + 5;
        let bytes = lower.as_bytes();
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if j < bytes.len() && bytes[j] == b'=' {
            return Some(i);
        }
        search = i + 5;
    }
    None
}

fn write_style_tag(html: &str, lower: &str, out: &mut String, from: usize, tag_at: usize) -> usize {
    out.push_str(&html[from..tag_at]);
    let after_name = tag_at + "<style".len();
    let Some(rel_gt) = html[after_name..].find('>') else {
        out.push_str(&html[tag_at..]);
        return html.len();
    };
    let gt = after_name + rel_gt;
    out.push_str(&html[tag_at..=gt]);
    if html.as_bytes()[gt.saturating_sub(1)] == b'/' {
        return gt + 1;
    }
    let close = lower[gt + 1..]
        .find("</style>")
        .map(|r| gt + 1 + r);
    let Some(close_at) = close else {
        let rewritten = rewrite_css_font_families(&html[gt + 1..]);
        out.push_str(&rewritten);
        return html.len();
    };
    let inner = &html[gt + 1..close_at];
    out.push_str(&rewrite_css_font_families(inner));
    out.push_str(&html[close_at..close_at + "</style>".len()]);
    close_at + "</style>".len()
}

fn write_style_attr(html: &str, out: &mut String, from: usize, attr_at: usize) -> usize {
    out.push_str(&html[from..attr_at]);
    let bytes = html.as_bytes();
    let mut i = attr_at + 5;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if i >= bytes.len() || bytes[i] != b'=' {
        out.push_str(&html[attr_at..attr_at + 5]);
        return attr_at + 5;
    }
    i += 1;
    while i < bytes.len() && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if i >= bytes.len() || (bytes[i] != b'"' && bytes[i] != b'\'') {
        out.push_str(&html[attr_at..i]);
        return i;
    }
    let quote = bytes[i];
    let val_start = i + 1;
    let mut j = val_start;
    while j < bytes.len() && bytes[j] != quote {
        j += 1;
    }
    out.push_str(&html[attr_at..val_start]);
    out.push_str(&rewrite_css_font_families(&html[val_start..j]));
    if j < bytes.len() {
        out.push(quote as char);
        j += 1;
    }
    j
}

fn is_at_font_face(lower: &str, i: usize) -> bool {
    const NEEDLE: &str = "@font-face";
    if i + NEEDLE.len() > lower.len() || !lower[i..].starts_with(NEEDLE) {
        return false;
    }
    let after = i + NEEDLE.len();
    let bytes = lower.as_bytes();
    after >= bytes.len() || !is_ident_char(bytes[after])
}

fn skip_brace_block(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && bytes[i] != b'{' {
        if starts_comment(bytes, i) {
            i = skip_comment(bytes, i);
            continue;
        }
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            i = skip_string(bytes, i);
            continue;
        }
        i += 1;
    }
    let mut depth = 0;
    while i < bytes.len() {
        if starts_comment(bytes, i) {
            i = skip_comment(bytes, i);
            continue;
        }
        let c = bytes[i];
        if c == b'"' || c == b'\'' {
            i = skip_string(bytes, i);
            continue;
        }
        if c == b'{' {
            depth += 1;
        } else if c == b'}' {
            depth -= 1;
            i += 1;
            if depth == 0 {
                return i;
            }
            continue;
        }
        i += 1;
    }
    bytes.len()
}

fn is_font_family_at(lower: &str, i: usize) -> bool {
    const NEEDLE: &str = "font-family";
    if i + NEEDLE.len() > lower.len() || !lower[i..].starts_with(NEEDLE) {
        return false;
    }
    let bytes = lower.as_bytes();
    if i > 0 && is_ident_char(bytes[i - 1]) {
        return false;
    }
    let after = i + NEEDLE.len();
    if after < bytes.len() && is_ident_char(bytes[after]) {
        return false;
    }
    true
}

fn map_font_family_value(value: &str) -> String {
    let trimmed = value.trim_end();
    let (main, important) = split_important(trimmed);
    let main = main.trim();
    if is_css_wide_keyword(main) {
        return trimmed.to_string();
    }
    let family = match classify_family_list(main) {
        FontSlot::Mono => FAMILY_MONO,
        FontSlot::Sans => FAMILY_SANS,
        _ => FAMILY_SERIF,
    };
    if important {
        format!("\"{family}\" !important")
    } else {
        format!("\"{family}\"")
    }
}

fn classify_family_list(value: &str) -> FontSlot {
    let mut saw_sans = false;
    for item in split_comma_list(value) {
        let token = unquote(item.trim()).to_ascii_lowercase();
        match token.as_str() {
            "monospace" | "ui-monospace" => return FontSlot::Mono,
            "sans-serif" | "ui-sans-serif" | "system-ui" => saw_sans = true,
            _ => {}
        }
    }
    if saw_sans {
        FontSlot::Sans
    } else {
        FontSlot::Serif
    }
}

fn split_comma_list(value: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut quote: Option<char> = None;
    for (i, ch) in value.char_indices() {
        match (quote, ch) {
            (None, '"' | '\'') => quote = Some(ch),
            (Some(q), c) if c == q => quote = None,
            (None, ',') => {
                parts.push(&value[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&value[start..]);
    parts
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    if s.len() >= 2 {
        let bytes = s.as_bytes();
        if (bytes[0] == b'"' && bytes[s.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[s.len() - 1] == b'\'')
        {
            return &s[1..s.len() - 1];
        }
    }
    s
}

fn split_important(value: &str) -> (&str, bool) {
    let trimmed = value.trim_end();
    let lower = trimmed.to_ascii_lowercase();
    if let Some(rest) = lower.strip_suffix("!important") {
        let cut = rest.len();
        (trimmed[..cut].trim_end(), true)
    } else {
        (trimmed, false)
    }
}

fn is_css_wide_keyword(value: &str) -> bool {
    matches!(
        value.to_ascii_lowercase().as_str(),
        "inherit" | "initial" | "unset" | "revert" | "revert-layer"
    )
}

fn starts_comment(bytes: &[u8], i: usize) -> bool {
    i + 1 < bytes.len() && bytes[i] == b'/' && bytes[i + 1] == b'*'
}

fn skip_comment(bytes: &[u8], i: usize) -> usize {
    let mut j = i + 2;
    while j + 1 < bytes.len() {
        if bytes[j] == b'*' && bytes[j + 1] == b'/' {
            return j + 2;
        }
        j += 1;
    }
    bytes.len()
}

fn skip_string(bytes: &[u8], i: usize) -> usize {
    let quote = bytes[i];
    let mut j = i + 1;
    while j < bytes.len() {
        if bytes[j] == b'\\' {
            j = (j + 2).min(bytes.len());
            continue;
        }
        if bytes[j] == quote {
            return j + 1;
        }
        j += 1;
    }
    bytes.len()
}

fn scan_declaration_value(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() {
        if starts_comment(bytes, i) {
            i = skip_comment(bytes, i);
            continue;
        }
        let c = bytes[i];
        if c == b'"' || c == b'\'' {
            i = skip_string(bytes, i);
            continue;
        }
        if c == b';' || c == b'}' || c == b'{' {
            return i;
        }
        i += 1;
    }
    bytes.len()
}

fn is_ident_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'-' || c == b'_'
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ranges(spec: &str) -> Vec<(u32, u32)> {
        spec.split(',')
            .map(|part| {
                let part = part.trim().trim_start_matches("U+");
                if let Some((a, b)) = part.split_once('-') {
                    (u32::from_str_radix(a, 16).unwrap(), u32::from_str_radix(b, 16).unwrap())
                } else {
                    let v = u32::from_str_radix(part, 16).unwrap();
                    (v, v)
                }
            })
            .collect()
    }

    fn in_ranges(spec: &str, cp: u32) -> bool {
        parse_ranges(spec).iter().any(|(a, b)| cp >= *a && cp <= *b)
    }

    #[test]
    fn sniff_known_headers() {
        assert_eq!(sniff_font(b"\0\x01\0\0xxxx"), Some(FontKind::Ttf));
        assert_eq!(sniff_font(b"OTTOxxxx"), Some(FontKind::Otf));
        assert_eq!(sniff_font(b"ttcfxxxx"), Some(FontKind::Ttc));
        assert_eq!(sniff_font(b"wOFFxxxx"), Some(FontKind::Woff));
        assert_eq!(sniff_font(b"wOF2xxxx"), Some(FontKind::Woff2));
        assert_eq!(sniff_font(b"XXXX"), None);
        assert_eq!(sniff_font(b"AB"), None);
    }

    #[test]
    fn cjk_range_covers_han_kana_hangul_not_pua_or_latin() {
        assert!(in_ranges(CJK_UNICODE_RANGE, 0x4E00));
        assert!(in_ranges(CJK_UNICODE_RANGE, 0x3042));
        assert!(in_ranges(CJK_UNICODE_RANGE, 0x30A2));
        assert!(in_ranges(CJK_UNICODE_RANGE, 0xAC00));
        assert!(in_ranges(CJK_UNICODE_RANGE, 0x3000));
        assert!(in_ranges(CJK_UNICODE_RANGE, 0xFF01));
        assert!(!in_ranges(CJK_UNICODE_RANGE, 0x0041));
        assert!(!in_ranges(CJK_UNICODE_RANGE, 0xE000));
        assert!(!in_ranges(CJK_UNICODE_RANGE, 0xF8FF));
        assert!(!in_ranges(LATIN_UNICODE_RANGE, 0x4E00));
        assert!(!in_ranges(LATIN_UNICODE_RANGE, 0x1100));
        assert!(in_ranges(LATIN_UNICODE_RANGE, 0x0041));
    }

    #[test]
    fn rewrite_maps_generics_and_named_stacks() {
        let css = r#"
/* font-family: serif; */
body { font-family: "Times New Roman", Georgia, serif; }
.ui { font-family: Arial, sans-serif; }
code { font-family: Consolas, monospace; }
p { font-family: "SimSun"; }
em { font-family: inherit; }
h1 { font-family: "PingFang SC", sans-serif !important; }
"#;
        let out = rewrite_css_font_families(css);
        assert!(out.contains("/* font-family: serif; */"), "{out}");
        assert!(out.contains("body { font-family: \"IcedReaderSerif\"; }"), "{out}");
        assert!(out.contains(".ui { font-family: \"IcedReaderSans\"; }"), "{out}");
        assert!(out.contains("code { font-family: \"IcedReaderMono\"; }"), "{out}");
        assert!(out.contains("p { font-family: \"IcedReaderSerif\"; }"), "{out}");
        assert!(out.contains("em { font-family: inherit; }"), "{out}");
        assert!(
            out.contains("h1 { font-family: \"IcedReaderSans\" !important; }"),
            "{out}"
        );
    }

    #[test]
    fn rewrite_skips_embedded_font_face() {
        let css = r#"
@font-face { font-family: "MyEmb"; src: url(x.ttf); }
body { font-family: "MyEmb", serif; }
"#;
        let out = rewrite_css_font_families(css);
        assert!(out.contains("@font-face { font-family: \"MyEmb\"; src: url(x.ttf); }"), "{out}");
        assert!(out.contains("body { font-family: \"IcedReaderSerif\"; }"), "{out}");
    }

    #[test]
    fn rewrite_html_style_tag_and_attribute() {
        let html = r#"<html><head><style>p { font-family: serif; }</style></head>
<body><p style="font-family: sans-serif; color: red">hi</p></body></html>"#;
        let out = rewrite_html_fonts(html);
        assert!(out.contains("font-family: \"IcedReaderSerif\""), "{out}");
        assert!(out.contains("font-family: \"IcedReaderSans\""), "{out}");
        assert!(out.contains("color: red"), "{out}");
    }

    #[test]
    fn apply_injects_marker_and_cjk_overlay() {
        let html = "<html><head></head><body><p>你好</p></body></html>";
        let urls = FontUrls {
            serif: "http://icedreader.localhost/fonts/serif".into(),
            sans: "http://icedreader.localhost/fonts/sans".into(),
            mono: "http://icedreader.localhost/fonts/mono".into(),
            cjk: "http://icedreader.localhost/fonts/cjk".into(),
        };
        let out = apply_custom_fonts(html, &urls);
        assert!(out.contains(STYLE_MARKER));
        assert!(out.contains("IcedReaderSerif"));
        assert!(out.contains("unicode-range: U+1100-11FF"));
        assert!(out.contains("/fonts/cjk"));
        assert_eq!(apply_custom_fonts(&out, &urls), out);

        let with_book = apply_custom_fonts(
            "<html><head><style>body { font-family: sans-serif; }</style></head><body></body></html>",
            &urls,
        );
        let ours = with_book.find(STYLE_MARKER).expect("injected");
        let book = with_book.find("IcedReaderSans").expect("rewritten book css");
        assert!(ours < book, "reader @font-face must precede publisher CSS so body sans still wins");
    }
}
