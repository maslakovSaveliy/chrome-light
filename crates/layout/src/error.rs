//! Error type for `cl-layout`.
//!
//! Almost every path through this crate is total: [`crate::style_adapt::adapt`] is total over
//! any `&ComputedValues` stylo hands back (every computed-value shape it reads has a
//! documented fold onto [`crate::geom::Length`]/[`crate::geom::Display`]/etc., even for values
//! M1a's CSS scope does not implement — see that function's docs), [`crate::box_tree::build`]
//! is total over any [`cl_style::StyledDocument`], including one built from an empty
//! [`cl_dom::Document`] or one whose styles were hand-assembled into an inconsistent shape,
//! and every geometry computation saturates rather than overflowing (see [`crate::au`]).
//!
//! The one thing that can fail is font plumbing: Task 18's shaper
//! (`crate::text::TextShaper`) must be able to name the face `parley` shaped each glyph run
//! with, because a run whose [`cl_fonts::FontKey`] is unknown cannot be rasterised downstream
//! (Task 21 looks glyphs up by `(FontKey, glyph id, size)`). Dropping such a run silently
//! would turn a font-plumbing bug into invisible text, so it is reported instead.

/// Errors produced by `cl-layout`.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LayoutError {
    /// The shaper produced a glyph run whose font is not one of the bundled faces, so it
    /// cannot be mapped back to a [`cl_fonts::FontKey`].
    ///
    /// Unreachable in practice: every font stack this crate builds ends in a bundled family,
    /// and `cl_fonts::FontDb` never enumerates host fonts (see its docs). Reaching this means
    /// the shaper's font database and `FontDb` disagreed about which faces exist — a bug in
    /// this crate or in `cl-fonts`, not a property of the document.
    #[error("text was shaped with a font that is not bundled: {family}")]
    FontNotBundled {
        /// The `font-family` list that was requested, joined with `", "`.
        family: String,
    },
}
