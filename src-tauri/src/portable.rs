use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub fn exe_dir() -> io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    exe.parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "executable has no parent"))
}

pub fn data_dir() -> io::Result<PathBuf> {
    Ok(exe_dir()?.join("data"))
}

pub fn library_dir() -> io::Result<PathBuf> {
    Ok(data_dir()?.join("library"))
}

pub fn progress_file() -> io::Result<PathBuf> {
    Ok(data_dir()?.join("progress.json"))
}

pub fn settings_file() -> io::Result<PathBuf> {
    Ok(data_dir()?.join("settings.json"))
}

pub fn fonts_dir() -> io::Result<PathBuf> {
    Ok(data_dir()?.join("fonts"))
}

pub fn webview_dir() -> io::Result<PathBuf> {
    Ok(data_dir()?.join("webview"))
}

pub fn ensure_layout() -> io::Result<PathBuf> {
    fs::create_dir_all(library_dir()?)?;
    fs::create_dir_all(webview_dir()?)?;
    fs::create_dir_all(fonts_dir()?)?;
    data_dir()
}

/// WebView2 reads this before the first webview is created (Windows).
pub fn prepare_webview_env() {
    if ensure_layout().is_ok() {
        if let Ok(dir) = webview_dir() {
            std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", dir);
        }
    }
}

pub fn import_book(src: &Path) -> io::Result<PathBuf> {
    ensure_layout()?;
    import_book_to(src, &library_dir()?)
}

pub fn import_book_to(src: &Path, library: &Path) -> io::Result<PathBuf> {
    fs::create_dir_all(library)?;
    let src_n = normalize_path(src);
    let lib_n = normalize_path(library);
    if src_n.starts_with(&lib_n) {
        return Ok(src_n);
    }
    if !src_n.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("not a file: {}", src.display()),
        ));
    }
    let name = src
        .file_name()
        .map(|n| safe_filename(&n.to_string_lossy()))
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "book.epub".into());
    let dest = lib_n.join(&name);
    if dest.exists() {
        return Ok(dest);
    }
    fs::copy(&src_n, &dest)?;
    Ok(dest)
}

fn safe_filename(name: &str) -> String {
    let trimmed = name.trim().trim_start_matches('.');
    let mapped: String = trimmed
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    if mapped.is_empty() {
        "book.epub".into()
    } else {
        mapped
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    let canon = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let s = canon.to_string_lossy();
    PathBuf::from(s.strip_prefix(r"\\?\").unwrap_or(&s))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_into_library_and_skips_when_already_there() {
        let root = std::env::temp_dir().join("icedreader-import-test");
        let lib = root.join("library");
        let src_dir = root.join("src");
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&src_dir).unwrap();
        let src = src_dir.join("demo.epub");
        fs::write(&src, b"epub-bytes").unwrap();

        let copied = import_book_to(&src, &lib).unwrap();
        assert_eq!(copied.file_name().unwrap(), "demo.epub");
        assert_eq!(fs::read(&copied).unwrap(), b"epub-bytes");
        assert!(copied.starts_with(normalize_path(&lib)));

        let again = import_book_to(&copied, &lib).unwrap();
        assert_eq!(again, copied);

        let second_src = src_dir.join("demo.epub");
        fs::write(&second_src, b"other").unwrap();
        let again_named = import_book_to(&second_src, &lib).unwrap();
        assert_eq!(again_named, copied);
        assert_eq!(fs::read(&copied).unwrap(), b"epub-bytes");
    }
}
