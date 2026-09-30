//! Golden [`cl_layout::dump::fragment_tree_dump`] snapshots for the same five documents
//! `crates/style/tests/computed_style_goldens.rs` and `tests/box_tree_goldens.rs` snapshot —
//! reused here by relative path (`include_str!`), so all three crates' goldens can never
//! drift apart into divergent copies of one fixture.
//!
//! Unlike `tests/block.rs` (which resets `body { margin: 0 }` in every fixture to isolate one
//! behaviour at a time), these goldens run the *real* UA stylesheet unmodified — see each
//! test's doc comment for what to check by eye in its `.snap` file: `<body>`'s 8px UA margin,
//! `<p>` margins collapsing between siblings, and the Task 17 inline-layout placeholder's line
//! heights (see `cl_layout::block`'s module docs for exactly what the placeholder does).
#![allow(
    clippy::expect_used,
    reason = "a failed setup step in a test should abort that test, loudly"
)]

#[path = "common/mod.rs"]
mod common;

use cl_layout::dump::fragment_tree_dump;

/// Runs the full pipeline (parse, style, build the box tree, lay it out) over `html` and
/// returns its fragment tree dump.
fn dump(html: &str) -> String {
    let (tree, styled) = common::layout_html(html);
    let fonts = cl_fonts::FontDb::bundled().expect("bundled font db");
    fragment_tree_dump(&tree, styled.document(), &fonts)
}

/// The smallest real document: doctype, `<head>` (`display: none`, so it lays out nothing)
/// and one `<p>`. Check by eye: `<body>`'s border box is inset 8px from the viewport on every
/// side (the UA sheet's `body { margin: 8px }`, uncollapsed — `<html>` has no margin of its
/// own to collapse with, and `<body>` has no preceding sibling); `<p>`'s single line box is
/// exactly its `line-height` tall (Ahem/Noto Sans are never consulted by the M1a placeholder,
/// only the computed `line-height`, so this is legible arithmetic, not a font metric).
#[test]
fn minimal_document() {
    insta::assert_snapshot!(dump(include_str!("../../style/tests/golden/minimal.html")));
}

/// Nested block boxes with author margins, padding, borders and an explicit width. Check by
/// eye: `.outer`'s `margin: 20px 10px` collapses with `<body>`'s 8px top margin to `20px`
/// (the greater of the two, CSS 2.1 §8.3.1 — this is the sibling case, since `<body>`'s only
/// child is `.outer`, i.e. `.outer`'s top margin *is* the gap before it, exactly the way
/// `tests/block.rs`'s `parent_and_first_child_top_margins_should_collapse` exercises this
/// same mechanism in isolation); `.outer`'s `border: 2px solid` and `padding: 5px` show up as
/// a 7px inset between its border and content boxes; the two `.inner` divs' `margin-top: 1em`
/// (16px, the default `font-size`) collapses with nothing between them, since `.outer` has
/// non-zero padding — this is `padding_should_prevent_margin_collapsing`'s exact shape,
/// reappearing between the two `.inner` siblings *and* between `.outer`'s padding and its
/// first child.
#[test]
fn nested_divs_with_margins() {
    insta::assert_snapshot!(dump(include_str!(
        "../../style/tests/golden/nested-divs.html"
    )));
}

/// Inline `<span>`s inside a styled `<p>`: every child is inline-level, so `<p>` itself is the
/// M1a placeholder's leaf. Check by eye: exactly one `Line` fragment (no `<br>` in this
/// document, so there is only ever one line group), `line_height` tall (`<p>`'s own
/// `line-height: 1.5` at `font-size: 20px` = 30px); four `Text` children under it — one per
/// `InlineText` box the flattening walk finds, including the two nested inside `<span>`s (see
/// `cl_layout::block`'s module docs on flattening `Inline` boxes into their container's line
/// groups) — for `"plain "`, `"quiet"`, `" and "` and `"loud"`;
/// every `Text` fragment is a zero-size rect at the line's content-box origin (no shaping
/// until Task 18).
#[test]
fn inline_spans() {
    insta::assert_snapshot!(dump(include_str!(
        "../../style/tests/golden/inline-spans.html"
    )));
}

/// `<pre>`: proves the placeholder does not attempt real line breaking — the whole
/// preformatted text node (spaces, embedded newline included) is one `InlineText` box, so
/// `<pre>` still produces exactly one `Line` fragment despite `white-space: pre` and the
/// embedded `\n` (real line breaking on `white-space: pre`'s hard breaks is Task 18's job,
/// not this placeholder's — see `cl_layout::block`'s module docs).
#[test]
fn preformatted_text() {
    insta::assert_snapshot!(dump(include_str!("../../style/tests/golden/pre.html")));
}

/// Two colliding `<style>` sheets: the fragment tree only cares about the cascade's final
/// answer, so this mostly exercises that geometry longhands still come through correctly once
/// several tie-breakers have already picked a winner — including the two `<p>`s' sibling
/// margin collapsing (both get the UA sheet's `p { margin: 1em 0 }`, so the gap between them
/// is one collapsed `1em`, not two stacked).
#[test]
fn conflicting_sheets_resolve_by_specificity_then_order() {
    insta::assert_snapshot!(dump(include_str!(
        "../../style/tests/golden/cascade-conflict.html"
    )));
}
