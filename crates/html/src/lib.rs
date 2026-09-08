//! `ChromeLight` HTML pipeline (M1a subset): the encoding sniffer that decides how a
//! document's raw bytes become text before html5ever's tokenizer ever sees them.
//!
//! Encoding precedence, per <https://html.spec.whatwg.org/multipage/parsing.html#determining-the-character-encoding>,
//! is BOM → transport-layer label → `<meta>` prescan → default — see [`sniff_encoding`] and
//! [`EncodingSource`]. `<meta>` prescan (HTML §13.2.3.2's bounded, byte-level scan of the
//! first 1024 bytes) lives in `prescan`, which is crate-private: `prescan_meta_charset` is
//! plumbing for [`sniff_encoding`], not something a caller should reach for directly.
//!
//! `encoding` and `prescan` were implemented in Task 7 of the M1a plan. Task 8 adds the
//! `TreeSink` that bridges html5ever's tree-construction callbacks onto `cl_dom`'s arena
//! `Document` (a `sink` module and the crate's actual `parse` entry point), building on the
//! decoded text this module produces.
#![forbid(unsafe_code)]
#![deny(missing_docs)]
#![deny(clippy::indexing_slicing)]

pub mod encoding;
pub mod error;
mod prescan;

pub use encoding::{EncodingSource, decode, sniff_encoding};
pub use error::HtmlError;
