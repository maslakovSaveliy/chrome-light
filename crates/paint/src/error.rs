//! Error type for `cl-paint`.
//!
//! [`crate::build::build`] is total over any [`cl_layout::FragmentTree`], including a
//! hand-assembled one whose bookkeeping is inconsistent with itself (a fragment id that
//! resolves to nothing, a style id likewise — see that function's docs): every such case is
//! skipped or falls back to [`cl_layout::LayoutStyle::initial`] rather than failing. That
//! mirrors `cl_layout::LayoutError`'s own starting point (see its
//! docs) before Task 18 gave that crate its one fallible path (font plumbing); `cl-paint` has
//! no such path yet — nothing in a display list needs to name a font by anything richer than
//! the [`cl_fonts::FontKey`] a [`cl_layout::GlyphRun`] already carries, so there is nothing
//! here that can fail.
//!
//! `PaintError` exists anyway, uninhabited today, so:
//!
//! * a caller that already writes `Result<T, PaintError>` (matching the rest of the
//!   pipeline's `Result`-returning public API) has a stable type to name, instead of every
//!   downstream crate reaching for `Infallible` or a bespoke never-type stand-in;
//! * Task 20's validator, which the M1a plan gives its own `DisplayListError` (a sibling
//!   type, not a variant grafted onto this one — see Task 19's controller ruling), has a
//!   `cl-paint`-shaped error identity already in place to sit beside.

/// Errors produced by `cl-paint`.
///
/// Uninhabited for now — see the module docs for why building a display list cannot fail in
/// M1a, and for what future failure modes (none yet) would extend this rather than
/// [`crate::error`] gaining a second type.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PaintError {}
