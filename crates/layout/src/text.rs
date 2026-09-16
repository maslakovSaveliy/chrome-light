//! Shaped-text data types: [`Glyph`] and [`GlyphRun`].
//!
//! These are plain data — nothing in this module shapes text. Task 17's inline placeholder
//! (see `crate::block`'s module docs) only ever produces `GlyphRun`s with an empty `glyphs`
//! vector; Task 18 introduces the `parley`/`swash` shaping pipeline that actually fills them
//! in and, with it, whatever `FontDb`-driven selection logic decides `GlyphRun::font`. The
//! shape of both types is fixed now (rather than in Task 18) so [`crate::fragment::FragmentKind::Text`]
//! has a stable field type across the boundary between the two tasks.

use cl_fonts::FontKey;

use crate::au::Au;
use crate::geom::{Point, Rgba8};

/// One positioned glyph within a [`GlyphRun`].
///
/// Coordinates are relative to [`GlyphRun::origin`], in app units, matching every other
/// geometry type in this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Glyph {
    /// The glyph id within [`GlyphRun::font`] (a `skrifa`/`swash` glyph index, not a
    /// Unicode code point).
    pub id: u16,
    /// Horizontal offset from [`GlyphRun::origin`] to this glyph's origin.
    pub x: Au,
    /// Vertical offset from [`GlyphRun::origin`] to this glyph's origin.
    pub y: Au,
    /// How far this glyph advances the pen, in app units.
    pub advance: Au,
}

/// One run of shaped glyphs: a contiguous stretch of text rendered in a single font, size
/// and color (a shaper never mixes fonts or sizes within a run — a style or font-fallback
/// change starts a new one).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GlyphRun {
    /// The font face every glyph in this run is drawn from.
    pub font: FontKey,
    /// The font size this run was shaped at, in app units.
    pub size: Au,
    /// The run's origin (its text baseline's left end), in the same absolute coordinate
    /// space as [`crate::fragment::Fragment`]'s rects.
    pub origin: Point,
    /// The run's glyphs, in visual (left-to-right) order.
    pub glyphs: Vec<Glyph>,
    /// The color the glyphs paint with (the CSS `color` in effect where the text was
    /// generated).
    pub color: Rgba8,
}
