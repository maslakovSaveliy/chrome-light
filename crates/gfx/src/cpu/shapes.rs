//! Rect and border rasterisation, plus the app-unit geometry helpers the whole CPU backend
//! shares.
//!
//! # Why there is no `tiny_skia::Mask`
//!
//! `cl_paint::DisplayItem::PushClip` only ever carries an axis-aligned rectangle (CSS 2.1
//! §11.1.1's `overflow: hidden` clip to the padding box — `cl_paint::build`'s docs), and
//! intersecting axis-aligned rectangles is closed: the intersection of any number of them is
//! one rectangle. So the clip stack is a stack of *rects*, and clipping a draw is
//! [`intersect`] plus a skip when the result is empty. A `tiny_skia::Mask` would compute the
//! same coverage through an 8-bit buffer the size of the canvas, allocated once per clip
//! level — pure cost with no behaviour to show for it, and (with anti-aliasing on) a source
//! of fringe pixels along clip edges that the rect intersection cannot produce.
//!
//! Non-rectangular clips (`border-radius`, `clip-path`, transformed clips) are not in M1a's
//! display list at all; whichever milestone adds them is the one that must add a mask path.

use cl_layout::{Au, Point, Rect, Rgba8, Sides, Size};
use tiny_skia::{Paint, Pixmap, Transform};

/// The intersection of two rects, in app units, or an empty rect if they do not overlap.
///
/// Total: the subtraction saturates rather than overflowing, so a rect at `Au`'s extremes
/// yields a nonsensical-but-finite size rather than panicking. The result is only ever fed
/// to [`fill`] (which skips empty rects) or pushed on the clip stack (where an empty rect
/// correctly means "nothing below this may paint").
pub(crate) fn intersect(a: Rect, b: Rect) -> Rect {
    let left = a.origin.x.max(b.origin.x);
    let top = a.origin.y.max(b.origin.y);
    let right = a.right().min(b.right());
    let bottom = a.bottom().min(b.bottom());
    if right <= left || bottom <= top {
        return Rect::default();
    }
    Rect::new(
        Point { x: left, y: top },
        Size {
            w: right.saturating_sub(left),
            h: bottom.saturating_sub(top),
        },
    )
}

/// Converts an app-unit rect to the `f32` rect `tiny-skia` draws with.
///
/// The single point in this crate where rect geometry becomes floating point (see the crate
/// docs). Built from the four *edges* rather than origin-plus-size so the right and bottom
/// edges are exactly [`Rect::right`]/[`Rect::bottom`] converted, with no `x + w` rounding
/// happening a second time in `f32`. Returns `None` for a degenerate or non-finite rect,
/// which `tiny_skia::Rect::from_ltrb` refuses to build.
fn to_skia(rect: Rect) -> Option<tiny_skia::Rect> {
    tiny_skia::Rect::from_ltrb(
        rect.origin.x.to_px(),
        rect.origin.y.to_px(),
        rect.right().to_px(),
        rect.bottom().to_px(),
    )
}

/// Fills `rect` with `color`, clipped to `clip`, compositing source-over onto `pixmap`.
///
/// Anti-aliasing is off: an integer-pixel rect must cover exactly the pixels it names, with
/// no half-covered fringe row. That is what makes the reftests of Task 23 comparable
/// byte-for-byte across platforms, and it is correct for M1a's display list, which only ever
/// contains axis-aligned rects. A fully transparent colour and an empty (or fully clipped
/// away) rect both draw nothing.
pub(crate) fn fill(pixmap: &mut Pixmap, rect: Rect, color: Rgba8, clip: Rect) {
    if color.a == 0 {
        return;
    }
    let clipped = intersect(rect, clip);
    if clipped.is_empty() {
        return;
    }
    let Some(skia_rect) = to_skia(clipped) else {
        return;
    };
    let mut paint = Paint {
        anti_alias: false,
        ..Paint::default()
    };
    // `tiny_skia::Color` takes straight (non-premultiplied) 8-bit channels, exactly the form
    // `cl_layout::Rgba8` stores, and premultiplies internally on the blit.
    paint.set_color_rgba8(color.r, color.g, color.b, color.a);
    pixmap.fill_rect(skia_rect, &paint, Transform::identity(), None);
}

/// Paints up to four solid border strips inside `rect`, clipped to `clip`.
///
/// M1a has no mitred corners: the top and bottom strips span the box's full width and each
/// corner therefore belongs to them, while the left and right strips fill only the band
/// between top and bottom. Drawing the real mitre needs four trapezoids (CSS backgrounds-3
/// §4.5), which in turn needs anti-aliased path filling, which would make every bordered box
/// produce platform-dependent fringe pixels — a trade M1a's exact-pixel reftests are not
/// willing to make. `cl_paint::build` already zeroes the width of any side it decided not to
/// paint, so a zero-width side here simply produces an empty strip and draws nothing.
///
/// `cl_paint::validate` guarantees each width is individually non-negative and no larger than
/// the box's corresponding dimension, but *not* that top + bottom fits inside the height, so
/// the middle band's height is computed with saturating arithmetic and skipped when it
/// collapses.
pub(crate) fn fill_border(
    pixmap: &mut Pixmap,
    rect: Rect,
    widths: Sides<Au>,
    colors: Sides<Rgba8>,
    clip: Rect,
) {
    let clamp = |w: Au, extent: Au| w.max(Au::ZERO).min(extent.max(Au::ZERO));
    let top = clamp(widths.top, rect.size.h);
    let bottom = clamp(widths.bottom, rect.size.h);
    let left = clamp(widths.left, rect.size.w);
    let right = clamp(widths.right, rect.size.w);

    let origin = rect.origin;
    let (w, h) = (rect.size.w, rect.size.h);

    fill(
        pixmap,
        Rect::new(origin, Size { w, h: top }),
        colors.top,
        clip,
    );
    fill(
        pixmap,
        Rect::new(
            Point {
                x: origin.x,
                y: rect.bottom().saturating_sub(bottom),
            },
            Size { w, h: bottom },
        ),
        colors.bottom,
        clip,
    );

    let band_y = origin.y.saturating_add(top);
    let band_h = h.saturating_sub(top).saturating_sub(bottom);
    if band_h <= Au::ZERO {
        return;
    }
    fill(
        pixmap,
        Rect::new(
            Point {
                x: origin.x,
                y: band_y,
            },
            Size { w: left, h: band_h },
        ),
        colors.left,
        clip,
    );
    fill(
        pixmap,
        Rect::new(
            Point {
                x: rect.right().saturating_sub(right),
                y: band_y,
            },
            Size {
                w: right,
                h: band_h,
            },
        ),
        colors.right,
        clip,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intersect_should_return_the_overlap() {
        let a = Rect::from_px(0.0, 0.0, 10.0, 10.0);
        let b = Rect::from_px(5.0, 5.0, 10.0, 10.0);
        assert_eq!(intersect(a, b), Rect::from_px(5.0, 5.0, 5.0, 5.0));
    }

    #[test]
    fn intersect_should_be_empty_for_disjoint_rects() {
        let a = Rect::from_px(0.0, 0.0, 10.0, 10.0);
        let b = Rect::from_px(20.0, 20.0, 5.0, 5.0);
        assert!(intersect(a, b).is_empty());
    }

    #[test]
    fn intersect_should_be_empty_for_touching_rects() {
        let a = Rect::from_px(0.0, 0.0, 10.0, 10.0);
        let b = Rect::from_px(10.0, 0.0, 10.0, 10.0);
        assert!(intersect(a, b).is_empty());
    }

    #[test]
    fn intersect_should_saturate_at_the_extremes() {
        let a = Rect::new(
            Point {
                x: Au(i32::MIN),
                y: Au(i32::MIN),
            },
            Size {
                w: Au::MAX,
                h: Au::MAX,
            },
        );
        let b = Rect::new(
            Point {
                x: Au::ZERO,
                y: Au::ZERO,
            },
            Size {
                w: Au::MAX,
                h: Au::MAX,
            },
        );
        // Must not panic; the exact size is uninteresting, only that it is finite.
        let _ = intersect(a, b);
    }

    #[test]
    fn to_skia_should_reject_an_inverted_rect() {
        // `tiny_skia::Rect::from_ltrb` accepts a zero-size rect (`left == right`) but not an
        // inverted one. A zero-size rect never reaches `to_skia` anyway: `fill` drops it on
        // `Rect::is_empty` first.
        let inverted = Rect::new(
            Point {
                x: Au::ZERO,
                y: Au::ZERO,
            },
            Size {
                w: Au::from_px(-1.0),
                h: Au::from_px(10.0),
            },
        );
        assert!(to_skia(inverted).is_none());
    }
}
