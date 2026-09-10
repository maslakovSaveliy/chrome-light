//! `ChromeLight` HTML pipeline (M1a subset): decodes a document's raw bytes and parses them
//! into a `cl_dom::Document`, the arena tree every later crate (`cl-style`'s cascade,
//! `cl-layout`'s box generation, Task 9's html5lib conformance harness) reads.
//!
//! Two stages, run in order by [`parse_document`]:
//!
//! 1. **Encoding** ([`encoding`], `prescan`): per
//!    <https://html.spec.whatwg.org/multipage/parsing.html#determining-the-character-encoding>,
//!    BOM → transport-layer label → `<meta>` prescan → default — see [`sniff_encoding`] and
//!    [`EncodingSource`]. `<meta>` prescan (HTML §13.2.3.2's bounded, byte-level scan of the
//!    first 1024 bytes) lives in `prescan`, which is crate-private: `prescan_meta_charset` is
//!    plumbing for [`sniff_encoding`], not something a caller should reach for directly.
//!    Implemented in Task 7 of the M1a plan.
//! 2. **Tree construction** (`sink`, crate-private): `sink::DomSink` implements html5ever's
//!    `TreeSink` over a `RefCell<cl_dom::Document>` — the only `RefCell` in this pipeline; see
//!    its module docs for why and how it stays crate-private — so html5ever's own tree-building
//!    algorithm does the actual HTML5-conformant parsing (implicit `<html>`/`<head>`/`<body>`,
//!    the adoption agency algorithm, foreign content, ...) and this crate only translates its
//!    callbacks into `cl_dom::Document` mutations. Implemented in Task 8.
//!
//! html5ever's tokenizer only ever sees already-decoded text: it is never asked to sniff an
//! encoding itself, and it has no failure channel of its own (parse errors are recovered from,
//! not fatal — see [`ParseOutput::parse_errors`]), so [`parse_document`]/[`parse_document_str`]
//! are effectively total over their input; see their doc comments for why `Result` is still the
//! signature.
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(clippy::indexing_slicing)]

pub mod encoding;
pub mod error;
mod prescan;
mod sink;

use cl_net::Url;
use encoding_rs::Encoding;
use html5ever::driver::{ParseOpts, parse_document as drive_parse_document};
use html5ever::tendril::TendrilSink;
use html5ever::tree_builder::TreeBuilderOpts;

pub use cl_dom::Document;
pub use encoding::{EncodingSource, decode, sniff_encoding};
pub use error::HtmlError;

/// The result of parsing an HTML document: the tree html5ever built, which character encoding
/// was used to decode it and why (see [`crate::encoding`]), and every parse error html5ever
/// reported along the way.
#[derive(Debug)]
pub struct ParseOutput {
    /// The parsed document tree.
    pub document: Document,
    /// The character encoding the document's bytes were decoded with. For
    /// [`parse_document_str`], always `encoding_rs::UTF_8` — see its doc comment.
    pub encoding: &'static Encoding,
    /// Which stage of encoding sniffing chose [`ParseOutput::encoding`].
    pub encoding_source: EncodingSource,
    /// Every parse error html5ever reported while building the tree, in report order. The HTML
    /// tree-construction algorithm is defined to always recover from a parse error rather than
    /// abort (see <https://html.spec.whatwg.org/multipage/parsing.html#parse-errors>), so this
    /// is diagnostic-only: a non-empty list does not mean parsing failed, and [`parse_document`]
    /// returns `Ok` regardless of whether any were reported.
    pub parse_errors: Vec<String>,
}

/// Parses `bytes` — a document's raw, possibly attacker-controlled bytes — into a
/// [`ParseOutput`].
///
/// Encoding is determined first ([`decode`]: BOM, then `transport_label` — e.g. an HTTP
/// `Content-Type` header's `charset` parameter, when the caller has one — then a `<meta>`
/// prescan, then the UTF-8 default), and only the decoded text is handed to html5ever: the
/// tokenizer never sees raw bytes and never guesses an encoding itself.
///
/// # Errors
/// Never actually fails today: [`HtmlError`] is uninhabited (see its module docs), [`decode`]
/// is total over its input (malformed byte sequences become U+FFFD, not an error), and
/// html5ever's `TreeSink` has no failure channel of its own — parse errors are collected into
/// [`ParseOutput::parse_errors`], never surfaced as `Err`. `Result` is still the signature so
/// that a future resource-limit rejection (documents pathological enough to be worth aborting
/// rather than recovering from) can become an inhabited `HtmlError` variant without an API
/// break.
#[allow(
    clippy::unnecessary_wraps,
    reason = "see the doc comment: infallible today by design, Result kept for API stability"
)]
pub fn parse_document(
    bytes: &[u8],
    base_url: &Url,
    transport_label: Option<&str>,
) -> Result<ParseOutput, HtmlError> {
    let (text, encoding, encoding_source) = decode(bytes, transport_label);
    let output = run_parser(&text, base_url);
    Ok(ParseOutput {
        document: output.document,
        encoding,
        encoding_source,
        parse_errors: output.parse_errors,
    })
}

/// Parses `html` — already-decoded Unicode text — into a [`ParseOutput`].
///
/// For callers that already have text rather than raw bytes (Task 9's html5lib
/// tree-construction conformance harness, which feeds `.dat`-file fixtures directly; in-memory
/// test fixtures; anywhere a document's encoding was already resolved upstream): skips encoding
/// sniffing entirely. [`ParseOutput::encoding`] is reported as UTF-8 with
/// [`EncodingSource::Default`] — not because sniffing ran and found nothing, but because no
/// sniffing ran at all and UTF-8 is this crate's documented default for "no better answer" (see
/// [`EncodingSource::Default`]'s docs).
///
/// # Errors
/// See [`parse_document`]: never actually fails today, for the same reasons.
#[allow(
    clippy::unnecessary_wraps,
    reason = "see the doc comment: infallible today by design, Result kept for API stability"
)]
pub fn parse_document_str(html: &str, base_url: &Url) -> Result<ParseOutput, HtmlError> {
    let output = run_parser(html, base_url);
    Ok(ParseOutput {
        document: output.document,
        encoding: encoding_rs::UTF_8,
        encoding_source: EncodingSource::Default,
        parse_errors: output.parse_errors,
    })
}

/// Shared driver behind [`parse_document`]/[`parse_document_str`]: builds a fresh
/// [`sink::DomSink`] rooted at `base_url`, runs html5ever's tree builder over already-decoded
/// `text` in one shot (`TendrilSink::one`), and returns the sink's owned output.
///
/// `scripting_enabled: false` in the tree builder options: `ChromeLight` never executes script
/// in M1a (there is no JS engine yet), and per the HTML spec this also governs how `<noscript>`
/// is parsed — with scripting disabled its contents parse as a normal tree of nodes rather than
/// a single opaque text node, which is the behaviour a script-free renderer wants.
/// `drop_doctype: false`: the `DOCTYPE` is kept in the tree (as a [`cl_dom::NodeKind::Doctype`]
/// node) rather than discarded, matching the html5lib tree-construction corpus's expected
/// output, which Task 9 diffs against.
fn run_parser(text: &str, base_url: &Url) -> sink::ParseOutputInner {
    let sink = sink::DomSink::new(base_url.as_str());
    let opts = ParseOpts {
        tree_builder: TreeBuilderOpts {
            drop_doctype: false,
            scripting_enabled: false,
            ..TreeBuilderOpts::default()
        },
        ..ParseOpts::default()
    };
    drive_parse_document(sink, opts).one(text)
}
