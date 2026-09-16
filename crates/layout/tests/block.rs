//! Failing-tests-first coverage for [`cl_layout::block::layout`]'s block formatting context:
//! width/margin resolution (CSS 2.1 §10.3.3/§10.4), height resolution (§10.5/§10.7), vertical
//! margin collapsing (§8.3.1), `position: relative` (§10.6.4), and that a `display: none`
//! subtree (already absent from the box tree — Task 16) contributes no layout at all.
//!
//! Every fixture resets `body { margin: 0 }` so the arithmetic in each test's doc comment
//! starts from a clean `(0, 0)` origin at `<body>`'s content box — see
//! `crates/layout/tests/fragment_tree_goldens.rs` for coverage of the *real* UA-sheet
//! geometry (the 8px `<body>` margin, `<p>` margins colliding between siblings, …) this file
//! deliberately avoids so each test isolates one behaviour.
#![allow(
    clippy::expect_used,
    reason = "a failed setup step in a test should abort that test, loudly"
)]

#[path = "common/mod.rs"]
mod common;

use cl_dom::{Document, NodeId, local_name};
use cl_layout::{Au, Fragment, FragmentTree, Point, Rect, Size};

/// The first element in `doc` whose `id` attribute is `id`.
fn find_by_id(doc: &Document, id: &str) -> NodeId {
    common::find_element(doc, |el| {
        el.attrs
            .iter()
            .any(|a| a.name.local == local_name!("id") && &*a.value == id)
    })
    .expect("fixture must have an element with the expected id")
}

/// The fragment generated for the element with `id`, found by walking `tree` from its root
/// (an explicit stack, not recursion, matching the rest of this crate's traversals).
fn fragment_by_id<'a>(tree: &'a FragmentTree, doc: &Document, id: &str) -> &'a Fragment {
    let node = find_by_id(doc, id);
    let mut stack = vec![tree.root];
    let mut found = None;
    while let Some(fid) = stack.pop() {
        let Some(f) = tree.get(fid) else {
            continue;
        };
        if f.node == Some(node) {
            found = Some(f);
            break;
        }
        stack.extend(f.children.iter().copied());
    }
    found.expect("fragment tree must contain a fragment for the element with the expected id")
}

fn px(x: f32) -> Au {
    Au::from_px(x)
}

fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect {
        origin: Point { x: px(x), y: px(y) },
        size: Size { w: px(w), h: px(h) },
    }
}

/// The wall-clock budget for [`deep_first_child_chain_should_lay_out_in_linear_time`]: 5s
/// normally, 15s when `CL_SMOKE_SLOW=1` is set — the same escape hatch the plan's other smoke
/// tests (`docs/superpowers/plans/2026-09-07-m1a-static-pipeline.md`) use for a slow or loaded
/// CI runner.
#[allow(
    clippy::disallowed_methods,
    reason = "`std::env::var` is restricted to `cl-platform` in production code so env access \
              stays centralized/testable there; this is a test-only smoke-test escape hatch \
              matching the plan's own documented convention for this exact env var, not \
              production configuration"
)]
fn smoke_time_limit() -> std::time::Duration {
    if std::env::var("CL_SMOKE_SLOW").as_deref() == Ok("1") {
        std::time::Duration::from_secs(15)
    } else {
        std::time::Duration::from_secs(5)
    }
}

/// `width: auto` on a child of an 800px-wide containing block fills it exactly (CSS 2.1
/// §10.3.3): a 10px-tall, no-margin `<div>` becomes an 800×10 border box at the origin.
#[test]
fn auto_width_should_fill_containing_block() {
    let html = r#"
        <style>body { margin: 0 } #target { height: 10px }</style>
        <body><div id="target"></div></body>
    "#;
    let (tree, styled) = common::layout_html(html);
    let target = fragment_by_id(&tree, styled.document(), "target");
    assert_eq!(target.border_box, rect(0.0, 0.0, 800.0, 10.0));
    assert_eq!(target.content_box, rect(0.0, 0.0, 800.0, 10.0));
}

/// A definite `width` with `margin-left`/`margin-right: auto` centers the box in its
/// containing block (CSS 2.1 §10.3.3): 400px in an 800px containing block leaves 400px to
/// split, 200px each side.
#[test]
fn fixed_width_with_auto_margins_should_center() {
    let html = r#"
        <style>
            body { margin: 0 }
            #target { width: 400px; height: 10px; margin-left: auto; margin-right: auto }
        </style>
        <body><div id="target"></div></body>
    "#;
    let (tree, styled) = common::layout_html(html);
    let target = fragment_by_id(&tree, styled.document(), "target");
    assert_eq!(target.border_box, rect(200.0, 0.0, 400.0, 10.0));
}

/// A percentage `width` resolves against the containing block's *content* box width, not the
/// viewport: 50% of a 500px `#outer` is 250px, regardless of the 800px viewport around it.
#[test]
fn percent_width_should_resolve_against_parent_content_box() {
    let html = r#"
        <style>
            body { margin: 0 }
            #outer { width: 500px }
            #target { width: 50%; height: 10px }
        </style>
        <body><div id="outer"><div id="target"></div></div></body>
    "#;
    let (tree, styled) = common::layout_html(html);
    let target = fragment_by_id(&tree, styled.document(), "target");
    assert_eq!(target.border_box, rect(0.0, 0.0, 250.0, 10.0));
}

/// `box-sizing: border-box` makes a specified `width`/`height` describe the *border* box:
/// 300×100 with 10px padding and a 5px border all round still produces a 300×100 border box,
/// with a 270×70 content box inset by border+padding (15px each side).
#[test]
fn border_box_sizing_should_include_padding_and_border() {
    let html = r#"
        <style>
            body { margin: 0 }
            #target {
                width: 300px; height: 100px; padding: 10px; border: 5px solid black;
                box-sizing: border-box;
            }
        </style>
        <body><div id="target"></div></body>
    "#;
    let (tree, styled) = common::layout_html(html);
    let target = fragment_by_id(&tree, styled.document(), "target");
    assert_eq!(target.border_box, rect(0.0, 0.0, 300.0, 100.0));
    assert_eq!(target.content_box, rect(15.0, 15.0, 270.0, 70.0));
}

/// `min-width` clamps upward *after* a percentage resolves (CSS 2.1 §10.4): 10% of a 200px
/// containing block is 20px, but `min-width: 100px` forces the used width to 100px.
#[test]
fn min_width_should_clamp_percent_result() {
    let html = r#"
        <style>
            body { margin: 0 }
            #outer { width: 200px }
            #target { width: 10%; min-width: 100px; height: 10px }
        </style>
        <body><div id="outer"><div id="target"></div></div></body>
    "#;
    let (tree, styled) = common::layout_html(html);
    let target = fragment_by_id(&tree, styled.document(), "target");
    assert_eq!(target.border_box, rect(0.0, 0.0, 100.0, 10.0));
}

/// When `min-width` exceeds `max-width`, CSS 2.1 §10.4 treats `max-width` as if it equalled
/// `min-width` — so the used width is `min-width`, not `max-width` (which is lower) and not
/// the unclamped specified `width` (which is higher). This is the exact regression a bug in
/// `clamp_au` produced: clamping to `min` first and only then checking `max < min` left an
/// oversized `v` unclamped whenever it already exceeded `min`, returning `300px` instead of
/// the correct `200px` here.
#[test]
fn min_width_greater_than_max_width_should_use_min() {
    let html = r#"
        <style>
            body { margin: 0 }
            #target { width: 300px; min-width: 200px; max-width: 50px; height: 10px }
        </style>
        <body><div id="target"></div></body>
    "#;
    let (tree, styled) = common::layout_html(html);
    let target = fragment_by_id(&tree, styled.document(), "target");
    assert_eq!(target.border_box, rect(0.0, 0.0, 200.0, 10.0));
}

/// The height mirror of [`min_width_greater_than_max_width_should_use_min`] (CSS 2.1 §10.7):
/// `min-height: 200px` exceeding `max-height: 50px` means the used height is `200px`, not the
/// unclamped specified `300px`.
#[test]
fn min_height_greater_than_max_height_should_use_min() {
    let html = r#"
        <style>
            body { margin: 0 }
            #target { width: 100px; height: 300px; min-height: 200px; max-height: 50px }
        </style>
        <body><div id="target"></div></body>
    "#;
    let (tree, styled) = common::layout_html(html);
    let target = fragment_by_id(&tree, styled.document(), "target");
    assert_eq!(target.border_box, rect(0.0, 0.0, 100.0, 200.0));
}

/// `max-height` clamps an `auto`-computed content height (CSS 2.1 §10.7): two 30px children
/// stacked with no margin produce 60px of auto content height, but `max-height: 20px` caps
/// the container's own border box at 20px tall (the children themselves keep their own,
/// unclamped, 30px height — clamping the parent does not shrink them).
#[test]
fn max_height_should_clamp_auto_height() {
    let html = r#"
        <style>
            body { margin: 0 }
            #target { width: 200px; max-height: 20px }
            #target > div { height: 30px; margin: 0 }
        </style>
        <body><div id="target"><div></div><div></div></div></body>
    "#;
    let (tree, styled) = common::layout_html(html);
    let target = fragment_by_id(&tree, styled.document(), "target");
    assert_eq!(target.border_box, rect(0.0, 0.0, 200.0, 20.0));
    assert_eq!(target.content_box, rect(0.0, 0.0, 200.0, 20.0));
}

/// Two sibling blocks' vertical margins collapse to the greater of the two (CSS 2.1 §8.3.1),
/// not their sum: a 20px `margin-bottom` meeting a 10px `margin-top` leaves a 20px gap, so
/// the second box's border box starts at `10 (first box's height) + 20 (collapsed margin)`.
#[test]
fn sibling_vertical_margins_should_collapse_to_max() {
    let html = r#"
        <style>
            body { margin: 0 }
            #first { height: 10px; margin-bottom: 20px }
            #second { height: 10px; margin-top: 10px }
        </style>
        <body><div id="first"></div><div id="second"></div></body>
    "#;
    let (tree, styled) = common::layout_html(html);
    let first = fragment_by_id(&tree, styled.document(), "first");
    let second = fragment_by_id(&tree, styled.document(), "second");
    assert_eq!(
        first.border_box.origin,
        Point {
            x: px(0.0),
            y: px(0.0)
        }
    );
    assert_eq!(
        second.border_box.origin,
        Point {
            x: px(0.0),
            y: px(30.0)
        }
    );
}

/// A container's top margin collapses with its first block-level child's top margin when
/// nothing (no padding, no border) separates them (CSS 2.1 §8.3.1): `#parent` has no margin
/// of its own, but its first child's `margin-top: 20px` becomes `#parent`'s own *effective*
/// top margin, so `#parent` — not just its child — ends up pushed 20px below its preceding
/// sibling `#before`, with the child sitting flush at `#parent`'s own top (no *additional*
/// internal gap).
#[test]
fn parent_and_first_child_top_margins_should_collapse() {
    let html = r#"
        <style>
            body { margin: 0 }
            #before { height: 10px }
            #parent { width: 200px }
            #parent > div { height: 10px; margin-top: 20px }
        </style>
        <body><div id="before"></div><div id="parent"><div id="child"></div></div></body>
    "#;
    let (tree, styled) = common::layout_html(html);
    let parent = fragment_by_id(&tree, styled.document(), "parent");
    let child = fragment_by_id(&tree, styled.document(), "child");
    assert_eq!(
        parent.border_box.origin,
        Point {
            x: px(0.0),
            y: px(30.0)
        }
    );
    assert_eq!(
        child.border_box.origin,
        Point {
            x: px(0.0),
            y: px(30.0)
        }
    );
}

/// Padding breaks the parent/first-child margin collapse (CSS 2.1 §8.3.1: only "nothing"
/// between them collapses): with `padding-top: 5px` on `#parent`, its first child's
/// `margin-top: 20px` creates a real 20px gap *inside* the padding, rather than being
/// absorbed into `#parent`'s own position.
#[test]
fn padding_should_prevent_margin_collapsing() {
    let html = r#"
        <style>
            body { margin: 0 }
            #parent { width: 200px; padding-top: 5px }
            #parent > div { height: 10px; margin-top: 20px }
        </style>
        <body><div id="parent"><div id="child"></div></div></body>
    "#;
    let (tree, styled) = common::layout_html(html);
    let parent = fragment_by_id(&tree, styled.document(), "parent");
    let child = fragment_by_id(&tree, styled.document(), "child");
    assert_eq!(
        parent.border_box.origin,
        Point {
            x: px(0.0),
            y: px(0.0)
        }
    );
    assert_eq!(
        child.border_box.origin,
        Point {
            x: px(0.0),
            y: px(25.0)
        }
    );
}

/// Top-margin collapsing chains through *every* generation of first-in-flow-block children,
/// not just one level (CSS 2.1 §8.3.1) — `<body><div><div><div>x</div></div></div></body>` is
/// a three-deep chain of `margin-top: 10px`/`20px`/`30px`, nothing (no padding, no border)
/// separating any two adjacent boxes in it, so the whole chain collapses to one value: the
/// greatest of the three. That collapsed `30px` is used exactly once — to position `#outer`
/// relative to whatever precedes it (here, `<body>`, itself flush against the viewport) — and
/// every box inside the chain sits flush against its own parent's content edge (the margin
/// was "spent" positioning the outermost box, not re-applied at each level), so `#outer` and
/// `#inner` end up at the *same* absolute `y`.
#[test]
fn parent_and_first_child_top_margins_should_collapse_through_a_chain() {
    let html = r#"
        <style>
            body { margin: 0 }
            #outer { margin-top: 10px }
            #mid { margin-top: 20px }
            #inner { margin-top: 30px }
        </style>
        <body><div id="outer"><div id="mid"><div id="inner">x</div></div></div></body>
    "#;
    let (tree, styled) = common::layout_html(html);
    let outer = fragment_by_id(&tree, styled.document(), "outer");
    let mid = fragment_by_id(&tree, styled.document(), "mid");
    let inner = fragment_by_id(&tree, styled.document(), "inner");
    assert_eq!(
        outer.border_box.origin,
        Point {
            x: px(0.0),
            y: px(30.0)
        }
    );
    assert_eq!(
        mid.border_box.origin,
        Point {
            x: px(0.0),
            y: px(30.0)
        }
    );
    assert_eq!(
        inner.border_box.origin,
        Point {
            x: px(0.0),
            y: px(30.0)
        }
    );
}

/// The bottom-margin mirror of the top-margin chain test above: `#inner`'s, `#mid`'s and
/// `#outer`'s bottom margins (`30px`/`20px`/`10px`) all collapse into one `30px` gap after
/// `#outer` (each of `#outer`/`#mid` has `height: auto`, no padding/border and no `min-height`
/// — CSS 2.1 §8.3.1's eligibility for the bottom case), rather than three independent gaps
/// (which would total `60px`) or only the outermost pair collapsing (which would give `20px`,
/// missing `#inner`'s `30px`).
#[test]
fn bottom_margins_should_collapse_through_a_chain() {
    let html = r#"
        <style>
            body { margin: 0 }
            #outer { margin-bottom: 10px }
            #mid { margin-bottom: 20px }
            #inner { height: 5px; margin-bottom: 30px }
            #after { height: 5px }
        </style>
        <body><div id="outer"><div id="mid"><div id="inner">x</div></div></div><div id="after"></div></body>
    "#;
    let (tree, styled) = common::layout_html(html);
    let after = fragment_by_id(&tree, styled.document(), "after");
    // #outer sits at y=0 (body's only preceding content), is 5px tall (all three boxes'
    // border boxes have zero internal gap, only #inner's own 5px height contributes), so
    // #after starts at 5px (#outer's own bottom) + 30px (the collapsed chain) = 35px.
    assert_eq!(
        after.border_box.origin,
        Point {
            x: px(0.0),
            y: px(35.0)
        }
    );
}

/// Padding stops the margin chain exactly where it occurs, but does not erase the margin
/// collected *above* that point: `#outer`'s own `margin-top: 10px` still applies in full
/// (nothing above `#outer` to collapse it away further — `<body>` has no margin of its own
/// here), but `#outer`'s `padding-top: 1px` means `#outer`'s relationship with `#inner` is not
/// "nothing separating them", so `#inner`'s `margin-top: 20px` creates a real internal gap
/// rather than being folded into `#outer`'s own `effective_margin_top`.
#[test]
fn padding_should_stop_margin_chain() {
    let html = r#"
        <style>
            body { margin: 0 }
            #outer { margin-top: 10px; padding-top: 1px }
            #inner { margin-top: 20px }
        </style>
        <body><div id="outer"><div id="inner">x</div></div></body>
    "#;
    let (tree, styled) = common::layout_html(html);
    let outer = fragment_by_id(&tree, styled.document(), "outer");
    let inner = fragment_by_id(&tree, styled.document(), "inner");
    assert_eq!(
        outer.border_box.origin,
        Point {
            x: px(0.0),
            y: px(10.0)
        }
    );
    assert_eq!(
        inner.border_box.origin,
        Point {
            x: px(0.0),
            y: px(31.0)
        }
    );
}

/// `position: relative` shifts a fragment (and its whole subtree, if it has one) without
/// affecting layout flow: `#b`'s `top`/`left` move only `#b` itself, and `#c` — which comes
/// after it — is positioned exactly as if `#b` had never been offset.
#[test]
fn relative_position_should_offset_fragment_only() {
    let html = r#"
        <style>
            body { margin: 0 }
            #a { height: 10px }
            #b { position: relative; top: 5px; left: 3px; height: 10px }
            #c { height: 10px }
        </style>
        <body><div id="a"></div><div id="b"></div><div id="c"></div></body>
    "#;
    let (tree, styled) = common::layout_html(html);
    let a = fragment_by_id(&tree, styled.document(), "a");
    let b = fragment_by_id(&tree, styled.document(), "b");
    let c = fragment_by_id(&tree, styled.document(), "c");
    assert_eq!(
        a.border_box.origin,
        Point {
            x: px(0.0),
            y: px(0.0)
        }
    );
    assert_eq!(
        b.border_box.origin,
        Point {
            x: px(3.0),
            y: px(15.0)
        }
    );
    assert_eq!(
        c.border_box.origin,
        Point {
            x: px(0.0),
            y: px(20.0)
        }
    );
}

/// A `display: none` element generates no box at all (Task 16), so it contributes no space
/// to layout whatsoever: `#c` sits directly below `#a`'s 10px, as if `#hidden`'s 1000px
/// height did not exist.
#[test]
fn display_none_child_should_take_no_space() {
    let html = r#"
        <style>
            body { margin: 0 }
            #a { height: 10px }
            #hidden { display: none; height: 1000px }
            #c { height: 10px }
        </style>
        <body><div id="a"></div><div id="hidden"></div><div id="c"></div></body>
    "#;
    let (tree, styled) = common::layout_html(html);
    let c = fragment_by_id(&tree, styled.document(), "c");
    assert_eq!(
        c.border_box.origin,
        Point {
            x: px(0.0),
            y: px(10.0)
        }
    );
}

/// Nested block boxes stack vertically inside their container in document order, each
/// starting where the previous one's border box ended (CSS 2.1 §10.3.3's normal flow,
/// no margins in play): three children of differing heights end up at cumulative offsets.
#[test]
fn nested_blocks_should_stack_vertically() {
    let html = r#"
        <style>
            body { margin: 0 }
            #outer { width: 200px }
            #a { height: 10px; margin: 0 }
            #b { height: 20px; margin: 0 }
            #c { height: 30px; margin: 0 }
        </style>
        <body><div id="outer"><div id="a"></div><div id="b"></div><div id="c"></div></div></body>
    "#;
    let (tree, styled) = common::layout_html(html);
    let outer = fragment_by_id(&tree, styled.document(), "outer");
    let a = fragment_by_id(&tree, styled.document(), "a");
    let b = fragment_by_id(&tree, styled.document(), "b");
    let c = fragment_by_id(&tree, styled.document(), "c");
    assert_eq!(
        a.border_box.origin,
        Point {
            x: px(0.0),
            y: px(0.0)
        }
    );
    assert_eq!(
        b.border_box.origin,
        Point {
            x: px(0.0),
            y: px(10.0)
        }
    );
    assert_eq!(
        c.border_box.origin,
        Point {
            x: px(0.0),
            y: px(30.0)
        }
    );
    assert_eq!(
        outer.border_box.size,
        Size {
            w: px(200.0),
            h: px(60.0)
        }
    );
}

/// A single-child chain `DEPTH` boxes deep — `<div style="margin-top:1px">` nested `DEPTH`
/// times — is exactly the shape `effective_margin_top`'s memoization exists to defend against
/// (see its "Complexity" doc section): without it, placing each of the `n` boxes in the chain
/// would re-walk the remainder of the chain below it, `n + (n-1) + … + 1 = O(n²)` total. Every
/// margin in this fixture is `1px` (all non-negative), so the whole chain collapses to `1px`,
/// applied once between `<html>` and `<body>` (the root/viewport boundary — see the module
/// docs — is the only place nothing absorbs it further); every box below `<body>`, all the
/// way down to `#innermost`, then sits flush against its parent's content edge, so
/// `#innermost`'s border box ends up at the same absolute `y = 1px` as `<body>` itself. The
/// HTML string is built with a loop, not recursion, matching this crate's own no-recursion
/// discipline for anything sized by a hostile/pathological document.
///
/// `DEPTH = 5_000`, not the 50,000 the review that requested this test named: measured with a
/// throwaway per-stage timer (parse → style → box tree → layout) before picking a depth,
/// `cl_html::parse_document_str` and `cl_style::StyleEngine::resolve` are themselves
/// quadratic in nesting depth — a pre-existing characteristic of those two crates, unrelated
/// to this fix — costing (this machine, debug build) `parse=2.67s style=1.97s` at depth
/// 8,000 alone and climbing so steeply that depth 50,000 did not finish parsing+styling in
/// several minutes (killed, not measured to completion). `cl_layout::box_tree::build` and
/// `cl_layout::block::layout` (this task's own code) were confirmed **linear** at every depth
/// sampled up to 8,000 (`layout`: `1.7ms → 2.4ms → 4.7ms → 9.2ms → 18.7ms` at depths `500,
/// 1000, 2000, 4000, 8000` — each depth doubling roughly doubles the time, not quadruples it).
/// At `DEPTH = 5_000`, total pipeline wall time measured `≈ 1.9s` — comfortable under the 5s
/// bound below — while still being deep enough that a regression back to `O(n²)` in *this*
/// crate's own margin-chain walk (the thing this test actually guards) would cost on the
/// order of `n/2 ≈ 2,500×` this run's `layout` time alone (chain-node visits go from `O(n)` to
/// `O(n²/2)`), i.e. tens of seconds — decisively over budget, not a close call. See the fix
/// report for the full per-depth measurement table.
///
/// Bounded at 5s for `cl_layout::layout` ALONE — parse, style and font-database construction
/// run before the timer starts (they are other crates' cost: parse/style are super-linear at
/// this depth, a recorded branch debt, and on the Windows CI runner they alone took ~4.8 s).
/// An `O(n)` `layout` at this depth finishes in milliseconds, so the bound is a ~100× margin,
/// not a close call. `CL_SMOKE_SLOW=1` raises it to 15s, the same escape hatch the plan's other
/// smoke tests use, for a slow/loaded CI runner.
#[test]
fn deep_first_child_chain_should_lay_out_in_linear_time() {
    const DEPTH: usize = 5_000;

    let mut html = String::from(r#"<body style="margin:0">"#);
    for _ in 0..DEPTH - 1 {
        html.push_str(r#"<div style="margin-top:1px">"#);
    }
    html.push_str(r#"<div id="innermost" style="margin-top:1px">x</div>"#);
    for _ in 0..DEPTH - 1 {
        html.push_str("</div>");
    }
    html.push_str("</body>");

    // Parse, style and font-database construction happen OUTSIDE the timer: they are other
    // crates' cost (parse/style are themselves super-linear at this depth — a recorded branch
    // debt, not this crate's), and on the Windows CI runner they alone consumed ~4.8 s of the
    // 5 s bound before Task 18 pushed the total over it. This test's claim is about
    // `cl_layout::layout` only, so that is the only thing it times.
    let styled = common::styled_document(&html);
    let mut fonts = cl_fonts::FontDb::bundled().expect("bundled font db");
    let viewport = cl_layout::Viewport::new(800.0, 600.0);

    let start = std::time::Instant::now();
    let tree = cl_layout::layout(&styled, viewport, &mut fonts).expect("layout");
    let elapsed = start.elapsed();

    let limit = smoke_time_limit();
    assert!(
        elapsed < limit,
        "layout of a {DEPTH}-deep first-child chain took {elapsed:?}, expected well under \
         {limit:?} — an O(n) walk should take milliseconds; this smells like the margin-chain \
         memoization regressed to O(n^2). Set CL_SMOKE_SLOW=1 to raise the bound to 15s on a \
         slow/loaded machine."
    );

    let innermost = fragment_by_id(&tree, styled.document(), "innermost");
    assert_eq!(
        innermost.border_box.origin,
        Point {
            x: px(0.0),
            y: px(1.0)
        }
    );
}
