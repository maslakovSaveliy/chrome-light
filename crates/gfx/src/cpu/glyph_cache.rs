//! [`GlyphBitmap`] and [`GlyphCache`]: the rasterised alpha masks `swash` produced, keyed so
//! the same glyph at the same size is only ever outlined and scan-converted once per
//! [`crate::cpu::rasterize`] call.
//!
//! The cache is a pure lookup: it never changes *what* is painted, only how many times a
//! glyph is rasterised. Nothing in this module (or anywhere reachable from
//! [`crate::cpu::rasterize`]) iterates the `HashMap`, so its randomised iteration order can
//! never reach the output — see the determinism note in [`crate::cpu`]'s docs.

use std::collections::HashMap;
use std::sync::Arc;

use cl_fonts::FontKey;

/// What identifies one rasterised glyph: the face it came from, its glyph id within that
/// face, and the size it was scaled at.
///
/// The size is the run's `cl_layout::Au`, i.e. an exact 1/60-pixel integer, not the `f32`
/// handed to the scaler: two runs whose sizes differ by less than a rounding step must not
/// collide, and an `f32` is not a sound `HashMap` key anyway (`NaN != NaN`, `-0.0 == 0.0`).
/// Stored as `u32` because a run's size is never negative by the time it gets here.
pub(crate) type GlyphKey = (FontKey, u16, u32);

/// One glyph's 8-bit alpha coverage mask, plus where it sits relative to the pen position.
///
/// `left`/`top` are `zeno::Placement`'s, unchanged: `left` is the offset from the pen to the
/// mask's left edge, and `top` the distance from the pen *up* to the mask's top edge (the
/// mask is produced with `zeno`'s `Origin::BottomLeft`, so `top` grows upward while device
/// y grows downward — hence the subtraction in [`crate::cpu::text`]'s blit).
///
/// A glyph with no outline at all (a space, or an id the face does not draw) is stored as a
/// zero-sized bitmap rather than as an absent entry, so repeatedly painting it is a cache
/// hit rather than a repeated failed rasterisation.
#[derive(Debug, Default)]
pub(crate) struct GlyphBitmap {
    /// Horizontal offset from the pen to the mask's left edge, in whole device pixels.
    pub(crate) left: i32,
    /// Distance from the pen *upward* to the mask's top edge, in whole device pixels.
    pub(crate) top: i32,
    /// The mask's width in pixels.
    pub(crate) width: u32,
    /// The mask's height in pixels.
    pub(crate) height: u32,
    /// Row-major coverage, one byte per pixel, `width * height` long.
    pub(crate) alpha: Vec<u8>,
}

impl GlyphBitmap {
    /// The coverage byte at `(col, row)`, or `None` if either is out of range or the mask's
    /// data is shorter than its declared size.
    ///
    /// Every read of `alpha` goes through here: `clippy::indexing_slicing` is denied in this
    /// workspace precisely so that a length `swash` reported and a length it actually filled
    /// can never become an out-of-bounds read in the gpu process.
    pub(crate) fn coverage(&self, col: u32, row: u32) -> Option<u8> {
        if col >= self.width || row >= self.height {
            return None;
        }
        let index = usize::try_from(row)
            .ok()?
            .checked_mul(usize::try_from(self.width).ok()?)?
            .checked_add(usize::try_from(col).ok()?)?;
        self.alpha.get(index).copied()
    }

    /// How many bytes this bitmap costs the cache, for [`GlyphCache`]'s budget.
    pub(crate) fn byte_size(&self) -> usize {
        self.alpha.len()
    }
}

/// The largest total mask payload [`GlyphCache`] will hold, in bytes.
///
/// A display list may name up to `cl_paint::MAX_GLYPHS_PER_RUN` (65536) glyphs per run across
/// up to `cl_paint::MAX_ITEMS` runs, each at any size up to [`crate::MAX_GLYPH_PX`]. Caching
/// every distinct one unconditionally would let a hostile list turn "render this page" into
/// a multi-gigabyte allocation. Past the budget the cache simply stops accepting new entries
/// — glyphs still paint, they are just re-rasterised — so the cap changes performance, never
/// pixels.
const MAX_CACHE_BYTES: usize = 16 * 1024 * 1024;

/// Per-`rasterize`-call store of rasterised glyph masks.
#[derive(Debug, Default)]
pub(crate) struct GlyphCache {
    map: HashMap<GlyphKey, Arc<GlyphBitmap>>,
    bytes: usize,
}

impl GlyphCache {
    /// Looks `key` up, returning a shared handle to the mask if it is already rasterised.
    pub(crate) fn get(&self, key: &GlyphKey) -> Option<Arc<GlyphBitmap>> {
        self.map.get(key).map(Arc::clone)
    }

    /// Stores `bitmap` under `key` (unless the byte budget is exhausted) and returns the
    /// shared handle either way, so the caller can paint with it regardless.
    pub(crate) fn insert(&mut self, key: GlyphKey, bitmap: GlyphBitmap) -> Arc<GlyphBitmap> {
        let handle = Arc::new(bitmap);
        let cost = handle.byte_size();
        if self.bytes.saturating_add(cost) <= MAX_CACHE_BYTES {
            self.bytes = self.bytes.saturating_add(cost);
            self.map.insert(key, Arc::clone(&handle));
        }
        handle
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> GlyphKey {
        // `FontKey` has no public constructor outside `cl-fonts`, so a real one is borrowed
        // from the bundled database.
        #[allow(
            clippy::expect_used,
            reason = "a failed setup step in a test should abort that test, loudly"
        )]
        let db = cl_fonts::FontDb::bundled().expect("bundled font db");
        #[allow(
            clippy::expect_used,
            reason = "a failed setup step in a test should abort that test, loudly"
        )]
        let font = db.key_for("Ahem").expect("Ahem is bundled");
        (font, 7, 960)
    }

    #[test]
    fn coverage_should_read_row_major() {
        let bitmap = GlyphBitmap {
            left: 0,
            top: 0,
            width: 2,
            height: 2,
            alpha: vec![1, 2, 3, 4],
        };
        assert_eq!(bitmap.coverage(0, 0), Some(1));
        assert_eq!(bitmap.coverage(1, 0), Some(2));
        assert_eq!(bitmap.coverage(0, 1), Some(3));
        assert_eq!(bitmap.coverage(1, 1), Some(4));
    }

    #[test]
    fn coverage_should_be_none_out_of_range() {
        let bitmap = GlyphBitmap {
            left: 0,
            top: 0,
            width: 2,
            height: 2,
            alpha: vec![1, 2, 3, 4],
        };
        assert_eq!(bitmap.coverage(2, 0), None);
        assert_eq!(bitmap.coverage(0, 2), None);
    }

    #[test]
    fn coverage_should_be_none_when_data_is_shorter_than_declared() {
        let bitmap = GlyphBitmap {
            left: 0,
            top: 0,
            width: 4,
            height: 4,
            alpha: vec![9],
        };
        assert_eq!(bitmap.coverage(0, 0), Some(9));
        assert_eq!(bitmap.coverage(3, 3), None);
    }

    #[test]
    fn insert_should_make_the_next_get_a_hit() {
        let mut cache = GlyphCache::default();
        let k = key();
        assert!(cache.get(&k).is_none());
        cache.insert(
            k,
            GlyphBitmap {
                left: 1,
                top: 2,
                width: 1,
                height: 1,
                alpha: vec![255],
            },
        );
        let hit = cache.get(&k);
        assert!(hit.is_some());
        assert_eq!(hit.map(|b| b.left), Some(1));
    }

    #[test]
    fn insert_should_stop_caching_past_the_byte_budget() {
        let mut cache = GlyphCache::default();
        let (font, _, _) = key();
        // One entry that alone exhausts the budget, then a second that must not be stored.
        let big = GlyphBitmap {
            left: 0,
            top: 0,
            width: 1,
            height: 1,
            alpha: vec![0; MAX_CACHE_BYTES],
        };
        cache.insert((font, 1, 960), big);
        cache.insert(
            (font, 2, 960),
            GlyphBitmap {
                left: 0,
                top: 0,
                width: 1,
                height: 1,
                alpha: vec![255],
            },
        );
        assert!(cache.get(&(font, 1, 960)).is_some());
        assert!(
            cache.get(&(font, 2, 960)).is_none(),
            "the second entry must be dropped once the budget is spent"
        );
    }
}
