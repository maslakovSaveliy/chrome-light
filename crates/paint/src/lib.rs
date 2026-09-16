//! `ChromeLight` paint (M1a Task 19): turns a [`cl_layout::FragmentTree`] — the positioned
//! output of the layout pipeline (`cl-html` → `cl-dom` → `cl-style` → `cl-layout`) — into a
//! [`DisplayList`]: a flat, paint-order sequence of draw commands a rasteriser (`cl-gfx`,
//! Task 21) can execute without knowing anything about CSS, the box tree, or fragments.
//!
//! Two modules:
//!
//! * [`list`] — [`list::DisplayItem`]/[`list::DisplayList`], the plain data types a display
//!   list is made of. No behaviour lives here, only vocabulary, mirroring how
//!   `cl_layout::geom` relates to `cl_layout::block`.
//! * [`mod@build`] — [`build::build`], which walks a [`cl_layout::FragmentTree`] into a
//!   [`DisplayList`] in CSS 2.1 Appendix E paint order (the M1a subset: background, then
//!   solid borders, then an `overflow: hidden` clip, then children, then text — see that
//!   module's docs for the exact order and for §14.2's canvas-background propagation).
//!   `build` also culls any item whose geometry does not intersect the viewport widened by
//!   [`validate::OFFSCREEN_MARGIN_PX`] — see [`mod@build`]'s "Culling offscreen items" — so a
//!   page of any height still produces a `DisplayList` [`validate::validate`] accepts at the
//!   real viewport size, not just one small enough to contain every item outright.
//!
//! [`dump::display_list_dump`] renders a [`DisplayList`] for snapshot tests and for
//! eyeballing what [`build::build`] actually produced, the same way `cl_layout::dump`
//! renders a `FragmentTree`.
//!
//! [`validate::validate`] (Task 20) is the schema check the M1b gpu process runs on a
//! [`DisplayList`] before rasterising it — see that module's docs for the trust boundary and
//! the exact rules.
//!
//! # Scope (M1a)
//!
//! Painted: element backgrounds and solid borders on `Block`/element fragments, shaped text,
//! and `overflow: hidden` clipping (CSS 2.1 §11.1.1's clipping to the padding box). Not
//! painted, all deferred to a later milestone: inline-level element backgrounds/borders (no
//! fragment exists for them in M1a — `cl_layout::fragment`'s module docs, Task 18's
//! decision), `border-radius`, box shadows, background images/gradients, `dashed`/`dotted`/
//! `double` border styles (M1a's [`cl_layout::geom::LayoutStyle::border_solid`] only
//! distinguishes solid from "not solid"), stacking contexts and `z-index` (paint order here
//! is purely document order, CSS 2.1 Appendix E's "normal flow" case — no positioned or
//! floated layer reordering, since M1a's layout does not produce any), and transforms/
//! filters/opacity compositing.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod build;
pub mod dump;
pub mod error;
pub mod list;
pub mod validate;

pub use build::build;
pub use error::PaintError;
pub use list::{DisplayItem, DisplayList};
pub use validate::{
    DisplayListError, MAX_CLIP_DEPTH, MAX_GLYPHS_PER_RUN, MAX_ITEMS, OFFSCREEN_MARGIN_PX, validate,
};
