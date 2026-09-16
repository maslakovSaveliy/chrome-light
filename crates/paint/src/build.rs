//! [`build`]: walks a [`FragmentTree`] into a [`DisplayList`] in CSS 2.1 Appendix E paint
//! order (the M1a subset — see `crate` module docs for exactly what is and is not painted).
//!
//! # Paint order, per fragment
//!
//! In document order, for every fragment the walk visits:
//!
//! 1. **Background** — a [`DisplayItem::Rect`] over the fragment's border box, only for a
//!    [`FragmentKind::Block`] fragment (an [`FragmentKind::AnonymousBlock`]/
//!    [`FragmentKind::Line`] has no background of its own — CSS 2.1 §9.2.2.1 gives an
//!    anonymous box only the *initial* value for non-inherited properties, and
//!    `background-color`'s initial value is transparent, which [`LayoutStyle::initial`] and
//!    [`LayoutStyle::anonymous_block`] already encode — see those functions' docs), skipped
//!    when the color is fully transparent (`color.a == 0`) or the border box is empty
//!    ([`Rect::is_empty`]).
//! 2. **Border** — a [`DisplayItem::Border`], only for a `Block` fragment, only if at least
//!    one side has [`LayoutStyle::border_solid`] `true` and a nonzero
//!    [`LayoutStyle::border_width`]; the emitted `widths` zero out every side that does not
//!    qualify (a `dashed`/`none`/zero-width side), so a rasteriser paints exactly the solid
//!    sides without re-deriving which sides those are.
//! 3. **Clip** — a [`DisplayItem::PushClip`] over the fragment's *padding* box (CSS 2.1
//!    §11.1.1), only when [`LayoutStyle::overflow`] is [`Overflow::Hidden`].
//! 4. **Children** — each child fragment, recursively, in the same order.
//! 5. **Clip close** — the matching [`DisplayItem::PopClip`], only if step 3 pushed one.
//!
//! A [`FragmentKind::Text`] fragment paints one [`DisplayItem::Text`] per
//! [`cl_layout::GlyphRun`] it holds (cloned), skipping any run with no glyphs; it has no
//! background/border of its own (`Text`'s style is [`LayoutStyle::inherited_text_style`],
//! which forces `background`/`border_solid` to their transparent/false initial values, but
//! the `Block`-only check above makes that doubly certain).
//!
//! # Culling offscreen items
//!
//! A document can be far taller (or wider) than the viewport — an ordinary "long page", not
//! a hostile one — and [`crate::validate::validate`] only accepts an item whose geometry
//! *intersects* the viewport widened by [`crate::validate::OFFSCREEN_MARGIN_PX`] on every
//! side (4096px), or (for a [`DisplayItem::Text`] run specifically) whose
//! [`cl_layout::GlyphRun::origin`] lies within that same region — see `validate`'s own
//! `check_rect`/`check_text_run`. Without culling, `build` would still emit an item for every
//! fragment regardless of how far past that margin it falls, and a hostile document could make
//! the gpu process reason about arbitrarily many items it can never show. So:
//!
//! * A [`FragmentKind::Block`] fragment's background/border ([`DisplayItem::Rect`]/
//!   [`DisplayItem::Border`]) is skipped when its border box does not **intersect** that
//!   widened region (`InflatedViewport::intersects_rect`) — the *exact* condition `validate`
//!   itself checks for that item, so an item surviving culling is *guaranteed*, not merely
//!   likely, to pass validation.
//!
//!   Both tests were originally full *containment*, which is the one direction that loses
//!   content rather than merely allowing extra: a 5000px-tall `<div>` with a background starts
//!   at the top of the viewport and is plainly visible, but its border box is not contained in
//!   `600 + 4096px`, so its background was culled and the page rendered blank
//!   (`crates/paint/tests/build.rs`'s `tall_container_background_should_survive_culling`).
//!   Culling may only ever drop what nothing can see.
//! * A [`FragmentKind::Text`] fragment culls each [`cl_layout::GlyphRun`] independently
//!   against `InflatedViewport::contains_point` — again `validate`'s own exact
//!   `check_text_run` condition on the run's `origin`, which is *all* `validate` ever checks
//!   a text run's position against (it bounds individual glyph offsets only against
//!   arithmetic overflow, never against the viewport). One run of a multi-run text fragment
//!   can be onscreen while another, on a much later line, is not, so each is decided on its
//!   own — culling is always per item, never propagated from a fragment to its descendants
//!   or siblings (CSS 2.1 §11.1.1's `overflow: visible` lets a child extend past a parent
//!   whose own box happens to fall outside the region, so a parent's culling decision must
//!   never suppress its children's).
//! * A [`DisplayItem::PushClip`]/[`DisplayItem::PopClip`] pair is **never** dropped by
//!   culling — dropping one half would unbalance the clip stack
//!   [`crate::validate::validate`] checks for. Instead, an `overflow: hidden` fragment's
//!   clip rect (the fragment's *padding* box) is clamped to lie within the widened region
//!   before it is emitted (`InflatedViewport::clamp`): a clip rect that starts inside the
//!   region and extends past it is trimmed to the region's edge, and a clip rect that falls
//!   entirely outside collapses to a zero-size rect pinned at the nearest corner — either
//!   way, always a valid, in-bounds rect, never an omitted one.
//! * The canvas-background [`DisplayItem::Rect`] (below) is never culled: it is always
//!   exactly the viewport's own bounds, trivially within any margin around them.
//!
//! The margin itself ([`crate::validate::OFFSCREEN_MARGIN_PX`]) is shared through one public
//! constant rather than two independently-chosen numbers, and the per-axis overlap rule through
//! one `pub(crate)` function (`crate::validate::axis_overlaps`), specifically so this module's
//! culling and `validate`'s own check can never silently drift apart — neither in *how far*
//! they each allow nor in *what* they decide about a rect at that distance. Dropping an item is
//! a real content change, so culling is never allowed to be the stricter of the two.
//!
//! # Canvas background propagation (CSS 2.1 §14.2, css-backgrounds-3 §2.11.2)
//!
//! Before any fragment is visited, the walk decides the *canvas* background: if the
//! `<html>` element's fragment has a non-transparent `background-color`, that color is
//! painted as a [`DisplayItem::Rect`] over the whole viewport (`tree.viewport`), as the very
//! first item — and `<html>`'s own border box does **not** paint that background again (CSS
//! 2.1 §14.2: propagating it to the canvas resets the element's own used background to
//! transparent, it does not duplicate it). Otherwise, if `<body>`'s background is
//! non-transparent, *that* color is painted over the viewport instead, and it is `<body>`'s
//! own border box that skips repainting it (`<html>`'s background, having been transparent,
//! was never painted anywhere to begin with). If neither has a usable background (including:
//! this is a hand-built tree with no `<html>`/`<body>` at all), no canvas item is emitted.
//!
//! `<html>`/`<body>` are identified by walking the tree for the first fragment whose
//! [`Fragment::node`] is that element, per the controller ruling — not by tree position
//! (root/first child), so a `<body>` reached any other way in a hand-built tree is still
//! found correctly.
//!
//! # Never panics, no recursion
//!
//! The walk uses one explicit stack (a fragment id, or a "this clip needs closing" marker),
//! bounded by [`FragmentTree::len`] the same way `cl_layout::dump`'s walks are: a
//! [`FragmentId`] that does not resolve via [`FragmentTree::get`] (a dangling reference — see
//! `tests/build.rs`'s malformed-tree test, which corrupts a real tree's own
//! [`FragmentTree::root`]) is skipped, never a panic, and the bound stops an adversarial
//! cycle (a fragment listing itself as its own descendant) from looping forever. When that
//! bound fires inside a clipped subtree the walk drains every still-open clip on its way out
//! (`drain_pending_pop_clips`), so even a malformed tree yields a list whose clip stack
//! balances — a truncated list is still a *valid* list. A
//! [`StyleId`] that does not resolve via [`FragmentTree::style`] falls back to
//! [`LayoutStyle::initial`] rather than panicking.
//!
//! # Deviation from the task brief's signature
//!
//! The brief names this function `build(tree: &FragmentTree) -> DisplayList`. Canvas
//! background propagation needs to identify the `<html>`/`<body>` elements, and a
//! [`FragmentTree`] does not borrow the [`cl_dom::Document`] it was built from (see
//! `cl_layout::fragment`'s module docs) — [`Fragment::node`] is only an index into one. This
//! function therefore also takes `doc: &cl_dom::Document`, exactly the deviation
//! `cl_layout::dump::fragment_tree_dump(tree, doc, fonts)` already made for the same reason
//! (see that function's docs) and that the controller ruling for this task calls out
//! explicitly.

use cl_dom::{Document, LocalName, local_name};
use cl_layout::{
    Au, Fragment, FragmentId, FragmentKind, FragmentTree, LayoutStyle, Overflow, Point, Rect,
    Rgba8, Sides, Size, StyleId,
};

use crate::list::{DisplayItem, DisplayList};
use crate::validate::{OFFSCREEN_MARGIN_PX, axis_overlaps};

/// Builds the paint-order display list for `tree`, resolving element identity (for canvas
/// background propagation) against `doc`. See the module docs for the full paint order, the
/// offscreen-item culling that keeps a tall/wide page's list within
/// [`crate::validate::validate`]'s bounds, and the totality guarantees.
#[must_use]
pub fn build(tree: &FragmentTree, doc: &Document) -> DisplayList {
    let bounds = Rect::new(Point::default(), tree.viewport);
    let mut items = Vec::new();
    let viewport = InflatedViewport::new(bounds);

    let canvas = canvas_background(tree, doc);
    if let Some(color) = canvas.color
        && !bounds.is_empty()
    {
        items.push(DisplayItem::Rect {
            rect: bounds,
            color,
        });
    }

    walk(tree, canvas.suppressed_fragment, &viewport, &mut items);

    DisplayList { items, bounds }
}

/// The viewport rect widened by [`OFFSCREEN_MARGIN_PX`] on every side — see the module docs'
/// "Culling offscreen items" for why `build` needs this and why its two containment checks
/// deliberately mirror [`crate::validate::validate`]'s own, rather than using a looser test.
#[derive(Clone, Copy)]
struct InflatedViewport {
    /// The region's top-left corner.
    min: Point,
    /// The region's bottom-right corner.
    max: Point,
}

impl InflatedViewport {
    /// Widens `viewport_bounds` (the exact viewport rect `build` rasterises into) by
    /// [`OFFSCREEN_MARGIN_PX`] on every side, saturating rather than overflowing — matching
    /// [`crate::validate`]'s own `inflate` (not shared as code, since that function is
    /// private to its module and returns a differently-shaped type, but computed from the
    /// exact same public constant so the two regions are always identical in practice).
    fn new(viewport_bounds: Rect) -> Self {
        let margin = Au::from_px(OFFSCREEN_MARGIN_PX);
        InflatedViewport {
            min: Point {
                x: viewport_bounds.origin.x.saturating_sub(margin),
                y: viewport_bounds.origin.y.saturating_sub(margin),
            },
            max: Point {
                x: viewport_bounds.right().saturating_add(margin),
                y: viewport_bounds.bottom().saturating_add(margin),
            },
        }
    }

    /// Whether `rect` overlaps this region at all — deliberately the *exact* condition
    /// [`crate::validate::validate`] itself checks for a `Rect`/`Border` item's rect (mirroring
    /// that module's own private `InflatedBounds::intersects_rect`, degenerate-rect rule
    /// included), so an item that survives this call is *guaranteed* to pass the corresponding
    /// `validate` check, not merely likely to.
    ///
    /// Both sides used to test full *containment* instead, which was wrong in the direction
    /// that actually loses content: a container taller than `viewport height + 4096px` — a
    /// `background` on a 5000px-tall div, or ~90 ordinary stacked paragraphs — has its top
    /// firmly onscreen, yet its border box is not contained in the region, so its background
    /// and border were culled here and the page rendered blank where the reader could plainly
    /// see it should not (`crates/paint/tests/build.rs`'s
    /// `tall_container_background_should_survive_culling`). Culling must only ever drop what
    /// nothing can see, so both this test and `validate`'s are intersections.
    fn intersects_rect(self, rect: Rect) -> bool {
        axis_overlaps(rect.origin.x, rect.right(), self.min.x, self.max.x)
            && axis_overlaps(rect.origin.y, rect.bottom(), self.min.y, self.max.y)
    }

    /// Whether `point` lies within this region — the exact condition
    /// [`crate::validate::validate`] checks against a [`DisplayItem::Text`] run's
    /// `GlyphRun::origin` (it does not itself bound a run's individual glyph offsets against
    /// the viewport at all, only their arithmetic — see that function's `check_text_run`), so
    /// this is what a `Text` item is culled against: no bounding-box estimate is needed, and
    /// none can be more correct than checking the exact thing `validate` checks.
    fn contains_point(self, point: Point) -> bool {
        point.x >= self.min.x
            && point.x <= self.max.x
            && point.y >= self.min.y
            && point.y <= self.max.y
    }

    /// Clamps `rect` to lie entirely within this region, without ever dropping it: a `rect`
    /// that starts inside the region and extends past it is trimmed at the region's edge; a
    /// `rect` that does not intersect the region at all collapses to a zero-size rect pinned
    /// at the nearest corner. Either way the result always satisfies
    /// [`crate::validate::validate`]'s bounds check — used only for a clip rect
    /// (see the module docs), which must never disappear or the clip stack would unbalance.
    fn clamp(self, rect: Rect) -> Rect {
        let left = rect.origin.x.clamp(self.min.x, self.max.x);
        let top = rect.origin.y.clamp(self.min.y, self.max.y);
        let right = rect.right().clamp(self.min.x, self.max.x).max(left);
        let bottom = rect.bottom().clamp(self.min.y, self.max.y).max(top);
        Rect {
            origin: Point { x: left, y: top },
            size: Size {
                w: right.saturating_sub(left),
                h: bottom.saturating_sub(top),
            },
        }
    }
}

/// One entry of [`walk`]'s explicit stack: either a fragment still to be painted, or a
/// previously pushed clip that must be closed once every fragment nested inside it has been.
enum Frame {
    /// Paint this fragment (and, after it, its children).
    Fragment(FragmentId),
    /// Emit the [`DisplayItem::PopClip`] matching an earlier [`DisplayItem::PushClip`].
    PopClip,
}

/// The result of deciding CSS 2.1 §14.2's canvas background propagation — see the module
/// docs.
struct CanvasBackground {
    /// The color to paint over the whole viewport, if either `<html>` or `<body>` has a
    /// non-transparent background.
    color: Option<Rgba8>,
    /// The fragment whose own background must *not* be painted on its border box, because
    /// `color` already is that fragment's background propagated to the canvas (CSS 2.1
    /// §14.2 resets the source element's own used background to transparent) — either
    /// `<html>`'s fragment or `<body>`'s, whichever supplied `color`.
    suppressed_fragment: Option<FragmentId>,
}

/// Walks `tree` from its root, appending every [`DisplayItem`] in paint order to `items`.
/// `suppressed_fragment`, when set, is the one fragment whose own background
/// [`paint_fragment`] must skip (see [`CanvasBackground`]). `viewport` is the region
/// [`paint_fragment`] culls offscreen items against, and that a clip rect is clamped into
/// (see the module docs' "Culling offscreen items"). See the module docs for the algorithm
/// and its totality guarantees.
fn walk(
    tree: &FragmentTree,
    suppressed_fragment: Option<FragmentId>,
    viewport: &InflatedViewport,
    items: &mut Vec<DisplayItem>,
) {
    let mut stack = vec![Frame::Fragment(tree.root)];
    let mut remaining = tree.len();

    while let Some(frame) = stack.pop() {
        match frame {
            Frame::PopClip => items.push(DisplayItem::PopClip),
            Frame::Fragment(id) => {
                if remaining == 0 {
                    // The cycle guard fired: this walk has popped more fragment frames than
                    // the tree holds fragments, so a malformed tree is looping. Stopping is
                    // right, but stopping *silently* would leave every `PushClip` already
                    // emitted unmatched — an `UnbalancedClip` list, which
                    // `crate::validate::validate` rejects outright, turning a merely
                    // malformed tree into an unrenderable one. Close them all instead.
                    drain_pending_pop_clips(&stack, items);
                    return;
                }
                remaining -= 1;

                let Some(fragment) = tree.get(id) else {
                    continue;
                };
                let style = resolve_style(tree, fragment.style);
                let suppress_background = suppressed_fragment == Some(id);

                paint_fragment(fragment, &style, suppress_background, viewport, items);

                let push_clip = style.overflow == Overflow::Hidden;
                if push_clip {
                    items.push(DisplayItem::PushClip {
                        // Clamped, never dropped: an offscreen `overflow: hidden` fragment's
                        // clip must still balance its matching `PopClip` — see the module
                        // docs.
                        rect: viewport.clamp(fragment.padding_box),
                    });
                    stack.push(Frame::PopClip);
                }

                let mut children = fragment.children.clone();
                children.reverse();
                stack.extend(children.into_iter().map(Frame::Fragment));
            }
        }
    }
}

/// Emits a [`DisplayItem::PopClip`] for every [`Frame::PopClip`] still pending on `stack`,
/// innermost first (stack order, top down) — [`walk`]'s exit path when its cycle guard fires
/// part-way through a clipped subtree, so the list it produced still balances.
///
/// Takes the stack by shared reference: the walk is returning immediately afterwards, so
/// draining it in place would buy nothing.
fn drain_pending_pop_clips(stack: &[Frame], items: &mut Vec<DisplayItem>) {
    for frame in stack.iter().rev() {
        if matches!(frame, Frame::PopClip) {
            items.push(DisplayItem::PopClip);
        }
    }
}

/// Emits the background and border items for one fragment (steps 1-2 of the module docs'
/// paint order), or its glyph runs if it is a `Text` fragment. A `Line`/`AnonymousBlock`
/// fragment never has its own background or border. Culls any item whose geometry does not
/// intersect `viewport` — see the module docs' "Culling offscreen items" — independently per
/// item (a `Block`'s background/border share one culling decision since they share one rect;
/// each of a `Text` fragment's runs is culled on its own).
fn paint_fragment(
    fragment: &Fragment,
    style: &LayoutStyle,
    suppress_background: bool,
    viewport: &InflatedViewport,
    items: &mut Vec<DisplayItem>,
) {
    match &fragment.kind {
        FragmentKind::Block => {
            if !viewport.intersects_rect(fragment.border_box) {
                return;
            }
            if !suppress_background && style.background.a > 0 && !fragment.border_box.is_empty() {
                items.push(DisplayItem::Rect {
                    rect: fragment.border_box,
                    color: style.background,
                });
            }
            if let Some(widths) = solid_border_widths(style) {
                items.push(DisplayItem::Border {
                    rect: fragment.border_box,
                    widths,
                    colors: style.border_color,
                });
            }
        }
        FragmentKind::AnonymousBlock | FragmentKind::Line => {}
        FragmentKind::Text { runs } => {
            for run in runs {
                if run.glyphs.is_empty() {
                    continue;
                }
                if viewport.contains_point(run.origin) {
                    items.push(DisplayItem::Text { run: run.clone() });
                }
            }
        }
    }
}

/// The border widths to paint: `style.border_width`'s side wherever that side is solid and
/// nonzero, [`Au::ZERO`] everywhere else. `None` if no side qualifies (nothing to paint).
fn solid_border_widths(style: &LayoutStyle) -> Option<Sides<Au>> {
    let widths = Sides {
        top: solid_width(style.border_solid.top, style.border_width.top),
        right: solid_width(style.border_solid.right, style.border_width.right),
        bottom: solid_width(style.border_solid.bottom, style.border_width.bottom),
        left: solid_width(style.border_solid.left, style.border_width.left),
    };
    let any = widths.top > Au::ZERO
        || widths.right > Au::ZERO
        || widths.bottom > Au::ZERO
        || widths.left > Au::ZERO;
    any.then_some(widths)
}

/// One side's painted width: `width` if `solid` and positive, [`Au::ZERO`] otherwise.
fn solid_width(solid: bool, width: Au) -> Au {
    if solid && width > Au::ZERO {
        width
    } else {
        Au::ZERO
    }
}

/// Looks up `id`'s style, falling back to [`LayoutStyle::initial`] if it does not resolve
/// (see the module docs' totality guarantee).
fn resolve_style(tree: &FragmentTree, id: StyleId) -> LayoutStyle {
    tree.style(id).cloned().unwrap_or_else(LayoutStyle::initial)
}

/// Decides CSS 2.1 §14.2's canvas background propagation: `<html>`'s background if
/// non-transparent, else `<body>`'s if that is non-transparent, else nothing. Whichever
/// element supplied the canvas color has its *own* used background reset to transparent by
/// the same propagation (CSS 2.1 §14.2: "the used background ... becomes transparent" on the
/// element the value was taken from) — so `suppressed_fragment` is always that element's
/// fragment, never the other one.
fn canvas_background(tree: &FragmentTree, doc: &Document) -> CanvasBackground {
    let html_id = find_element_fragment(tree, doc, &local_name!("html"));
    if let Some(bg) = html_id.map(|id| fragment_background(tree, id))
        && bg.a > 0
    {
        return CanvasBackground {
            color: Some(bg),
            suppressed_fragment: html_id,
        };
    }

    let body_id = find_element_fragment(tree, doc, &local_name!("body"));
    if let Some(bg) = body_id.map(|id| fragment_background(tree, id))
        && bg.a > 0
    {
        return CanvasBackground {
            color: Some(bg),
            suppressed_fragment: body_id,
        };
    }

    CanvasBackground {
        color: None,
        suppressed_fragment: None,
    }
}

/// `id`'s resolved `background-color`, or fully transparent if `id` does not resolve (see
/// the module docs' totality guarantee).
fn fragment_background(tree: &FragmentTree, id: FragmentId) -> Rgba8 {
    match tree.get(id) {
        Some(fragment) => resolve_style(tree, fragment.style).background,
        None => Rgba8::TRANSPARENT,
    }
}

/// The first fragment in `tree` (document order) whose [`Fragment::node`] is an element
/// named `local` (e.g. `html`, `body`), or `None` if there is none. Uses the same bounded,
/// explicit-stack walk as [`walk`], for the same never-panics-on-a-cycle reason.
fn find_element_fragment(
    tree: &FragmentTree,
    doc: &Document,
    local: &LocalName,
) -> Option<FragmentId> {
    let mut stack = vec![tree.root];
    let mut remaining = tree.len();

    while let Some(id) = stack.pop() {
        if remaining == 0 {
            break;
        }
        remaining -= 1;

        let Some(fragment) = tree.get(id) else {
            continue;
        };
        let is_match = fragment
            .node
            .and_then(|node| doc.element(node))
            .is_some_and(|element| &element.name.local == local);
        if is_match {
            return Some(id);
        }

        let mut children = fragment.children.clone();
        children.reverse();
        stack.extend(children);
    }

    None
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "a failed setup step in a test should abort that test, loudly"
)]
mod tests {
    use super::{DisplayItem, Frame, drain_pending_pop_clips};
    use cl_layout::{FragmentId, Rect, Viewport};

    /// A real [`FragmentId`] — the type has no public constructor (ids only ever come out of a
    /// [`cl_layout::FragmentTree`]), so the cheapest way to name one in a test is to lay out a
    /// trivial document and take its root.
    fn a_real_fragment_id() -> FragmentId {
        let base = cl_net::Url::parse("file:///paint/test.html").expect("base url");
        let doc = cl_html::parse_document_str("<!DOCTYPE html><html><body></body></html>", &base)
            .expect("parse")
            .document;
        let mut engine = cl_style::StyleEngine::new((800.0, 600.0), 1.0).expect("engine");
        engine.add_ua_sheet().expect("ua sheet");
        let styled = engine.resolve(doc).expect("resolve");
        let mut fonts = cl_fonts::FontDb::bundled().expect("bundled font db");
        let tree =
            cl_layout::layout(&styled, Viewport::new(800.0, 600.0), &mut fonts).expect("layout");
        tree.root
    }

    /// [`walk`]'s cycle guard (`remaining == 0`) can fire part-way through a clipped subtree of
    /// a malformed fragment tree. Every `PushClip` already emitted is represented on the stack
    /// by a pending [`Frame::PopClip`], so bailing out must close them all — innermost (top of
    /// stack) first — or the list ends `UnbalancedClip` and
    /// [`crate::validate::validate`] rejects it wholesale. Pending `Frame::Fragment`s
    /// contribute nothing.
    ///
    /// Exercised at this level rather than end to end because the guard is unreachable from a
    /// well-formed [`cl_layout::FragmentTree`], and a cyclic one cannot be assembled from
    /// outside `cl-layout` (`FragmentTree::fragments` is `pub(crate)` there).
    #[test]
    fn drain_pending_pop_clips_should_close_every_open_clip_innermost_first() {
        let id = a_real_fragment_id();
        let stack = vec![
            Frame::PopClip,
            Frame::Fragment(id),
            Frame::PopClip,
            Frame::Fragment(id),
        ];
        let mut items = vec![
            DisplayItem::PushClip {
                rect: Rect::from_px(0.0, 0.0, 10.0, 10.0),
            },
            DisplayItem::PushClip {
                rect: Rect::from_px(1.0, 1.0, 5.0, 5.0),
            },
        ];

        drain_pending_pop_clips(&stack, &mut items);

        assert_eq!(
            items.len(),
            4,
            "one PopClip per pending PopClip frame, nothing per pending Fragment frame"
        );
        assert!(matches!(items.get(2), Some(DisplayItem::PopClip)));
        assert!(matches!(items.get(3), Some(DisplayItem::PopClip)));

        let list = crate::list::DisplayList {
            items,
            bounds: Rect::from_px(0.0, 0.0, 800.0, 600.0),
        };
        assert_eq!(crate::validate::validate(&list, list.bounds), Ok(()));
    }

    /// A stack with nothing pending emits nothing: the guard firing outside any clip must not
    /// invent a `PopClip`, which would be a `PopWithoutPush` of its own.
    #[test]
    fn drain_pending_pop_clips_should_emit_nothing_when_no_clip_is_open() {
        let stack = vec![Frame::Fragment(a_real_fragment_id())];
        let mut items = Vec::new();
        drain_pending_pop_clips(&stack, &mut items);
        assert!(items.is_empty());
    }
}
