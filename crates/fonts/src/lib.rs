//! `ChromeLight` bundled font database: the engine's only two fonts (Ahem and Noto
//! Sans Regular), embedded at compile time, served through a `fontique::Collection`
//! that never enumerates or reads a host system font.
//!
//! Every reftest and text golden in the M1a static pipeline depends on this: if
//! text layout could ever see a font other than these two, the same HTML would
//! render differently on the owner's machine and in CI, and pixel/text
//! comparisons would be meaningless. See [`FontDb::bundled`] for exactly how
//! system fonts are kept out.
//!
//! Asset provenance, licenses, and checksums live in `assets/SHA256SUMS`,
//! `assets/LICENSE-Ahem`, and `assets/LICENSE-OFL`; `assets/fetch.sh` re-downloads
//! both fonts from their pinned upstream commits and verifies them.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod bundled;
mod db;
mod error;

pub use db::{FontDb, FontFace, FontKey};
pub use error::FontError;
