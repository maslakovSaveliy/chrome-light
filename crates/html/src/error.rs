//! Error type for `cl-html`.
//!
//! Nothing implemented so far in this crate can fail: [`crate::sniff_encoding`] and
//! [`crate::decode`] always resolve to *some* encoding — falling back to the UTF-8 default
//! (see [`crate::EncodingSource::Default`]) rather than returning an error — and
//! `encoding_rs`'s decoder is total over its input (malformed byte sequences are replaced
//! with U+FFFD, never surfaced as an `Err`). `HtmlError` is kept as a real, `#[non_exhaustive]`
//! type from the start rather than introduced later, so its identity in `cl-html`'s public API
//! is stable; Task 8 (the `TreeSink` → arena DOM bridge) is expected to add this type's first
//! inhabited variants (malformed input the tree builder cannot recover from, resource limits,
//! etc.).

/// Errors produced by `cl-html`.
///
/// Uninhabited today — see the module documentation above for why.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum HtmlError {}
