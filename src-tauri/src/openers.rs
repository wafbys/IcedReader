//! Opener registry — the single place that knows which formats exist.
//!
//! Every entry point that used to name `EpubOpener` directly (shelf scan, cover,
//! metadata re-read, `open_book`) goes through here, so adding a format is one
//! line in [`openers`] plus its crate. `AGENTS.md` keeps the layer rule: format
//! crates implement `Book`/`BookOpener`, this module only dispatches, and the
//! front-end keeps talking to the same command shapes.

use std::path::Path;

use iced_reader_core::{Book, BookOpener, CoreError};
use iced_reader_epub::EpubOpener;
use iced_reader_pdf::PdfOpener;

static EPUB: EpubOpener = EpubOpener;
static PDF: PdfOpener = PdfOpener;

/// Every format the reader can open, in probe order.
pub fn openers() -> [&'static dyn BookOpener; 2] {
    [&EPUB, &PDF]
}

/// The opener that claims `path` (by extension), if any.
pub fn opener_for(path: &Path) -> Option<&'static dyn BookOpener> {
    openers().into_iter().find(|opener| opener.can_open(path))
}

/// Open any supported book. Unsupported files report the offending path.
pub fn open_any(path: &Path) -> Result<Box<dyn Book>, CoreError> {
    let opener = opener_for(path)
        .ok_or_else(|| CoreError::UnsupportedFormat(path.display().to_string()))?;
    opener.open(path)
}

/// Whether the shelf scanner / importer should look at this file at all.
pub fn is_supported(path: &Path) -> bool {
    opener_for(path).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatches_by_extension() {
        assert_eq!(
            opener_for(Path::new("C:/books/三体.epub")).map(|o| o.format_id()),
            Some("epub")
        );
        assert_eq!(
            opener_for(Path::new("C:/books/經濟漩渦.pdf")).map(|o| o.format_id()),
            Some("pdf")
        );
        assert_eq!(
            opener_for(Path::new("C:/books/三体.PDF")).map(|o| o.format_id()),
            Some("pdf")
        );
        assert!(opener_for(Path::new("C:/books/notes.txt")).is_none());
        assert!(opener_for(Path::new("C:/books/no-extension")).is_none());
        assert!(open_any(Path::new("C:/books/notes.txt")).is_err());
    }
}
