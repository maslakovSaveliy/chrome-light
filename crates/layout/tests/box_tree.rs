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
use cl_layout::build;

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
