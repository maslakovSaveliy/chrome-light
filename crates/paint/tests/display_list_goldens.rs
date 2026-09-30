//! Golden [`cl_paint::dump::display_list_dump`] snapshots for the same five documents
//! `crates/style/tests/computed_style_goldens.rs`, `crates/layout/tests/box_tree_goldens.rs`
//! and `crates/layout/tests/fragment_tree_goldens.rs` snapshot — reused here by relative path
//! (`include_str!`), so all four crates' goldens can never drift apart into divergent copies
//! of one fixture.
//!
//! None of the five fixtures sets a `background` on `<html>` or `<body>` (the M1a UA sheet
//! gives neither one), so none of these five snapshots has a canvas item — see each test's
//! doc comment for what else to check by eye in its `.snap` file.
#![allow(
    clippy::expect_used,
    reason = "a failed setup step in a test should abort that test, loudly"
)]

#[path = "common/mod.rs"]
mod common;

use cl_paint::build;
use cl_paint::dump::display_list_dump;

/// Runs the full pipeline (parse, style, build the box tree, lay it out, build the display
/// list) over `html` and returns its display list dump.
fn dump(html: &str) -> String {
    let (tree, styled) = common::layout_html(html);
    let dl = build(&tree, styled.document());
    display_list_dump(&dl)
}

/// The smallest real document: doctype, `<head>` (`display: none`, so it paints nothing) and
/// one `<p>`. Check by eye: no canvas rect (UA sheet gives `<html>`/`<body>` no background);
/// no `Rect`/`Border` items at all (`<html>`/`<body>`/`<p>` are all UA-default transparent
/// with no border), so the whole list is `Text` items for "hi"'s single run, at the same
/// origin `fragment_tree_dump`'s `minimal_document` golden shows the line box starting
/// (`<body>`'s 8px UA margin inset).
#[test]
fn minimal_document() {
    insta::assert_snapshot!(dump(include_str!("../../style/tests/golden/minimal.html")));
}

/// Nested block boxes with author margins, padding, borders and an explicit width. Check by
/// eye: exactly one `Border` item, for `.outer`'s `border: 2px solid black` (all four widths
/// 2px, all four colors black) at `.outer`'s border box (`fragment_tree_dump`'s
/// `nested_divs_with_margins` golden's own `Block <div>` line, inset 8px from the left/right
/// viewport edge by `.outer`'s `margin: 20px 10px` and 20px from the top by that margin
/// collapsing with `<body>`'s UA margin — see that golden's doc comment); no fixture rule
/// sets `background-color`, so no `Rect` item; two `Text` items, one per `.inner` div's
/// single-character run ("a", "b") — `.inner` itself sets no border or background, so
/// nothing paints for the `.inner` boxes beyond their text.
#[test]
fn nested_divs_with_margins() {
    insta::assert_snapshot!(dump(include_str!(
        "../../style/tests/golden/nested-divs.html"
    )));
}

/// Inline `<span>`s inside a styled `<p>`: `span.loud` sets `background-color: yellow`, but
/// per M1a's scope (no fragment is generated for an inline element — Task 18's decision,
/// documented in `cl_layout::fragment`'s module docs) that background is never painted.
/// Check by eye: no `Rect`/`Border` items despite the yellow rule existing in the fixture's
/// CSS (proving the "inline backgrounds are not painted in M1a" scope decision); four `Text`
/// items, one per `InlineText` box's run ("plain ", "quiet", " and ", "loud"), at the origins
/// `fragment_tree_dump`'s `inline_spans` golden's `Text`/`Run` lines show.
#[test]
fn inline_spans() {
    insta::assert_snapshot!(dump(include_str!(
        "../../style/tests/golden/inline-spans.html"
    )));
}

/// `<pre>`: the fixture's only rule sets `border-left-width: 4px` with no `border-style`, so
/// `border-style`'s initial value (`none`) makes every side, including the left one, not
/// solid — `border_solid` is `false` regardless of the specified width (see
/// `cl_layout::geom::LayoutStyle::border_width`'s docs on the *used* width already being
/// zeroed for a non-solid side). Check by eye: no `Border` item despite the fixture spending
/// a whole rule on `border-left-width` — proving M1a really does gate every side on
/// `border-style: solid`, not merely on a nonzero width; two `Text` items, not one — the
/// embedded newline is a hard break under `white-space: pre` (Task 18's real line breaking,
/// not the Task 17 placeholder), so "  two spaces" and "and a newline" land in two separate
/// `Line`/`Text` fragments, each contributing its own run, both starting at the same 16px
/// left inset from the viewport (`<body>`'s 8px UA margin plus `<pre>`'s own 8px
/// `padding-left`; the non-solid `border-left` contributes 0 regardless of its 4px width).
#[test]
fn preformatted_text() {
    insta::assert_snapshot!(dump(include_str!("../../style/tests/golden/pre.html")));
}

/// Two colliding `<style>` sheets, resolved by specificity then source order — the fragment
/// tree only cares about the cascade's final answer, and neither sheet sets a background or
/// border, so this golden is purely about `Text` item order and count: two `Text` items, one
/// per `<p>`'s single run ("first sheet only", "both sheets"), matching
/// `fragment_tree_dump`'s `conflicting_sheets_resolve_by_specificity_then_order` golden.
#[test]
fn conflicting_sheets_resolve_by_specificity_then_order() {
    insta::assert_snapshot!(dump(include_str!(
        "../../style/tests/golden/cascade-conflict.html"
    )));
}
