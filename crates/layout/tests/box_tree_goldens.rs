//! Golden [`cl_layout::dump::box_tree_dump`] snapshots for the same five documents
//! `cl-style`'s `crates/style/tests/computed_style_goldens.rs` snapshots — reused here by
//! relative path (`include_str!`) rather than copied, so the two crates' goldens can never
//! drift apart into two divergent copies of one fixture.
#![allow(
    clippy::expect_used,
    reason = "a failed setup step in a test should abort that test, loudly"
)]

#[path = "common/mod.rs"]
mod common;

use cl_layout::build;
use cl_layout::dump::box_tree_dump;

/// Runs the full pipeline over `html` (parse, style, build the box tree) and returns its box
/// tree dump.
fn dump(html: &str) -> String {
    let styled = common::styled_document(html);
    let tree = build(&styled);
    box_tree_dump(&tree)
}

/// The smallest real document: doctype, `<head>` (`display: none`, so its `<title>` never
/// reaches the box tree) and one `<p>`.
#[test]
fn minimal_document() {
    insta::assert_snapshot!(dump(include_str!("../../style/tests/golden/minimal.html")));
}

/// Nested block boxes with author margins, padding, borders and an explicit width — the
/// geometry longhands [`cl_layout::style_adapt::adapt`] reads first.
#[test]
fn nested_divs_with_margins() {
    insta::assert_snapshot!(dump(include_str!(
        "../../style/tests/golden/nested-divs.html"
    )));
}

/// Inline `<span>`s inside a styled `<p>`: every child is inline-level, so no anonymous
/// block is synthesized — this exercises the "homogeneous children" half of the
/// mixed-inline-run rule, the golden `nested-divs`/`cascade-conflict` documents do not.
#[test]
fn inline_spans() {
    insta::assert_snapshot!(dump(include_str!(
        "../../style/tests/golden/inline-spans.html"
    )));
}

/// `<pre>`: proves `white-space: pre` (from the UA sheet) reaches [`cl_layout::LayoutStyle`]
/// unchanged through the box tree.
#[test]
fn preformatted_text() {
    insta::assert_snapshot!(dump(include_str!("../../style/tests/golden/pre.html")));
}

/// Two colliding `<style>` sheets: the box tree only cares about the cascade's final
/// answer, so this mostly exercises that `display`/geometry still come through correctly
/// once several tie-breakers have already picked a winner.
#[test]
fn conflicting_sheets_resolve_by_specificity_then_order() {
    insta::assert_snapshot!(dump(include_str!(
        "../../style/tests/golden/cascade-conflict.html"
    )));
}
