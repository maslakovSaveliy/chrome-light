//! The CPU raster backend: [`rasterize`] executes a validated [`DisplayList`] onto a
//! `tiny-skia` pixmap.
//!
//! # How a list is executed
//!
//! One linear pass, no recursion (a `DisplayList` is flat by construction — see
//! `cl_paint::list`'s docs), carrying a stack of clip rectangles. Each item:
//!
//! | Item | Drawn as |
//! |---|---|
//! | [`DisplayItem::Rect`] | one anti-aliasing-off `fill_rect` in the item's colour |
//! | [`DisplayItem::Border`] | four `fill_rect`s: full-width top and bottom strips, left and right strips filling the band between them (M1a: no mitred corners) |
//! | [`DisplayItem::Text`] | one `swash` alpha mask per glyph, composited at a whole-pixel pen position |
//! | [`DisplayItem::PushClip`] | intersects the item's rect with the current clip and pushes it |
//! | [`DisplayItem::PopClip`] | pops back to the previous clip |
//!
//! The clip stack starts with the canvas rect, so every draw is clipped to the pixmap even
//! before a `PushClip` appears; `cl_paint::validate` bounds its depth at
//! `cl_paint::MAX_CLIP_DEPTH`. See `cpu::shapes`'s docs for why the clip is a stack of rects
//! rather than a stack of `tiny_skia::Mask`es.
//!
//! # Determinism
//!
//! Same display list, same fonts, same canvas → same bytes, on every platform. Nothing here
//! reads a clock, spawns a thread, touches the filesystem, or enumerates system fonts
//! (`cl_fonts::FontDb` cannot); the only `HashMap` is the glyph cache, which is a pure
//! lookup that never affects what is painted; all glyph compositing is integer arithmetic;
//! and anti-aliasing is off for every rect and border, so no rasteriser fast-path can differ
//! between builds. `rasterize_should_be_deterministic` asserts it directly.

mod glyph_cache;
mod shapes;
mod text;

use cl_fonts::FontDb;
use cl_layout::{Point, Rect, Size};
use cl_paint::{DisplayItem, DisplayList};
use tiny_skia::{Color, Pixmap};

use crate::error::GfxError;
use text::TextRasterizer;

/// Counters describing the work one [`rasterize_with_stats`] call did.
///
/// Observability, not output: nothing here changes a pixel, and two calls that differ only in
/// their stats painted the same image. It exists because the glyph cache is otherwise
/// invisible from outside the crate — `glyph_cache_should_hit_on_second_use` asserts the
/// cache works without the test needing access to the cache itself, which keeps `GlyphCache`
/// `pub(crate)` as the brief specifies.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct RasterStats {
    /// How many glyph masks were actually rasterised (cache misses).
    pub glyphs_rendered: u64,
    /// How many glyph draws were served from an already-rasterised mask.
    pub cache_hits: u64,
}

/// Rasterises `dl` onto a fresh opaque-white `width` x `height` pixmap.
///
/// The canvas starts opaque white (CSS 2.1 §14.2's "the canvas background" is painted by
/// `cl_paint::build` as an ordinary [`DisplayItem::Rect`] when the document specifies one;
/// white is the UA default underneath it) and every item composites source-over.
///
/// # Order of checks
///
/// 1. `width`/`height` must be in `1..=`[`crate::MAX_DIMENSION`] — a canvas size is the
///    *caller's* argument, not part of the (possibly hostile) display list, so it gets
///    [`GfxError::PixmapTooLarge`] rather than being folded into the list's validation and
///    misreported as the renderer's fault.
/// 2. `cl_paint::validate(dl, bounds)` with `bounds = (0, 0, width, height)`. This runs
///    before the pixmap is allocated and before any pixel is touched, which is the property
///    `docs/SECURITY.md` §3's "renderer → gpu" row asks for.
/// 3. Only then is the pixmap allocated and the list executed.
///
/// # Errors
/// [`GfxError::PixmapTooLarge`], [`GfxError::Invalid`], [`GfxError::FontNotBundled`],
/// [`GfxError::Font`] — see [`GfxError`] for what each means.
pub fn rasterize(
    dl: &DisplayList,
    width: u32,
    height: u32,
    fonts: &FontDb,
) -> Result<Pixmap, GfxError> {
    rasterize_with_stats(dl, width, height, fonts).map(|(pixmap, _stats)| pixmap)
}

/// [`rasterize`], plus the [`RasterStats`] describing the glyph work it did.
///
/// # Errors
/// Identical to [`rasterize`].
pub fn rasterize_with_stats(
    dl: &DisplayList,
    width: u32,
    height: u32,
    fonts: &FontDb,
) -> Result<(Pixmap, RasterStats), GfxError> {
    if width == 0 || height == 0 || width > crate::MAX_DIMENSION || height > crate::MAX_DIMENSION {
        return Err(GfxError::PixmapTooLarge { width, height });
    }
    let bounds = canvas_bounds(width, height);
    cl_paint::validate(dl, bounds)?;

    let mut pixmap =
        Pixmap::new(width, height).ok_or(GfxError::PixmapTooLarge { width, height })?;
    pixmap.fill(Color::WHITE);

    // The canvas rect is the bottom of the clip stack, so every draw is clipped to the pixmap
    // whether or not the list pushes a clip of its own.
    let mut clips: Vec<Rect> = Vec::with_capacity(8);
    clips.push(bounds);
    let mut text = TextRasterizer::new();
    let mut stats = RasterStats::default();

    for item in &dl.items {
        let clip = *clips.last().unwrap_or(&bounds);
        match item {
            DisplayItem::Rect { rect, color } => shapes::fill(&mut pixmap, *rect, *color, clip),
            DisplayItem::Border {
                rect,
                widths,
                colors,
            } => shapes::fill_border(&mut pixmap, *rect, *widths, *colors, clip),
            DisplayItem::Text { run } => {
                text.paint_run(&mut pixmap, run, fonts, clip, &mut stats)?;
            }
            DisplayItem::PushClip { rect } => clips.push(shapes::intersect(clip, *rect)),
            DisplayItem::PopClip => {
                // `cl_paint::validate` already rejected an unbalanced list, so the stack can
                // never be down to its canvas entry here; the guard keeps that a no-op rather
                // than a panic if the invariant is ever weakened.
                if clips.len() > 1 {
                    clips.pop();
                }
            }
        }
    }

    Ok((pixmap, stats))
}

/// The display-list bounds a canvas of `width` x `height` device pixels validates against.
///
/// `width`/`height` are already known to be at most [`crate::MAX_DIMENSION`] (16384), so the
/// `u32` -> `f32` conversion is exact: every integer below 2^24 is representable.
#[allow(
    clippy::cast_precision_loss,
    reason = "callers check width/height <= MAX_DIMENSION (16384) first, far below f32's \
              2^24 exact-integer limit"
)]
fn canvas_bounds(width: u32, height: u32) -> Rect {
    Rect::new(
        Point::default(),
        Size {
            w: cl_layout::Au::from_px(width as f32),
            h: cl_layout::Au::from_px(height as f32),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_bounds_should_be_exact_for_the_largest_allowed_canvas() {
        let bounds = canvas_bounds(crate::MAX_DIMENSION, crate::MAX_DIMENSION);
        assert_eq!(bounds.origin, Point::default());
        assert_eq!(bounds.size.w, cl_layout::Au::from_px(16_384.0));
        assert_eq!(bounds.size.h, cl_layout::Au::from_px(16_384.0));
    }
}
