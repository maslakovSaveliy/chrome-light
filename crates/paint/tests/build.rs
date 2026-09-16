//! Behavioural tests for [`cl_paint::build`]: exact item sequences and exact geometry/color
//! values, built through the real parse -> style -> layout pipeline (`crates/paint/tests/
//! common/mod.rs`) rather than a hand-assembled fixture, per Task 19's TDD step.
#![allow(
    clippy::expect_used,
    reason = "a failed setup step in a test should abort that test, loudly"
)]

#[path = "common/mod.rs"]
mod common;

use cl_layout::{Au, FragmentId, FragmentTree, GlyphRun, Rect, Rgba8, Sides};
use cl_paint::{DisplayItem, build};

/// Collects every fragment id reachable from `tree.root`, via the same public
/// `get`/`children` walk `cl_paint::build` itself uses — used only to manufacture a
/// [`FragmentId`] that is real (came from an actual `FragmentTree`) but foreign to a
/// *different*, smaller tree, for [`build_should_not_panic_on_a_dangling_fragment_id`].
fn collect_ids(tree: &FragmentTree) -> Vec<FragmentId> {
    let mut ids = Vec::new();
    let mut stack = vec![tree.root];
    let mut remaining = tree.len();
    while let Some(id) = stack.pop() {
        if remaining == 0 {
            break;
        }
        remaining -= 1;
        ids.push(id);
        if let Some(fragment) = tree.get(id) {
            stack.extend(fragment.children.iter().copied());
        }
    }
    ids
}

/// Collects every glyph run held by a `Text` fragment, in the order [`cl_paint::build`]
/// visits them — used to grab the exact [`GlyphRun`] real shaping produced, so a test can
/// assert `build` carried it into a [`DisplayItem::Text`] unchanged rather than
/// re-implementing shaping by hand.
fn collect_runs(tree: &FragmentTree) -> Vec<GlyphRun> {
    let mut runs = Vec::new();
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
        if let cl_layout::FragmentKind::Text { runs: text_runs } = &fragment.kind {
            runs.extend(text_runs.iter().cloned());
        }
        let mut children = fragment.children.clone();
        children.reverse();
        stack.extend(children);
    }
    runs
}

#[test]
fn build_should_emit_background_before_border_before_children() {
    let html = r#"<!DOCTYPE html>
<html><head><style>
  html, body { margin: 0; padding: 0 }
  #outer { background: rgb(10, 20, 30); border: 4px solid rgb(40, 50, 60); width: 100px; height: 80px }
  #inner { background: rgb(70, 80, 90); width: 50px; height: 30px }
</style></head>
<body><div id="outer"><div id="inner"></div></div></body></html>"#;
    let (tree, styled) = common::layout_html(html);

    let dl = build(&tree, styled.document());

    let outer_border_box = Rect::from_px(0.0, 0.0, 108.0, 88.0);
    let inner_border_box = Rect::from_px(4.0, 4.0, 50.0, 30.0);
    assert_eq!(
        dl.items,
        vec![
            DisplayItem::Rect {
                rect: outer_border_box,
                color: Rgba8 {
                    r: 10,
                    g: 20,
                    b: 30,
                    a: 255
                },
            },
            DisplayItem::Border {
                rect: outer_border_box,
                widths: Sides::uniform(Au::from_px(4.0)),
                colors: Sides::uniform(Rgba8 {
                    r: 40,
                    g: 50,
                    b: 60,
                    a: 255
                }),
            },
            DisplayItem::Rect {
                rect: inner_border_box,
                color: Rgba8 {
                    r: 70,
                    g: 80,
                    b: 90,
                    a: 255
                },
            },
        ]
    );
}

#[test]
fn build_should_skip_transparent_backgrounds() {
    let html = r#"<!DOCTYPE html>
<html><head><style>
  html, body { margin: 0; padding: 0 }
  #box { background: transparent; border: 2px solid rgb(1, 2, 3); width: 10px; height: 10px }
  #empty { background: rgb(9, 9, 9); width: 0; height: 20px }
</style></head>
<body><div id="box"></div><div id="empty"></div></body></html>"#;
    let (tree, styled) = common::layout_html(html);

    let dl = build(&tree, styled.document());

    // #box: a transparent background is skipped, but its solid border still paints.
    // #empty: an opaque background over a zero-area border box is skipped too (a zero-width
    // box would be a no-op fill either way).
    assert_eq!(
        dl.items,
        vec![DisplayItem::Border {
            rect: Rect::from_px(0.0, 0.0, 14.0, 14.0),
            widths: Sides::uniform(Au::from_px(2.0)),
            colors: Sides::uniform(Rgba8 {
                r: 1,
                g: 2,
                b: 3,
                a: 255
            }),
        }]
    );
}

#[test]
fn overflow_hidden_should_wrap_children_in_clip() {
    let html = r#"<!DOCTYPE html>
<html><head><style>
  html, body { margin: 0; padding: 0 }
  #clip {
    overflow: hidden;
    background: rgb(5, 6, 7);
    border: 2px solid rgb(9, 9, 9);
    padding: 3px;
    width: 40px;
    height: 40px;
  }
  #child { background: rgb(11, 12, 13); width: 200px; height: 20px }
</style></head>
<body><div id="clip"><div id="child"></div></div></body></html>"#;
    let (tree, styled) = common::layout_html(html);

    let dl = build(&tree, styled.document());

    let clip_border_box = Rect::from_px(0.0, 0.0, 50.0, 50.0);
    let clip_padding_box = Rect::from_px(2.0, 2.0, 46.0, 46.0);
    let child_border_box = Rect::from_px(5.0, 5.0, 200.0, 20.0);
    assert_eq!(
        dl.items,
        vec![
            DisplayItem::Rect {
                rect: clip_border_box,
                color: Rgba8 {
                    r: 5,
                    g: 6,
                    b: 7,
                    a: 255
                },
            },
            DisplayItem::Border {
                rect: clip_border_box,
                widths: Sides::uniform(Au::from_px(2.0)),
                colors: Sides::uniform(Rgba8 {
                    r: 9,
                    g: 9,
                    b: 9,
                    a: 255
                }),
            },
            DisplayItem::PushClip {
                rect: clip_padding_box,
            },
            DisplayItem::Rect {
                rect: child_border_box,
                color: Rgba8 {
                    r: 11,
                    g: 12,
                    b: 13,
                    a: 255
                },
            },
            DisplayItem::PopClip,
        ]
    );
}

#[test]
fn body_background_should_propagate_to_canvas() {
    let html = "<!DOCTYPE html><html><head><style>body { background: #00f; margin: 0; height: 50px }</style></head><body></body></html>";
    let (tree, styled) = common::layout_html(html);

    let dl = build(&tree, styled.document());

    // The canvas rect (the whole 800x600 viewport) carries body's blue background as its
    // first and only item: body's own border box does NOT get a second, redundant rect.
    assert_eq!(
        dl.items,
        vec![DisplayItem::Rect {
            rect: Rect::from_px(0.0, 0.0, 800.0, 600.0),
            color: Rgba8 {
                r: 0,
                g: 0,
                b: 255,
                a: 255
            },
        }]
    );
}

#[test]
fn html_background_should_win_over_body() {
    let html = "<!DOCTYPE html><html><head><style>html { background: #f00 } body { background: #00f; margin: 0; height: 50px }</style></head><body></body></html>";
    let (tree, styled) = common::layout_html(html);

    let dl = build(&tree, styled.document());

    // html's opaque background wins the canvas slot, so html's own border box does NOT
    // repaint it a second time (CSS 2.1 §14.2 resets the source element's own used
    // background to transparent); body's background was never touched by the propagation,
    // so it paints normally on its own border box.
    assert_eq!(
        dl.items,
        vec![
            DisplayItem::Rect {
                rect: Rect::from_px(0.0, 0.0, 800.0, 600.0),
                color: Rgba8 {
                    r: 255,
                    g: 0,
                    b: 0,
                    a: 255
                },
            },
            DisplayItem::Rect {
                rect: Rect::from_px(0.0, 0.0, 800.0, 50.0),
                color: Rgba8 {
                    r: 0,
                    g: 0,
                    b: 255,
                    a: 255
                },
            },
        ]
    );
}

#[test]
fn text_fragment_should_emit_text_item_with_color() {
    let html = "<!DOCTYPE html><html><head><style>body { margin: 0 } p { color: rgb(1, 2, 3) }</style></head><body><p>hi</p></body></html>";
    let (tree, styled) = common::layout_html(html);

    let runs = collect_runs(&tree);
    assert_eq!(runs.len(), 1, "expected exactly one shaped run for \"hi\"");
    let run = runs.first().expect("checked len() == 1 above");
    assert_eq!(
        run.color,
        Rgba8 {
            r: 1,
            g: 2,
            b: 3,
            a: 255
        }
    );

    let dl = build(&tree, styled.document());

    // html/body/p all have transparent backgrounds and no border in M1a's UA sheet, so the
    // only item in the whole list is the text run itself, carried through unchanged.
    assert_eq!(dl.items, vec![DisplayItem::Text { run: run.clone() }]);
}

/// Constraint: `build` must never panic, even handed a [`FragmentTree`] whose own bookkeeping
/// is inconsistent with itself. `FragmentTree`'s fields are private to `cl-layout` (no public
/// constructor exists outside it), so this test cannot fabricate a `FragmentTree` from
/// scratch; instead it corrupts a real one's public `root` field with a [`FragmentId`] that
/// is itself real (taken from a different, larger real tree) but foreign to the smaller tree
/// it is planted in — the same "a fragment id does not resolve in this tree" condition
/// [`cl_paint::build`]'s docs describe, reached the only way the public API allows.
#[test]
fn build_should_not_panic_on_a_dangling_fragment_id() {
    let (mut small_tree, small_styled) =
        common::layout_html("<!DOCTYPE html><html><body></body></html>");
    let (big_tree, _big_styled) = common::layout_html(
        "<!DOCTYPE html><html><body><p>one</p><p>two</p><p>three</p><p>four</p><p>five</p></body></html>",
    );

    let dangling = collect_ids(&big_tree)
        .into_iter()
        .find(|id| small_tree.get(*id).is_none())
        .expect("the larger tree must contain a fragment id absent from the smaller one");

    small_tree.root = dangling;

    let dl = build(&small_tree, small_styled.document());

    assert!(dl.items.is_empty());
    assert_eq!(dl.bounds, Rect::from_px(0.0, 0.0, 800.0, 600.0));
}
