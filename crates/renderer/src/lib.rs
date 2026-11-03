//! `ChromeLight` renderer document pipeline (M1b Task 5): HTML bytes → DOM → style → layout →
//! paint → [`cl_paint::DisplayList`], shared by the renderer process and `cl-testshell`, so
//! the three-process render and the single-process one run the same code.
//!
//! The pipeline is split where the renderer process has to wait for the browser:
//!
//! 1. [`ParsedDocument::parse`] parses the bytes and lists the subresources the document
//!    references ([`ParsedDocument::subresource_requests`]) without loading any — the
//!    renderer cannot read files; it asks the browser for all of them in one batch
//!    (ADR-0016 §6).
//! 2. [`ParsedDocument::attach`] takes the loaded bytes (or the reasons they are missing) as
//!    a [`SubresourceMap`] and gives a [`LoadedDocument`].
//! 3. [`LoadedDocument::render`] produces a [`Rendered`] display list for a viewport, as
//!    often as asked: a resize re-runs style, layout and paint from the stored DOM.
//!
//! No IO, no threads, no process state: the testshell loads subresources with
//! `cl_net::load_file`, the renderer process gets them over IPC.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod document;
mod error;

pub use document::{LoadedDocument, ParsedDocument, Rendered, StageDumps, Stages, SubresourceMap};
pub use error::RenderError;
