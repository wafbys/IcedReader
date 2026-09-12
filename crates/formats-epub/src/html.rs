//! Lenient HTML helpers for street-quality EPUBs.
//! rbook's rewriter is XML-strict; many Chinese EPUBs are not.

const VOID_TAGS: &[&str] = &[
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "param", "source",
    "track", "wbr",
];

pub fn split_href(href: &str) -> (&str, Option<&str>) {
    let hash = href.find('#');
    let file = match hash {
        Some(i) => href[..i].split('?').next().unwrap_or(&href[..i]),
        None => href.split('?').next().unwrap_or(href),
    };
    let fragment = hash.and_then(|i| {
        let frag = href[i + 1..].split('?').next().unwrap_or(&href[i + 1..]);
        if frag.is_empty() {
            None
        } else {
            Some(frag)
        }
    });
    (file, fragment)
}

pub fn href_file_key(href: &str) -> String {
    split_href(href)
        .0
        .trim_start_matches('/')
        .to_ascii_lowercase()
}

/// Rewrite relative resource URLs without requiring well-formed XHTML.
pub fn rewrite_html_paths(html: &str, resource_base: &str, chapter_file: &str) -> String {
    let bytes = html.as_bytes();
    let mut out = String::with_capacity(html.len() + 64);
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            let start = i;
            while i < bytes.len() && bytes[i] != b'<' {
                i += 1;
            }
            out.push_str(&html[start..i]);
            continue;
        }
        if html[i..].starts_with("<!--") {
            let end = html[i + 4..]
                .find("-->")
                .map(|rel| i + 4 + rel + 3)
                .unwrap_or(html.len());
            out.push_str(&html[i..end]);
            i = end;
            continue;
        }
        let Some(tag_end) = tag_close(html, i) else {
            out.push_str(&html[i..]);
            break;
        };
        out.push_str(&rewrite_tag(&html[i..tag_end], resource_base, chapter_file));
        i = tag_end;
    }
    out
}

/// Keep `html/head/body` and the section from `start_id` until `end_id`.
/// Missing ids leave the document unchanged.
pub fn slice_chapter(html: &str, start_id: Option<&str>, end_id: Option<&str>) -> String {
    if start_id.is_none() && end_id.is_none() {
        return html.to_string();
    }
    let body_end = find_body_open_end(html);
    let start = match start_id {
        Some(id) => match find_element_start(html, id) {
            Some(pos) => pos,
            None => return html.to_string(),
        },
        None => body_end,
    };
    let end = match end_id {
        Some(id) => find_element_start(html, id).unwrap_or_else(|| body_close_or_end(html)),
        None => body_close_or_end(html),
    };
    if start >= end {
        return html.to_string();
    }

    let prefix_end = if body_end > 0 && body_end <= start {
        body_end
    } else {
        0
    };
    let ancestors = if prefix_end > 0 && start > prefix_end {
        unclosed_opening_tags(&html[prefix_end..start])
    } else {
        Vec::new()
    };

    let mut out = String::with_capacity((end - start) + prefix_end + 64);
    if prefix_end > 0 {
        out.push_str(&html[..prefix_end]);
    }
    for tag in &ancestors {
        out.push_str(tag);
    }
    out.push_str(&html[start..end]);
    for tag in ancestors.iter().rev() {
        out.push_str(&close_tag(tag));
    }
    if prefix_end > 0 && find_ci(&html[..prefix_end], "<body").is_some() {
        let lower = out.to_ascii_lowercase();
        if !lower.contains("</body>") {
            out.push_str("</body></html>");
        }
    }
    out
}

fn rewrite_tag(tag: &str, resource_base: &str, chapter_file: &str) -> String {
    if tag.starts_with("</") || tag.starts_with("<!") || tag.starts_with("<?") {
        return tag.to_string();
    }
    let bytes = tag.as_bytes();
    let mut i = 1;
    while i < bytes.len() && !is_space(bytes[i]) && bytes[i] != b'>' && bytes[i] != b'/' {
        i += 1;
    }
    let tag_name = &tag[1..i];
    let mut out = String::with_capacity(tag.len() + 32);
    out.push('<');
    out.push_str(tag_name);

    while i < bytes.len() {
        let c = bytes[i];
        if c == b'>' {
            out.push('>');
            return out;
        }
        if is_space(c) || c == b'/' {
            out.push(c as char);
            i += 1;
            continue;
        }
        let name_start = i;
        while i < bytes.len()
            && !is_space(bytes[i])
            && bytes[i] != b'='
            && bytes[i] != b'>'
            && bytes[i] != b'/'
        {
            i += 1;
        }
        let name = &tag[name_start..i];
        out.push_str(name);
        while i < bytes.len() && is_space(bytes[i]) {
            out.push(bytes[i] as char);
            i += 1;
        }
        if i >= bytes.len() || bytes[i] != b'=' {
            continue;
        }
        out.push('=');
        i += 1;
        while i < bytes.len() && is_space(bytes[i]) {
            out.push(bytes[i] as char);
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }
        let (value, quote, next) = parse_attr_value(tag, i);
        i = next;
        let new_val = if should_rewrite(tag_name, name) {
            rewrite_attr(name, value, resource_base, chapter_file)
        } else {
            value.to_string()
        };
        match quote {
            Some(q) => {
                out.push(q);
                out.push_str(&new_val);
                out.push(q);
            }
            None => {
                if new_val
                    .bytes()
                    .any(|b| is_space(b) || b == b'>' || b == b'"' || b == b'\'')
                {
                    out.push('"');
                    out.push_str(&new_val);
                    out.push('"');
                } else {
                    out.push_str(&new_val);
                }
            }
        }
    }
    out
}

fn parse_attr_value(tag: &str, start: usize) -> (&str, Option<char>, usize) {
    let bytes = tag.as_bytes();
    if start >= bytes.len() {
        return ("", None, start);
    }
    let q = bytes[start];
    if q == b'"' || q == b'\'' {
        let quote = q as char;
        let mut i = start + 1;
        while i < bytes.len() && bytes[i] != q {
            i += 1;
        }
        let value = &tag[start + 1..i];
        let next = if i < bytes.len() { i + 1 } else { i };
        (value, Some(quote), next)
    } else {
        let mut i = start;
        while i < bytes.len() && !is_space(bytes[i]) && bytes[i] != b'>' {
            i += 1;
        }
        (&tag[start..i], None, i)
    }
}

fn should_rewrite(tag_name: &str, attr: &str) -> bool {
    let attr = attr.to_ascii_lowercase();
    match attr.as_str() {
        "src" | "srcset" | "href" | "poster" | "xlink:href" => true,
        "data" => tag_name.eq_ignore_ascii_case("object"),
        _ => false,
    }
}

fn rewrite_attr(name: &str, value: &str, resource_base: &str, chapter_file: &str) -> String {
    if name.eq_ignore_ascii_case("srcset") {
        return rewrite_srcset(value, resource_base, chapter_file);
    }
    rewrite_url(value, resource_base, chapter_file)
}

fn rewrite_srcset(value: &str, resource_base: &str, chapter_file: &str) -> String {
    value
        .split(',')
        .map(|part| {
            let part = part.trim();
            if part.is_empty() {
                return String::new();
            }
            let mut bits = part.split_whitespace();
            let Some(url) = bits.next() else {
                return part.to_string();
            };
            let rewritten = rewrite_url(url, resource_base, chapter_file);
            let rest: Vec<&str> = bits.collect();
            if rest.is_empty() {
                rewritten
            } else {
                format!("{rewritten} {}", rest.join(" "))
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn rewrite_url(value: &str, resource_base: &str, chapter_file: &str) -> String {
    let value = value.trim();
    if value.is_empty() || value.starts_with('#') || has_scheme(value) {
        return value.to_string();
    }
    let resolved = resolve_relative(chapter_file, value);
    if has_scheme(&resolved) {
        return resolved;
    }
    join_resource_url(resource_base, &resolved)
}

fn join_resource_url(resource_base: &str, book_path: &str) -> String {
    let path = book_path.trim_start_matches('/');
    if resource_base.ends_with('/') {
        format!("{resource_base}{path}")
    } else {
        format!("{resource_base}/{path}")
    }
}

fn resolve_relative(base_file: &str, rel: &str) -> String {
    let (path, suffix) = split_query_frag(rel);
    if path.starts_with('/') {
        return format!("{path}{suffix}");
    }
    let base_dir = parent_path(split_href(base_file).0);
    let mut parts: Vec<&str> = base_dir.split('/').filter(|s| !s.is_empty()).collect();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            s => parts.push(s),
        }
    }
    format!("/{}{}", parts.join("/"), suffix)
}

fn split_query_frag(href: &str) -> (&str, &str) {
    match href.find(['?', '#']) {
        Some(i) => (&href[..i], &href[i..]),
        None => (href, ""),
    }
}

fn parent_path(href: &str) -> &str {
    match href.rfind('/') {
        Some(0) => "/",
        Some(i) => &href[..i],
        None => "",
    }
}

fn has_scheme(href: &str) -> bool {
    let bytes = href.as_bytes();
    let Some(colon) = bytes.iter().position(|&c| c == b':') else {
        return false;
    };
    if colon == 0 || !bytes[0].is_ascii_alphabetic() {
        return false;
    }
    bytes[1..colon]
        .iter()
        .all(|c| c.is_ascii_alphanumeric() || matches!(*c, b'+' | b'.' | b'-'))
}

fn find_element_start(html: &str, id: &str) -> Option<usize> {
    if id.is_empty() {
        return None;
    }
    find_element_start_exact(html, id).or_else(|| {
        let decoded = percent_decode(id);
        if decoded != id && !decoded.is_empty() {
            find_element_start_exact(html, &decoded)
        } else {
            None
        }
    })
}

fn find_element_start_exact(html: &str, id: &str) -> Option<usize> {
    let mut search = 0;
    while let Some(rel) = html[search..].find(id) {
        let at = search + rel;
        if id_attr_at(html, at, id.len()) {
            return html[..at].rfind('<');
        }
        search = at + id.len();
    }
    None
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
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

/// Index just past the matching `>` of the tag that starts at `from` (`<`).
/// Quote-aware so `>` inside attribute values does not end the tag.
fn tag_close(html: &str, from: usize) -> Option<usize> {
    let bytes = html.as_bytes();
    let mut i = from;
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

fn id_attr_at(html: &str, value_start: usize, len: usize) -> bool {
    let bytes = html.as_bytes();
    let after = value_start + len;
    if after < bytes.len() {
        let c = bytes[after];
        if c != b'"' && c != b'\'' && !c.is_ascii_whitespace() && c != b'>' && c != b'/' {
            return false;
        }
    }
    if value_start == 0 {
        return false;
    }
    let mut i = value_start - 1;
    if bytes[i] == b'"' || bytes[i] == b'\'' {
        if i == 0 {
            return false;
        }
        i -= 1;
    }
    while i > 0 && is_space(bytes[i]) {
        i -= 1;
    }
    if bytes[i] != b'=' {
        return false;
    }
    if i == 0 {
        return false;
    }
    i -= 1;
    while i > 0 && is_space(bytes[i]) {
        i -= 1;
    }
    let name_end = i + 1;
    let mut name_start = name_end;
    while name_start > 0 {
        let p = bytes[name_start - 1];
        if p.is_ascii_alphanumeric() || p == b':' || p == b'-' || p == b'_' {
            name_start -= 1;
        } else {
            break;
        }
    }
    let name = &html[name_start..name_end];
    name.eq_ignore_ascii_case("id")
        || name.eq_ignore_ascii_case("name")
        || name.eq_ignore_ascii_case("xml:id")
}

fn find_body_open_end(html: &str) -> usize {
    let bytes = html.as_bytes();
    let needle: &[u8] = b"<body";
    let mut i = 0;
    while i + needle.len() <= bytes.len() {
        // 字节扫描：i + needle.len() 可能落在中文字符内部，只能按字节比较，
        // 不能切 &str（`<p>中` 这类文档曾在此 panic）。
        if bytes[i] == b'<' && bytes[i..i + needle.len()].eq_ignore_ascii_case(needle) {
            let next = bytes.get(i + needle.len()).copied().unwrap_or(0);
            if next == b'>' || is_space(next) || next == b'/' {
                return tag_close(html, i).unwrap_or(0);
            }
        }
        i += 1;
    }
    0
}

fn body_close_or_end(html: &str) -> usize {
    find_ci(html, "</body>").unwrap_or(html.len())
}

fn unclosed_opening_tags(html: &str) -> Vec<String> {
    let mut stack: Vec<String> = Vec::new();
    let bytes = html.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] != b'<' {
            i += 1;
            continue;
        }
        if html[i..].starts_with("<!--") {
            i = html[i + 4..]
                .find("-->")
                .map(|rel| i + 4 + rel + 3)
                .unwrap_or(html.len());
            continue;
        }
        if html[i..].starts_with("</") {
            let name_start = i + 2;
            let mut name_end = name_start;
            while name_end < bytes.len() && bytes[name_end].is_ascii_alphanumeric() {
                name_end += 1;
            }
            let name = html[name_start..name_end].to_ascii_lowercase();
            if let Some(idx) = stack
                .iter()
                .rposition(|t| start_tag_name(t).eq_ignore_ascii_case(&name))
            {
                stack.truncate(idx);
            }
            i = tag_close(html, i).unwrap_or(html.len());
            continue;
        }
        if html[i..].starts_with("<!") || html[i..].starts_with("<?") {
            i = tag_close(html, i).unwrap_or(html.len());
            continue;
        }
        let tag_end = tag_close(html, i).unwrap_or(html.len());
        let tag = &html[i..tag_end];
        let name = start_tag_name(tag).to_ascii_lowercase();
        let self_close = tag.trim_end().ends_with("/>") || VOID_TAGS.contains(&name.as_str());
        if !self_close && !name.is_empty() {
            if tag.ends_with('>') {
                stack.push(tag.to_string());
            } else {
                stack.push(format!("{tag}>"));
            }
        }
        i = tag_end;
    }
    stack
}

fn start_tag_name(tag: &str) -> &str {
    let s = tag.trim_start_matches('<').trim_start();
    let end = s
        .find(|c: char| c.is_ascii_whitespace() || c == '>' || c == '/')
        .unwrap_or(s.len());
    &s[..end]
}

fn close_tag(open: &str) -> String {
    format!("</{}>", start_tag_name(open))
}

fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    hay.as_bytes()
        .windows(needle.len())
        .position(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
}

fn is_space(b: u8) -> bool {
    b.is_ascii_whitespace()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_keeps_fragment() {
        assert_eq!(
            split_href("/OEBPS/Text/a.xhtml#sigil_toc_id_3"),
            ("/OEBPS/Text/a.xhtml", Some("sigil_toc_id_3"))
        );
        assert_eq!(split_href("OPS/chapter3.html"), ("OPS/chapter3.html", None));
    }

    #[test]
    fn rewrite_unclosed_img() {
        let html = r#"<p><img src="pic.png" width="63"></p><link href="../css/main.css">"#;
        let out = rewrite_html_paths(html, "http://icedreader.localhost/book/t/", "/OPS/ch.html");
        assert!(
            out.contains("http://icedreader.localhost/book/t/OPS/pic.png"),
            "{out}"
        );
        assert!(
            out.contains("http://icedreader.localhost/book/t/css/main.css"),
            "{out}"
        );
        assert!(out.contains("<img"));
        assert!(!out.contains("src=\"pic.png\""), "{out}");
    }

    #[test]
    fn rewrite_skips_fragment_and_http() {
        let html = r##"<a href="#x">in</a><a href="https://ex.com">out</a>"##;
        let out = rewrite_html_paths(html, "http://icedreader.localhost/book/t/", "/OPS/ch.html");
        assert!(out.contains(r##"href="#x""##), "{out}");
        assert!(out.contains("href=\"https://ex.com\""), "{out}");
    }

    #[test]
    fn rewrite_ignores_gt_inside_quoted_attr() {
        let html = r#"<img alt="n > 0" src="pic.png">"#;
        let out = rewrite_html_paths(html, "http://icedreader.localhost/book/t/", "/OPS/ch.html");
        assert!(
            out.contains("http://icedreader.localhost/book/t/OPS/pic.png"),
            "{out}"
        );
        assert!(out.contains("alt=\"n > 0\""), "{out}");
    }

    #[test]
    fn slice_between_ids() {
        let html = r#"<html><head><title>t</title></head>
<body class="b"><div class="wrap">
<h1 id="a">A章</h1><p>aaa</p>
<h1 id="b">B章</h1><p>bbb</p>
</div></body></html>"#;
        let a = slice_chapter(html, Some("a"), Some("b"));
        assert!(a.contains("A章"), "{a}");
        assert!(a.contains("aaa"), "{a}");
        assert!(!a.contains("B章"), "{a}");
        assert!(a.contains("<div class=\"wrap\">"), "{a}");
        assert!(a.contains("</div>"), "{a}");
        assert!(a.contains("</body>"), "{a}");

        let b = slice_chapter(html, Some("b"), None);
        assert!(b.contains("B章"), "{b}");
        assert!(b.contains("bbb"), "{b}");
        assert!(!b.contains("A章"), "{b}");
        assert!(!b.contains("aaa"), "{b}");
    }

    #[test]
    fn slice_from_file_start_until_id() {
        let html = r#"<html><head></head><body>
<p>front</p><h1 id="c1">One</h1><p>rest</p>
</body></html>"#;
        let out = slice_chapter(html, None, Some("c1"));
        assert!(out.contains("front"), "{out}");
        assert!(!out.contains("One"), "{out}");
    }

    #[test]
    fn slice_survives_cjk_right_after_a_tag() {
        // `<body` 探测是逐字节扫描，窗口右端可能落在中文字符内部（曾 panic）。
        // 片段里没有 <body>，所以整个串都会被扫到。
        let html = r#"<p>中<i id="x">y</i></p><p id="z">尾</p>"#;
        let head = slice_chapter(html, None, Some("x"));
        assert!(head.contains("中"), "{head}");
        assert!(!head.contains(">y<"), "{head}");
        let tail = slice_chapter(html, Some("x"), None);
        assert!(tail.contains(">y<"), "{tail}");
        assert!(tail.contains("尾"), "{tail}");
        assert_eq!(find_body_open_end("<p>中<i>x</i></p>"), 0);
    }

    #[test]
    fn body_probe_finds_body_after_cjk() {
        // find_body_open_end 返回 <body ...> 之后的位置。
        let html = "<p>中文</p><body class=\"c\"><p>x</p></body>";
        let at = find_body_open_end(html);
        assert!(html[..at].ends_with("<body class=\"c\">"), "{at} {html}");
        assert_eq!(find_body_open_end("<html><body>"), 12);
        assert_eq!(find_body_open_end("<html><BODY>"), 12);
        assert_eq!(find_body_open_end("<bodyguard>"), 0);
    }

    #[test]
    fn slice_matches_percent_encoded_fragment() {
        let html = r#"<html><body><h1 id="英租界">英</h1><p>aaa</p><h1 id="下">下</h1><p>bbb</p></body></html>"#;
        let encoded = percent_decode("%E8%8B%B1%E7%A7%9F%E7%95%8C");
        assert_eq!(encoded, "英租界");
        let a = slice_chapter(html, Some("%E8%8B%B1%E7%A7%9F%E7%95%8C"), Some("%E4%B8%8B"));
        assert!(a.contains("英"), "{a}");
        assert!(a.contains("aaa"), "{a}");
        assert!(!a.contains(">下<"), "{a}");
        assert!(!a.contains("bbb"), "{a}");
    }
}
