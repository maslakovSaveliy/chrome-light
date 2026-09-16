//! [`DisplayItem`] and [`DisplayList`]: the plain data a display list is made of.
//!
//! Nothing here has behaviour — [`crate::build::build`] is the only thing that produces a
//! `DisplayList`, and [`crate::dump::display_list_dump`] the only thing that renders one for
//! humans. This mirrors how `cl_layout::geom` is pure vocabulary and `cl_layout::block`/
//! `cl_layout::dump` are the modules that do something with it.

use cl_layout::{Au, GlyphRun, Rect, Rgba8, Sides};

/// One paint-order draw command.
///
/// A display list is a flat sequence of these — no tree, no nesting — so a rasteriser
/// (`cl-gfx`, Task 21) executes it top to bottom with a plain clip-rect stack, never needing
/// to walk a [`cl_layout::FragmentTree`] itself. See [`crate::build::build`]'s docs for the
/// CSS 2.1 Appendix E paint order that produces this sequence.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum DisplayItem {
    /// Fills `rect` with a solid, non-premultiplied `color`.
    ///
    /// Emitted for an element's `background-color` (CSS 2.1 §14.2.1) and, once, for the
    /// canvas background CSS 2.1 §14.2 propagates from `<html>`/`<body>` — see
    /// [`crate::build::build`]'s docs. [`crate::build::build`] never emits one with `color.a
    /// == 0` (fully transparent) or an empty `rect` (see [`cl_layout::Rect::is_empty`]):
    /// both would be a no-op draw call, so the builder skips them instead of leaving that
    /// judgment call to every rasteriser.
    Rect {
        /// The rect to fill, in the same absolute (viewport-relative) coordinate space as
        /// every other rect in a `DisplayList`.
        rect: Rect,
        /// The fill color.
        color: Rgba8,
    },
    /// Strokes up to four sides of `rect` with independent widths and colors.
    ///
    /// Emitted for an element's solid borders (CSS 2.1 §8.5.3, `border-style: solid` only in
    /// M1a — see [`crate::build::build`]'s docs for the borders-only-if-solid-and-nonzero
    /// rule). A side [`crate::build::build`] does not paint (`border-style` not solid, or a
    /// solid side with a zero used width) always carries [`cl_layout::Au::ZERO`] in `widths`
    /// for that side, so a rasteriser that draws exactly what `widths` says paints exactly
    /// the sides this crate decided are solid, without re-deriving that decision itself.
    Border {
        /// The border box these widths are measured inward from.
        rect: Rect,
        /// Each side's stroke width; `Au::ZERO` for a side that is not painted.
        widths: Sides<Au>,
        /// Each side's stroke color (meaningless for a side whose `widths` entry is zero).
        colors: Sides<Rgba8>,
    },
    /// Paints one shaped glyph run.
    ///
    /// Carries the [`cl_layout::GlyphRun`] unchanged (a clone of the one
    /// [`cl_layout::FragmentKind::Text`] holds): `cl-paint` neither reshapes nor
    /// repositions text, it only decides *whether* and *in what order* each run gets
    /// painted — see [`crate::build::build`]'s docs on why a run with no glyphs is skipped.
    Text {
        /// The glyphs to paint, already positioned in absolute coordinates.
        run: GlyphRun,
    },
    /// Pushes `rect` onto the rasteriser's clip stack: nothing drawn until the matching
    /// [`DisplayItem::PopClip`] may paint outside it.
    ///
    /// Emitted once per fragment whose style has `overflow: hidden`, clipping to that
    /// fragment's *padding* box (CSS 2.1 §11.1.1: the clip is the padding edge, not the
    /// border edge) — see [`crate::build::build`]'s docs.
    PushClip {
        /// The clip rectangle: the clipping fragment's padding box.
        rect: Rect,
    },
    /// Pops the most recently pushed clip rectangle, restoring the previous one (or no clip,
    /// if the stack is now empty).
    ///
    /// Always paired with exactly one preceding [`DisplayItem::PushClip`] in a `DisplayList`
    /// [`crate::build::build`] produced from a well-formed [`cl_layout::FragmentTree`] — see
    /// that function's docs for the one case (a malformed tree with a dangling fragment
    /// reference) where a `PushClip` can be left unmatched.
    PopClip,
}

/// A flat, paint-order sequence of [`DisplayItem`]s for one [`cl_layout::FragmentTree`], plus
/// the rect it was painted against.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct DisplayList {
    /// The draw commands, in the order a rasteriser must execute them to get correct paint
    /// order (CSS 2.1 Appendix E).
    pub items: Vec<DisplayItem>,
    /// The list's overall bounds: the viewport rect `(0, 0, viewport.w, viewport.h)` the
    /// [`cl_layout::FragmentTree`] was laid out against — not a bounding box of `items`
    /// (which may be smaller, or, for content that overflows the viewport, larger).
    pub bounds: Rect,
}
