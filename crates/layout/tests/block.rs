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
