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
//! Everything downstream of this crate (Task 17 onward: the block formatting context,
//! fragment tree, painting) reads only [`geom::LayoutStyle`] and [`box_tree::BoxTree`] —
//! never a stylo type directly.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod au;
pub mod box_tree;
pub mod dump;
pub mod error;
pub mod geom;
pub mod style_adapt;

pub use au::Au;
pub use box_tree::{BoxId, BoxKind, BoxTree, LayoutBox, build};
pub use error::LayoutError;
pub use geom::{
    BoxSizing, Display, Length, Overflow, Point, Position, Rect, Rgba8, Sides, Size, TextAlign,
    WhiteSpace, LayoutStyle,
};
