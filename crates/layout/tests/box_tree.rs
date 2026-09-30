//! Integration tests for [`cl_layout::box_tree::build`]: box generation rules (CSS 2.1
//! §9.2) exercised through the crate's public API on real styled documents.
#![allow(
    clippy::expect_used,
    reason = "a failed setup step in a test should abort that test, loudly"
)]

#[path = "common/mod.rs"]
mod common;

use cl_dom::{NodeId, local_name};
use cl_layout::box_tree::{BoxId, BoxKind, BoxTree};
use cl_layout::{Au, Display, Length, Rgba8, build};

/// The first box (depth-first) generated for `node`, or `None` if its whole subtree was
/// skipped (a `display: none` element, or one nested inside one).
fn find_box_by_node(tree: &BoxTree, node: NodeId) -> Option<BoxId> {
    let mut stack = vec![tree.root];
    while let Some(id) = stack.pop() {
        let b = tree.get(id)?;
        if b.node == Some(node) {
            return Some(id);
        }
        stack.extend(b.children.iter().copied());
    }
    None
}

/// A container box's children, by index, panicking (via `expect`, not `[]`) if `i` is out of
/// range — every call site first asserts the exact expected length.
fn nth_child(tree: &BoxTree, id: BoxId, i: usize) -> BoxId {
    tree.get(id)
        .and_then(|b| b.children.get(i))
        .copied()
        .expect("child index in range")
}

/// `<div>a<p>b</p>c</div>`: `a` and `c` are inline-level (text) runs either side of the
/// block-level `<p>`, so CSS 2.1 §9.2.1.1 wraps each run in its own anonymous block —
/// `Block(div) { AnonymousBlock { InlineText }, Block(p) { InlineText }, AnonymousBlock {
/// InlineText } }`.
#[test]
fn box_tree_should_wrap_inline_runs_in_anonymous_blocks() {
    let styled =
        common::styled_document("<!DOCTYPE html><html><body><div>a<p>b</p>c</div></body></html>");
    let tree = build(&styled);

    let div = common::find_element(styled.document(), |el| el.name.local == local_name!("div"))
        .expect("fixture has a <div>");
    let div_box_id = find_box_by_node(&tree, div).expect("<div> generates a box");
    let div_box = tree.get(div_box_id).expect("box exists");
    assert_eq!(div_box.kind, BoxKind::Block);
    assert_eq!(div_box.children.len(), 3);

    let first_anon = tree
        .get(nth_child(&tree, div_box_id, 0))
        .expect("first child exists");
    assert_eq!(first_anon.kind, BoxKind::AnonymousBlock);
    assert_eq!(first_anon.children.len(), 1);
    assert!(matches!(
        first_anon
            .children
            .first()
            .and_then(|&id| tree.get(id))
            .map(|b| &b.kind),
        Some(BoxKind::InlineText(_))
    ));

    let p_box_id = nth_child(&tree, div_box_id, 1);
    let p_box = tree.get(p_box_id).expect("<p> box exists");
    assert_eq!(p_box.kind, BoxKind::Block);
    assert_eq!(p_box.children.len(), 1);
    assert!(matches!(
        tree.get(nth_child(&tree, p_box_id, 0)).map(|b| &b.kind),
        Some(BoxKind::InlineText(_))
    ));

    let last_anon = tree
        .get(nth_child(&tree, div_box_id, 2))
        .expect("third child exists");
    assert_eq!(last_anon.kind, BoxKind::AnonymousBlock);
    assert_eq!(last_anon.children.len(), 1);
    assert!(matches!(
        last_anon
            .children
            .first()
            .and_then(|&id| tree.get(id))
            .map(|b| &b.kind),
        Some(BoxKind::InlineText(_))
    ));
}

/// `<div style="display:none">` (and everything inside it) generates no box at all: the
/// element and its descendants are skipped, and the rest of the document is unaffected.
#[test]
fn box_tree_should_skip_display_none() {
    let styled = common::styled_document(concat!(
        "<!DOCTYPE html><html><body>",
        r#"<div style="display:none"><p>hidden</p></div>"#,
        "<p>visible</p>",
        "</body></html>",
    ));
    let tree = build(&styled);

    let hidden_div =
        common::find_element(styled.document(), |el| el.name.local == local_name!("div"))
            .expect("fixture has a <div>");
    assert_eq!(find_box_by_node(&tree, hidden_div), None);

    let paragraphs: Vec<NodeId> = styled
        .document()
        .descendants(styled.document().root())
        .filter(|id| {
            styled
                .document()
                .element(*id)
                .is_some_and(|el| el.name.local == local_name!("p"))
        })
        .collect();
    assert_eq!(
        paragraphs.len(),
        2,
        "fixture has a hidden and a visible <p>"
    );
    let hidden_p = *paragraphs.first().expect("hidden <p> present");
    let visible_p = *paragraphs.get(1).expect("visible <p> present");
    assert_eq!(find_box_by_node(&tree, hidden_p), None);

    let visible_box_id = find_box_by_node(&tree, visible_p).expect("visible <p> generates a box");
    let visible_box = tree.get(visible_box_id).expect("box exists");
    assert_eq!(visible_box.kind, BoxKind::Block);
    assert_eq!(visible_box.children.len(), 1);
    assert!(matches!(
        tree.get(nth_child(&tree, visible_box_id, 0))
            .map(|b| &b.kind),
        Some(BoxKind::InlineText(_))
    ));
}

/// `<p>a<br>b</p>`: `<br>` generates a `LineBreak` box in the middle of the inline run,
/// alongside (not wrapping) the surrounding text — all three children stay direct children
/// of `<p>` since every one of them is inline-level (no anonymous block needed).
#[test]
fn box_tree_should_emit_line_break_for_br() {
    let styled = common::styled_document("<!DOCTYPE html><html><body><p>a<br>b</p></body></html>");
    let tree = build(&styled);

    let p = common::find_element(styled.document(), |el| el.name.local == local_name!("p"))
        .expect("fixture has a <p>");
    let p_box_id = find_box_by_node(&tree, p).expect("<p> generates a box");
    let p_box = tree.get(p_box_id).expect("box exists");
    assert_eq!(p_box.kind, BoxKind::Block);
    assert_eq!(p_box.children.len(), 3);

    assert!(matches!(
        tree.get(nth_child(&tree, p_box_id, 0)).map(|b| &b.kind),
        Some(BoxKind::InlineText(_))
    ));
    assert!(matches!(
        tree.get(nth_child(&tree, p_box_id, 1)).map(|b| &b.kind),
        Some(BoxKind::LineBreak)
    ));
    assert!(matches!(
        tree.get(nth_child(&tree, p_box_id, 2)).map(|b| &b.kind),
        Some(BoxKind::InlineText(_))
    ));
}

/// `<div><span>x<div>b</div>y</span><span>plain</span></div>`: the first `<span>` is
/// inline-level by its own `display`, but directly contains a block-level `<div>` child. CSS
/// 2.1 §9.2.1.1 would split it around the block; M1a instead blockifies it (see
/// `box_tree.rs`'s module docs, "Invariant: no block-level box is ever a child of an
/// `Inline` box") — its own box becomes `Block`, `style.display` is forced to
/// `Display::Block` to match (the other "Invariant" section: `style.display` always agrees
/// with `kind`), and the usual anonymous-block wrapping then applies to it exactly like any
/// other block container: `Block(span) { AnonymousBlock { InlineText }, Block(div) {
/// InlineText }, AnonymousBlock { InlineText } }`. The second `<span>` has no block-level
/// child, so — for contrast — it is left alone: `Inline` with `style.display ==
/// Display::Inline`.
#[test]
fn box_tree_should_blockify_inline_with_block_child() {
    let styled = common::styled_document(concat!(
        "<!DOCTYPE html><html><body>",
        "<div><span>x<div>b</div>y</span><span>plain</span></div>",
        "</body></html>",
    ));
    let tree = build(&styled);

    let spans: Vec<NodeId> = styled
        .document()
        .descendants(styled.document().root())
        .filter(|id| {
            styled
                .document()
                .element(*id)
                .is_some_and(|el| el.name.local == local_name!("span"))
        })
        .collect();
    assert_eq!(
        spans.len(),
        2,
        "fixture has a blockified <span> and a plain inline <span>"
    );
    let span = *spans.first().expect("blockified <span> present");
    let plain_span = *spans.get(1).expect("plain <span> present");

    let span_box_id = find_box_by_node(&tree, span).expect("<span> generates a box");
    let span_box = tree.get(span_box_id).expect("box exists");
    assert_eq!(
        span_box.kind,
        BoxKind::Block,
        "an inline element with a block-level child must be blockified"
    );
    assert_eq!(
        span_box.style.display,
        Display::Block,
        "a blockified box's style.display must be forced to match its kind"
    );
    assert_eq!(span_box.children.len(), 3);

    let first_anon = tree
        .get(nth_child(&tree, span_box_id, 0))
        .expect("first child exists");
    assert_eq!(first_anon.kind, BoxKind::AnonymousBlock);
    assert_eq!(first_anon.children.len(), 1);
    assert!(matches!(
        first_anon
            .children
            .first()
            .and_then(|&id| tree.get(id))
            .map(|b| &b.kind),
        Some(BoxKind::InlineText(_))
    ));

    let inner_div_box_id = nth_child(&tree, span_box_id, 1);
    let inner_div_box = tree.get(inner_div_box_id).expect("inner <div> box exists");
    assert_eq!(inner_div_box.kind, BoxKind::Block);
    assert_eq!(inner_div_box.children.len(), 1);
    assert!(matches!(
        tree.get(nth_child(&tree, inner_div_box_id, 0))
            .map(|b| &b.kind),
        Some(BoxKind::InlineText(_))
    ));

    let last_anon = tree
        .get(nth_child(&tree, span_box_id, 2))
        .expect("third child exists");
    assert_eq!(last_anon.kind, BoxKind::AnonymousBlock);
    assert_eq!(last_anon.children.len(), 1);
    assert!(matches!(
        last_anon
            .children
            .first()
            .and_then(|&id| tree.get(id))
            .map(|b| &b.kind),
        Some(BoxKind::InlineText(_))
    ));

    // For contrast: a plain inline `<span>` with no block-level child is not blockified —
    // it stays `Inline`, with `style.display` still `Display::Inline`.
    let plain_span_box_id =
        find_box_by_node(&tree, plain_span).expect("plain <span> generates a box");
    let plain_span_box = tree.get(plain_span_box_id).expect("box exists");
    assert_eq!(plain_span_box.kind, BoxKind::Inline);
    assert_eq!(plain_span_box.style.display, Display::Inline);
}

/// An `InlineText` box's `style` carries only the *inherited* half of its container's style
/// (`cl_layout::LayoutStyle::inherited_text_style`): `<p style="margin:16px;border:2px solid
/// red;color:blue">` must not leak its own `margin`/`border` onto its text run's box — those
/// are not inherited CSS properties — while `color`, which is inherited, does carry over.
#[test]
fn box_tree_inline_text_style_should_only_carry_inherited_properties() {
    let styled = common::styled_document(concat!(
        "<!DOCTYPE html><html><body>",
        r#"<p style="margin:16px;border:2px solid red;color:blue">text</p>"#,
        "</body></html>",
    ));
    let tree = build(&styled);

    let p = common::find_element(styled.document(), |el| el.name.local == local_name!("p"))
        .expect("fixture has a <p>");
    let p_box_id = find_box_by_node(&tree, p).expect("<p> generates a box");
    let text_box_id = nth_child(&tree, p_box_id, 0);
    let text_box = tree.get(text_box_id).expect("text box exists");
    assert!(matches!(text_box.kind, BoxKind::InlineText(_)));

    // Not inherited: must be at their initial values, not the <p>'s 16px margin / 2px red
    // border.
    assert_eq!(text_box.style.margin.top, Length::Px(Au::ZERO));
    assert_eq!(text_box.style.border_width.top, Au::ZERO);
    // Inherited: carries over from the <p>.
    assert_eq!(
        text_box.style.color,
        Rgba8 {
            r: 0,
            g: 0,
            b: 255,
            a: 255
        }
    );
}

/// CSS 2.1 §9.2.2.1: a text node between two block-level siblings that is nothing but
/// collapsible whitespace under `white-space: normal` (ordinary markup indentation) generates
/// no box at all — not even the anonymous block CSS 2.1 §9.2.1.1's mixed-content rule would
/// otherwise wrap it in. `<div>\n  <p>a</p>\n  <p>b</p>\n</div>` must produce exactly two
/// `Block(p)` children, not `Block(p), AnonymousBlock, Block(p)`.
#[test]
fn box_tree_should_not_generate_boxes_for_collapsible_whitespace_between_blocks() {
    let styled = common::styled_document(
        "<!DOCTYPE html><html><body><div>\n  <p>a</p>\n  <p>b</p>\n</div></body></html>",
    );
    let tree = build(&styled);

    let div = common::find_element(styled.document(), |el| el.name.local == local_name!("div"))
        .expect("fixture has a <div>");
    let div_box_id = find_box_by_node(&tree, div).expect("<div> generates a box");
    let div_box = tree.get(div_box_id).expect("box exists");
    assert_eq!(
        div_box.children.len(),
        2,
        "the whitespace-only text nodes around the two <p>s must generate no boxes"
    );
    assert!(matches!(
        tree.get(nth_child(&tree, div_box_id, 0)).map(|b| &b.kind),
        Some(BoxKind::Block)
    ));
    assert!(matches!(
        tree.get(nth_child(&tree, div_box_id, 1)).map(|b| &b.kind),
        Some(BoxKind::Block)
    ));
}

/// The same shape under `white-space: pre`: the leading text is no longer collapsible (CSS
/// 2.1 §9.2.2.1 only drops what *would* collapse), so it still generates an `AnonymousBlock`
/// wrapping an `InlineText`, exactly as `box_tree_should_wrap_inline_runs_in_anonymous_blocks`
/// exercises for non-whitespace text.
#[test]
fn box_tree_should_keep_whitespace_under_white_space_pre() {
    let styled = common::styled_document(concat!(
        "<!DOCTYPE html><html><body>",
        "<div style=\"white-space:pre\">\n<p>a</p></div>",
        "</body></html>",
    ));
    let tree = build(&styled);

    let div = common::find_element(styled.document(), |el| el.name.local == local_name!("div"))
        .expect("fixture has a <div>");
    let div_box_id = find_box_by_node(&tree, div).expect("<div> generates a box");
    let div_box = tree.get(div_box_id).expect("box exists");
    assert_eq!(
        div_box.children.len(),
        2,
        "the leading newline must still generate a box under white-space: pre"
    );

    let first = tree
        .get(nth_child(&tree, div_box_id, 0))
        .expect("first child exists");
    assert_eq!(first.kind, BoxKind::AnonymousBlock);
    assert!(matches!(
        first
            .children
            .first()
            .and_then(|&id| tree.get(id))
            .map(|b| &b.kind),
        Some(BoxKind::InlineText(_))
    ));

    assert!(matches!(
        tree.get(nth_child(&tree, div_box_id, 1)).map(|b| &b.kind),
        Some(BoxKind::Block)
    ));
}
