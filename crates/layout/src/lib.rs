//! `ChromeLight` layout foundations (M1a Task 16): app units, geometry, the adapter from
//! stylo's `ComputedValues` to this crate's own [`geom::LayoutStyle`], and the box tree.
//!
//! This is where stylo's computed values stop being stylo's problem and become geometry.
//! Four modules, read in dependency order:
//!
//! * [`au`] — [`au::Au`], the `1/60`-CSS-pixel integer app unit every layout computation
//!   downstream of this crate works in;
//! * [`geom`] — [`geom::Point`]/[`geom::Size`]/[`geom::Rect`]/[`geom::Sides`] built on `Au`,
//!   plus the CSS value shapes (`Length`, `Display`, `Position`, …) and
//!   [`geom::LayoutStyle`] itself;
//! * [`style_adapt`] — [`style_adapt::adapt`], the *only* function in this crate (indeed,
//!   the only one anywhere outside `crates/style/**`) allowed to name a stylo type
//!   (ADR-0015 §2, `tools/check-stylo-scope.sh`). Converts one element's
//!   `style::properties::ComputedValues` into a [`geom::LayoutStyle`];
//! * [`box_tree`] — [`box_tree::BoxTree`]/[`box_tree::LayoutBox`]/[`box_tree::BoxKind`], and
//!   [`box_tree::build`], which walks a `cl_style::StyledDocument` into a box tree: one box
//!   per generated CSS box, `display: none` subtrees skipped, mixed block/inline runs
//!   wrapped in anonymous blocks per CSS 2.1 §9.2.1.1.
//!
//! [`dump::box_tree_dump`] renders a [`box_tree::BoxTree`] for snapshot tests, the same way
//! `cl_style::dump::computed_style_dump` renders a `StyledDocument`.
//!
//! Task 17 adds the block formatting context and the fragment tree it produces, and Task 18
//! the inline formatting context inside it:
//!
//! * [`block`] — [`block::layout`], which walks a [`box_tree::BoxTree`] into a positioned,
//!   sized [`fragment::FragmentTree`] (CSS 2.1 §10 box dimensions, §8.3.1 margin collapsing);
//! * [`fragment`] — [`fragment::Fragment`]/[`fragment::FragmentTree`]/[`fragment::Viewport`],
//!   the immutable output of [`block::layout`];
//! * [`text`] — [`text::Glyph`]/[`text::GlyphRun`], the plain shaped-text data types
//!   [`fragment::FragmentKind::Text`] carries, plus the private `parley` shaper that fills
//!   them in.
//!
//! Three private modules implement the inline formatting context, between them owning every
//! `parley`/`fontique` type this crate touches (they appear nowhere in its public API):
//! `whitespace` (CSS Text 3 §4.1.1 whitespace processing, applied before shaping), `text`'s
//! own `TextShaper` (shaping, line breaking, `text-align`), and `inline` (flattening a
//! block's inline-level boxes, and turning the shaper's lines into `Line`/`Text` fragments).
//!
//! [`dump::fragment_tree_dump`] renders a [`fragment::FragmentTree`] the same way
//! [`dump::box_tree_dump`] renders a [`box_tree::BoxTree`].
//!
//! Everything downstream of this crate (Task 19 onward: painting) reads only
//! [`fragment::FragmentTree`] — never a stylo type directly, and never [`box_tree::BoxTree`]
//! either (that is an implementation detail of [`block::layout`]).
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod au;
pub mod block;
pub mod box_tree;
pub mod dump;
pub mod error;
pub mod fragment;
pub mod geom;
mod inline;
pub mod style_adapt;
pub mod text;
mod whitespace;

pub use au::Au;
pub use block::layout;
pub use box_tree::{BoxId, BoxKind, BoxTree, LayoutBox, build};
pub use error::LayoutError;
pub use fragment::{Fragment, FragmentId, FragmentKind, FragmentTree, StyleId, Viewport};
pub use geom::{
    BoxSizing, Display, LayoutStyle, Length, Overflow, Point, Position, Rect, Rgba8, Sides, Size,
    TextAlign, WhiteSpace,
};
pub use text::{Glyph, GlyphRun};
