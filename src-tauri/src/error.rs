//! One error type for this crate. Non-command helpers return [`Result`]; the
//! Tauri commands use it too, because it serialises to the same message string
//! the UI already received, so the IPC shape does not change.
//!
//! Every variant's `Display` equals the message the old `String` errors carried
//! (`io` and the format errors use their own `Display`, unprefixed), so error
//! text on screen is unchanged.

use serde::Serialize;

/// `Result` with this crate's [`Error`].
pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A domain message with no more specific type (most validation errors).
    #[error("{0}")]
    Message(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Core(#[from] iced_reader_core::CoreError),
    #[error("{0}")]
    Pdf(#[from] iced_reader_pdf::PdfError),
}

impl Error {
    pub fn msg(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

impl From<String> for Error {
    fn from(message: String) -> Self {
        Self::Message(message)
    }
}

impl From<&str> for Error {
    fn from(message: &str) -> Self {
        Self::Message(message.to_string())
    }
}

/// A poisoned mutex folds into a plain message (same text as the old
/// `map_err(|e| e.to_string())`), so `?` works on `Mutex::lock()` too.
impl<T> From<std::sync::PoisonError<T>> for Error {
    fn from(err: std::sync::PoisonError<T>) -> Self {
        Self::Message(err.to_string())
    }
}

impl Serialize for Error {
    fn serialize<S: serde::Serializer>(
        &self,
        serializer: S,
    ) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}
