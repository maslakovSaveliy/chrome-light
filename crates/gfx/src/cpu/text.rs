//! Glyph-run rasterisation: `swash` outlines to 8-bit alpha masks, composited source-over
//! onto the pixmap at whole-pixel positions.
//!
//! # Whole-pixel glyph placement (M1a)
//!
//! A glyph's pen position (`run.origin + glyph.x/y`) is an exact 1/60-pixel
//! [`cl_layout::Au`], but the mask `swash` produces is rasterised once per
//! `(face, glyph, size)` with no sub-pixel phase, so the pen is rounded to the nearest whole
//! device pixel before the mask is blitted. Two reasons, both binding for M1a:
//!
//! * **Determinism.** Sub-pixel positioning means one cached mask per phase (typically 3 or
//!   4 horizontal phases), and which phase a glyph lands in depends on accumulated advance
//!   arithmetic. Whole-pixel placement makes a run's pixels a function of the display list
//!   alone.
//! * **The Ahem reftests.** Task 23's references are Ahem-only precisely so that every
//!   painted shape is a geometric square; a half-pixel horizontal phase would put an
//!   anti-aliased fringe on the left and right edges of every one of them.
//!
//! Vertical sub-pixel positioning is not a thing any engine does, so only the horizontal case
//! is a deliberate omission. Both are M1b+ work, together with LCD/sub-pixel anti-aliasing
//! (which needs `zeno::Format::Subpixel` and a per-display filter, neither of which belongs
//! in a headless reftest backend).
//!
//! # Compositing
//!
//! The mask is coverage, not colour: the effective alpha of a glyph pixel is
//! `mask_alpha × run.color.a`, and the destination is the standard source-over blend in
//! premultiplied 8-bit space. All of it is integer arithmetic ([`mul_255`]), so the result is
//! bit-identical on every platform — `tiny-skia`'s own blitters are not used here, because
//! there is no `tiny-skia` API that composites an external alpha mask with a solid colour
//! without first materialising a full-canvas `Mask` per glyph.

use std::sync::Arc;

use cl_fonts::FontDb;
use cl_layout::{Au, GlyphRun, Rect, Rgba8};
use swash::FontRef;
use swash::scale::{Render, ScaleContext, Source};
use swash::zeno::Format;
use tiny_skia::{Pixmap, PremultipliedColorU8};

use crate::cpu::RasterStats;
use crate::cpu::glyph_cache::{GlyphBitmap, GlyphCache};
use crate::error::GfxError;

/// The glyph sources this backend rasterises from: scalable outlines only.
///
/// Deliberately *not* `Source::Bitmap`/`Source::ColorBitmap`/`Source::ColorOutline`: embedded
/// bitmap strikes and colour glyphs (emoji) are M1b+ work, and neither bundled face has any.
/// A `const` because `swash::scale::Render::new` borrows the slice for the builder's lifetime.
const SOURCES: &[Source] = &[Source::Outline];

/// App units per CSS pixel, mirrored from `cl_layout::au` (whose own constant is private).
///
/// Used only for the two *integer* pixel-grid conversions this module needs — rounding a pen
/// position to the nearest pixel and snapping a clip rect to whole pixels — where going
/// through `Au::to_px`'s `f32` would reintroduce floating point for something that is exact
/// integer division. `cl_layout::Au`'s docs pin this to stylo's `app_units::AU_PER_PX`; a
/// change there is a change to a type this crate does not own, and `Au::from_px(1.0).0`
/// asserts the two agree (see this module's tests).
const AU_PER_PX: i32 = 60;

/// A whole-pixel, half-open device rectangle: `x0 <= x < x1`, `y0 <= y < y1`.
///
/// Glyph masks are blitted pixel by pixel, so the clip they are tested against has to be in
/// pixels too. Snapped *inward* from the app-unit clip (see [`pixel_clip`]).
#[derive(Debug, Clone, Copy)]
pub(crate) struct PixelClip {
    x0: i64,
    y0: i64,
    x1: i64,
    y1: i64,
}

/// Snaps an app-unit clip rect inward to whole device pixels, or `None` if nothing survives.
///
/// Inward (`ceil` the left/top edge, `floor` the right/bottom) rather than outward: a pixel
/// only partly inside the clip is dropped. For the integer-pixel clips M1a's layout actually
/// produces the two agree exactly; for a fractional clip edge this errs toward showing less,
/// which is the safe direction for `overflow: hidden` and keeps glyph edges from bleeding a
/// partially-covered pixel past the clip.
pub(crate) fn pixel_clip(clip: Rect) -> Option<PixelClip> {
    let x0 = i64::from(ceil_px(clip.origin.x));
    let y0 = i64::from(ceil_px(clip.origin.y));
    let x1 = i64::from(floor_px(clip.right()));
    let y1 = i64::from(floor_px(clip.bottom()));
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(PixelClip { x0, y0, x1, y1 })
}

/// Largest whole pixel at or below `au`.
fn floor_px(au: Au) -> i32 {
    au.0.div_euclid(AU_PER_PX)
}

/// Smallest whole pixel at or above `au`.
fn ceil_px(au: Au) -> i32 {
    let floor = au.0.div_euclid(AU_PER_PX);
    if au.0.rem_euclid(AU_PER_PX) == 0 {
        floor
    } else {
        floor.saturating_add(1)
    }
}

/// Nearest whole pixel to `au`, halves rounding up (toward positive y/x).
///
/// Ties break in one fixed direction rather than away from zero so that a run translated by a
/// whole number of pixels paints identically wherever it is on the canvas.
fn round_px(au: Au) -> i32 {
    let floor = au.0.div_euclid(AU_PER_PX);
    if au.0.rem_euclid(AU_PER_PX) * 2 >= AU_PER_PX {
        floor.saturating_add(1)
    } else {
        floor
    }
}

/// `a * b / 255`, rounded, in integers — the standard 8-bit alpha multiply.
///
/// Exact at the endpoints that matter: `mul_255(x, 255) == x` and `mul_255(x, 0) == 0`.
fn mul_255(a: u8, b: u8) -> u8 {
    let t = u32::from(a) * u32::from(b) + 128;
    let scaled = (t + (t >> 8)) >> 8;
    #[allow(
        clippy::cast_possible_truncation,
        reason = "t <= 255*255+128 = 65153, so the shifted result is at most 255"
    )]
    {
        scaled as u8
    }
}

/// Source-over composite of `color` at coverage `mask` onto the premultiplied `dst` pixel.
///
/// Returns `None` when there is nothing to do (zero effective alpha), so the caller can skip
/// the write entirely. The premultiplied invariant (`r,g,b <= a`) is preserved: each channel
/// is `mul_255(c, a) + mul_255(dst_c, 255 - a)` with `c <= 255` and `dst_c <= dst_a`, and
/// [`mul_255`] is monotone in its first argument, so each channel stays under the same sum
/// computed for alpha.
fn source_over(dst: PremultipliedColorU8, color: Rgba8, mask: u8) -> Option<PremultipliedColorU8> {
    let alpha = mul_255(mask, color.a);
    if alpha == 0 {
        return None;
    }
    let inv = 255 - alpha;
    let blend =
        |src: u8, dst_channel: u8| mul_255(src, alpha).saturating_add(mul_255(dst_channel, inv));
    PremultipliedColorU8::from_rgba(
        blend(color.r, dst.red()),
        blend(color.g, dst.green()),
        blend(color.b, dst.blue()),
        alpha.saturating_add(mul_255(dst.alpha(), inv)),
    )
}

/// The glyph rasteriser: `swash`'s scaling context plus this call's glyph cache.
///
/// One per [`crate::cpu::rasterize`] call, never shared and never stored: the cache lives
/// exactly as long as the list being rasterised, so nothing about one call can influence
/// another.
pub(crate) struct TextRasterizer {
    context: ScaleContext,
    cache: GlyphCache,
}

impl TextRasterizer {
    /// A rasteriser with an empty cache.
    pub(crate) fn new() -> TextRasterizer {
        TextRasterizer {
            context: ScaleContext::new(),
            cache: GlyphCache::default(),
        }
    }

    /// Paints `run` onto `pixmap`, clipped to `clip`, counting rasterisations and cache hits
    /// into `stats`.
    ///
    /// # Errors
    /// [`GfxError::FontNotBundled`] if `run.font` names no face in `fonts`;
    /// [`GfxError::Font`] if `swash` cannot parse a bundled face's bytes (see that variant's
    /// docs for why that is unreachable).
    pub(crate) fn paint_run(
        &mut self,
        pixmap: &mut Pixmap,
        run: &GlyphRun,
        fonts: &FontDb,
        clip: Rect,
        stats: &mut RasterStats,
    ) -> Result<(), GfxError> {
        let face = fonts
            .face(run.font)
            .ok_or(GfxError::FontNotBundled { key: run.font })?;

        // Everything below is a no-op draw; the font lookup above still runs first, so a
        // stale font key is reported even for a run that would have painted nothing.
        if run.glyphs.is_empty() || run.color.a == 0 {
            return Ok(());
        }
        let Some(pixel_clip) = pixel_clip(clip) else {
            return Ok(());
        };
        let size_px = run.size.to_px();
        if run.size <= Au::ZERO || !size_px.is_finite() || size_px > crate::MAX_GLYPH_PX {
            // Zero/negative sizes paint nothing; oversized ones are skipped rather than
            // clamped -- see `crate::MAX_GLYPH_PX`.
            return Ok(());
        }
        let face_index = usize::try_from(face.index).unwrap_or(usize::MAX);
        let font = FontRef::from_index(face.data, face_index).ok_or_else(|| {
            GfxError::Font(format!(
                "{} face {} did not parse as a font",
                face.family, face.index
            ))
        })?;
        let size_key = u32::try_from(run.size.0).unwrap_or(u32::MAX);

        // Destructured so `context` and `cache` are two disjoint borrows of `self`: the
        // scaler holds `context` mutably for the whole run while the cache is read and
        // written per glyph.
        let TextRasterizer { context, cache } = self;
        let mut scaler = context.builder(font).size(size_px).hint(false).build();
        let mut render = Render::new(SOURCES);
        render.format(Format::Alpha);

        for glyph in &run.glyphs {
            let key = (run.font, glyph.id, size_key);
            let bitmap: Arc<GlyphBitmap> = if let Some(hit) = cache.get(&key) {
                stats.cache_hits = stats.cache_hits.saturating_add(1);
                hit
            } else {
                stats.glyphs_rendered = stats.glyphs_rendered.saturating_add(1);
                let image = render.render(&mut scaler, glyph.id);
                let bitmap = image.map_or_else(GlyphBitmap::default, |image| GlyphBitmap {
                    left: image.placement.left,
                    top: image.placement.top,
                    width: image.placement.width,
                    height: image.placement.height,
                    alpha: image.data,
                });
                cache.insert(key, bitmap)
            };
            let pen_x = round_px(run.origin.x.saturating_add(glyph.x));
            let pen_y = round_px(run.origin.y.saturating_add(glyph.y));
            blit(pixmap, &bitmap, pen_x, pen_y, run.color, pixel_clip);
        }
        Ok(())
    }
}

/// Composites one glyph mask onto `pixmap` with its pen at `(pen_x, pen_y)` device pixels.
///
/// Every coordinate is computed in `i64` and every pixel written through `get_mut`, so a mask
/// placed far off the canvas (or a `FontKey`/`Placement` pair a hostile display list arranged
/// to be extreme) clips to nothing instead of overflowing or indexing out of bounds. The
/// visible column and row ranges are computed up front rather than testing each pixel, so a
/// mostly-offscreen glyph costs no work per invisible pixel.
fn blit(
    pixmap: &mut Pixmap,
    bitmap: &GlyphBitmap,
    pen_x: i32,
    pen_y: i32,
    color: Rgba8,
    clip: PixelClip,
) {
    if bitmap.width == 0 || bitmap.height == 0 {
        return;
    }
    // `zeno`'s placement uses a bottom-left origin: `left` moves right, `top` measures
    // upward, while device y grows downward -- hence the subtraction.
    let dest_x = i64::from(pen_x) + i64::from(bitmap.left);
    let dest_y = i64::from(pen_y) - i64::from(bitmap.top);

    let canvas_w = i64::from(pixmap.width());
    let canvas_h = i64::from(pixmap.height());
    let x0 = clip.x0.max(0);
    let y0 = clip.y0.max(0);
    let x1 = clip.x1.min(canvas_w);
    let y1 = clip.y1.min(canvas_h);

    let col_start = (x0 - dest_x).max(0);
    let col_end = (x1 - dest_x).min(i64::from(bitmap.width));
    let row_start = (y0 - dest_y).max(0);
    let row_end = (y1 - dest_y).min(i64::from(bitmap.height));
    if col_start >= col_end || row_start >= row_end {
        return;
    }

    let Ok(stride) = usize::try_from(canvas_w) else {
        return;
    };
    let pixels = pixmap.pixels_mut();

    for row in row_start..row_end {
        let Ok(row_u32) = u32::try_from(row) else {
            continue;
        };
        let Ok(y) = usize::try_from(dest_y + row) else {
            continue;
        };
        let Some(row_base) = y.checked_mul(stride) else {
            continue;
        };
        for col in col_start..col_end {
            let Ok(col_u32) = u32::try_from(col) else {
                continue;
            };
            let Some(mask) = bitmap.coverage(col_u32, row_u32) else {
                continue;
            };
            if mask == 0 {
                continue;
            }
            let Ok(x) = usize::try_from(dest_x + col) else {
                continue;
            };
            let Some(index) = row_base.checked_add(x) else {
                continue;
            };
            let Some(pixel) = pixels.get_mut(index) else {
                continue;
            };
            if let Some(blended) = source_over(*pixel, color, mask) {
                *pixel = blended;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn au_per_px_should_match_cl_layout() {
        assert_eq!(Au::from_px(1.0).0, AU_PER_PX);
    }

    #[test]
    fn floor_ceil_round_should_agree_on_whole_pixels() {
        let au = Au::from_px(7.0);
        assert_eq!(floor_px(au), 7);
        assert_eq!(ceil_px(au), 7);
        assert_eq!(round_px(au), 7);
    }

    #[test]
    fn floor_ceil_round_should_handle_fractions() {
        let au = Au::from_px(7.25);
        assert_eq!(floor_px(au), 7);
        assert_eq!(ceil_px(au), 8);
        assert_eq!(round_px(au), 7);
        let au = Au::from_px(7.5);
        assert_eq!(round_px(au), 8, "halves round up");
        let au = Au::from_px(7.75);
        assert_eq!(round_px(au), 8);
    }

    #[test]
    fn floor_ceil_round_should_handle_negatives() {
        let au = Au::from_px(-7.25);
        assert_eq!(floor_px(au), -8);
        assert_eq!(ceil_px(au), -7);
        assert_eq!(round_px(au), -7);
    }

    #[test]
    fn pixel_helpers_should_not_overflow_at_the_extremes() {
        for au in [Au(i32::MIN), Au(i32::MAX), Au::ZERO] {
            let _ = floor_px(au);
            let _ = ceil_px(au);
            let _ = round_px(au);
        }
    }

    #[test]
    fn mul_255_should_be_exact_at_the_endpoints() {
        for v in [0u8, 1, 51, 128, 204, 254, 255] {
            assert_eq!(mul_255(v, 255), v, "x * 255 / 255 == x for {v}");
            assert_eq!(mul_255(v, 0), 0);
        }
    }

    #[test]
    fn source_over_should_blend_black_over_white() {
        #[allow(
            clippy::expect_used,
            reason = "a failed setup step in a test should abort that test, loudly"
        )]
        let white =
            PremultipliedColorU8::from_rgba(255, 255, 255, 255).expect("opaque white is valid");
        let black = Rgba8::BLACK;
        assert_eq!(
            source_over(white, black, 255).map(|p| (p.red(), p.alpha())),
            Some((0, 255))
        );
        assert_eq!(
            source_over(white, black, 204).map(|p| (p.red(), p.alpha())),
            Some((51, 255))
        );
        assert_eq!(
            source_over(white, black, 52).map(|p| (p.red(), p.alpha())),
            Some((203, 255))
        );
        assert_eq!(source_over(white, black, 0), None);
    }

    #[test]
    fn pixel_clip_should_snap_inward() {
        let clip = pixel_clip(Rect::from_px(1.5, 2.5, 6.0, 6.0));
        let clip = clip.unwrap_or(PixelClip {
            x0: 0,
            y0: 0,
            x1: 0,
            y1: 0,
        });
        assert_eq!((clip.x0, clip.y0, clip.x1, clip.y1), (2, 3, 7, 8));
    }

    #[test]
    fn pixel_clip_should_be_none_for_a_sub_pixel_clip() {
        assert!(pixel_clip(Rect::from_px(1.2, 1.2, 0.3, 0.3)).is_none());
    }
}
