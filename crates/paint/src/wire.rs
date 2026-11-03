//! [`encode_display_list`] / [`decode_display_list`]: the encoding a [`DisplayList`] travels in
//! from the renderer to the gpu process (M1b Task 6; `docs/adr/0016-…` §1, §4).
//!
//! The bytes are the plain `postcard` encoding of the derived `Serialize` — nothing is added
//! on top. What this module adds is a decoder that is safe against a hostile renderer. The
//! derived `Deserialize` would be correct but not bounded: a [`DisplayItem::PopClip`] is one
//! byte on the wire and ≈ 56 bytes in memory, a [`cl_layout::Glyph`] ≈ 4 bytes and 16, so
//! the 16 MiB input cap alone would let one message grow into ≈ 1 GiB before
//! [`crate::validate()`] ever ran. [`decode_display_list`] instead counts as it goes and
//! fails with [`WireError::LimitsExceeded`] the moment the list would hold more than
//! [`MAX_ITEMS`] items, a run more than [`MAX_GLYPHS_PER_RUN`] glyphs, or the whole list
//! more than [`MAX_TOTAL_GLYPHS`] glyphs — before the memory for the excess is reserved.
//!
//! # Memory bound
//!
//! A declared sequence length is only trusted up to the limit it is counted against: a
//! length over the limit is refused before anything is allocated, a length within it is
//! preallocated exactly (postcard only reports a length it has at least that many bytes
//! left for, so this is never more than the input could fill), and a sequence whose length
//! postcard did not report grows in steps that never request capacity past the limit. So
//! one decode holds at most the input (≤ [`MAX_DISPLAY_LIST_BYTES`], copied out of shm by
//! the caller), ≈ 14 MiB of items and 16 MiB of glyphs.
//!
//! The decoder still accepts any geometry: [`crate::validate()`] runs on the result, as for
//! a list from any other source.

use std::cell::Cell;
use std::fmt;

use cl_fonts::FontKey;
use cl_layout::{Au, Glyph, GlyphRun, Point, Rect, Rgba8};
use serde::Deserialize;
use serde::de::{self, DeserializeSeed, EnumAccess, SeqAccess, Unexpected, VariantAccess, Visitor};

use crate::list::{DisplayItem, DisplayList};
use crate::validate::{MAX_GLYPHS_PER_RUN, MAX_ITEMS, MAX_TOTAL_GLYPHS};

/// The largest encoded display list the gpu accepts: 16 MiB.
///
/// Set in M1b Task 6 from a measurement (owner decision 2026-11-04): the 1 MB smoke page's
/// list encodes to 49 749 bytes (133 items, 7 182 glyphs) and the largest reftest page's to
/// 132, so this is ≈ 337× the smoke page. A list at every other limit — [`MAX_ITEMS`] items
/// at ≈ 30 bytes plus [`MAX_TOTAL_GLYPHS`] glyphs at ≈ 6 — encodes to ≈ 14 MiB, so the cap
/// does not cut off a list the item and glyph limits allow.
pub const MAX_DISPLAY_LIST_BYTES: usize = 16 * 1024 * 1024;

/// Why a display list could not be encoded or decoded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WireError {
    /// The encoded list is longer than the cap; for a decode, nothing was read.
    #[error("encoded display list of {size} bytes exceeds the limit of {max}")]
    TooLarge {
        /// The encoded length.
        size: usize,
        /// The cap it was checked against.
        max: usize,
    },
    /// The bytes are not a postcard-encoded [`DisplayList`] (truncated, an unknown item
    /// kind, a malformed varint, …). Encoding never produces this for a list in memory.
    #[error("display list is undecodable: {0}")]
    Undecodable(String),
    /// The list holds more than [`MAX_ITEMS`] items, a run more than [`MAX_GLYPHS_PER_RUN`]
    /// glyphs, or all runs together more than [`MAX_TOTAL_GLYPHS`].
    #[error("display list exceeds an item or glyph limit")]
    LimitsExceeded,
    /// A whole list decoded, followed by this many unread bytes.
    #[error("{0} bytes after the end of the display list")]
    Trailing(usize),
}

/// Encodes `dl` for the renderer → gpu channel.
///
/// Refuses a list [`decode_display_list`] would refuse, so a renderer finds out before it
/// sends — a list the gpu rejects gets the renderer killed for a bad message.
///
/// # Errors
/// [`WireError::LimitsExceeded`] if `dl` is over [`MAX_ITEMS`], [`MAX_GLYPHS_PER_RUN`] or
/// [`MAX_TOTAL_GLYPHS`]; [`WireError::TooLarge`] if the encoding is longer than
/// [`MAX_DISPLAY_LIST_BYTES`].
pub fn encode_display_list(dl: &DisplayList) -> Result<Vec<u8>, WireError> {
    check_limits(dl)?;
    let bytes = postcard::to_allocvec(dl).map_err(|e| WireError::Undecodable(e.to_string()))?;
    if bytes.len() > MAX_DISPLAY_LIST_BYTES {
        return Err(WireError::TooLarge {
            size: bytes.len(),
            max: MAX_DISPLAY_LIST_BYTES,
        });
    }
    Ok(bytes)
}

/// Decodes a display list from `bytes`, which must hold exactly one list and be at most
/// `max` bytes long (normally [`MAX_DISPLAY_LIST_BYTES`]). The item and glyph limits are
/// enforced while decoding — see the module docs.
///
/// # Errors
/// [`WireError::TooLarge`] before reading anything if `bytes` is longer than `max`;
/// [`WireError::LimitsExceeded`], [`WireError::Undecodable`], [`WireError::Trailing`].
pub fn decode_display_list(bytes: &[u8], max: usize) -> Result<DisplayList, WireError> {
    if bytes.len() > max {
        return Err(WireError::TooLarge {
            size: bytes.len(),
            max,
        });
    }
    let budget = Budget::default();
    let mut deserializer = postcard::Deserializer::from_bytes(bytes);
    let dl = match (DisplayListSeed { budget: &budget }).deserialize(&mut deserializer) {
        Ok(dl) => dl,
        // postcard turns a custom error into a message-less `SerdeDeCustom`, so the budget
        // remembers that the failure was a limit, not malformed input.
        Err(_) if budget.exceeded.get() => return Err(WireError::LimitsExceeded),
        Err(e) => return Err(WireError::Undecodable(e.to_string())),
    };
    let rest = deserializer
        .finalize()
        .map_err(|e| WireError::Undecodable(e.to_string()))?;
    if !rest.is_empty() {
        return Err(WireError::Trailing(rest.len()));
    }
    Ok(dl)
}

/// The encoder's copy of the decoder's limits, on a list already in memory.
fn check_limits(dl: &DisplayList) -> Result<(), WireError> {
    if dl.items.len() > MAX_ITEMS {
        return Err(WireError::LimitsExceeded);
    }
    let mut total: usize = 0;
    for item in &dl.items {
        if let DisplayItem::Text { run } = item {
            total = total.saturating_add(run.glyphs.len());
            if run.glyphs.len() > MAX_GLYPHS_PER_RUN || total > MAX_TOTAL_GLYPHS {
                return Err(WireError::LimitsExceeded);
            }
        }
    }
    Ok(())
}

/// State shared by every seed of one decode.
#[derive(Default)]
struct Budget {
    /// Glyphs decoded so far, across all runs; never above [`MAX_TOTAL_GLYPHS`].
    glyphs: Cell<usize>,
    /// Set when a limit stopped the decode (see [`decode_display_list`]).
    exceeded: Cell<bool>,
}

impl Budget {
    /// Records that a limit was hit and returns the error that aborts the decode.
    fn exceeded<E: de::Error>(&self) -> E {
        self.exceeded.set(true);
        E::custom("display list limits exceeded")
    }
}

/// A sequence length over the limit it is counted against.
#[derive(Debug, PartialEq, Eq)]
struct OverLimit;

/// How much to preallocate for a sequence that may hold at most `limit` elements, given the
/// length postcard reported (`None` when the declared length is larger than the bytes left,
/// i.e. the sequence cannot be complete — nothing is preallocated for it).
fn initial_capacity(size_hint: Option<usize>, limit: usize) -> Result<usize, OverLimit> {
    match size_hint {
        Some(len) if len > limit => Err(OverLimit),
        Some(len) => Ok(len),
        None => Ok(0),
    }
}

/// The smallest step [`push_bounded`] grows by.
const MIN_GROWTH: usize = 64;

/// Pushes `value`, growing `vec` like `Vec::push` (doubling) but never requesting capacity
/// past `limit`. The caller has checked `vec.len() < limit`.
fn push_bounded<T>(vec: &mut Vec<T>, value: T, limit: usize) {
    if vec.len() == vec.capacity() {
        let growth = vec
            .len()
            .max(MIN_GROWTH)
            .min(limit.saturating_sub(vec.len()))
            .max(1);
        vec.reserve_exact(growth);
    }
    vec.push(value);
}

/// Reads the next field of a struct-shaped value.
fn field<'de, A, T>(seq: &mut A, index: usize, expected: &'static str) -> Result<T, A::Error>
where
    A: SeqAccess<'de>,
    T: Deserialize<'de>,
{
    seq.next_element()?
        .ok_or_else(|| de::Error::invalid_length(index, &expected))
}

/// [`DisplayList`]: `items` (counted), then `bounds` — the derived field order.
#[derive(Clone, Copy)]
struct DisplayListSeed<'b> {
    budget: &'b Budget,
}

impl<'de> DeserializeSeed<'de> for DisplayListSeed<'_> {
    type Value = DisplayList;

    fn deserialize<D: de::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<DisplayList, D::Error> {
        deserializer.deserialize_struct("DisplayList", &["items", "bounds"], self)
    }
}

impl<'de> Visitor<'de> for DisplayListSeed<'_> {
    type Value = DisplayList;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a display list")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<DisplayList, A::Error> {
        const EXPECTED: &str = "a display list of 2 fields";
        let items = seq
            .next_element_seed(ItemsSeed {
                budget: self.budget,
            })?
            .ok_or_else(|| de::Error::invalid_length(0, &EXPECTED))?;
        let bounds: Rect = field(&mut seq, 1, EXPECTED)?;
        Ok(DisplayList { items, bounds })
    }
}

/// `DisplayList::items`, at most [`MAX_ITEMS`].
#[derive(Clone, Copy)]
struct ItemsSeed<'b> {
    budget: &'b Budget,
}

impl<'de> DeserializeSeed<'de> for ItemsSeed<'_> {
    type Value = Vec<DisplayItem>;

    fn deserialize<D: de::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Vec<DisplayItem>, D::Error> {
        deserializer.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for ItemsSeed<'_> {
    type Value = Vec<DisplayItem>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a sequence of display items")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<DisplayItem>, A::Error> {
        let capacity = initial_capacity(seq.size_hint(), MAX_ITEMS)
            .map_err(|OverLimit| self.budget.exceeded())?;
        let mut items = Vec::with_capacity(capacity);
        while let Some(item) = seq.next_element_seed(ItemSeed {
            budget: self.budget,
        })? {
            if items.len() >= MAX_ITEMS {
                return Err(self.budget.exceeded());
            }
            push_bounded(&mut items, item, MAX_ITEMS);
        }
        Ok(items)
    }
}

/// One [`DisplayItem`]. Variant indices and field orders are the derived ones (declaration
/// order); `decoder_should_read_the_derived_postcard_encoding` in `tests/wire.rs` holds the
/// two together.
#[derive(Clone, Copy)]
struct ItemSeed<'b> {
    budget: &'b Budget,
}

impl<'de> DeserializeSeed<'de> for ItemSeed<'_> {
    type Value = DisplayItem;

    fn deserialize<D: de::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<DisplayItem, D::Error> {
        deserializer.deserialize_enum(
            "DisplayItem",
            &["Rect", "Border", "Text", "PushClip", "PopClip"],
            self,
        )
    }
}

impl<'de> Visitor<'de> for ItemSeed<'_> {
    type Value = DisplayItem;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a display item")
    }

    fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<DisplayItem, A::Error> {
        let (variant, access): (u32, A::Variant) = data.variant()?;
        let fields: &'static [&'static str] = match variant {
            0 => &["rect", "color"],
            1 => &["rect", "widths", "colors"],
            2 => &["run"],
            3 => &["rect"],
            4 => {
                access.unit_variant()?;
                return Ok(DisplayItem::PopClip);
            }
            other => {
                return Err(de::Error::invalid_value(
                    Unexpected::Unsigned(other.into()),
                    &"a display item variant index in 0..=4",
                ));
            }
        };
        access.struct_variant(
            fields,
            ItemFields {
                variant,
                budget: self.budget,
            },
        )
    }
}

/// The fields of one struct-shaped [`DisplayItem`] variant (`variant` is 0..=3).
struct ItemFields<'b> {
    variant: u32,
    budget: &'b Budget,
}

impl<'de> Visitor<'de> for ItemFields<'_> {
    type Value = DisplayItem;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the fields of a display item")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<DisplayItem, A::Error> {
        const EXPECTED: &str = "the fields of a display item";
        Ok(match self.variant {
            0 => DisplayItem::Rect {
                rect: field(&mut seq, 0, EXPECTED)?,
                color: field(&mut seq, 1, EXPECTED)?,
            },
            1 => DisplayItem::Border {
                rect: field(&mut seq, 0, EXPECTED)?,
                widths: field(&mut seq, 1, EXPECTED)?,
                colors: field(&mut seq, 2, EXPECTED)?,
            },
            2 => DisplayItem::Text {
                run: seq
                    .next_element_seed(GlyphRunSeed {
                        budget: self.budget,
                    })?
                    .ok_or_else(|| de::Error::invalid_length(0, &EXPECTED))?,
            },
            _ => DisplayItem::PushClip {
                rect: field(&mut seq, 0, EXPECTED)?,
            },
        })
    }
}

/// [`GlyphRun`]: `font`, `size`, `origin`, `glyphs` (counted), `color` — the derived order.
#[derive(Clone, Copy)]
struct GlyphRunSeed<'b> {
    budget: &'b Budget,
}

impl<'de> DeserializeSeed<'de> for GlyphRunSeed<'_> {
    type Value = GlyphRun;

    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<GlyphRun, D::Error> {
        deserializer.deserialize_struct(
            "GlyphRun",
            &["font", "size", "origin", "glyphs", "color"],
            self,
        )
    }
}

impl<'de> Visitor<'de> for GlyphRunSeed<'_> {
    type Value = GlyphRun;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a glyph run")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<GlyphRun, A::Error> {
        const EXPECTED: &str = "a glyph run of 5 fields";
        let font: FontKey = field(&mut seq, 0, EXPECTED)?;
        let size: Au = field(&mut seq, 1, EXPECTED)?;
        let origin: Point = field(&mut seq, 2, EXPECTED)?;
        let glyphs = seq
            .next_element_seed(GlyphsSeed {
                budget: self.budget,
            })?
            .ok_or_else(|| de::Error::invalid_length(3, &EXPECTED))?;
        let color: Rgba8 = field(&mut seq, 4, EXPECTED)?;
        Ok(GlyphRun {
            font,
            size,
            origin,
            glyphs,
            color,
        })
    }
}

/// `GlyphRun::glyphs`, at most [`MAX_GLYPHS_PER_RUN`] and at most what is left of
/// [`MAX_TOTAL_GLYPHS`].
#[derive(Clone, Copy)]
struct GlyphsSeed<'b> {
    budget: &'b Budget,
}

impl<'de> DeserializeSeed<'de> for GlyphsSeed<'_> {
    type Value = Vec<Glyph>;

    fn deserialize<D: de::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Vec<Glyph>, D::Error> {
        deserializer.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for GlyphsSeed<'_> {
    type Value = Vec<Glyph>;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a sequence of glyphs")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<Glyph>, A::Error> {
        let decoded = self.budget.glyphs.get();
        let limit = MAX_GLYPHS_PER_RUN.min(MAX_TOTAL_GLYPHS.saturating_sub(decoded));
        let capacity =
            initial_capacity(seq.size_hint(), limit).map_err(|OverLimit| self.budget.exceeded())?;
        let mut glyphs = Vec::with_capacity(capacity);
        while let Some(glyph) = seq.next_element::<Glyph>()? {
            if glyphs.len() >= limit {
                return Err(self.budget.exceeded());
            }
            push_bounded(&mut glyphs, glyph, limit);
        }
        self.budget.glyphs.set(decoded.saturating_add(glyphs.len()));
        Ok(glyphs)
    }
}

#[cfg(test)]
mod tests {
    use super::{MIN_GROWTH, OverLimit, initial_capacity, push_bounded};

    #[test]
    fn a_declared_length_over_the_limit_should_be_refused_before_allocating() {
        assert_eq!(initial_capacity(Some(11), 10), Err(OverLimit));
        assert_eq!(initial_capacity(Some(10), 10), Ok(10));
    }

    #[test]
    fn an_unreported_length_should_preallocate_nothing() {
        // postcard reports no length when the declared one exceeds the bytes left — the
        // 10^9-item PopClip flood lands here.
        assert_eq!(initial_capacity(None, 10), Ok(0));
    }

    #[test]
    fn bounded_growth_should_never_request_capacity_past_the_limit() {
        let limit = 1000;
        let mut vec: Vec<u8> = Vec::new();
        for i in 0..limit {
            push_bounded(&mut vec, 0, limit);
            assert!(
                vec.capacity() <= limit,
                "capacity {} after {i}",
                vec.capacity()
            );
        }
        assert_eq!(vec.len(), limit);
        assert_eq!(vec.capacity(), limit);
    }

    #[test]
    fn bounded_growth_should_double_below_the_limit() {
        let mut vec = vec![0_u8; MIN_GROWTH * 4];
        vec.shrink_to_fit();
        push_bounded(&mut vec, 0, usize::MAX);
        assert!(vec.capacity() >= MIN_GROWTH * 8);
    }
}
