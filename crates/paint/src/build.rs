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
//! cycle (a fragment listing itself as its own descendant) from looping forever. A
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
    Rgba8, Sides, StyleId,
};

use crate::list::{DisplayItem, DisplayList};

/// Builds the paint-order display list for `tree`, resolving element identity (for canvas
/// background propagation) against `doc`. See the module docs for the full paint order and
/// the totality guarantees.
#[must_use]
pub fn build(tree: &FragmentTree, doc: &Document) -> DisplayList {
    let bounds = Rect::new(Point::default(), tree.viewport);
    let mut items = Vec::new();

    let canvas = canvas_background(tree, doc);
    if let Some(color) = canvas.color
        && !bounds.is_empty()
    {
        items.push(DisplayItem::Rect {
            rect: bounds,
            color,
        });
    }

    walk(tree, canvas.suppressed_fragment, &mut items);

    DisplayList { items, bounds }
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
/// [`paint_fragment`] must skip (see [`CanvasBackground`]). See the module docs for the
/// algorithm and its totality guarantees.
fn walk(
    tree: &FragmentTree,
    suppressed_fragment: Option<FragmentId>,
    items: &mut Vec<DisplayItem>,
) {
    let mut stack = vec![Frame::Fragment(tree.root)];
    let mut remaining = tree.len();

    while let Some(frame) = stack.pop() {
        match frame {
            Frame::PopClip => items.push(DisplayItem::PopClip),
            Frame::Fragment(id) => {
                if remaining == 0 {
                    break;
                }
                remaining -= 1;

                let Some(fragment) = tree.get(id) else {
                    continue;
                };
                let style = resolve_style(tree, fragment.style);
                let suppress_background = suppressed_fragment == Some(id);

                paint_fragment(fragment, &style, suppress_background, items);

                let push_clip = style.overflow == Overflow::Hidden;
                if push_clip {
                    items.push(DisplayItem::PushClip {
                        rect: fragment.padding_box,
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

/// Emits the background and border items for one fragment (steps 1-2 of the module docs'
/// paint order), or its glyph runs if it is a `Text` fragment. A `Line`/`AnonymousBlock`
/// fragment never has its own background or border.
fn paint_fragment(
    fragment: &Fragment,
    style: &LayoutStyle,
    suppress_background: bool,
    items: &mut Vec<DisplayItem>,
) {
    match &fragment.kind {
        FragmentKind::Block => {
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
                if !run.glyphs.is_empty() {
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
