use iced_reader_epub::is_document;
use tauri::http::{header, StatusCode};
use tauri::{Manager, UriSchemeContext};

use crate::AppState;

/// Windows/Android: `http://icedreader.localhost/book/{id}/...`
/// macOS/Linux/iOS: `icedreader://localhost/book/{id}/...`
pub fn resource_base(book_id: &str) -> String {
    #[cfg(any(windows, target_os = "android"))]
    {
        format!("http://icedreader.localhost/book/{book_id}/")
    }
    #[cfg(not(any(windows, target_os = "android")))]
    {
        format!("icedreader://localhost/book/{book_id}/")
    }
}

pub fn handle<R: tauri::Runtime>(
    ctx: UriSchemeContext<'_, R>,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    if request.method() == "OPTIONS" {
        return cors(StatusCode::NO_CONTENT, "text/plain", Vec::new());
    }

    let Some((book_id, href)) = parse_book_uri(request.uri()) else {
        return cors(
            StatusCode::BAD_REQUEST,
            "text/plain; charset=utf-8",
            b"invalid icedreader uri".to_vec(),
        );
    };

    let state = ctx.app_handle().state::<AppState>();
    let Ok(books) = state.books.lock() else {
        return cors(
            StatusCode::INTERNAL_SERVER_ERROR,
            "text/plain; charset=utf-8",
            b"lock poisoned".to_vec(),
        );
    };
    let Some(book) = books.get(&book_id) else {
        return cors(
            StatusCode::NOT_FOUND,
            "text/plain; charset=utf-8",
            b"book not open".to_vec(),
        );
    };

    let media_guess = media_hint(&href);
    if is_document(&media_guess, &href) {
        match book.chapter_html(&href, &resource_base(&book_id)) {
            Ok(html) => cors(
                StatusCode::OK,
                "text/html; charset=utf-8",
                html.into_bytes(),
            ),
            Err(err) => cors(
                StatusCode::NOT_FOUND,
                "text/plain; charset=utf-8",
                err.to_string().into_bytes(),
            ),
        }
    } else {
        match book.resource(&href) {
            Ok(res) => cors(StatusCode::OK, &res.media_type, res.data),
            Err(err) => cors(
                StatusCode::NOT_FOUND,
                "text/plain; charset=utf-8",
                err.to_string().into_bytes(),
            ),
        }
    }
}

fn parse_book_uri(uri: &tauri::http::Uri) -> Option<(String, String)> {
    let path = percent_decode(uri.path());
    let rest = path.strip_prefix("/book/")?;
    let (id, href) = rest.split_once('/')?;
    if id.is_empty() || href.is_empty() {
        return None;
    }
    Some((id.to_string(), format!("/{href}")))
}

fn percent_decode(s: &str) -> String {
    percent_encoding_lite(s)
}

fn percent_encoding_lite(s: &str) -> String {
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

fn media_hint(href: &str) -> String {
    let ext = href
        .rsplit('.')
        .next()
        .unwrap_or("")
        .split(['#', '?'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "xhtml" | "html" | "htm" => "application/xhtml+xml".into(),
        "css" => "text/css".into(),
        _ => "application/octet-stream".into(),
    }
}

fn cors(status: StatusCode, content_type: &str, body: Vec<u8>) -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(header::CACHE_CONTROL, "no-store")
        .body(body)
        .unwrap_or_else(|_| {
            tauri::http::Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(b"response build failed".to_vec())
                .expect("fallback response")
        })
}
