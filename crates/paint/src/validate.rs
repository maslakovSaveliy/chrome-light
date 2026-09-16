//! [`validate`]: schema-level validation of a [`DisplayList`] at the point it crosses the
//! renderer → GPU process boundary (M1b — see `docs/ARCHITECTURE.md`'s process map and
//! `docs/SECURITY.md` §3's "renderer → gpu" trust boundary row).
//!
//! A [`DisplayList`] is produced by [`crate::build::build`] from a well-formed
//! [`cl_layout::FragmentTree`], but M1b hands it to `cl-gfx` (Task 21) over shared memory
//! from a sandboxed, potentially compromised renderer process — the same "the renderer is
//! hostile" posture `CLAUDE.md` §3.1 states for every browser → renderer boundary, mirrored
//! here for the reverse direction. `validate` is the schema check the receiving gpu process
//! runs *before* touching any of a `DisplayList`'s geometry: it never judges whether the
//! list *looks right* (paint order, which colors were used, which fonts), only whether every
//! number in it is safe for a rasteriser to consume without its own bounds checks — that is
//! `cl-gfx`'s job to rely on, not repeat.
//!
//! # What `validate` does not judge
//!
//! * **Font identity.** [`cl_fonts::FontKey`]'s `u32` is treated as opaque data: any value is
//!   a well-formed key at this layer, `FontKey` being valid-for-some-renderer's-`FontDb` is a
//!   fact only the process holding that `FontDb` can check. A key `cl-gfx`'s own database
//!   does not recognise is a normal, expected raster-time condition (a stale key from a
//!   renderer racing a font-database rebuild), not a validation failure.
//! * **Colour.** Every [`cl_layout::Rgba8`] bit pattern is a valid colour (straight alpha,
//!   0-255 range is exactly what the type's fields already enforce).
//!
//! # What it does judge
//!
//! One linear pass over [`DisplayList::items`] (no recursion, no allocation beyond a `usize`
//! clip-depth counter), checking:
//!
//! * `bounds` itself is a valid, non-empty rect ([`DisplayListError::InvalidBounds`]).
//! * The list holds at most [`MAX_ITEMS`] items ([`DisplayListError::TooManyItems`]).
//! * Every [`DisplayItem::Rect`]/[`DisplayItem::Border`]/[`DisplayItem::PushClip`] rect has
//!   non-negative size and computes `origin + size` without `i32` overflow
//!   ([`DisplayListError::InvalidRect`]), and lies within `bounds` inflated by 4096 pixels
//!   (`OFFSCREEN_MARGIN_PX`) on every side — offscreen content within that margin is
//!   legitimate, not a validation failure ([`DisplayListError::OutOfBounds`]).
//! * A [`DisplayItem::Border`]'s widths are each non-negative and no larger than the rect's
//!   corresponding dimension (top/bottom against height, left/right against width) — also
//!   [`DisplayListError::InvalidRect`], since an oversized or negative stroke width is the
//!   same "this rect's geometry cannot be rasterised as given" failure as a malformed rect.
//! * A [`DisplayItem::Text`] run holds at most [`MAX_GLYPHS_PER_RUN`] glyphs
//!   ([`DisplayListError::TooManyGlyphs`]), its `origin` lies within the inflated bounds
//!   ([`DisplayListError::OutOfBounds`]), and every glyph's offset added to that origin does
//!   not overflow `i32` — reusing [`DisplayListError::InvalidRect`] rather than a dedicated
//!   variant, documented here: a glyph offset that overflows against its run's origin is the
//!   same "this geometry cannot be represented" failure a malformed rect is, just measured
//!   from a point instead of a rect corner.
//! * [`DisplayItem::PushClip`]/[`DisplayItem::PopClip`] are balanced: a `PopClip` with no
//!   open `PushClip` is [`DisplayListError::PopWithoutPush`], clip nesting past
//!   [`MAX_CLIP_DEPTH`] is [`DisplayListError::TooDeepClip`] (a variant beyond the brief's
//!   six, documented: without it a hostile list can make the rasteriser allocate one clip
//!   mask per nesting level, unbounded), and a nonzero depth at the end of the list is
//!   [`DisplayListError::UnbalancedClip`].
//!
//! [`DisplayListError::InvalidBounds`] is likewise a variant beyond the brief's six named
//! error variants, documented: `bounds` is the frame every other check measures against, so a
//! malformed `bounds` (negative/zero size, or one that itself overflows) must be rejected on
//! its own dedicated variant rather than misreported as item 0's fault.

use cl_layout::{Au, GlyphRun, Point, Rect, Sides};

use crate::list::{DisplayItem, DisplayList};

/// The largest number of [`DisplayItem`]s a [`DisplayList`] may hold.
///
/// A generous ceiling (real pages produce thousands, not millions, of items): it exists so a
/// hostile renderer cannot make the gpu process allocate an unbounded amount of per-item
/// rasteriser state from a single IPC message.
pub const MAX_ITEMS: usize = 1_000_000;

/// The largest number of glyphs a single [`DisplayItem::Text`] run may hold.
pub const MAX_GLYPHS_PER_RUN: usize = 65_536;

/// The largest [`DisplayItem::PushClip`] nesting depth a [`DisplayList`] may reach.
///
/// Each open clip is a mask the rasteriser must keep live until its matching `PopClip`; this
/// bounds how many a single list can ask for at once.
pub const MAX_CLIP_DEPTH: usize = 256;

/// How far outside `bounds` an item's geometry may extend and still be accepted as
/// legitimately offscreen content, in CSS pixels — a document with content well past the
/// viewport (a tall page, an element positioned off to the side) is ordinary, not hostile.
///
/// `pub`, and reused as-is (not merely copied) by [`mod@crate::build`]'s own offscreen-item
/// culling: `build` skips emitting an item whose geometry does not *intersect* the viewport
/// widened by this same margin, so an item that survives culling here is guaranteed to have
/// at least a chance of passing this module's *full-containment* check against that same
/// margin. Sharing the one constant is what keeps the two bounds from ever drifting apart —
/// see `crate::build`'s module docs, "Culling offscreen items".
pub const OFFSCREEN_MARGIN_PX: f32 = 4096.0;

/// Errors [`validate`] reports against a [`DisplayList`]. Every variant carries the offending
/// item's index into [`DisplayList::items`] (`UnbalancedClip` excepted: an imbalance is a
/// property of the whole list, not of one item) so a caller can report exactly which item a
/// hostile or buggy renderer sent.
///
/// The first six variants are named in Task 20's brief; [`DisplayListError::TooDeepClip`] and
/// [`DisplayListError::InvalidBounds`] are added beyond it — see the module docs for why
/// each is needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DisplayListError {
    /// The list holds more than [`MAX_ITEMS`] items.
    #[error("display list has {count} items, over the limit of {max}")]
    TooManyItems {
        /// The list's actual item count.
        count: usize,
        /// [`MAX_ITEMS`].
        max: usize,
    },
    /// Item `index`'s glyph run holds more than [`MAX_GLYPHS_PER_RUN`] glyphs.
    #[error("item {index}: glyph run exceeds the per-run glyph limit")]
    TooManyGlyphs {
        /// The offending item's index into [`DisplayList::items`].
        index: usize,
    },
    /// Item `index`'s rect (or border widths, or a text run's glyph offset) is malformed:
    /// negative size, negative or oversized border width, or arithmetic that would overflow
    /// `i32` — see the module docs for exactly which checks land here.
    #[error("item {index}: invalid rect geometry")]
    InvalidRect {
        /// The offending item's index into [`DisplayList::items`].
        index: usize,
    },
    /// Item `index`'s geometry lies outside `bounds` inflated by the offscreen margin.
    #[error("item {index}: geometry lies outside the validated bounds")]
    OutOfBounds {
        /// The offending item's index into [`DisplayList::items`].
        index: usize,
    },
    /// Item `index` is a [`DisplayItem::PopClip`] with no open [`DisplayItem::PushClip`].
    #[error("item {index}: PopClip with no matching PushClip")]
    PopWithoutPush {
        /// The offending item's index into [`DisplayList::items`].
        index: usize,
    },
    /// Item `index` is a [`DisplayItem::PushClip`] that would nest deeper than
    /// [`MAX_CLIP_DEPTH`].
    #[error("item {index}: clip nesting exceeds the limit of {max}")]
    TooDeepClip {
        /// The offending item's index into [`DisplayList::items`].
        index: usize,
        /// [`MAX_CLIP_DEPTH`].
        max: usize,
    },
    /// The list ends with `depth_at_end` [`DisplayItem::PushClip`]s still open (never
    /// matched by a [`DisplayItem::PopClip`]).
    #[error("clip stack unbalanced: depth {depth_at_end} left open at the end of the list")]
    UnbalancedClip {
        /// The clip nesting depth still open when the list ended.
        depth_at_end: i32,
    },
    /// `bounds` itself is not a valid, non-empty rect (negative or zero size, or arithmetic
    /// that would overflow `i32`).
    #[error("display list bounds are invalid")]
    InvalidBounds,
}

/// The `bounds` rect widened by [`OFFSCREEN_MARGIN_PX`] on every side — the region every
/// item's geometry must stay within. Computed once per [`validate`] call, not per item.
struct InflatedBounds {
    min_x: Au,
    min_y: Au,
    max_x: Au,
    max_y: Au,
}

impl InflatedBounds {
    /// Whether `point` lies within (inclusive of) this region.
    fn contains_point(&self, point: Point) -> bool {
        point.x >= self.min_x
            && point.x <= self.max_x
            && point.y >= self.min_y
            && point.y <= self.max_y
    }

    /// Whether `rect` lies entirely within this region. Only meaningful for a `rect` that has
    /// already passed [`rect_is_valid`] — an invalid rect's `right()`/`bottom()` would
    /// saturate rather than reflect its true (overflowing) extent.
    fn contains_rect(&self, rect: Rect) -> bool {
        rect.origin.x >= self.min_x
            && rect.origin.y >= self.min_y
            && rect.right() <= self.max_x
            && rect.bottom() <= self.max_y
    }
}

/// Whether `bounds` is a valid, non-empty rect: strictly positive size, `origin + size`
/// computable without `i32` overflow.
fn bounds_is_valid(bounds: Rect) -> bool {
    bounds.size.w.0 > 0
        && bounds.size.h.0 > 0
        && bounds.origin.x.0.checked_add(bounds.size.w.0).is_some()
        && bounds.origin.y.0.checked_add(bounds.size.h.0).is_some()
}

/// Widens `bounds` by [`OFFSCREEN_MARGIN_PX`] on every side, with saturating arithmetic.
/// Callers must first confirm `bounds` passes [`bounds_is_valid`]; no bounds this function is
/// ever called with is expected to actually reach `Au`'s saturation limits, but saturating
/// (rather than checked) is correct here regardless: a viewport near `i32`'s extremes should
/// widen up to the representable limit, not fail validation over its own margin.
fn inflate(bounds: Rect) -> InflatedBounds {
    let margin = Au::from_px(OFFSCREEN_MARGIN_PX);
    InflatedBounds {
        min_x: bounds.origin.x.saturating_sub(margin),
        min_y: bounds.origin.y.saturating_sub(margin),
        max_x: bounds.right().saturating_add(margin),
        max_y: bounds.bottom().saturating_add(margin),
    }
}

/// Whether `rect` has non-negative size and `origin + size` does not overflow `i32`.
fn rect_is_valid(rect: Rect) -> bool {
    rect.size.w.0 >= 0
        && rect.size.h.0 >= 0
        && rect.origin.x.0.checked_add(rect.size.w.0).is_some()
        && rect.origin.y.0.checked_add(rect.size.h.0).is_some()
}

/// Whether a [`DisplayItem::Border`]'s `widths` are each non-negative and no larger than
/// `rect`'s corresponding dimension (top/bottom measured against height, left/right against
/// width).
fn border_widths_valid(rect: Rect, widths: Sides<Au>) -> bool {
    let non_negative =
        widths.top.0 >= 0 && widths.right.0 >= 0 && widths.bottom.0 >= 0 && widths.left.0 >= 0;
    non_negative
        && widths.top <= rect.size.h
        && widths.bottom <= rect.size.h
        && widths.left <= rect.size.w
        && widths.right <= rect.size.w
}

/// Validates one `Rect`/`Border`/`PushClip` rect: well-formed, then within `inflated` bounds.
fn check_rect(rect: Rect, index: usize, inflated: &InflatedBounds) -> Result<(), DisplayListError> {
    if !rect_is_valid(rect) {
        return Err(DisplayListError::InvalidRect { index });
    }
    if !inflated.contains_rect(rect) {
        return Err(DisplayListError::OutOfBounds { index });
    }
    Ok(())
}

/// Validates one [`DisplayItem::Text`] run: glyph count, origin within bounds, then every
/// glyph's offset against that origin.
fn check_text_run(
    run: &GlyphRun,
    index: usize,
    inflated: &InflatedBounds,
) -> Result<(), DisplayListError> {
    if run.glyphs.len() > MAX_GLYPHS_PER_RUN {
        return Err(DisplayListError::TooManyGlyphs { index });
    }
    if !inflated.contains_point(run.origin) {
        return Err(DisplayListError::OutOfBounds { index });
    }
    for glyph in &run.glyphs {
        let x_ok = run.origin.x.0.checked_add(glyph.x.0).is_some();
        let y_ok = run.origin.y.0.checked_add(glyph.y.0).is_some();
        if !x_ok || !y_ok {
            return Err(DisplayListError::InvalidRect { index });
        }
    }
    Ok(())
}

/// Validates `dl` against `bounds` — see the module docs for the full rule set.
///
/// One linear pass over `dl.items`, no recursion, no allocation beyond a `usize` clip-depth
/// counter, no `unwrap`/`expect`/indexing: every arithmetic step that could overflow uses
/// `checked_add`, and `dl.items` is walked with `enumerate()`.
///
/// # Errors
/// See [`DisplayListError`] for every way `dl` (or `bounds` itself) can fail validation.
pub fn validate(dl: &DisplayList, bounds: Rect) -> Result<(), DisplayListError> {
    if !bounds_is_valid(bounds) {
        return Err(DisplayListError::InvalidBounds);
    }

    let count = dl.items.len();
    if count > MAX_ITEMS {
        return Err(DisplayListError::TooManyItems {
            count,
            max: MAX_ITEMS,
        });
    }

    let inflated = inflate(bounds);
    let mut depth: usize = 0;

    for (index, item) in dl.items.iter().enumerate() {
        match item {
            DisplayItem::Rect { rect, .. } => check_rect(*rect, index, &inflated)?,
            DisplayItem::Border { rect, widths, .. } => {
                check_rect(*rect, index, &inflated)?;
                if !border_widths_valid(*rect, *widths) {
                    return Err(DisplayListError::InvalidRect { index });
                }
            }
            DisplayItem::Text { run } => check_text_run(run, index, &inflated)?,
            DisplayItem::PushClip { rect } => {
                check_rect(*rect, index, &inflated)?;
                depth += 1;
                if depth > MAX_CLIP_DEPTH {
                    return Err(DisplayListError::TooDeepClip {
                        index,
                        max: MAX_CLIP_DEPTH,
                    });
                }
            }
            DisplayItem::PopClip => {
                if depth == 0 {
                    return Err(DisplayListError::PopWithoutPush { index });
                }
                depth -= 1;
            }
        }
    }

    if depth != 0 {
        let depth_at_end = i32::try_from(depth).unwrap_or(i32::MAX);
        return Err(DisplayListError::UnbalancedClip { depth_at_end });
    }

    Ok(())
}
