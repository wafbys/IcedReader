//! Publisher font declarations as written in the current chapter.
//! This is the book's CSS, not the used glyph or our override families.

use std::collections::HashSet;

use serde::Serialize;

const MAX_DECLS: usize = 80;
const MAX_FACES: usize = 32;
const MAX_FILES: usize = 24;
const MAX_IMPORT_DEPTH: usize = 6;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PublisherFontDecl {
    pub selector: String,
    pub value: String,
    pub source: String,
}

#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PublisherFontReport {
    pub declarations: Vec<PublisherFontDecl>,
    pub faces: Vec<String>,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChapterView {
    pub html: String,
    pub publisher_fonts: PublisherFontReport,
}

pub fn collect_publisher_fonts(
    html: &str,
    resource_base: &str,
    chapter_href: &str,
    mut load: impl FnMut(&str) -> Option<String>,
) -> PublisherFontReport {
    let mut ctx = CollectCtx {
        declarations: Vec::new(),
        faces: Vec::new(),
        seen_files: HashSet::new(),
        seen_decl: HashSet::new(),
        truncated: false,
    };
    let chapter_key = normalize_book_href(chapter_href);

    collect_html(html, resource_base, &chapter_key, &mut load, &mut ctx);
    PublisherFontReport {
        declarations: ctx.declarations,
        faces: ctx.faces,
        truncated: ctx.truncated,
    }
}

struct CollectCtx {
    declarations: Vec<PublisherFontDecl>,
    faces: Vec<String>,
    seen_files: HashSet<String>,
    seen_decl: HashSet<String>,
    truncated: bool,
}

fn collect_html(
    html: &str,
    resource_base: &str,
    chapter_key: &str,
    load: &mut impl FnMut(&str) -> Option<String>,
    ctx: &mut CollectCtx,
) {
    let lower = html.to_ascii_lowercase();
    collect_pi_stylesheets(html, &lower, resource_base, chapter_key, load, ctx);
    collect_link_stylesheets(html, &lower, resource_base, chapter_key, load, ctx);
    collect_style_elements(html, &lower, chapter_key, load, ctx);
    collect_style_attributes(html, &lower, ctx);
    collect_font_face_attrs(html, &lower, ctx);
}

fn collect_pi_stylesheets(
    html: &str,
    lower: &str,
    resource_base: &str,
    chapter_key: &str,
    load: &mut impl FnMut(&str) -> Option<String>,
    ctx: &mut CollectCtx,
) {
    let mut search = 0;
    while let Some(rel) = lower[search..].find("<?xml-stylesheet") {
        let start = search + rel;
        let Some(end_rel) = html[start..].find("?>") else {
            break;
        };
        let tag = &html[start..start + end_rel];
        let attrs = parse_attrs(tag.trim_start_matches("<?xml-stylesheet"));
        if let Some(href) = attr(&attrs, "href") {
            ingest_href(href, resource_base, chapter_key, load, ctx, 0);
        }
        search = start + end_rel + 2;
    }
}

fn collect_link_stylesheets(
    html: &str,
    lower: &str,
    resource_base: &str,
    chapter_key: &str,
    load: &mut impl FnMut(&str) -> Option<String>,
    ctx: &mut CollectCtx,
) {
    let mut search = 0;
    while let Some(rel) = lower[search..].find("<link") {
        let start = search + rel;
        let after = start + 5;
        let next = lower.as_bytes().get(after).copied().unwrap_or(b'>');
        if next != b'>' && !next.is_ascii_whitespace() && next != b'/' {
            search = after;
            continue;
        }
        let Some(gt) = html[after..].find('>') else {
            break;
        };
        let tag = &html[start + 5..after + gt];
        let attrs = parse_attrs(tag);
        let rel_v = attr(&attrs, "rel").map(|s| s.to_ascii_lowercase());
        let type_v = attr(&attrs, "type").map(|s| s.to_ascii_lowercase());
        let is_css = rel_v.as_deref().is_some_and(|r| r.split_whitespace().any(|p| p == "stylesheet"))
            || type_v.as_deref() == Some("text/css");
        if is_css {
            if let Some(href) = attr(&attrs, "href") {
                ingest_href(href, resource_base, chapter_key, load, ctx, 0);
            }
        }
        search = after + gt + 1;
    }
}

fn collect_style_elements(
    html: &str,
    lower: &str,
    chapter_key: &str,
    load: &mut impl FnMut(&str) -> Option<String>,
    ctx: &mut CollectCtx,
) {
    let mut search = 0;
    while let Some(rel) = lower[search..].find("<style") {
        let start = search + rel;
        let after = start + 6;
        let next = lower.as_bytes().get(after).copied().unwrap_or(b'>');
        if next != b'>' && next != b'/' && !next.is_ascii_whitespace() {
            search = after;
            continue;
        }
        let Some(gt_rel) = html[after..].find('>') else {
            break;
        };
        let gt = after + gt_rel;
        let open = &html[start..gt + 1];
        if open.to_ascii_lowercase().contains("data-icedreader-fonts") {
            search = gt + 1;
            continue;
        }
        if html.as_bytes().get(gt.saturating_sub(1)) == Some(&b'/') {
            search = gt + 1;
            continue;
        }
        let close = lower[gt + 1..].find("</style>").map(|r| gt + 1 + r);
        let Some(close_at) = close else {
            break;
        };
        let inner = strip_html_comment_wrappers(&html[gt + 1..close_at]);
        ingest_css(
            &inner,
            "本章 <style>",
            chapter_key,
            load,
            ctx,
            0,
            "",
        );
        search = close_at + 8;
    }
}

fn collect_style_attributes(html: &str, lower: &str, ctx: &mut CollectCtx) {
    let mut search = 0;
    while let Some(rel) = lower[search..].find("style") {
        let i = search + rel;
        if i > 0 {
            let prev = lower.as_bytes()[i - 1];
            if !prev.is_ascii_whitespace() {
                search = i + 5;
                continue;
            }
        }
        let bytes = lower.as_bytes();
        let mut j = i + 5;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if j >= bytes.len() || bytes[j] != b'=' {
            search = i + 5;
            continue;
        }
        j += 1;
        while j < bytes.len() && bytes[j].is_ascii_whitespace() {
            j += 1;
        }
        if j >= html.len() {
            break;
        }
        let quote = html.as_bytes()[j];
        if quote != b'"' && quote != b'\'' {
            search = j;
            continue;
        }
        let val_start = j + 1;
        let mut k = val_start;
        let raw = html.as_bytes();
        while k < raw.len() && raw[k] != quote {
            k += 1;
        }
        let value = &html[val_start..k];
        let selector = tag_name_before(html, i)
            .map(|t| format!("{t}[style]"))
            .unwrap_or_else(|| "[style 属性]".into());
        take_props_from_block(value, &selector, "本章 HTML", ctx);
        search = if k < raw.len() { k + 1 } else { k };
    }
}

fn collect_font_face_attrs(html: &str, lower: &str, ctx: &mut CollectCtx) {
    let mut search = 0;
    while let Some(rel) = lower[search..].find("<font") {
        let start = search + rel;
        let after = start + 5;
        let next = lower.as_bytes().get(after).copied().unwrap_or(b'>');
        if next != b'>' && !next.is_ascii_whitespace() && next != b'/' {
            search = after;
            continue;
        }
        let Some(gt) = html[after..].find('>') else {
            break;
        };
        let attrs = parse_attrs(&html[start + 5..after + gt]);
        if let Some(face) = attr(&attrs, "face") {
            push_decl(ctx, "font", face, "本章 HTML face");
        }
        search = after + gt + 1;
    }
}

fn ingest_href(
    href: &str,
    resource_base: &str,
    base_file: &str,
    load: &mut impl FnMut(&str) -> Option<String>,
    ctx: &mut CollectCtx,
    depth: usize,
) {
    let resolved = resolve_href(href, resource_base, base_file);
    if resolved.is_empty() || is_external(&resolved) {
        return;
    }
    ingest_file(&resolved, load, ctx, depth);
}

fn ingest_file(
    book_href: &str,
    load: &mut impl FnMut(&str) -> Option<String>,
    ctx: &mut CollectCtx,
    depth: usize,
) {
    if depth > MAX_IMPORT_DEPTH || ctx.seen_files.len() >= MAX_FILES {
        ctx.truncated = true;
        return;
    }
    let key = normalize_book_href(book_href);
    if key.is_empty() || !ctx.seen_files.insert(key.clone()) {
        return;
    }
    let css = load(&key).or_else(|| load(&format!("/{key}")));
    let Some(css) = css else {
        return;
    };
    let source = file_name(&key);
    ingest_css(&css, &source, &key, load, ctx, depth, "");
}

fn ingest_css(
    css: &str,
    source: &str,
    file_href: &str,
    load: &mut impl FnMut(&str) -> Option<String>,
    ctx: &mut CollectCtx,
    depth: usize,
    media: &str,
) {
    let stripped = strip_css_comments(css);
    walk_stylesheet(&stripped, &mut |kind| match kind {
        Statement::Rule { selector, body } => {
            let sel = with_media(media, &compact_ws(selector));
            take_props_from_block(body, &sel, source, ctx);
        }
        Statement::At { name, prelude, block } => {
            let name_l = name.to_ascii_lowercase();
            if name_l == "import" {
                if let Some(url) = parse_import_target(prelude) {
                    ingest_href(&url, "", file_href, load, ctx, depth + 1);
                }
            } else if name_l == "font-face" {
                if let Some(block) = block {
                    if let Some(family) = prop_value(block, "font-family") {
                        push_face(ctx, &strip_quotes(family.trim()));
                    }
                }
            } else if matches!(name_l.as_str(), "media" | "supports" | "layer") {
                if let Some(block) = block {
                    let label = format!("@{name_l} {}", compact_ws(prelude));
                    let nested = if media.is_empty() {
                        label
                    } else {
                        format!("{media} · {label}")
                    };
                    ingest_css(block, source, file_href, load, ctx, depth, &nested);
                }
            }
        }
    });
}

fn take_props_from_block(block: &str, selector: &str, source: &str, ctx: &mut CollectCtx) {
    for (name, value) in iter_properties(block) {
        let lname = name.to_ascii_lowercase();
        if lname == "font-family" {
            push_decl(ctx, selector, value.trim(), source);
        } else if lname == "font" {
            if let Some(family) = family_from_font_shorthand(value) {
                push_decl(ctx, selector, &family, source);
            }
        }
    }
}

fn push_decl(ctx: &mut CollectCtx, selector: &str, value: &str, source: &str) {
    let selector = if selector.is_empty() {
        "[未写选择器]".to_string()
    } else {
        compact_ws(selector)
    };
    let value = compact_ws(value);
    if value.is_empty() {
        return;
    }
    let key = format!("{selector}\n{value}\n{source}");
    if !ctx.seen_decl.insert(key) {
        return;
    }
    if ctx.declarations.len() >= MAX_DECLS {
        ctx.truncated = true;
        return;
    }
    ctx.declarations.push(PublisherFontDecl {
        selector,
        value,
        source: source.to_string(),
    });
}

fn push_face(ctx: &mut CollectCtx, family: &str) {
    if family.is_empty() {
        return;
    }
    if ctx.faces.iter().any(|f| f == family) {
        return;
    }
    if ctx.faces.len() >= MAX_FACES {
        ctx.truncated = true;
        return;
    }
    ctx.faces.push(family.to_string());
}

enum Statement<'a> {
    Rule {
        selector: &'a str,
        body: &'a str,
    },
    At {
        name: &'a str,
        prelude: &'a str,
        block: Option<&'a str>,
    },
}

fn walk_stylesheet<'a>(css: &'a str, on: &mut impl FnMut(Statement<'a>)) {
    let bytes = css.as_bytes();
    let n = bytes.len();
    let mut i = 0;
    while i < n {
        while i < n && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= n {
            break;
        }
        if bytes[i] == b'@' {
            let name_start = i + 1;
            let mut name_end = name_start;
            while name_end < n && is_ident_char(bytes[name_end]) {
                name_end += 1;
            }
            let name = &css[name_start..name_end];
            i = name_end;
            let prelude_start = i;
            while i < n {
                if bytes[i] == b'"' || bytes[i] == b'\'' {
                    i = skip_string(bytes, i);
                    continue;
                }
                if bytes[i] == b'{' || bytes[i] == b';' {
                    break;
                }
                i += 1;
            }
            let prelude = css[prelude_start..i].trim();
            if i < n && bytes[i] == b'{' {
                let (body, end) = slice_block(css, i);
                on(Statement::At {
                    name,
                    prelude,
                    block: Some(body),
                });
                i = end;
            } else {
                on(Statement::At {
                    name,
                    prelude,
                    block: None,
                });
                if i < n && bytes[i] == b';' {
                    i += 1;
                }
            }
            continue;
        }
        let sel_start = i;
        while i < n {
            if bytes[i] == b'"' || bytes[i] == b'\'' {
                i = skip_string(bytes, i);
                continue;
            }
            if bytes[i] == b'{' || bytes[i] == b'}' {
                break;
            }
            i += 1;
        }
        if i >= n {
            break;
        }
        if bytes[i] == b'}' {
            i += 1;
            continue;
        }
        let selector = css[sel_start..i].trim();
        let (body, end) = slice_block(css, i);
        on(Statement::Rule { selector, body });
        i = end;
    }
}

fn slice_block(css: &str, open_at: usize) -> (&str, usize) {
    let bytes = css.as_bytes();
    let mut i = open_at;
    let mut depth = 0;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'"' || c == b'\'' {
            i = skip_string(bytes, i);
            continue;
        }
        if c == b'{' {
            depth += 1;
        } else if c == b'}' {
            depth -= 1;
            if depth == 0 {
                return (&css[open_at + 1..i], i + 1);
            }
        }
        i += 1;
    }
    (&css[open_at + 1..], bytes.len())
}

fn iter_properties(block: &str) -> Vec<(&str, &str)> {
    let bytes = block.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b';') {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let name_start = i;
        while i < bytes.len() && is_ident_char(bytes[i]) {
            i += 1;
        }
        let name = &block[name_start..i];
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b':' {
            i += 1;
            continue;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        let val_start = i;
        let val_end = scan_declaration_value(bytes, i);
        if !name.is_empty() {
            out.push((name, block[val_start..val_end].trim()));
        }
        i = val_end;
        if i < bytes.len() && bytes[i] == b';' {
            i += 1;
        }
    }
    out
}

fn prop_value<'a>(block: &'a str, name: &str) -> Option<&'a str> {
    let needle = name.to_ascii_lowercase();
    iter_properties(block)
        .into_iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(&needle))
        .map(|(_, v)| v)
}

fn family_from_font_shorthand(value: &str) -> Option<String> {
    let trimmed = value.trim();
    let (main, important) = split_important(trimmed);
    let main = main.trim();
    if main.is_empty() {
        return None;
    }
    if is_css_wide_keyword(main) {
        return Some(keep_important(main, important));
    }
    let tokens = tokenize(main);
    let mut i = 0;
    while i < tokens.len() {
        if is_font_size(&tokens[i]) {
            i += 1;
            if i < tokens.len() && tokens[i] == "/" {
                i += 1;
                if i < tokens.len() {
                    i += 1;
                }
            }
            if i >= tokens.len() {
                return None;
            }
            let family = tokens[i..].join(" ");
            return Some(keep_important(&family, important));
        }
        i += 1;
    }
    None
}

fn keep_important(value: &str, important: bool) -> String {
    if important {
        format!("{value} !important")
    } else {
        value.to_string()
    }
}

fn is_font_size(tok: &str) -> bool {
    let t = tok.to_ascii_lowercase();
    matches!(
        t.as_str(),
        "xx-small"
            | "x-small"
            | "small"
            | "medium"
            | "large"
            | "x-large"
            | "xx-large"
            | "xxx-large"
            | "smaller"
            | "larger"
            | "math"
    ) || t.starts_with(|c: char| c.is_ascii_digit() || c == '.')
}

fn tokenize(s: &str) -> Vec<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        if bytes[i] == b'/' {
            out.push("/".into());
            i += 1;
            continue;
        }
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let end = skip_string(bytes, i);
            out.push(s[i..end].to_string());
            i = end;
            continue;
        }
        let start = i;
        while i < bytes.len()
            && !bytes[i].is_ascii_whitespace()
            && bytes[i] != b'/'
            && bytes[i] != b'"'
            && bytes[i] != b'\''
        {
            i += 1;
        }
        out.push(s[start..i].to_string());
    }
    out
}

fn parse_import_target(prelude: &str) -> Option<String> {
    let s = prelude.trim();
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("url(") {
        let rest = &s[4..];
        let inner = rest.split(')').next().unwrap_or(rest).trim();
        return Some(strip_quotes(inner));
    }
    if s.starts_with('"') || s.starts_with('\'') {
        let end = skip_string(s.as_bytes(), 0);
        return Some(strip_quotes(&s[..end]));
    }
    None
}

fn resolve_href(href: &str, resource_base: &str, base_file: &str) -> String {
    let href = href.trim();
    let href = href.split(['#', '?']).next().unwrap_or(href).trim();
    if href.is_empty() {
        return String::new();
    }
    if !resource_base.is_empty() {
        if let Some(rest) = href.strip_prefix(resource_base) {
            return normalize_book_href(rest);
        }
    }
    if let Some(rest) = strip_icedreader_book_path(href) {
        return normalize_book_href(rest);
    }
    if is_external(href) {
        return href.to_string();
    }
    if href.starts_with('/') {
        return normalize_book_href(href);
    }
    join_href(base_file, href)
}

fn strip_icedreader_book_path(href: &str) -> Option<&str> {
    const PREFIXES: [&str; 3] = [
        "http://icedreader.localhost/book/",
        "https://icedreader.localhost/book/",
        "icedreader://localhost/book/",
    ];
    for prefix in PREFIXES {
        if let Some(rest) = href.strip_prefix(prefix) {
            return rest.split_once('/').map(|(_, path)| path);
        }
    }
    None
}

fn join_href(base_file: &str, rel: &str) -> String {
    let rel = normalize_book_href(rel);
    if rel.starts_with("http:") || rel.starts_with("https:") || rel.starts_with("icedreader:") {
        return rel;
    }
    let base = normalize_book_href(base_file);
    let dir = base.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let mut parts: Vec<&str> = if dir.is_empty() {
        Vec::new()
    } else {
        dir.split('/').filter(|s| !s.is_empty()).collect()
    };
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    parts.join("/")
}

fn normalize_book_href(href: &str) -> String {
    href.replace('\\', "/")
        .trim()
        .trim_start_matches('/')
        .split(['#', '?'])
        .next()
        .unwrap_or("")
        .to_string()
}

fn is_external(href: &str) -> bool {
    let l = href.to_ascii_lowercase();
    (l.starts_with("http://") || l.starts_with("https://") || l.starts_with("icedreader:"))
        && !l.contains("icedreader.localhost")
        && !l.starts_with("icedreader://localhost/")
}

fn file_name(href: &str) -> String {
    href.rsplit('/').next().filter(|s| !s.is_empty()).unwrap_or(href).to_string()
}

fn with_media(media: &str, selector: &str) -> String {
    if media.is_empty() {
        selector.to_string()
    } else {
        format!("{media} · {selector}")
    }
}

fn strip_html_comment_wrappers(s: &str) -> String {
    s.replace("<!--", " ").replace("-->", " ")
}

fn compact_ws(s: &str) -> String {
    let mut out = String::new();
    let mut gap = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            gap = true;
        } else {
            if gap && !out.is_empty() {
                out.push(' ');
            }
            gap = false;
            out.push(ch);
        }
    }
    out
}

fn strip_quotes(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 {
        let b = s.as_bytes();
        if (b[0] == b'"' && b[s.len() - 1] == b'"') || (b[0] == b'\'' && b[s.len() - 1] == b'\'') {
            return s[1..s.len() - 1].to_string();
        }
    }
    s.to_string()
}

fn split_important(value: &str) -> (&str, bool) {
    let trimmed = value.trim_end();
    let lower = trimmed.to_ascii_lowercase();
    if let Some(rest) = lower.strip_suffix("!important") {
        (trimmed[..rest.len()].trim_end(), true)
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

fn parse_attrs(tag: &str) -> Vec<(String, String)> {
    let bytes = tag.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        while i < bytes.len() && (bytes[i].is_ascii_whitespace() || bytes[i] == b'/' || bytes[i] == b'?')
        {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let name_start = i;
        while i < bytes.len() && is_ident_char(bytes[i]) {
            i += 1;
        }
        if i == name_start {
            i += 1;
            continue;
        }
        let name = tag[name_start..i].to_ascii_lowercase();
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            out.push((name, String::new()));
            continue;
        }
        i += 1;
        while i < bytes.len() && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= bytes.len() {
            out.push((name, String::new()));
            break;
        }
        let value = if bytes[i] == b'"' || bytes[i] == b'\'' {
            let end = skip_string(bytes, i);
            let raw = &tag[i..end];
            i = end;
            strip_quotes(raw)
        } else {
            let start = i;
            while i < bytes.len() && !bytes[i].is_ascii_whitespace() && bytes[i] != b'/' && bytes[i] != b'>'
            {
                i += 1;
            }
            tag[start..i].to_string()
        };
        out.push((name, value));
    }
    out
}

fn attr<'a>(attrs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attrs
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
        .filter(|v| !v.is_empty())
}

fn tag_name_before(html: &str, attr_at: usize) -> Option<String> {
    let before = &html[..attr_at];
    let lt = before.rfind('<')?;
    let rest = before[lt + 1..].trim_start();
    let name: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == ':')
        .collect();
    if name.is_empty() {
        None
    } else {
        Some(name.to_ascii_lowercase())
    }
}

fn strip_css_comments(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut i = 0;
    while i < bytes.len() {
        if starts_comment(bytes, i) {
            i = skip_comment(bytes, i);
            out.push(' ');
            continue;
        }
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let end = skip_string(bytes, i);
            out.push_str(&input[i..end]);
            i = end;
            continue;
        }
        let ch = input[i..].chars().next().expect("char boundary");
        out.push(ch);
        i += ch.len_utf8();
    }
    out
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
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            i = skip_string(bytes, i);
            continue;
        }
        if bytes[i] == b';' || bytes[i] == b'}' || bytes[i] == b'{' {
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
    use std::collections::HashMap;

    fn report(html: &str, files: &[(&str, &str)]) -> PublisherFontReport {
        let map: HashMap<String, String> = files
            .iter()
            .map(|(k, v)| (normalize_book_href(k), v.to_string()))
            .collect();
        collect_publisher_fonts(
            html,
            "http://icedreader.localhost/book/test/",
            "EPUB/ch1.xhtml",
            |href| map.get(&normalize_book_href(href)).cloned(),
        )
    }

    #[test]
    fn empty_when_book_specifies_nothing() {
        let r = report("<html><body><p>hi</p></body></html>", &[]);
        assert!(r.declarations.is_empty());
        assert!(r.faces.is_empty());
        assert!(!r.truncated);
    }

    #[test]
    fn reads_style_tag_inline_and_generics() {
        let html = r#"<html><head><style>
            body { font-family: serif; }
            h1 { font-family: "Source Han Serif SC", sans-serif; }
            code { font: 12px/1.4 monospace; }
        </style></head>
        <body><p style="font-family: sans-serif">x</p></body></html>"#;
        let r = report(html, &[]);
        let values: Vec<_> = r.declarations.iter().map(|d| (d.selector.as_str(), d.value.as_str())).collect();
        assert!(values.contains(&("body", "serif")), "{values:?}");
        assert!(
            values.iter().any(|(s, v)| *s == "h1" && v.contains("Source Han Serif SC") && v.contains("sans-serif")),
            "{values:?}"
        );
        assert!(values.contains(&("code", "monospace")), "{values:?}");
        assert!(values.contains(&("p[style]", "sans-serif")), "{values:?}");
    }

    #[test]
    fn reads_linked_css_import_and_font_face() {
        let html = r#"<html><head>
            <link rel="stylesheet" href="http://icedreader.localhost/book/test/EPUB/style.css"/>
        </head><body></body></html>"#;
        let r = report(
            html,
            &[
                (
                    "EPUB/style.css",
                    r#"@import url("extra.css");
@font-face { font-family: "MyEmb"; src: url(x.ttf); }
body { font-family: "MyEmb", serif; }"#,
                ),
                ("EPUB/extra.css", "h2 { font-family: sans-serif; }"),
            ],
        );
        assert!(r.faces.iter().any(|f| f == "MyEmb"), "{:?}", r.faces);
        assert!(
            r.declarations.iter().any(|d| d.selector == "body" && d.value.contains("MyEmb") && d.value.contains("serif")),
            "{:?}",
            r.declarations
        );
        assert!(
            r.declarations.iter().any(|d| d.selector == "h2" && d.value == "sans-serif"),
            "{:?}",
            r.declarations
        );
    }

    #[test]
    fn skips_comments_and_keeps_media() {
        let html = r#"<html><head><style>
            /* body { font-family: fantasy; } */
            @media print { p { font-family: serif; } }
        </style></head></html>"#;
        let r = report(html, &[]);
        assert!(r.declarations.iter().all(|d| !d.value.contains("fantasy")));
        assert!(
            r.declarations.iter().any(|d| d.selector.contains("@media") && d.selector.contains("p") && d.value == "serif"),
            "{:?}",
            r.declarations
        );
    }
}
