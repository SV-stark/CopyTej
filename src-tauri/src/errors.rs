use thiserror::Error;

#[derive(Error, Debug)]
pub enum CopyError {
    #[error("Source file does not exist: {0}")]
    NotFound(String),

    #[error("I/O error for '{path}': {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },

    #[error("Checksum mismatch for '{path}': expected {expected}, got {actual}")]
    ChecksumMismatch {
        path: String,
        expected: String,
        actual: String,
    },

    #[error("Source file size changed from {expected} to {actual} bytes during transfer")]
    SizeChanged { expected: u64, actual: u64 },

    #[error("Database error: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("{0}")]
    Custom(String),
}

pub type Result<T> = std::result::Result<T, CopyError>;
