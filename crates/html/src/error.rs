//! Error type for `cl-html`.
//!
//! Still uninhabited after Task 8 (the `TreeSink` → arena DOM bridge): [`crate::sniff_encoding`]
//! and [`crate::decode`] always resolve to *some* encoding — falling back to the UTF-8 default
//! (see [`crate::EncodingSource::Default`]) rather than returning an error — `encoding_rs`'s
//! decoder is total over its input (malformed byte sequences are replaced with U+FFFD, never
//! surfaced as an `Err`), and html5ever's `TreeSink` trait (see `crate::sink`) has no failure
//! channel of its own: a malformed or hostile document produces a wrong-looking tree plus
//! [`crate::ParseOutput::parse_errors`] entries, never an `Err`. `HtmlError` is kept as a real,
//! `#[non_exhaustive]` type anyway, so its identity in `cl-html`'s public API is stable for
//! whenever a future task (a resource limit worth aborting on, rather than recovering from,
//! comes to mind first) gives it its first inhabited variant.

/// Errors produced by `cl-html`.
///
/// Uninhabited today — see the module documentation above for why.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum HtmlError {}
