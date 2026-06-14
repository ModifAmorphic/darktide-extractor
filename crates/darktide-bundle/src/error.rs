//! Error types for darktide-bundle.

/// Errors returned by darktide-bundle operations.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An I/O error (file read/seek/write).
    #[error(transparent)]
    Io(#[from] std::io::Error),

    /// The bundle data is malformed, truncated, or has an invalid header.
    #[error("invalid bundle: {0}")]
    InvalidBundle(String),

    /// Failed to load the Oodle shared library.
    #[error("failed to load Oodle library: {0}")]
    OodleLoad(String),

    /// Oodle decompression returned failure (result == 0).
    #[error("Oodle decompression failed")]
    OodleDecompress,
}

/// Convenience Result alias.
pub type Result<T> = std::result::Result<T, Error>;
