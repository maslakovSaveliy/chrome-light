//! The block formatting context: [`layout`] walks a [`BoxTree`] (Task 16) and produces a
//! positioned, sized [`FragmentTree`] (CSS 2.1 §10 — box dimensions — and §8.3.1 — margin
//! collapsing).
//!
//! # Scope: this is M1a's placeholder, not Task 18's inline layout
//!
//! **`M1a Task 17 placeholder, replaced in Task 18`.** A block box whose box-tree children
//! are all inline-level (`Inline`/`InlineText`/`LineBreak` — see [`BoxTree`]'s invariants) is
//! not really laid out here: `layout_inline_placeholder` flattens that content into *line
//! groups* — maximal runs of inline-level descendants between `LineBreak` boxes, found by
//! walking into (but not laying out) any nested `Inline` boxes — and emits one `Line`
//! fragment per group, `line_height` tall, with one empty `Text { runs: vec![] }` child
//! fragment per `InlineText` box in that group. A group with zero `InlineText` boxes (an
//! empty line, e.g. two adjacent `<br>`s, or the sole group of an element with no inline
//! content at all) is still emitted as a `Line` fragment, but contributes `0`, not
//! `line_height`, to both its own height and the container's total content height — this is
//! the exact reading of the controller ruling's parenthetical ("a run with zero `InlineText`
//! boxes … contributes zero height"). A box with *no children at all* (box-tree `children`
//! empty — nothing to flatten, not even a trivial empty group) short-circuits to content
//! height `0` with no `Line`/`Text` fragments whatsoever, per the same ruling's final
//! sentence — this is `layout_leaf`'s `children.is_empty()` branch. Every `Text` fragment's
//! three rects are a zero-size rect at its `Line`'s content-box origin: there is no shaping
//! yet to place glyphs with, so no position is claimed. Task 18 replaces all of this with
//! real inline layout (line breaking, glyph runs, per-run baselines) and most likely
//! restructures `Line`'s children to be per-run rather than per-`InlineText`-box.
//!
//! # The walk has no recursion
//!
//! Like [`crate::box_tree::build`], [`layout`] uses one explicit stack (`Frame`) rather
//! than recursing over the box tree: a document's nesting depth is attacker-controlled, and
//! a recursive walk would let a hostile document overflow the call stack. Only a
//! [`crate::box_tree::BoxKind::Block`]/[`crate::box_tree::BoxKind::AnonymousBlock`] box whose
//! own box-tree children are themselves block-level ever gets pushed as a `Frame` (a
//! "container"); one whose children are inline-level or absent is resolved in a single,
//! non-recursive call to `layout_leaf` (a "leaf") without growing the stack at all — an
//! `Inline` box is never itself a stack frame, since by [`BoxTree`]'s own invariant its
//! children are always inline-level too, so it is only ever encountered while *flattening* a
//! leaf's inline content, never as a block-formatting-context participant in its own right.
//!
//! # Margin collapsing: chained through first/last-child, not just one level
//!
//! CSS 2.1 §8.3.1 lets a box's top (or bottom) margin collapse through an unbroken chain of
//! first-child (or last-child) boxes with nothing separating them, arbitrarily deep —
//! `<body><div><p>text</p></div></body>` is the ordinary case: `<body>`'s top margin, `<div>`'s
//! top margin and `<p>`'s top margin are all one adjoining set, collapsing together to a
//! single value, not three independent pairwise collapses. `effective_margin_top`/
//! `effective_margin_bottom` implement exactly this: each walks its box's first/last-in-flow-
//! block-child chain with a loop (not recursion — see "The walk has no recursion" above),
//! collecting every box's own specified margin along the way for as long as each box in the
//! chain has nothing separating it from its own next child (no top/bottom padding or border;
//! for the bottom case, additionally `height: auto` and `min-height: 0` — a definite height or
//! a positive `min-height` gives the box's content a floor a collapsed-through margin would
//! silently violate), then combines the *whole* collected set at once via
//! `collapse_margin_set` — not by folding pairwise, which would discard information for a
//! chain of three or more mixed-sign margins (see that function's docs for a worked
//! counterexample). Resolving each step's percentage margin needs that step's own containing
//! block width, which is the *previous* step's content width, so the walk re-resolves each
//! intermediate box's box model via `resolve_box_model` as it descends — the same computation
//! that box gets "for real" once the main layout walk reaches it as a `Frame`/leaf; redoing it
//! here is bounded by the chain's length (bounded by document depth, like every walk in this
//! crate) and mutates nothing.
//!
//! What is still *not* implemented, deliberately out of M1a's scope:
//! - **Negative-margin edge cases beyond the max-positive/min-negative rule** (e.g. CSS 2.1's
//!   notes on clamping when a collapsed negative margin would pull a box's content above its
//!   containing block) — `collapse_margin_set`'s general rule is applied uniformly and never
//!   special-cased further.
//! - **Self-collapsing empty blocks** (CSS 2.1 §8.3.1: a block with `height: auto`, no
//!   padding/border and no in-flow content collapses its own top and bottom margins
//!   *together*, then that combined margin can itself adjoin its neighbours' margins). This
//!   crate's chains only ever walk *into* a box's children, never treat a box's own top and
//!   bottom as adjoining each other.
//! - **Clearance** (`clear` interacting with floats) — M1a implements no floats at all.
//! - **BFC roots stopping the chain** — `overflow` other than `visible` establishes a new
//!   block formatting context per CSS 2.1 §9.4.1, which must not let margins collapse through
//!   it; this crate reads `overflow` into `LayoutStyle` (for paint, per the controller ruling)
//!   but does not yet consult it here. Not exercised by any of the 12 tests or 5 goldens.
//!
//! # Margins never collapse through the document root
//!
//! Per the controller ruling, the root box's (`<html>`'s) own margins are never treated as
//! collapsing with its first/last child's (`<body>`'s) margins — [`layout`] enforces this by
//! constructing the root `Frame` with `first_child_collapse_eligible` hardcoded to `false`
//! regardless of the root's own padding/border, which is the only thing that flag controls.

use cl_dom::NodeId;
use cl_fonts::FontDb;
use cl_style::StyledDocument;

use crate::au::Au;
use crate::box_tree::{BoxId, BoxKind, BoxTree, LayoutBox, build};
use crate::error::LayoutError;
use crate::fragment::{Fragment, FragmentId, FragmentKind, FragmentTree, StyleId, Viewport};
use crate::geom::{BoxSizing, LayoutStyle, Length, Point, Position, Rect, Sides, Size};

/// Lays out `doc`'s box tree against `viewport`, producing an absolutely-positioned
/// [`FragmentTree`].
///
/// `fonts` is threaded through only so Task 18's text-shaping pass can slot in without
/// changing this function's signature or its callers — M1a's inline placeholder (see the
/// module docs) never shapes text and does not read it.
///
/// Total over any [`StyledDocument`]: never panics, however deeply nested, however extreme
/// the CSS values (`Au`'s saturating arithmetic — see [`crate::au`] — absorbs overflow from a
/// document adversarially chosen to make it, e.g., a `1e9px` margin ten levels deep).
///
/// # Errors
/// Never returns `Err` today — see [`LayoutError`]'s docs for why the type exists anyway.
#[allow(
    clippy::unnecessary_wraps,
    reason = "the task brief's public interface is `Result<FragmentTree, LayoutError>`, and \
              `LayoutError` is deliberately kept as a real, non_exhaustive, uninhabited type \
              for forward-compatibility — see its module docs — so a future fallible case can \
              gain a variant without an API break"
)]
pub fn layout(
    doc: &StyledDocument,
    viewport: Viewport,
    // Reserved for Task 18's shaping pass — the M1a inline placeholder (this module's docs)
    // never shapes text and does not read it; kept as a named, typed parameter (rather than
    // dropped and re-added later) so Task 18 does not need to change every caller's call site.
    _fonts: &mut FontDb,
) -> Result<FragmentTree, LayoutError> {
    let box_tree = build(doc);
    Ok(run(&box_tree, viewport))
}

/// One box's resolved box model: everything CSS 2.1 §10.3/§10.4/§10.6 can determine from a
/// box's own style plus its containing block's width (and, for `height`, its containing
/// block's height when definite) — independent of that box's own children.
struct BoxModel {
    margin_left: Au,
    margin_top: Au,
    margin_bottom: Au,
    padding: Sides<Au>,
    border: Sides<Au>,
    content_width: Au,
    min_height: Au,
    max_height: Option<Au>,
    /// `Some(h)` if this box's own `height` is definite (an explicit length, or a percentage
    /// resolved against a definite containing-block height) — already box-sizing-adjusted to
    /// a content-box height and clamped to `[min_height, max_height]`. `None` if the box's
    /// height must instead come from its content (`height: auto`, or a percentage against an
    /// indefinite containing block — CSS 2.1 §10.5 folds the latter onto `auto` too).
    known_height: Option<Au>,
}

/// One frame of the explicit stack [`run`] walks with instead of recursion: a block box whose
/// own box-tree children are block-level, so it is not yet finalized because they are still
/// being visited. See the module docs' "The walk has no recursion" section.
struct Frame {
    /// `fragments.len()` when this frame was pushed — every fragment index from here to the
    /// end of the vec, once this frame pops, is exactly this box's fragment plus its whole
    /// descendant subtree (see [`run`]'s docs for why that range is contiguous), the range
    /// [`finalize_top`] shifts for `position: relative`.
    frag_start: usize,
    node: Option<NodeId>,
    style: LayoutStyle,
    is_anonymous_block: bool,
    /// This box's absolute border-box origin — fixed the moment the frame is pushed (a
    /// block's horizontal and vertical position never depends on its own children, only on
    /// its containing block and its preceding siblings).
    border_box_origin: Point,
    /// This box's absolute content-box origin (`border_box_origin` plus its own
    /// border+padding) — precomputed since every child's position is expressed relative to
    /// it.
    content_origin: Point,
    padding: Sides<Au>,
    border: Sides<Au>,
    content_width: Au,
    min_height: Au,
    max_height: Option<Au>,
    /// This box's own resolved height, mirroring [`BoxModel::known_height`] — also exactly
    /// the containing-block height this frame's children resolve a percentage `height`
    /// against (CSS 2.1 §10.5: only a *definite* containing-block height counts).
    known_height: Option<Au>,
    /// This box's own containing block's width — needed again at pop time to resolve a
    /// `position: relative` percentage offset.
    cb_width: Au,
    /// This box's own containing block's height, mirroring [`Frame::cb_width`].
    cb_height: Option<Au>,
    /// Whether this box's top margin collapses with its first block-level child's top
    /// margin (CSS 2.1 §8.3.1: no padding, no border, nothing else separating them).
    /// Hardcoded `false` for the document root regardless of its own padding/border — see
    /// the module docs' "Margins never collapse through the document root" section.
    first_child_collapse_eligible: bool,
    /// Whether this box's bottom margin collapses with its last block-level child's bottom
    /// margin (CSS 2.1 §8.3.1: `height: auto`, no padding, no border).
    bottom_collapse_eligible: bool,
    /// This box's block-level box-tree children (guaranteed non-empty and all block-level —
    /// see [`is_block_container`] — whenever a `Frame` exists for it at all).
    block_children: Vec<BoxId>,
    /// Index into `block_children` of the next child to lay out.
    next_child: usize,
    /// The position, relative to this box's content-box top, of the bottom of the last
    /// child's border box placed so far (before any trailing margin — see
    /// [`Frame::bottom_collapse_eligible`]).
    running_content_bottom: Au,
    /// The previous child's [`effective_margin_bottom`], for collapsing with the next
    /// child's [`effective_margin_top`] (CSS 2.1 §8.3.1's sibling case). `None` before the
    /// first child is placed.
    prev_margin_bottom: Option<Au>,
    /// Finished child fragment ids, in box (≈ document) order.
    children_frag_ids: Vec<FragmentId>,
    /// This box's own [`effective_margin_bottom`], computed once when the frame was pushed
    /// (it only depends on this box's style and a peek at its last child's style — never on
    /// layout) and stashed here for [`finalize_top`] to hand to this box's *parent* once this
    /// frame pops.
    effective_margin_bottom_for_parent: Au,
}

/// Runs the whole block-formatting-context walk for `box_tree` against `viewport` and
/// returns the finished [`FragmentTree`]. See the module docs for the algorithm's shape.
fn run(box_tree: &BoxTree, viewport: Viewport) -> FragmentTree {
    let mut fragments: Vec<Fragment> = Vec::new();
    let mut styles: Vec<LayoutStyle> = Vec::new();

    let root_id = box_tree.root;
    let Some(root_box) = box_tree.get(root_id) else {
        // Unreachable: `BoxTree::build` always produces at least a root box. Kept total.
        return empty_tree(&mut fragments, &mut styles, viewport);
    };

    let cb_width = viewport.size.w;
    let cb_height = Some(viewport.size.h);
    let model = resolve_box_model(&root_box.style, cb_width, cb_height);
    // The document root's own margins are used as specified, never as an "effective"
    // (peeked-through-a-child) value — nothing precedes the root for them to collapse with,
    // and the module docs' "Margins never collapse through the document root" section
    // explains why the reverse (the root absorbing its first child's margin) is also
    // suppressed below.
    let root_origin = Point {
        x: model.margin_left,
        y: model.margin_top,
    };
    let is_anon_root = is_anonymous(&root_box.kind);

    if is_block_container(box_tree, root_id) {
        let content_origin = content_origin_of(root_origin, &model);
        let frame = Frame {
            frag_start: 0,
            node: root_box.node,
            style: root_box.style.clone(),
            is_anonymous_block: is_anon_root,
            border_box_origin: root_origin,
            content_origin,
            padding: model.padding,
            border: model.border,
            content_width: model.content_width,
            min_height: model.min_height,
            max_height: model.max_height,
            known_height: model.known_height,
            cb_width,
            cb_height,
            first_child_collapse_eligible: false,
            bottom_collapse_eligible: false,
            block_children: root_box.children.clone(),
            next_child: 0,
            running_content_bottom: Au::ZERO,
            prev_margin_bottom: None,
            children_frag_ids: Vec::new(),
            effective_margin_bottom_for_parent: Au::ZERO,
        };
        let mut stack = vec![frame];
        while let Some(has_next) = stack
            .last()
            .map(|top| top.next_child < top.block_children.len())
        {
            if has_next {
                advance_child(box_tree, &mut stack, &mut fragments, &mut styles);
            } else if let Some(tree) =
                finalize_top(&mut stack, &mut fragments, &mut styles, viewport)
            {
                return tree;
            }
        }
        // Unreachable: the loop above only exits via the early `return` inside
        // `finalize_top`'s `None`-parent case, which fires exactly when the stack becomes
        // empty. Kept total rather than diverging if that invariant is ever wrong.
        empty_tree(&mut fragments, &mut styles, viewport)
    } else {
        let leaf = LeafArgs {
            node: root_box.node,
            style: &root_box.style,
            is_anonymous_block: is_anon_root,
            model: &model,
            children: &root_box.children,
            origin: root_origin,
            cb_width,
            cb_height,
        };
        let (root, _height) = layout_leaf(box_tree, &leaf, &mut fragments, &mut styles);
        FragmentTree {
            fragments,
            styles,
            root,
            viewport: viewport.size,
        }
    }
}

/// The degenerate empty tree: a single, childless, styleless `AnonymousBlock` fragment.
/// Used only for the "unreachable in practice" fallbacks in [`run`] — see their docs.
fn empty_tree(
    fragments: &mut Vec<Fragment>,
    styles: &mut Vec<LayoutStyle>,
    viewport: Viewport,
) -> FragmentTree {
    let style_id = push_style(styles, LayoutStyle::initial());
    let root = push_fragment(
        fragments,
        Fragment {
            node: None,
            kind: FragmentKind::AnonymousBlock,
            border_box: Rect::default(),
            padding_box: Rect::default(),
            content_box: Rect::default(),
            style: style_id,
            children: Vec::new(),
        },
    );
    FragmentTree {
        fragments: std::mem::take(fragments),
        styles: std::mem::take(styles),
        root,
        viewport: viewport.size,
    }
}

/// Advances the top frame of `stack` by one child: resolves that child's box model and
/// position, then either pushes a new [`Frame`] for it (if it is itself a block container)
/// or resolves it immediately via [`layout_leaf`] (if it is a leaf), updating the top frame's
/// stacking cursor either way. See the module docs' "The walk has no recursion" section.
/// The parent frame's context [`place_child`] needs to position one child — bundled into one
/// value to keep [`place_child`]'s own argument count under
/// `clippy::too_many_arguments`' threshold.
struct ParentContext {
    content_width: Au,
    cb_height: Option<Au>,
    content_origin: Point,
    first_child_collapse_eligible: bool,
    prev_margin_bottom: Option<Au>,
    running_content_bottom: Au,
}

/// One child's resolved box model, absolute border-box origin, and the values its own parent
/// needs once it is finalized (an `effective_margin_bottom` to hand to whatever comes after
/// it, and its own containing block, for a later `position: relative` offset).
struct ChildPlacement {
    model: BoxModel,
    origin: Point,
    effective_margin_bottom: Au,
    cb_width: Au,
    cb_height: Option<Au>,
}

/// Resolves `child_box_id`'s box model and absolute position within `parent`, applying the
/// sibling/first-child margin-collapsing rules (CSS 2.1 §8.3.1) described in the module docs.
/// Does not decide whether `child_box_id` is a container or a leaf, nor mutate anything —
/// [`advance_child`]'s two callees do that with the result.
fn place_child(
    box_tree: &BoxTree,
    child_box_id: BoxId,
    style: &LayoutStyle,
    parent: &ParentContext,
) -> ChildPlacement {
    let model = resolve_box_model(style, parent.content_width, parent.cb_height);
    let eff_top = effective_margin_top(
        box_tree,
        child_box_id,
        model.margin_top,
        model.padding.top,
        model.border.top,
        model.content_width,
    );
    let eff_bottom = effective_margin_bottom(
        box_tree,
        child_box_id,
        model.margin_bottom,
        model.padding.bottom,
        model.border.bottom,
        model.known_height.is_none(),
        model.min_height,
        model.content_width,
    );

    let gap_before = match parent.prev_margin_bottom {
        Some(prev) => collapse_margins(prev, eff_top),
        None if parent.first_child_collapse_eligible => Au::ZERO,
        None => eff_top,
    };

    let origin = Point {
        x: parent.content_origin.x.saturating_add(model.margin_left),
        y: parent
            .content_origin
            .y
            .saturating_add(parent.running_content_bottom)
            .saturating_add(gap_before),
    };

    ChildPlacement {
        model,
        origin,
        effective_margin_bottom: eff_bottom,
        cb_width: parent.content_width,
        cb_height: parent.cb_height,
    }
}

/// Advances the top frame of `stack` by one child: resolves that child's box model and
/// position, then either pushes a new [`Frame`] for it (if it is itself a block container)
/// or resolves it immediately via [`layout_leaf`] (if it is a leaf), updating the top frame's
/// stacking cursor either way. See the module docs' "The walk has no recursion" section.
fn advance_child(
    box_tree: &BoxTree,
    stack: &mut Vec<Frame>,
    fragments: &mut Vec<Fragment>,
    styles: &mut Vec<LayoutStyle>,
) {
    let Some(top) = stack.last_mut() else {
        return;
    };
    let Some(child_box_id) = top.block_children.get(top.next_child).copied() else {
        return;
    };
    top.next_child += 1;
    let parent = ParentContext {
        content_width: top.content_width,
        cb_height: top.known_height,
        content_origin: top.content_origin,
        first_child_collapse_eligible: top.first_child_collapse_eligible,
        prev_margin_bottom: top.prev_margin_bottom,
        running_content_bottom: top.running_content_bottom,
    };

    let Some(child) = box_tree.get(child_box_id) else {
        return;
    };
    let placement = place_child(box_tree, child_box_id, &child.style, &parent);

    if is_block_container(box_tree, child_box_id) {
        push_container_frame(stack, fragments.len(), child, &placement);
    } else {
        resolve_leaf_child(box_tree, stack, fragments, styles, child, &placement);
    }
}

/// The container half of [`advance_child`]: pushes a new [`Frame`] for `child`, deferring its
/// fragment (its height may depend on grandchildren not yet visited) — see the module docs'
/// "The walk has no recursion" section.
fn push_container_frame(
    stack: &mut Vec<Frame>,
    frag_start: usize,
    child: &LayoutBox,
    placement: &ChildPlacement,
) {
    let content_origin = content_origin_of(placement.origin, &placement.model);
    let first_child_collapse_eligible =
        placement.model.padding.top == Au::ZERO && placement.model.border.top == Au::ZERO;
    let bottom_collapse_eligible = placement.model.known_height.is_none()
        && placement.model.padding.bottom == Au::ZERO
        && placement.model.border.bottom == Au::ZERO;
    stack.push(Frame {
        frag_start,
        node: child.node,
        style: child.style.clone(),
        is_anonymous_block: is_anonymous(&child.kind),
        border_box_origin: placement.origin,
        content_origin,
        padding: placement.model.padding,
        border: placement.model.border,
        content_width: placement.model.content_width,
        min_height: placement.model.min_height,
        max_height: placement.model.max_height,
        known_height: placement.model.known_height,
        cb_width: placement.cb_width,
        cb_height: placement.cb_height,
        first_child_collapse_eligible,
        bottom_collapse_eligible,
        block_children: child.children.clone(),
        next_child: 0,
        running_content_bottom: Au::ZERO,
        prev_margin_bottom: None,
        children_frag_ids: Vec::new(),
        effective_margin_bottom_for_parent: placement.effective_margin_bottom,
    });
}

/// The leaf half of [`advance_child`]: resolves `child` immediately via [`layout_leaf`] (no
/// stack growth needed) and updates the new top frame's (`child`'s parent's) stacking cursor.
fn resolve_leaf_child(
    box_tree: &BoxTree,
    stack: &mut [Frame],
    fragments: &mut Vec<Fragment>,
    styles: &mut Vec<LayoutStyle>,
    child: &LayoutBox,
    placement: &ChildPlacement,
) {
    let leaf = LeafArgs {
        node: child.node,
        style: &child.style,
        is_anonymous_block: is_anonymous(&child.kind),
        model: &placement.model,
        children: &child.children,
        origin: placement.origin,
        cb_width: placement.cb_width,
        cb_height: placement.cb_height,
    };
    let (frag_id, used_height) = layout_leaf(box_tree, &leaf, fragments, styles);
    if let Some(top) = stack.last_mut() {
        let rel_bottom = placement
            .origin
            .y
            .saturating_sub(top.content_origin.y)
            .saturating_add(used_height);
        top.running_content_bottom = rel_bottom;
        top.prev_margin_bottom = Some(placement.effective_margin_bottom);
        top.children_frag_ids.push(frag_id);
    }
}

/// Pops the top frame of `stack`, finalizes it into a `Block`/`AnonymousBlock` [`Fragment`],
/// applies its `position: relative` offset (if any) to itself and its whole subtree, and
/// hands the result to the new top frame (its parent) — or, if `stack` is now empty, returns
/// the finished [`FragmentTree`].
fn finalize_top(
    stack: &mut Vec<Frame>,
    fragments: &mut Vec<Fragment>,
    styles: &mut Vec<LayoutStyle>,
    viewport: Viewport,
) -> Option<FragmentTree> {
    let popped = stack.pop()?;

    let content_height = if let Some(h) = popped.known_height {
        h
    } else {
        let trailing = if popped.bottom_collapse_eligible {
            Au::ZERO
        } else {
            popped.prev_margin_bottom.unwrap_or(Au::ZERO)
        };
        clamp_au(
            popped.running_content_bottom.saturating_add(trailing),
            popped.min_height,
            popped.max_height,
        )
    };
    let border_box_size = Size {
        w: popped
            .content_width
            .saturating_add(popped.padding.left)
            .saturating_add(popped.padding.right)
            .saturating_add(popped.border.left)
            .saturating_add(popped.border.right),
        h: content_height
            .saturating_add(popped.padding.top)
            .saturating_add(popped.padding.bottom)
            .saturating_add(popped.border.top)
            .saturating_add(popped.border.bottom),
    };
    let border_box = Rect {
        origin: popped.border_box_origin,
        size: border_box_size,
    };
    let padding_box = inset(border_box, popped.border);
    let content_box = inset(padding_box, popped.padding);
    let kind = if popped.is_anonymous_block {
        FragmentKind::AnonymousBlock
    } else {
        FragmentKind::Block
    };
    let style_id = push_style(styles, popped.style.clone());
    let frag_id = push_fragment(
        fragments,
        Fragment {
            node: popped.node,
            kind,
            border_box,
            padding_box,
            content_box,
            style: style_id,
            children: popped.children_frag_ids,
        },
    );

    if popped.style.position == Position::Relative {
        let (dx, dy) = resolve_offset(&popped.style, popped.cb_width, popped.cb_height);
        if let Some(slice) = fragments.get_mut(popped.frag_start..) {
            for f in slice {
                shift_fragment(f, dx, dy);
            }
        }
    }

    match stack.last_mut() {
        Some(parent) => {
            let rel_bottom = popped
                .border_box_origin
                .y
                .saturating_sub(parent.content_origin.y)
                .saturating_add(border_box_size.h);
            parent.running_content_bottom = rel_bottom;
            parent.prev_margin_bottom = Some(popped.effective_margin_bottom_for_parent);
            parent.children_frag_ids.push(frag_id);
            None
        }
        None => Some(FragmentTree {
            fragments: std::mem::take(fragments),
            styles: std::mem::take(styles),
            root: frag_id,
            viewport: viewport.size,
        }),
    }
}

/// Bundles [`layout_leaf`]'s many read-only inputs into one value, keeping the function's
/// own argument count under `clippy::too_many_arguments`' threshold.
struct LeafArgs<'a> {
    node: Option<NodeId>,
    style: &'a LayoutStyle,
    is_anonymous_block: bool,
    model: &'a BoxModel,
    children: &'a [BoxId],
    /// This box's absolute border-box origin.
    origin: Point,
    /// This box's own containing block width (its parent's content width) — needed again
    /// here (distinct from `model.content_width`, this box's *own* content width) to resolve
    /// a `position: relative` offset.
    cb_width: Au,
    /// This box's own containing block height, mirroring `cb_width`.
    cb_height: Option<Au>,
}

/// Resolves one block box whose box-tree children are inline-level or absent (never
/// block-level) into a finished `Block`/`AnonymousBlock` fragment, entirely in one call: no
/// further stack frame is needed because nothing about it depends on grandchildren being
/// laid out first — see the module docs' "The walk has no recursion" section and, for the
/// content itself, "Scope: this is M1a's placeholder, not Task 18's inline layout".
///
/// Returns the new fragment's id and its used border-box height (so the caller can update
/// its own stacking cursor exactly as it would for a popped container [`Frame`]).
fn layout_leaf(
    box_tree: &BoxTree,
    args: &LeafArgs<'_>,
    fragments: &mut Vec<Fragment>,
    styles: &mut Vec<LayoutStyle>,
) -> (FragmentId, Au) {
    let frag_start = fragments.len();
    let content_origin = content_origin_of(args.origin, args.model);

    let (child_frag_ids, content_height_from_inline) = if args.children.is_empty() {
        (Vec::new(), Au::ZERO)
    } else {
        layout_inline_placeholder(
            box_tree,
            args.children,
            args.style,
            content_origin,
            args.model.content_width,
            fragments,
            styles,
        )
    };
    let content_height = args.model.known_height.unwrap_or_else(|| {
        clamp_au(
            content_height_from_inline,
            args.model.min_height,
            args.model.max_height,
        )
    });
    let border_box_size = Size {
        w: args
            .model
            .content_width
            .saturating_add(args.model.padding.left)
            .saturating_add(args.model.padding.right)
            .saturating_add(args.model.border.left)
            .saturating_add(args.model.border.right),
        h: content_height
            .saturating_add(args.model.padding.top)
            .saturating_add(args.model.padding.bottom)
            .saturating_add(args.model.border.top)
            .saturating_add(args.model.border.bottom),
    };
    let border_box = Rect {
        origin: args.origin,
        size: border_box_size,
    };
    let padding_box = inset(border_box, args.model.border);
    let content_box = inset(padding_box, args.model.padding);
    let kind = if args.is_anonymous_block {
        FragmentKind::AnonymousBlock
    } else {
        FragmentKind::Block
    };
    let style_id = push_style(styles, args.style.clone());
    let id = push_fragment(
        fragments,
        Fragment {
            node: args.node,
            kind,
            border_box,
            padding_box,
            content_box,
            style: style_id,
            children: child_frag_ids,
        },
    );

    if args.style.position == Position::Relative {
        let (dx, dy) = resolve_offset(args.style, args.cb_width, args.cb_height);
        if let Some(slice) = fragments.get_mut(frag_start..) {
            for f in slice {
                shift_fragment(f, dx, dy);
            }
        }
    }

    (id, border_box_size.h)
}

/// One token of a leaf's flattened inline content — see [`flatten_inline_children`].
enum InlineToken {
    /// One [`crate::box_tree::BoxKind::InlineText`] box.
    Text(BoxId),
    /// One [`crate::box_tree::BoxKind::LineBreak`] box: ends the current line group.
    Break,
}

/// Flattens `children` (a leaf block's own, inline-level, box-tree children) into a document
/// order sequence of [`InlineToken`]s, walking into (but not laying out) any nested `Inline`/
/// `AnonymousInline` box to reach the `InlineText`/`LineBreak` boxes inside it. Uses an
/// explicit stack, not recursion — see the module docs' "The walk has no recursion" section;
/// `Inline` boxes can themselves nest arbitrarily (`<span><span>…`), so this is exactly as
/// attacker-depth-controlled as the rest of the tree.
fn flatten_inline_children(box_tree: &BoxTree, children: &[BoxId]) -> Vec<InlineToken> {
    let mut tokens = Vec::new();
    let mut stack: Vec<BoxId> = children.to_vec();
    stack.reverse();
    // Bounded by the box tree's total size, the same hostile-input safeguard
    // `crate::dump::box_tree_dump`'s walk uses: never actually reached (no box tree has a
    // cycle), but keeps this total if that invariant is ever wrong.
    let mut remaining = box_tree.len();
    while let Some(id) = stack.pop() {
        if remaining == 0 {
            break;
        }
        remaining -= 1;
        let Some(b) = box_tree.get(id) else {
            continue;
        };
        match &b.kind {
            BoxKind::InlineText(_) => tokens.push(InlineToken::Text(id)),
            BoxKind::LineBreak => tokens.push(InlineToken::Break),
            BoxKind::Inline | BoxKind::AnonymousInline => {
                let mut kids = b.children.clone();
                kids.reverse();
                stack.extend(kids);
            }
            // Never a child of a leaf per the box tree invariant (a leaf's children are
            // never block-level) — skipped defensively rather than trusted, in case a
            // hand-assembled `BoxTree` breaks that invariant.
            BoxKind::Block | BoxKind::AnonymousBlock => {}
        }
    }
    tokens
}

/// Splits a flattened token sequence into line groups on [`InlineToken::Break`] — see the
/// module docs' "Scope" section for exactly what a group means. Always yields at least one
/// group (possibly empty), even for an empty `tokens` slice.
fn split_into_groups(tokens: Vec<InlineToken>) -> Vec<Vec<BoxId>> {
    let mut groups = Vec::new();
    let mut current = Vec::new();
    for token in tokens {
        match token {
            InlineToken::Text(id) => current.push(id),
            InlineToken::Break => groups.push(std::mem::take(&mut current)),
        }
    }
    groups.push(current);
    groups
}

/// Builds the `Line`/`Text` fragments for one leaf block's inline content — the placeholder
/// described in the module docs' "Scope" section. Returns the `Line` fragment ids (in order)
/// and the total content height they contribute.
fn layout_inline_placeholder(
    box_tree: &BoxTree,
    children: &[BoxId],
    container_style: &LayoutStyle,
    content_origin: Point,
    content_width: Au,
    fragments: &mut Vec<Fragment>,
    styles: &mut Vec<LayoutStyle>,
) -> (Vec<FragmentId>, Au) {
    let tokens = flatten_inline_children(box_tree, children);
    let groups = split_into_groups(tokens);

    let mut line_ids = Vec::with_capacity(groups.len());
    let mut cursor_y = Au::ZERO;
    for group in groups {
        let line_height = if group.is_empty() {
            Au::ZERO
        } else {
            container_style.line_height
        };
        let line_rect = Rect {
            origin: Point {
                x: content_origin.x,
                y: content_origin.y.saturating_add(cursor_y),
            },
            size: Size {
                w: content_width,
                h: line_height,
            },
        };

        let mut text_children = Vec::with_capacity(group.len());
        for text_box_id in group {
            let (node, text_style) = match box_tree.get(text_box_id) {
                Some(b) => (b.node, b.style.clone()),
                None => (None, LayoutStyle::initial()),
            };
            let zero_rect = Rect {
                origin: line_rect.origin,
                size: Size::default(),
            };
            let text_style_id = push_style(styles, text_style);
            let text_id = push_fragment(
                fragments,
                Fragment {
                    node,
                    kind: FragmentKind::Text { runs: Vec::new() },
                    border_box: zero_rect,
                    padding_box: zero_rect,
                    content_box: zero_rect,
                    style: text_style_id,
                    children: Vec::new(),
                },
            );
            text_children.push(text_id);
        }

        let line_style_id = push_style(styles, container_style.clone());
        let line_id = push_fragment(
            fragments,
            Fragment {
                node: None,
                kind: FragmentKind::Line,
                border_box: line_rect,
                padding_box: line_rect,
                content_box: line_rect,
                style: line_style_id,
                children: text_children,
            },
        );
        line_ids.push(line_id);
        cursor_y = cursor_y.saturating_add(line_height);
    }

    (line_ids, cursor_y)
}

/// Resolves one box's margin/padding/border/width/height from its own style and its
/// containing block's width (`cb_width`) and, for `height`, its containing block's height
/// (`cb_height`, `None` if indefinite) — CSS 2.1 §10.3.3 (width, auto margins), §10.4
/// (min/max-width), §10.5 (height), §10.7 (min/max-height).
fn resolve_box_model(style: &LayoutStyle, cb_width: Au, cb_height: Option<Au>) -> BoxModel {
    let padding = Sides {
        top: resolve_len_zero(style.padding.top, cb_width),
        right: resolve_len_zero(style.padding.right, cb_width),
        bottom: resolve_len_zero(style.padding.bottom, cb_width),
        left: resolve_len_zero(style.padding.left, cb_width),
    };
    let border = style.border_width;

    // `margin-right` only matters while solving the width/auto-margin equation below (it
    // does not affect this box's own position or size any further than that) — see
    // `resolve_width_and_margins`'s docs.
    let (content_width, margin_left, _margin_right) =
        resolve_width_and_margins(style, cb_width, padding, border);
    let margin_top = resolve_len_zero(style.margin.top, cb_width);
    let margin_bottom = resolve_len_zero(style.margin.bottom, cb_width);

    let min_height = resolve_len_zero(style.min_height, cb_height.unwrap_or(Au::ZERO));
    let max_height = style
        .max_height
        .map(|l| resolve_len_zero(l, cb_height.unwrap_or(Au::ZERO)));
    let known_height = resolve_height(style, cb_height, padding, border)
        .map(|h| clamp_au(h, min_height, max_height));

    BoxModel {
        margin_left,
        margin_top,
        margin_bottom,
        padding,
        border,
        content_width,
        min_height,
        max_height,
        known_height,
    }
}

/// The width half of [`resolve_box_model`] — CSS 2.1 §10.3.3 (the auto-width/auto-margin
/// equation) then §10.4 (min/max-width clamp, with auto margins re-solved against the
/// clamped width).
fn resolve_width_and_margins(
    style: &LayoutStyle,
    cb_width: Au,
    padding: Sides<Au>,
    border: Sides<Au>,
) -> (Au, Au, Au) {
    let non_content = padding
        .left
        .saturating_add(padding.right)
        .saturating_add(border.left)
        .saturating_add(border.right);

    let margin_left_auto = matches!(style.margin.left, Length::Auto);
    let margin_right_auto = matches!(style.margin.right, Length::Auto);
    let margin_left0 = resolve_len_zero(style.margin.left, cb_width);
    let margin_right0 = resolve_len_zero(style.margin.right, cb_width);

    if matches!(style.width, Length::Auto) {
        // CSS 2.1 §10.3.3, "otherwise, if 'width' is set to 'auto' …": any other 'auto'
        // value (i.e. an auto margin) becomes 0, and width fills the rest — no centering.
        let filled = cb_width
            .saturating_sub(margin_left0)
            .saturating_sub(non_content)
            .saturating_sub(margin_right0)
            .max(Au::ZERO);
        return (filled, margin_left0, margin_right0);
    }

    let specified = resolve_len_zero(style.width, cb_width);
    let mut content_width = match style.box_sizing {
        BoxSizing::ContentBox => specified,
        BoxSizing::BorderBox => specified.saturating_sub(non_content).max(Au::ZERO),
    };
    let min_width = resolve_len_zero(style.min_width, cb_width);
    let max_width = style.max_width.map(|l| resolve_len_zero(l, cb_width));
    content_width = clamp_au(content_width, min_width, max_width);

    let non_margin_total = non_content.saturating_add(content_width);
    let (margin_left, margin_right) = if margin_left_auto && margin_right_auto {
        let remaining = cb_width.saturating_sub(non_margin_total);
        if remaining.0 >= 0 {
            let half = remaining.0 / 2;
            (Au(half), Au(remaining.0 - half))
        } else {
            (Au::ZERO, Au::ZERO)
        }
    } else if margin_left_auto {
        (
            cb_width
                .saturating_sub(non_margin_total)
                .saturating_sub(margin_right0),
            margin_right0,
        )
    } else if margin_right_auto {
        (
            margin_left0,
            cb_width
                .saturating_sub(non_margin_total)
                .saturating_sub(margin_left0),
        )
    } else {
        // Over-constrained (neither margin auto, and the four+width don't sum to
        // `cb_width`): M1a does not implement CSS 2.1's "ignore margin-right in LTR" fixup —
        // no test in this task's scope produces this case — so both margins are used exactly
        // as specified.
        (margin_left0, margin_right0)
    };

    (content_width, margin_left, margin_right)
}

/// The height half of [`resolve_box_model`] — CSS 2.1 §10.5: `Some(content-box height)` if
/// `height` is definite, `None` (behave as `auto`) otherwise, including a percentage against
/// an indefinite containing block.
fn resolve_height(
    style: &LayoutStyle,
    cb_height: Option<Au>,
    padding: Sides<Au>,
    border: Sides<Au>,
) -> Option<Au> {
    let specified = match style.height {
        Length::Auto => None,
        Length::Px(au) => Some(au),
        Length::Percent(p) => cb_height.map(|h| h.mul_by_f32(p / 100.0)),
    };
    specified.map(|h| match style.box_sizing {
        BoxSizing::ContentBox => h,
        BoxSizing::BorderBox => {
            let non_content = padding
                .top
                .saturating_add(padding.bottom)
                .saturating_add(border.top)
                .saturating_add(border.bottom);
            h.saturating_sub(non_content).max(Au::ZERO)
        }
    })
}

/// The used gap contributed above a box for margin-collapsing purposes (CSS 2.1 §8.3.1): its
/// own top margin, collapsed together with the top margins of its *whole* first-in-flow-
/// block-child chain — its first block-level child, that child's first block-level child, and
/// so on — for as long as each box in the chain has no top padding and no top border (nothing
/// separating it from its own first child). See the module docs' "Margin collapsing" section:
/// this walks the chain with a loop, not recursion, and combines every margin in the chain at
/// once via [`collapse_margin_set`] (not pairwise — see that function's docs for why pairwise
/// would give a wrong answer for a chain of three or more mixed-sign margins).
///
/// A box further down the chain's own containing block is the previous box's *content* width,
/// not `content_width` (the box named by `box_id`'s containing block) — so each step resolves
/// that child's box model (for its margin, and for its own content width/padding/border, to
/// decide whether the chain continues and what the *next* step's containing block is) via
/// [`resolve_box_model`], the same computation that child will get "for real" once the main
/// layout walk reaches it as a [`Frame`]/leaf. Recomputing it here is bounded by the chain's
/// length (itself bounded by document depth, like every other walk in this crate) and mutates
/// nothing.
fn effective_margin_top(
    box_tree: &BoxTree,
    box_id: BoxId,
    own_margin_top: Au,
    padding_top: Au,
    border_top: Au,
    content_width: Au,
) -> Au {
    let mut margins = vec![own_margin_top];
    let mut eligible = padding_top == Au::ZERO && border_top == Au::ZERO;
    let mut current = box_id;
    let mut current_content_width = content_width;
    let mut remaining = box_tree.len();

    while eligible && remaining > 0 {
        remaining -= 1;
        let Some(child_id) = first_block_child(box_tree, current) else {
            break;
        };
        let Some(child) = box_tree.get(child_id) else {
            break;
        };
        let child_model = resolve_box_model(&child.style, current_content_width, None);
        margins.push(child_model.margin_top);
        eligible = child_model.padding.top == Au::ZERO && child_model.border.top == Au::ZERO;
        current_content_width = child_model.content_width;
        current = child_id;
    }

    collapse_margin_set(&margins)
}

/// The bottom-margin mirror of [`effective_margin_top`], walking the *last*-in-flow-block-
/// child chain instead. CSS 2.1 §8.3.1's bottom-margin case additionally requires each box in
/// the chain to have a definite `height` of `auto` and `min-height` of `0` (a definite height
/// or a positive `min-height` gives the box's content a floor that a collapsed-through margin
/// would silently violate) — checked at every step, not just the first, via each step's own
/// [`resolve_box_model`] result exactly as [`effective_margin_top`] re-derives padding/border.
fn effective_margin_bottom(
    box_tree: &BoxTree,
    box_id: BoxId,
    own_margin_bottom: Au,
    padding_bottom: Au,
    border_bottom: Au,
    known_height_is_none: bool,
    min_height: Au,
    content_width: Au,
) -> Au {
    let mut margins = vec![own_margin_bottom];
    let mut eligible = known_height_is_none
        && min_height == Au::ZERO
        && padding_bottom == Au::ZERO
        && border_bottom == Au::ZERO;
    let mut current = box_id;
    let mut current_content_width = content_width;
    let mut remaining = box_tree.len();

    while eligible && remaining > 0 {
        remaining -= 1;
        let Some(child_id) = last_block_child(box_tree, current) else {
            break;
        };
        let Some(child) = box_tree.get(child_id) else {
            break;
        };
        let child_model = resolve_box_model(&child.style, current_content_width, None);
        margins.push(child_model.margin_bottom);
        eligible = child_model.known_height.is_none()
            && child_model.min_height == Au::ZERO
            && child_model.padding.bottom == Au::ZERO
            && child_model.border.bottom == Au::ZERO;
        current_content_width = child_model.content_width;
        current = child_id;
    }

    collapse_margin_set(&margins)
}

/// `box_id`'s first box-tree child, if it is block-level (`Block`/`AnonymousBlock`) —
/// `None` if `box_id` has no children, or its children are inline-level (per the box tree's
/// own invariant, either all of a container's children are block-level or none are, so
/// checking the first one is enough).
fn first_block_child(box_tree: &BoxTree, box_id: BoxId) -> Option<BoxId> {
    let b = box_tree.get(box_id)?;
    let first = *b.children.first()?;
    let first_box = box_tree.get(first)?;
    matches!(first_box.kind, BoxKind::Block | BoxKind::AnonymousBlock).then_some(first)
}

/// The last-child mirror of [`first_block_child`].
fn last_block_child(box_tree: &BoxTree, box_id: BoxId) -> Option<BoxId> {
    let b = box_tree.get(box_id)?;
    let last = *b.children.last()?;
    let last_box = box_tree.get(last)?;
    matches!(last_box.kind, BoxKind::Block | BoxKind::AnonymousBlock).then_some(last)
}

/// Whether `box_id` is a block *container* — has box-tree children, and they are
/// block-level — as opposed to a *leaf* (no children, or inline-level children): the
/// distinction [`advance_child`]/[`run`] use to decide whether a box needs its own [`Frame`]
/// or can be resolved immediately via [`layout_leaf`]. Checking only the first child is
/// enough per the box tree's own invariant (see [`first_block_child`]'s docs).
fn is_block_container(box_tree: &BoxTree, box_id: BoxId) -> bool {
    box_tree.get(box_id).is_some_and(|b| {
        b.children
            .first()
            .is_some_and(|&id| box_tree.get(id).is_some_and(|c| is_block_level(&c.kind)))
    })
}

/// Whether a box kind is block-level for the purposes of [`is_block_container`]/
/// [`first_block_child`]/[`last_block_child`] — mirrors `crate::box_tree`'s own (private)
/// `is_block_level`.
fn is_block_level(kind: &BoxKind) -> bool {
    matches!(kind, BoxKind::Block | BoxKind::AnonymousBlock)
}

/// Whether a box kind is [`BoxKind::AnonymousBlock`] — the one bit [`FragmentKind::Block`]
/// vs [`FragmentKind::AnonymousBlock`] turns on. Any other kind (in practice only reached
/// defensively — see [`run`]'s leaf-root fallback) maps to `Block`.
fn is_anonymous(kind: &BoxKind) -> bool {
    matches!(kind, BoxKind::AnonymousBlock)
}

/// A box's absolute content-box origin, given its border-box origin and resolved box model.
fn content_origin_of(border_box_origin: Point, model: &BoxModel) -> Point {
    Point {
        x: border_box_origin
            .x
            .saturating_add(model.padding.left)
            .saturating_add(model.border.left),
        y: border_box_origin
            .y
            .saturating_add(model.padding.top)
            .saturating_add(model.border.top),
    }
}

/// Resolves a `position: relative` offset (CSS 2.1 §10.6.4/§9.4.3): `left`/`top` win over
/// `right`/`bottom` when both are set on an axis; a percentage resolves against the
/// containing block's width (`left`/`right`) or height (`top`/`bottom`, `0` if indefinite).
fn resolve_offset(style: &LayoutStyle, cb_width: Au, cb_height: Option<Au>) -> (Au, Au) {
    let dx = match style.offset.left {
        Length::Auto => match style.offset.right {
            Length::Auto => Au::ZERO,
            right => Au::ZERO.saturating_sub(resolve_len_zero(right, cb_width)),
        },
        left => resolve_len_zero(left, cb_width),
    };
    let dy = match style.offset.top {
        Length::Auto => match style.offset.bottom {
            Length::Auto => Au::ZERO,
            bottom => Au::ZERO.saturating_sub(resolve_len_against_height(bottom, cb_height)),
        },
        top => resolve_len_against_height(top, cb_height),
    };
    (dx, dy)
}

/// Resolves a `<length>`/`<percentage>`/`auto` value against a width-shaped containing
/// block dimension, treating `auto` as `0` — the shared resolution every box-model
/// longhand except `height` uses (`width` itself is handled separately by
/// [`resolve_width_and_margins`], since its `auto` means "fill remaining space", not `0`).
fn resolve_len_zero(l: Length, against: Au) -> Au {
    match l {
        Length::Auto => Au::ZERO,
        Length::Px(au) => au,
        Length::Percent(p) => against.mul_by_f32(p / 100.0),
    }
}

/// [`resolve_len_zero`]'s height-shaped counterpart: a percentage resolves against `against`
/// only if it is `Some` (a definite containing-block height), else `0` — used for `top`/
/// `bottom` offsets, which (unlike `margin`/`padding`, always width-relative) resolve
/// against the containing block's height.
fn resolve_len_against_height(l: Length, against: Option<Au>) -> Au {
    match l {
        Length::Auto => Au::ZERO,
        Length::Px(au) => au,
        Length::Percent(p) => against.map_or(Au::ZERO, |h| h.mul_by_f32(p / 100.0)),
    }
}

/// Clamps `v` to `[min, max]` per CSS 2.1 §10.4/§10.7: `min` always wins when the two
/// conflict (a `max` below `min` is treated as if it were `min`).
fn clamp_au(v: Au, min: Au, max: Option<Au>) -> Au {
    let v = v.max(min);
    match max {
        Some(m) if m < min => v,
        Some(m) => v.min(m),
        None => v,
    }
}

/// Collapses two adjoining margins per CSS 2.1 §8.3.1 — a thin wrapper over
/// [`collapse_margin_set`] for the common two-margin case (sibling collapsing, which only
/// ever adjoins exactly one box's bottom margin with the next box's top margin).
fn collapse_margins(a: Au, b: Au) -> Au {
    collapse_margin_set(&[a, b])
}

/// Collapses a whole set of adjoining margins per CSS 2.1 §8.3.1's general rule: if every
/// margin in the set is non-negative, the result is the greatest of them; otherwise, the
/// largest positive value in the set plus the most negative (smallest) value in the set, so a
/// positive/negative mix partially cancels rather than one value dominating outright.
///
/// This must be computed over the *whole* set at once, not by folding [`collapse_margins`]
/// pairwise over the set one element at a time: pairwise folding re-clamps each intermediate
/// result, which can discard a more extreme value seen earlier. For example, folding
/// `[5, -3, -2]` pairwise gives `collapse(collapse(5, -3), -2) = collapse(2, -2) = 0`, but the
/// spec's actual answer (computed over the whole set: max positive `5`, min negative `-3`) is
/// `2` — the `-2` never gets to compete with the original `5` once it has already been
/// blended into an intermediate `2`. [`effective_margin_top`]/[`effective_margin_bottom`]'s
/// first/last-child chains rely on this being computed set-wise, not pairwise, to collapse
/// correctly through a chain of three or more nested margins.
///
/// An empty set collapses to `0` (never actually passed — [`effective_margin_top`]/
/// [`effective_margin_bottom`] always seed the set with the box's own margin — but kept total
/// rather than assuming a non-empty slice).
fn collapse_margin_set(margins: &[Au]) -> Au {
    let mut max_positive = Au::ZERO;
    let mut min_negative = Au::ZERO;
    for &m in margins {
        max_positive = max_positive.max(m.max(Au::ZERO));
        min_negative = min_negative.min(m.min(Au::ZERO));
    }
    max_positive.saturating_add(min_negative)
}

/// Insets `rect` by `sides` on every edge (used to go border box → padding box → content
/// box), clamping the resulting size at zero rather than going negative (a box whose
/// border+padding alone exceeds its own border-box size — e.g. a huge `border-width` on an
/// attacker-controlled document — must not produce a negative-size `Rect`).
fn inset(rect: Rect, sides: Sides<Au>) -> Rect {
    Rect {
        origin: Point {
            x: rect.origin.x.saturating_add(sides.left),
            y: rect.origin.y.saturating_add(sides.top),
        },
        size: Size {
            w: rect
                .size
                .w
                .saturating_sub(sides.left)
                .saturating_sub(sides.right)
                .max(Au::ZERO),
            h: rect
                .size
                .h
                .saturating_sub(sides.top)
                .saturating_sub(sides.bottom)
                .max(Au::ZERO),
        },
    }
}

/// Shifts every rect of `f` by `(dx, dy)` — [`finalize_top`]/[`layout_leaf`]'s
/// `position: relative` step.
fn shift_fragment(f: &mut Fragment, dx: Au, dy: Au) {
    shift_rect(&mut f.border_box, dx, dy);
    shift_rect(&mut f.padding_box, dx, dy);
    shift_rect(&mut f.content_box, dx, dy);
}

/// Shifts one rect's origin by `(dx, dy)`, saturating rather than overflowing.
fn shift_rect(r: &mut Rect, dx: Au, dy: Au) {
    r.origin.x = r.origin.x.saturating_add(dx);
    r.origin.y = r.origin.y.saturating_add(dy);
}

/// Appends a new fragment to `fragments` and returns its id.
fn push_fragment(fragments: &mut Vec<Fragment>, f: Fragment) -> FragmentId {
    let id = FragmentId::from_index(fragments.len());
    fragments.push(f);
    id
}

/// Appends a new style to `styles` and returns its id.
fn push_style(styles: &mut Vec<LayoutStyle>, s: LayoutStyle) -> StyleId {
    let id = StyleId::from_index(styles.len());
    styles.push(s);
    id
}
