//! Error type shared by URL parsing and resource loading. Never constructed from a panic: every
//! fallible operation in this crate returns `Err(NetError)` instead of unwinding, because inputs
//! here (an `href`, a `file://` URL) are attacker-controlled.

/// Any failure produced by this crate.
#[derive(Debug, thiserror::Error)]
pub enum NetError {
    /// The input was not a valid URL, or a base/relative pair did not resolve. Carries the
    /// underlying parser's message (from the `url` crate, or our own description) rather than
    /// a structured cause, since callers only need this for diagnostics/logging, not matching.
    #[error("invalid url: {0}")]
    Url(String),
    /// The URL's scheme is not supported by the requested operation (e.g. `load_file` was
    /// asked to load `http://...`). Carries the offending scheme.
    #[error("unsupported url scheme: {0}")]
    UnsupportedScheme(String),
    /// The underlying filesystem operation (stat, open, read) failed.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// The resource exceeded the configured size cap. Detected either from `stat` metadata
    /// before reading, or from the bounded read itself if the file grew after `stat`.
    #[error("file too large: {size} bytes exceeds cap of {max} bytes")]
    TooLarge {
        /// The size we observed (from `stat`, or from how much we actually read).
        size: u64,
        /// The cap that was exceeded.
        max: u64,
    },
}
