//! From a frame to the window: the surface size for a window, and the blit (ADR-0016 §8).
//!
//! The page is rasterised at DPR 1 in CSS pixels; the browser scales the frame by an integer
//! factor `k` (nearest neighbour) for the window's scale factor. Real DPR raster comes after
//! M1b (`docs/SPEC_REGISTRY.md` records the deviation).

/// The surface to request for a window of `physical` pixels at `scale`: `(css width, css
/// height, k)` with `k = max(1, floor(scale))` and each CSS edge `ceil(physical / k)`, so the
/// frame covers the whole window (the browser crops the last partial pixel). A scale below
/// one, or not finite, is treated as one.
pub fn surface_for(physical: (u32, u32), scale: f64) -> (u32, u32, u32) {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "finite, ≥ 2, floored; `as` saturates at u32::MAX, which no window reaches"
    )]
    let k = if scale.is_finite() && scale >= 2.0 {
        scale.floor() as u32
    } else {
        1
    };
    (physical.0.div_ceil(k), physical.1.div_ceil(k), k)
}

/// Copies a premultiplied, opaque RGBA8 frame of `src_size` into a `0RGB` window buffer of
/// `dst_size`, each source pixel as a `k × k` block (nearest neighbour), cropped to the window
/// and padded with `background` where the frame does not reach. Never panics: a source shorter
/// than its size reads as background, a destination shorter than its size is filled as far as
/// it goes, `k = 0` counts as one.
///
/// The frame is always opaque (the canvas is painted white first), so premultiplied and
/// straight colour are the same and alpha is dropped.
pub fn blit(
    src: &[u8],
    src_size: (u32, u32),
    k: u32,
    dst: &mut [u32],
    dst_size: (u32, u32),
    background: u32,
) {
    let k = k.max(1);
    let dst_width = usize::try_from(dst_size.0).unwrap_or(usize::MAX).max(1);
    let src_width = usize::try_from(src_size.0).unwrap_or(0);
    let src_height = usize::try_from(src_size.1).unwrap_or(0);
    let k = usize::try_from(k).unwrap_or(usize::MAX);
    let rows = dst.chunks_mut(dst_width);
    let rows = rows.take(usize::try_from(dst_size.1).unwrap_or(usize::MAX));
    for (y, row) in rows.enumerate() {
        let sy = y / k;
        for (x, out) in row.iter_mut().enumerate() {
            let sx = x / k;
            *out = if sx < src_width && sy < src_height {
                pixel(src, sy.saturating_mul(src_width).saturating_add(sx)).unwrap_or(background)
            } else {
                background
            };
        }
    }
}

/// Pixel `index` of an RGBA8 buffer as `0RGB`, or `None` past its end.
fn pixel(src: &[u8], index: usize) -> Option<u32> {
    let start = index.checked_mul(4)?;
    let rgba = src.get(start..start.checked_add(4)?)?;
    match rgba {
        [r, g, b, _] => Some(u32::from(*r) << 16 | u32::from(*g) << 8 | u32::from(*b)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{blit, surface_for};

    const RED: [u8; 4] = [255, 0, 0, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];
    const BLUE_0RGB: u32 = 0x0000_00ff;

    #[test]
    fn an_integer_scale_divides_the_physical_size() {
        assert_eq!(surface_for((1600, 1200), 2.0), (800, 600, 2));
        assert_eq!(surface_for((800, 600), 1.0), (800, 600, 1));
    }

    #[test]
    fn fractional_scale_floors_k() {
        assert_eq!(surface_for((1200, 900), 1.5), (1200, 900, 1));
        assert_eq!(surface_for((1500, 900), 2.5), (750, 450, 2));
    }

    #[test]
    fn an_odd_physical_size_rounds_the_css_size_up() {
        // The frame covers the whole window; the last half-pixel column is cropped.
        assert_eq!(surface_for((801, 601), 2.0), (401, 301, 2));
    }

    #[test]
    fn a_scale_below_one_or_not_finite_uses_one() {
        assert_eq!(surface_for((800, 600), 0.5), (800, 600, 1));
        assert_eq!(surface_for((800, 600), f64::NAN), (800, 600, 1));
        assert_eq!(surface_for((800, 600), f64::INFINITY), (800, 600, 1));
    }

    #[test]
    fn blit_k2_nearest_and_crop() {
        // A 2×1 frame [red, green] at k = 2 into a 3×3 window: two rows of red red green
        // (the second green column cropped), then one row of background.
        let src = [RED, GREEN].concat();
        let mut dst = vec![0_u32; 9];
        blit(&src, (2, 1), 2, &mut dst, (3, 3), BLUE_0RGB);
        let (r, g, b) = (0x00ff_0000, 0x0000_ff00, BLUE_0RGB);
        assert_eq!(dst, [r, r, g, r, r, g, b, b, b]);
    }

    #[test]
    fn blit_pads_a_window_larger_than_the_frame() {
        let src = RED.to_vec();
        let mut dst = vec![0_u32; 4];
        blit(&src, (1, 1), 1, &mut dst, (2, 2), BLUE_0RGB);
        assert_eq!(dst, [0x00ff_0000, BLUE_0RGB, BLUE_0RGB, BLUE_0RGB]);
    }

    #[test]
    fn blit_never_panics_on_short_buffers() {
        // A source shorter than its claimed size and a destination shorter than its claimed
        // size: missing source pixels become background, the destination is filled as far as
        // it goes.
        let src = RED.to_vec();
        let mut dst = vec![0_u32; 3];
        blit(&src, (4, 4), 3, &mut dst, (8, 8), BLUE_0RGB);
        assert_eq!(dst, [0x00ff_0000, 0x00ff_0000, 0x00ff_0000]);
        blit(&[], (2, 2), 0, &mut dst, (3, 1), BLUE_0RGB);
        assert_eq!(dst, [BLUE_0RGB; 3]);
    }
}
