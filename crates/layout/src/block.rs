//! The block formatting context: [`layout`] walks a [`BoxTree`] (Task 16) and produces a
//! positioned, sized [`FragmentTree`] (CSS 2.1 §10 — box dimensions — and §8.3.1 — margin
//! collapsing).
//!
//! # Inline content is laid out by `crate::inline`
//!
//! A block box whose box-tree children are all inline-level (`Inline`/`InlineText`/
//! `LineBreak` — see [`BoxTree`]'s invariants) has its content whitespace-processed, shaped,
//! line-broken and aligned by `crate::inline::layout`, which appends the resulting `Line`
//! fragments (with their `Text` children) and reports the total height they contribute. This
//! module only decides *where* that content box is and how tall the block ends up; everything
//! between a text node's characters and a positioned glyph belongs to `crate::inline`,
//! [`crate::text`] and `crate::whitespace`. A block with no children at all short-circuits
//! to content height `0` without consulting them (`layout_leaf`'s `children.is_empty()`
//! branch), and so, in effect, does a block whose content is nothing but collapsible white
//! space: that processes to an empty string, which generates no line box at all.
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
//! chain has nothing separating it from its own next child (no top/bottom padding or border,
//! and `overflow: visible` — a box with any other `overflow` establishes a new block formatting
//! context per CSS 2.1 §9.4.1, and §8.3.1's "adjoining" requires both margins to live in the
//! same one, so the chain stops *at* such a box: its own margin still adjoins whatever is
//! outside it, but never its children's. See `establishes_bfc`; for the bottom case,
//! additionally `height: auto` and `min-height: 0` — a definite height or a positive
//! `min-height` gives the box's content a floor a collapsed-through margin would silently
//! violate), then combines the *whole* collected set at once via
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
//! - **CSS 2.1 §10.3.3's over-constrained fixup** ("ignore `margin-right` in a
//!   left-to-right containing block") — `resolve_width_and_margins` uses both specified
//!   margins verbatim instead, and floors a negative both-`auto` remainder at zero rather than
//!   splitting it. Both are recorded as deviations in `docs/SPEC_REGISTRY.md`.
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
use crate::geom::{BoxSizing, LayoutStyle, Length, Overflow, Point, Position, Rect, Sides, Size};
use crate::inline::{self, InlineArgs, InlineContext};
use crate::text::TextShaper;

/// Lays out `doc`'s box tree against `viewport`, producing an absolutely-positioned
/// [`FragmentTree`].
///
/// `fonts` is the bundled font database every glyph is shaped from ([`crate::text`]): layout
/// borrows it for the duration of one pass and reads nothing else from the outside world, so
/// the same `doc` and `viewport` always produce the same tree.
///
/// Total over any [`StyledDocument`]: never panics, however deeply nested, however extreme
/// the CSS values (`Au`'s saturating arithmetic — see [`crate::au`] — absorbs overflow from a
/// document adversarially chosen to make it, e.g., a `1e9px` margin ten levels deep) and
/// however hostile its text (an absurd `font-size`, a multi-megabyte text node and a
/// zero-width containing block are all clamped or absorbed, never fatal).
///
/// # Errors
/// [`LayoutError::FontNotBundled`] if text was shaped with a face that is not one of the
/// bundled ones — see that variant's docs for why it is unreachable in practice and why it is
/// reported rather than swallowed.
pub fn layout(
    doc: &StyledDocument,
    viewport: Viewport,
    fonts: &mut FontDb,
) -> Result<FragmentTree, LayoutError> {
    let box_tree = build(doc);
    let mut shaper = TextShaper::new(fonts);
    let mut ctx = Ctx {
        inline: InlineContext {
            box_tree: &box_tree,
            document: doc.document(),
            shaper: &mut shaper,
        },
        error: None,
    };
    let tree = run(&mut ctx, viewport);
    match ctx.error {
        Some(error) => Err(error),
        None => Ok(tree),
    }
}

/// Everything [`run`]'s walk needs besides the fragment arenas: the trees it reads, the
/// shaper it hands inline content to, and the first error that shaper reported.
///
/// The error is collected rather than propagated because the walk is an explicit stack
/// machine ([`Frame`]) whose every step would otherwise have to be `Result`-returning for a
/// case that cannot happen (see [`LayoutError::FontNotBundled`]); [`layout`] turns a
/// collected error into the `Err` its caller sees, and the partially built tree is discarded.
struct Ctx<'a> {
    inline: InlineContext<'a>,
    error: Option<LayoutError>,
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
    /// margin (CSS 2.1 §8.3.1: no padding, no border, nothing else separating them, and this
    /// box does not establish a block formatting context — see [`establishes_bfc`]).
    /// Hardcoded `false` for the document root regardless of its own padding/border — see
    /// the module docs' "Margins never collapse through the document root" section.
    first_child_collapse_eligible: bool,
    /// Whether this box's bottom margin collapses with its last block-level child's bottom
    /// margin (CSS 2.1 §8.3.1: `height: auto`, `min-height: 0`, no padding, no border, and not
    /// a block-formatting-context root). A positive `min-height` matters here for the same
    /// reason it does in [`effective_margin_bottom`]: it gives this box's content a floor that
    /// a margin collapsed out through the bottom edge would silently violate, so the trailing
    /// margin stays *inside* this box's own content height instead of being dropped on both
    /// sides.
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
/// Per-[`BoxId`] memo for [`effective_margin_top`]/[`effective_margin_bottom`], so the total
/// work `layout()` spends walking first/last-child margin-collapsing chains across the whole
/// document is `O(n)`, not `O(n²)` for an `n`-deep single-child chain (`<div><div><div>…`) —
/// see [`effective_margin_top`]'s docs for the complexity argument and why memoizing by
/// `BoxId` alone (no other key) is sound.
struct MarginMemo {
    top: Vec<Option<Au>>,
    bottom: Vec<Option<Au>>,
}

impl MarginMemo {
    /// Builds an empty memo sized for a box tree of `len` boxes (`BoxTree::len`).
    fn new(len: usize) -> MarginMemo {
        MarginMemo {
            top: vec![None; len],
            bottom: vec![None; len],
        }
    }
}

fn run(ctx: &mut Ctx<'_>, viewport: Viewport) -> FragmentTree {
    let box_tree = ctx.inline.box_tree;
    let mut fragments: Vec<Fragment> = Vec::new();
    let mut styles: Vec<LayoutStyle> = Vec::new();
    let mut memo = MarginMemo::new(box_tree.len());

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
                advance_child(ctx, &mut stack, &mut fragments, &mut styles, &mut memo);
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
        let (root, _height) = layout_leaf(ctx, &leaf, &mut fragments, &mut styles);
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
    memo: &mut MarginMemo,
) -> ChildPlacement {
    let model = resolve_box_model(style, parent.content_width, parent.cb_height);
    let is_bfc_root = establishes_bfc(style);
    let eff_top = effective_margin_top(
        box_tree,
        child_box_id,
        &OwnTopMargin {
            margin_top: model.margin_top,
            padding_top: model.padding.top,
            border_top: model.border.top,
            is_bfc_root,
        },
        model.content_width,
        &mut memo.top,
    );
    let eff_bottom = effective_margin_bottom(
        box_tree,
        child_box_id,
        &OwnBottomMargin {
            margin_bottom: model.margin_bottom,
            padding_bottom: model.padding.bottom,
            border_bottom: model.border.bottom,
            known_height_is_none: model.known_height.is_none(),
            min_height: model.min_height,
            is_bfc_root,
        },
        model.content_width,
        &mut memo.bottom,
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
    ctx: &mut Ctx<'_>,
    stack: &mut Vec<Frame>,
    fragments: &mut Vec<Fragment>,
    styles: &mut Vec<LayoutStyle>,
    memo: &mut MarginMemo,
) {
    let box_tree = ctx.inline.box_tree;
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
    let placement = place_child(box_tree, child_box_id, &child.style, &parent, memo);

    if is_block_container(box_tree, child_box_id) {
        push_container_frame(stack, fragments.len(), child, &placement);
    } else {
        resolve_leaf_child(ctx, stack, fragments, styles, child, &placement);
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
    let is_bfc_root = establishes_bfc(&child.style);
    let first_child_collapse_eligible = !is_bfc_root
        && placement.model.padding.top == Au::ZERO
        && placement.model.border.top == Au::ZERO;
    let bottom_collapse_eligible = !is_bfc_root
        && placement.model.known_height.is_none()
        && placement.model.min_height == Au::ZERO
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
    ctx: &mut Ctx<'_>,
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
    let (frag_id, used_height) = layout_leaf(ctx, &leaf, fragments, styles);
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
    ctx: &mut Ctx<'_>,
    args: &LeafArgs<'_>,
    fragments: &mut Vec<Fragment>,
    styles: &mut Vec<LayoutStyle>,
) -> (FragmentId, Au) {
    let frag_start = fragments.len();
    let content_origin = content_origin_of(args.origin, args.model);

    let (child_frag_ids, content_height_from_inline) = if args.children.is_empty() {
        (Vec::new(), Au::ZERO)
    } else {
        let inline_args = InlineArgs {
            children: args.children,
            container_style: args.style,
            content_origin,
            content_width: args.model.content_width,
        };
        match inline::layout(&mut ctx.inline, &inline_args, fragments, styles) {
            Ok(result) => result,
            Err(error) => {
                // Collected, not propagated — see `Ctx`'s docs. The block still gets a
                // fragment (with no inline content) so the walk's invariants hold until
                // `layout` discards the whole tree.
                ctx.error.get_or_insert(error);
                (Vec::new(), Au::ZERO)
            }
        }
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

    let v_non_content = vertical_non_content(padding, border);
    let min_height = used_content_size(
        resolve_len_zero(style.min_height, cb_height.unwrap_or(Au::ZERO)),
        style.box_sizing,
        v_non_content,
    );
    let max_height = style.max_height.map(|l| {
        used_content_size(
            resolve_len_zero(l, cb_height.unwrap_or(Au::ZERO)),
            style.box_sizing,
            v_non_content,
        )
    });
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
/// equation) then §10.4 (min/max-width clamp, with the whole equation re-run against the
/// clamped width whenever the clamp binds).
///
/// §10.4's clamp applies to an `auto` width exactly as it does to a specified one: the
/// tentative used width is computed first (for `auto`, "fill what the margins leave"), then
/// clamped by `max-width` and `min-width`. If — and only if — that clamp *changed* the value,
/// the section's "the rules above are applied again, but this time using the [clamped] value as
/// the computed value for `width`" kicks in: the width is now definite, so `auto` margins stop
/// being forced to zero and get the free space to split (centering). A clamp that did not bind
/// leaves an `auto` width behaving as `auto` throughout, autos-become-zero included.
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

    let width_is_auto = matches!(style.width, Length::Auto);
    let tentative = if width_is_auto {
        // CSS 2.1 §10.3.3, "otherwise, if 'width' is set to 'auto' …": any other 'auto'
        // value (i.e. an auto margin) becomes 0, and width fills the rest — no centering.
        cb_width
            .saturating_sub(margin_left0)
            .saturating_sub(non_content)
            .saturating_sub(margin_right0)
            .max(Au::ZERO)
    } else {
        used_content_size(
            resolve_len_zero(style.width, cb_width),
            style.box_sizing,
            non_content,
        )
    };
    let min_width = used_content_size(
        resolve_len_zero(style.min_width, cb_width),
        style.box_sizing,
        non_content,
    );
    let max_width = style
        .max_width
        .map(|l| used_content_size(resolve_len_zero(l, cb_width), style.box_sizing, non_content));
    let content_width = clamp_au(tentative, min_width, max_width);

    if width_is_auto && content_width == tentative {
        return (content_width, margin_left0, margin_right0);
    }

    let non_margin_total = non_content.saturating_add(content_width);
    let (margin_left, margin_right) = if margin_left_auto && margin_right_auto {
        let remaining = cb_width.saturating_sub(non_margin_total);
        if remaining.0 >= 0 {
            let half = remaining.0 / 2;
            (Au(half), Au(remaining.0 - half))
        } else {
            // CSS 2.1 §10.3.3: "If both 'margin-left' and 'margin-right' are 'auto', their
            // used values are equal" — and §10.4's re-run can make the box *wider* than its
            // containing block (a bound `min-width`, or a `width` larger than `cb_width`),
            // leaving a negative remainder to share. Halving it would be equal but would pull
            // the box left, out of its containing block on the side the writing mode starts
            // from; CSS 2.1 has no fixup for this case (§10.3.3's equality is stated for the
            // ordinary, non-negative one), so both used margins are floored at zero and the
            // box overflows to the right only. Documented as a deviation in
            // `docs/SPEC_REGISTRY.md`.
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
    specified.map(|h| used_content_size(h, style.box_sizing, vertical_non_content(padding, border)))
}

/// A box's border+padding along the block axis — the part `box-sizing: border-box` carves out
/// of a specified `height`/`min-height`/`max-height`.
fn vertical_non_content(padding: Sides<Au>, border: Sides<Au>) -> Au {
    padding
        .top
        .saturating_add(padding.bottom)
        .saturating_add(border.top)
        .saturating_add(border.bottom)
}

/// Turns one specified box dimension into the *content*-box value layout works in, applying
/// `box-sizing`: `border-box` means the specified length describes the border box, so this
/// box's own border+padding along that axis (`non_content`) is carved out of it (floored at
/// zero — a border alone can exceed the specified size); `content-box` passes it through.
///
/// Applies to `min-width`/`max-width`/`min-height`/`max-height` exactly as it does to
/// `width`/`height`: css-sizing-3 §6.2 defines `box-sizing` over "the box's size properties",
/// which is all six of them, not just the two definite ones. Clamping a content-box width
/// against a raw border-box bound instead is what made `box-sizing: border-box; width: 200px;
/// min-width: 180px; padding: 20px` inflate its border box to 220px.
fn used_content_size(specified: Au, box_sizing: BoxSizing, non_content: Au) -> Au {
    match box_sizing {
        BoxSizing::ContentBox => specified,
        BoxSizing::BorderBox => specified.saturating_sub(non_content).max(Au::ZERO),
    }
}

/// The used gap contributed above a box for margin-collapsing purposes (CSS 2.1 §8.3.1): its
/// own top margin, collapsed together with the top margins of its *whole* first-in-flow-
/// block-child chain — its first block-level child, that child's first block-level child, and
/// so on — for as long as each box in the chain has no top padding and no top border (nothing
/// separating it from its own first child). See the module docs' "Margin collapsing" section:
/// this walks the chain with a loop, not recursion, and combines every margin in the chain at
/// once via [`memoize_suffix_collapse`] (not pairwise — see [`collapse_margin_set`]'s docs for
/// why pairwise would give a wrong answer for a chain of three or more mixed-sign margins).
///
/// A box further down the chain's own containing block is the previous box's *content* width,
/// not `content_width` (the box named by `box_id`'s containing block) — so each step resolves
/// that child's box model (for its margin, and for its own content width/padding/border, to
/// decide whether the chain continues and what the *next* step's containing block is) via
/// [`resolve_box_model`], the same computation that child will get "for real" once the main
/// layout walk reaches it as a [`Frame`]/leaf. Recomputing it here is bounded by the chain's
/// length (itself bounded by document depth, like every other walk in this crate) and mutates
/// nothing but `memo`.
///
/// # Complexity: `O(n)` over the whole document, not `O(n)` per box
///
/// A naive version of this function (walk the chain, don't remember anything) costs `O(k)`
/// for a box `k` links from the end of its chain — call [`place_child`] on every box of an
/// `n`-deep single-child chain (`<div><div><div>…`, the shape this crate's whole no-recursion
/// design exists to defend against) and the *total* cost is `k=1 + 2 + … + n = O(n²)`, exactly
/// the blow-up a hostile document could trigger.
///
/// `memo` fixes this: every box has exactly one parent, so it can appear as a *descendant* in
/// at most one first-child chain-walk (the one started by its own parent, if that parent is
/// itself eligible and this box is its first child) — never two. So the walk below, on
/// discovering a chain `[box_id, child, grandchild, …]`, does not just return `box_id`'s own
/// answer: it hands the *whole chain* to [`memoize_suffix_collapse`], which fills in `memo`
/// for every box in it at once, in one backward pass over the chain (`O(chain length)`, not
/// `O(chain length²)` — see that function's docs for why a backward accumulation, not
/// `n` separate calls to [`collapse_margin_set`], is what makes that pass linear). By the time
/// the real layout walk (`advance_child`) later reaches `child`, `grandchild`, etc. as their
/// own `place_child` calls, each is a memo hit — an `O(1)` lookup, not a re-walk. Every edge
/// of the box tree is therefore walked by at most one chain-walk, total work `O(n)` over the
/// whole document.
///
/// This holds only because `run`'s traversal always finalizes a box's *own* `place_child` call
/// (and therefore this function, for that box) strictly before descending into any of that
/// box's children's own `place_child` calls — see `advance_child`'s docs — so no descendant
/// can already be memoized by an unrelated walk when this function is building a fresh chain;
/// the `memo.get(child_id...).is_some()` check inside the loop is a defensive stop (correctness
/// net, not a normal code path) for exactly that invariant, not something this proof depends
/// on holding by luck.
///
/// # Memo safety: keyed by `BoxId` alone, no other input
///
/// Percent margins resolve against a containing block *width*, and this function's own
/// `content_width` parameter (as well as each subsequent step's, derived from it) is *always*
/// the same value for a given `box_id`, regardless of which call path computed it: content
/// width resolution is purely top-down — a function of a box's own style and its ancestors'
/// styles alone, computed identically by [`resolve_box_model`] whether that call came from an
/// ancestor's speculative chain-walk (as here) or from that box's own, later, "real"
/// [`place_child`] call. There is no scenario where the same `box_id` legitimately needs two
/// different `content_width`s within one [`layout`] call, so memoizing by `BoxId` alone (no
/// width in the key) cannot serve a stale answer.
fn effective_margin_top(
    box_tree: &BoxTree,
    box_id: BoxId,
    own: &OwnTopMargin,
    content_width: Au,
    memo: &mut [Option<Au>],
) -> Au {
    let own_margin_top = own.margin_top;
    if let Some(cached) = memo.get(box_id.index()).copied().flatten() {
        return cached;
    }

    let mut chain: Vec<(BoxId, Au)> = vec![(box_id, own_margin_top)];
    let mut eligible =
        !own.is_bfc_root && own.padding_top == Au::ZERO && own.border_top == Au::ZERO;
    let mut current = box_id;
    let mut current_content_width = content_width;
    let mut remaining = box_tree.len();

    while eligible && remaining > 0 {
        remaining -= 1;
        let Some(child_id) = first_block_child(box_tree, current) else {
            break;
        };
        // See this function's "Complexity" docs: unreachable in correct operation (a
        // descendant cannot already be memoized while its ancestor's own chain-walk is
        // still in progress), kept as a defensive stop rather than an assumption.
        if memo.get(child_id.index()).copied().flatten().is_some() {
            break;
        }
        let Some(child) = box_tree.get(child_id) else {
            break;
        };
        let child_model = resolve_box_model(&child.style, current_content_width, None);
        chain.push((child_id, child_model.margin_top));
        eligible = !establishes_bfc(&child.style)
            && child_model.padding.top == Au::ZERO
            && child_model.border.top == Au::ZERO;
        current_content_width = child_model.content_width;
        current = child_id;
    }

    memoize_suffix_collapse(&chain, memo);
    memo.get(box_id.index())
        .copied()
        .flatten()
        .unwrap_or(own_margin_top)
}

/// [`effective_margin_top`]'s inputs describing `box_id`'s *own* box model and style —
/// bundled into one value (rather than four more scalar parameters) to keep that function's
/// argument count under `clippy::too_many_arguments`' threshold, mirroring
/// [`OwnBottomMargin`].
struct OwnTopMargin {
    margin_top: Au,
    padding_top: Au,
    border_top: Au,
    /// Whether this box establishes a block formatting context ([`establishes_bfc`]): if it
    /// does, margins never collapse between it and its own children (CSS 2.1 §9.4.1/§8.3.1),
    /// so the chain never descends past it — but its own margin still adjoins whatever is
    /// outside it, so it still seeds the chain.
    is_bfc_root: bool,
}

/// [`effective_margin_bottom`]'s inputs describing `box_id`'s *own* box model — bundled into
/// one value (rather than five more scalar parameters) to keep that function's argument count
/// under `clippy::too_many_arguments`' threshold once the `memo` parameter is added.
struct OwnBottomMargin {
    margin_bottom: Au,
    padding_bottom: Au,
    border_bottom: Au,
    known_height_is_none: bool,
    min_height: Au,
    /// The bottom-side mirror of [`OwnTopMargin::is_bfc_root`].
    is_bfc_root: bool,
}

/// The bottom-margin mirror of [`effective_margin_top`], walking the *last*-in-flow-block-
/// child chain instead. CSS 2.1 §8.3.1's bottom-margin case additionally requires each box in
/// the chain to have a definite `height` of `auto` and `min-height` of `0` (a definite height
/// or a positive `min-height` gives the box's content a floor that a collapsed-through margin
/// would silently violate) — checked at every step, not just the first, via each step's own
/// [`resolve_box_model`] result exactly as [`effective_margin_top`] re-derives padding/border.
/// See that function's docs for the complexity/memo-safety argument, which applies here
/// unchanged (with one addition: every step along the way is only reached at all because the
/// *previous* step's height was `auto`, i.e. indefinite — CSS 2.1 §10.5's "a percentage height
/// against an indefinite containing block computes as auto" rule — so passing `None` as
/// [`resolve_box_model`]'s `cb_height` at every step, rather than threading a real height
/// down, is not an approximation: it is what every step in an eligible chain always resolves
/// to anyway).
fn effective_margin_bottom(
    box_tree: &BoxTree,
    box_id: BoxId,
    own: &OwnBottomMargin,
    content_width: Au,
    memo: &mut [Option<Au>],
) -> Au {
    let own_margin_bottom = own.margin_bottom;
    if let Some(cached) = memo.get(box_id.index()).copied().flatten() {
        return cached;
    }

    let mut chain: Vec<(BoxId, Au)> = vec![(box_id, own_margin_bottom)];
    let mut eligible = !own.is_bfc_root
        && own.known_height_is_none
        && own.min_height == Au::ZERO
        && own.padding_bottom == Au::ZERO
        && own.border_bottom == Au::ZERO;
    let mut current = box_id;
    let mut current_content_width = content_width;
    let mut remaining = box_tree.len();

    while eligible && remaining > 0 {
        remaining -= 1;
        let Some(child_id) = last_block_child(box_tree, current) else {
            break;
        };
        // See `effective_margin_top`'s "Complexity" docs for why this is unreachable in
        // correct operation and only a defensive stop.
        if memo.get(child_id.index()).copied().flatten().is_some() {
            break;
        }
        let Some(child) = box_tree.get(child_id) else {
            break;
        };
        let child_model = resolve_box_model(&child.style, current_content_width, None);
        chain.push((child_id, child_model.margin_bottom));
        eligible = !establishes_bfc(&child.style)
            && child_model.known_height.is_none()
            && child_model.min_height == Au::ZERO
            && child_model.padding.bottom == Au::ZERO
            && child_model.border.bottom == Au::ZERO;
        current_content_width = child_model.content_width;
        current = child_id;
    }

    memoize_suffix_collapse(&chain, memo);
    memo.get(box_id.index())
        .copied()
        .flatten()
        .unwrap_or(own_margin_bottom)
}

/// Fills `memo[id]` for every `(BoxId, margin)` pair in `chain`, with the collapsed value of
/// *that box's own suffix* of the chain (its own margin combined with every margin after it in
/// `chain`) — i.e. `memo[chain[i].0]` ends up equal to what
/// `collapse_margin_set(&chain[i..].map(|(_, m)| m))` would compute, for every `i`, but
/// without `collapse_margin_set`'s `O(len)` cost paid once per suffix (which would make the
/// whole pass `O(len²)` again, defeating the point).
///
/// Instead, this walks `chain` once, back to front, keeping a running `(max_positive,
/// min_negative)` accumulator: processing the *last* element seeds it with a length-1 suffix;
/// each element processed after that folds one more margin *in* to the running accumulator
/// (never resets it), so by the time element `i` is processed, the accumulator already
/// reflects every margin from `i` to the end — exactly the suffix collapse for `i` — computed
/// in `O(1)` additional work per element, `O(len)` total.
fn memoize_suffix_collapse(chain: &[(BoxId, Au)], memo: &mut [Option<Au>]) {
    let mut max_positive = Au::ZERO;
    let mut min_negative = Au::ZERO;
    for &(id, margin) in chain.iter().rev() {
        max_positive = max_positive.max(margin.max(Au::ZERO));
        min_negative = min_negative.min(margin.min(Au::ZERO));
        if let Some(slot) = memo.get_mut(id.index()) {
            *slot = Some(max_positive.saturating_add(min_negative));
        }
    }
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

/// Whether a box with this style establishes a new block formatting context — in M1a's CSS
/// scope, exactly "`overflow` is not `visible`" (CSS 2.1 §9.4.1: "Floats, absolutely
/// positioned elements, block containers … that have `overflow` other than `visible`
/// establish new block formatting contexts"; M1a implements no floats, no
/// `display: inline-block`/`table-cell`, and folds `position: absolute`/`fixed` onto `static`,
/// so `overflow` is the only trigger that can occur). A BFC root's margins never collapse with
/// its own in-flow children's (§8.3.1's "adjoining" requires the two boxes to be in the same
/// block formatting context), which is what [`OwnTopMargin::is_bfc_root`] and the two `Frame`
/// eligibility flags consult; its *own* margins still adjoin its siblings' and its parent's,
/// since those live outside the context it establishes.
fn establishes_bfc(style: &LayoutStyle) -> bool {
    style.overflow != Overflow::Visible
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
/// conflict — a `max` below `min` is treated as if it were `min` itself, so the result is
/// exactly `min` whenever `v >= min` but the (raised) `max` is also `min`, not `v` left
/// unclamped. Fold `max` up to `min` *before* clamping `v`, not after: clamping `v` to `min`
/// first and only then checking whether `max < min` (the previous, buggy shape of this
/// function) leaves an oversized `v` unclamped whenever `v` already exceeds `min`, since the
/// `max < min` branch returned `v` verbatim instead of the folded `max` (i.e. `min`).
fn clamp_au(v: Au, min: Au, max: Option<Au>) -> Au {
    let effective_max = max.map(|m| m.max(min));
    let v = v.max(min);
    match effective_max {
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
