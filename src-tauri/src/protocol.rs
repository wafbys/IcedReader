use iced_reader_core::FontSlot;
use iced_reader_epub::is_document;
use tauri::http::{header, StatusCode};
use tauri::{Manager, UriSchemeContext};

use crate::fonts as reader_fonts;
use crate::AppState;

/// Windows/Android: `http://icedreader.localhost/...`
/// macOS/Linux/iOS: `icedreader://localhost/...`
pub fn origin() -> &'static str {
    #[cfg(any(windows, target_os = "android"))]
    {
        "http://icedreader.localhost"
    }
    #[cfg(not(any(windows, target_os = "android")))]
    {
        "icedreader://localhost"
    }
}

/// Windows/Android: `http://icedreader.localhost/book/{id}/...`
/// macOS/Linux/iOS: `icedreader://localhost/book/{id}/...`
pub fn resource_base(book_id: &str) -> String {
    format!("{}/book/{book_id}/", origin())
}

pub fn font_url(slot: &str) -> String {
    format!("{}/fonts/{slot}", origin())
}

pub fn handle<R: tauri::Runtime>(
    ctx: UriSchemeContext<'_, R>,
    request: tauri::http::Request<Vec<u8>>,
) -> tauri::http::Response<Vec<u8>> {
    if request.method() == "OPTIONS" {
        return cors(StatusCode::NO_CONTENT, "text/plain", Vec::new());
    }

    let path = percent_decode(request.uri().path());
    if let Some(slot) = path.strip_prefix("/fonts/") {
        return serve_font(ctx, slot);
    }
    if let Some(name) = path.strip_prefix("/library-cover/") {
        return serve_library_cover(name);
    }

    let Some((book_id, href)) = parse_book_path(&path) else {
        return cors(
            StatusCode::BAD_REQUEST,
            "text/plain; charset=utf-8",
            b"invalid icedreader uri".to_vec(),
        );
    };

    let state = ctx.app_handle().state::<AppState>();
    let fetched = {
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
            book.chapter_html(&href, &resource_base(&book_id))
                .map(Fetch::Html)
                .map_err(|e| e.to_string())
        } else {
            book.resource(&href)
                .map(|res| Fetch::Resource {
                    media: res.media_type,
                    data: res.data,
                    href: href.clone(),
                })
                .map_err(|e| e.to_string())
        }
    };

    match fetched {
        Ok(Fetch::Html(html)) => {
            let html = apply_html(html, &state);
            cors(StatusCode::OK, "text/html; charset=utf-8", html.into_bytes())
        }
        Ok(Fetch::Resource { media, data, href }) => {
            let data = if media.to_ascii_lowercase().contains("css")
                || href.rsplit('.').next().is_some_and(|e| e.eq_ignore_ascii_case("css"))
            {
                apply_css(data, &state)
            } else {
                data
            };
            cors(StatusCode::OK, &media, data)
        }
        Err(err) => cors(
            StatusCode::NOT_FOUND,
            "text/plain; charset=utf-8",
            err.into_bytes(),
        ),
    }
}

enum Fetch {
    Html(String),
    Resource {
        media: String,
        data: Vec<u8>,
        href: String,
    },
}

fn apply_html(html: String, state: &AppState) -> String {
    let Ok(settings) = state.settings.lock() else {
        return html;
    };
    reader_fonts::apply_html_if_active(html, &settings)
}

fn apply_css(css: Vec<u8>, state: &AppState) -> Vec<u8> {
    let Ok(settings) = state.settings.lock() else {
        return css;
    };
    reader_fonts::apply_css_if_active(css, &settings)
}

fn serve_font<R: tauri::Runtime>(
    ctx: UriSchemeContext<'_, R>,
    slot: &str,
) -> tauri::http::Response<Vec<u8>> {
    let Some(slot) = FontSlot::parse(slot.split(['#', '?']).next().unwrap_or(slot)) else {
        return cors(
            StatusCode::NOT_FOUND,
            "text/plain; charset=utf-8",
            b"unknown font slot".to_vec(),
        );
    };
    let state = ctx.app_handle().state::<AppState>();
    let Ok(settings) = state.settings.lock() else {
        return cors(
            StatusCode::INTERNAL_SERVER_ERROR,
            "text/plain; charset=utf-8",
            b"lock poisoned".to_vec(),
        );
    };
    match reader_fonts::read_slot_font(&settings, slot) {
        Some((data, mime)) => cors_cached(StatusCode::OK, mime, data),
        None => cors(
            StatusCode::NOT_FOUND,
            "text/plain; charset=utf-8",
            b"font not installed".to_vec(),
        ),
    }
}

fn serve_library_cover(file_name: &str) -> tauri::http::Response<Vec<u8>> {
    let name = file_name.split(['#', '?']).next().unwrap_or(file_name);
    let path = match crate::library::library_cover_path(name) {
        Ok(p) => p,
        Err(err) => {
            return cors(
                StatusCode::NOT_FOUND,
                "text/plain; charset=utf-8",
                err.into_bytes(),
            );
        }
    };
    match crate::library::cover_bytes(&path) {
        Ok((media, data)) => cors_cached(StatusCode::OK, &media, data),
        Err(err) => cors(
            StatusCode::NOT_FOUND,
            "text/plain; charset=utf-8",
            err.into_bytes(),
        ),
    }
}

fn parse_book_path(path: &str) -> Option<(String, String)> {
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
    cors_cache(status, content_type, body, "no-store")
}

fn cors_cached(status: StatusCode, content_type: &str, body: Vec<u8>) -> tauri::http::Response<Vec<u8>> {
    cors_cache(status, content_type, body, "private, max-age=31536000, immutable")
}

fn cors_cache(
    status: StatusCode,
    content_type: &str,
    body: Vec<u8>,
    cache: &str,
) -> tauri::http::Response<Vec<u8>> {
    tauri::http::Response::builder()
        .status(status)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(header::ACCESS_CONTROL_ALLOW_METHODS, "GET, OPTIONS")
        .header("Cross-Origin-Resource-Policy", "cross-origin")
        .header(header::CACHE_CONTROL, cache)
        .body(body)
        .unwrap_or_else(|_| {
            tauri::http::Response::builder()
                .status(StatusCode::INTERNAL_SERVER_ERROR)
                .body(b"response build failed".to_vec())
                .expect("fallback response")
        })
}
