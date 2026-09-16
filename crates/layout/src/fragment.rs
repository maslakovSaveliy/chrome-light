//! The fragment tree: the immutable, positioned output of [`crate::block::layout`].
//!
//! Where [`crate::box_tree::BoxTree`] says *what* CSS boxes exist, [`FragmentTree`] says
//! *where* each one ended up: every [`Fragment`] carries three absolute (viewport-relative,
//! not parent-relative) rectangles — [`Fragment::border_box`], [`Fragment::padding_box`],
//! [`Fragment::content_box`] — plus whatever a paint pass (Task 19) needs to draw it. A
//! `FragmentTree` does not borrow the [`cl_style::StyledDocument`] or
//! [`crate::box_tree::BoxTree`] it was built from — like [`crate::box_tree::build`], it
//! outlives them, and [`Fragment::node`] is the only link back (an index into whatever
//! `cl_dom::Document` the caller still has around).
//!
//! [`Fragment::style`] indexes into `FragmentTree::styles` rather than embedding a
//! [`crate::geom::LayoutStyle`] directly: several fragments (a `Line`, its `Text` children)
//! commonly share one container's style, and a paint pass wants to look properties up by a
//! cheap index rather than cloning a `LayoutStyle` (font family list included) per fragment.
//! [`crate::block::layout`] does not deduplicate — one style entry per fragment is enough
//! for M1a — but funnelling every read through [`FragmentTree::style`] keeps that free to add
//! later without an API break.

use cl_dom::NodeId;

use crate::geom::{LayoutStyle, Rect, Size};
use crate::text::GlyphRun;

/// The id of one [`Fragment`] in a [`FragmentTree`].
///
/// Mirrors [`crate::box_tree::BoxId`]: an index into [`FragmentTree`]'s internal arena,
/// meaningful only relative to the tree that produced it, saturating rather than panicking
/// if somehow more than `u32::MAX` fragments were ever built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FragmentId(u32);

impl FragmentId {
    /// Builds a `FragmentId` from a raw arena index, saturating to `u32::MAX` (never
    /// assigned to a real fragment by [`crate::block::layout`]) rather than panicking if
    /// `index` does not fit in a `u32`.
    #[must_use]
    pub(crate) fn from_index(index: usize) -> Self {
        Self(u32::try_from(index).unwrap_or(u32::MAX))
    }

    /// Returns the raw arena index this id names.
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// The id of one [`LayoutStyle`] in a `FragmentTree::styles`.
///
/// See [`FragmentId`] for the id-type conventions this mirrors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StyleId(u32);

impl StyleId {
    /// Builds a `StyleId` from a raw arena index, saturating to `u32::MAX` rather than
    /// panicking if `index` does not fit in a `u32`.
    #[must_use]
    pub(crate) fn from_index(index: usize) -> Self {
        Self(u32::try_from(index).unwrap_or(u32::MAX))
    }

    /// Returns the raw arena index this id names.
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// What kind of fragment a [`Fragment`] is.
///
/// Mirrors [`crate::box_tree::BoxKind`] at the granularity M1a needs: every
/// [`crate::box_tree::BoxKind::Block`]/[`crate::box_tree::BoxKind::AnonymousBlock`] box
/// becomes exactly one `Block`/`AnonymousBlock` fragment; a [`crate::box_tree::BoxKind::Inline`]
/// box produces no fragment of its own in M1a (only its descendants' text, shaped with its
/// inherited style — so M1a paints no inline border, padding or background) and neither does
/// a [`crate::box_tree::BoxKind::LineBreak`] (it only forces a break); each line the shaper
/// produced becomes one `Line` fragment, whose children are one `Text` fragment per source
/// text node contributing to that line.
#[derive(Debug, Clone, PartialEq)]
pub enum FragmentKind {
    /// The fragment for an element's own `Block`-level box.
    Block,
    /// The fragment for a [`crate::box_tree::BoxKind::AnonymousBlock`] box.
    AnonymousBlock,
    /// One line box of an inline formatting context: its rects span from where `text-align`
    /// put the line to the end of its content (a hanging trailing space excluded), and are
    /// the container's `line-height` tall.
    Line,
    /// One source text node's contribution to one line, always a child of a `Line` fragment.
    ///
    /// [`Fragment::node`] is that text node and [`Fragment::style`] the style it inherited,
    /// so a `<b>` inside a `<p>` produces its own `Text` fragment with its own
    /// `font-weight`. Empty only for a line with no glyphs at all.
    Text {
        /// The runs of shaped glyphs this text fragment paints, in visual order. A run is
        /// uniform in font, size and colour; a fragment has more than one only where font
        /// fallback split its text.
        runs: Vec<GlyphRun>,
    },
}

/// One positioned, sized fragment of the render tree.
#[derive(Debug, Clone, PartialEq)]
pub struct Fragment {
    /// The DOM node this fragment's box was generated for. `None` for an `AnonymousBlock`
    /// or a `Line` (neither has an originating element at all). A `Text` fragment carries
    /// the [`crate::box_tree::BoxKind::InlineText`] box's own node — the text node itself,
    /// not an element — exactly as that box did (`None` only in the degenerate case of a run
    /// no text node claims, which `crate::block::layout` does not produce).
    pub node: Option<NodeId>,
    /// What kind of fragment this is.
    pub kind: FragmentKind,
    /// The fragment's border box: its outermost visible edge (margin is never part of any
    /// fragment rect — it is empty space, not a box). Absolute: relative to the viewport
    /// origin, not to this fragment's parent.
    pub border_box: Rect,
    /// The fragment's padding box: `border_box` inset by the border widths. Absolute, like
    /// [`Fragment::border_box`].
    pub padding_box: Rect,
    /// The fragment's content box: `padding_box` inset by the padding. Absolute, like
    /// [`Fragment::border_box`]. For a `Line`/`Text` fragment (no padding or border of their
    /// own) this equals `border_box`/`padding_box`.
    pub content_box: Rect,
    /// This fragment's style, as an index into `FragmentTree::styles` — see the module
    /// docs for why this is an index rather than an inline [`LayoutStyle`].
    pub style: StyleId,
    /// This fragment's children, in fragment (≈ box, ≈ document) order — mirrors
    /// [`crate::box_tree::LayoutBox::children`], including its visibility (`pub`): a reader
    /// that only has a `Fragment` (not the enclosing tree) still has its children in hand.
    pub children: Vec<FragmentId>,
}

/// The viewport a [`FragmentTree`] was laid out against: the initial containing block for
/// the document's root element (CSS 2.1 §10.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    /// The viewport's size, in app units.
    pub size: Size,
}

impl Viewport {
    /// Builds a viewport from a CSS pixel width and height (the shape every other part of
    /// `ChromeLight`'s pipeline configures a viewport in — see
    /// `crates/layout/tests/common/mod.rs`'s `VIEWPORT`, matching `cl_style::StyleEngine::new`).
    #[must_use]
    pub fn new(width_px: f32, height_px: f32) -> Viewport {
        Viewport {
            size: Size {
                w: crate::au::Au::from_px(width_px),
                h: crate::au::Au::from_px(height_px),
            },
        }
    }
}

/// The tree of positioned fragments produced by [`crate::block::layout`] for one document.
#[derive(Debug, Clone)]
pub struct FragmentTree {
    pub(crate) fragments: Vec<Fragment>,
    pub(crate) styles: Vec<LayoutStyle>,
    /// The id of the tree's root fragment (the fragment for the document's root element, or
    /// a synthetic empty fragment for a document with none — mirroring
    /// [`crate::box_tree::BoxTree::root`]).
    pub root: FragmentId,
    /// The viewport this tree was laid out against, in app units (the task brief's
    /// interface names this field's type as [`Size`] rather than [`Viewport`] — the newtype
    /// wraps the same [`Size`] only where [`crate::block::layout`] takes it as a parameter).
    pub viewport: Size,
}

impl FragmentTree {
    /// Looks up a fragment by id. `None` if `id` is out of range.
    #[must_use]
    pub fn get(&self, id: FragmentId) -> Option<&Fragment> {
        self.fragments.get(id.index())
    }

    /// Looks up a style by id. `None` if `id` is out of range.
    #[must_use]
    pub fn style(&self, id: StyleId) -> Option<&LayoutStyle> {
        self.styles.get(id.index())
    }

    /// The number of fragments in the tree.
    #[must_use]
    pub fn len(&self) -> usize {
        self.fragments.len()
    }

    /// Whether the tree has no fragments at all. [`crate::block::layout`] always produces at
    /// least the root fragment, so this is always `false` in practice; provided for API
    /// symmetry with [`FragmentTree::len`] (clippy's `len_without_is_empty`).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fragments.is_empty()
    }
}
