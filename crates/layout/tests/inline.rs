//! Failing-tests-first coverage for the inline formatting context (Task 18): whitespace
//! processing (CSS Text 3 §4.1.1), `parley` shaping, line breaking against the container's
//! content-box width, `text-align`, `line-height`, and the `Line`/`Text` fragments the
//! shaped result becomes.
//!
//! Every fixture that asserts exact geometry uses the bundled **Ahem** face: every Ahem glyph
//! is a square exactly one em wide (`crates/fonts/src/db.rs`'s `ahem_glyph_should_be_square_em`
//! proves it against the embedded bytes), so at `font-size: 16px` one glyph advances exactly
//! `16px` = `Au(960)` and the arithmetic in each test's doc comment is exact rather than
//! font-version-dependent. Fixtures also reset `body { margin: 0 }` and the UA `<p>`/`<div>`
//! margins so each content box starts at the viewport origin, the same isolation
//! `tests/block.rs` uses.
//!
//! Note what a `Line` fragment's rect is and is not: per CSS 2.1 §9.4.2 it spans its
//! containing block's *content width*, whatever the text inside it does, so these tests
//! assert the text's own extent through `line_extent` (the runs' origins and advances) and
//! assert the `Line` rect separately, against the container.
#![allow(
    clippy::expect_used,
    reason = "a failed setup step in a test should abort that test, loudly"
)]

#[path = "common/mod.rs"]
mod common;

use cl_dom::{Document, NodeId, local_name};
use cl_layout::{Au, Fragment, FragmentKind, FragmentTree, GlyphRun};

/// The first element in `doc` whose `id` attribute is `id`.
fn find_by_id(doc: &Document, id: &str) -> NodeId {
    common::find_element(doc, |el| {
        el.attrs
            .iter()
            .any(|a| a.name.local == local_name!("id") && &*a.value == id)
    })
    .expect("fixture must have an element with the expected id")
}

/// Every fragment of `tree`, in a deterministic pre-order walk from the root (an explicit
/// stack, not recursion, matching the rest of this crate's traversals).
fn walk(tree: &FragmentTree) -> Vec<&Fragment> {
    let mut out = Vec::new();
    let mut stack = vec![tree.root];
    while let Some(id) = stack.pop() {
        let Some(f) = tree.get(id) else {
            continue;
        };
        out.push(f);
        let mut kids: Vec<_> = f.children.clone();
        kids.reverse();
        stack.extend(kids);
    }
    out
}

/// The fragment generated for the element with `id`.
fn fragment_by_id<'a>(tree: &'a FragmentTree, doc: &Document, id: &str) -> &'a Fragment {
    let node = find_by_id(doc, id);
    walk(tree)
        .into_iter()
        .find(|f| f.node == Some(node))
        .expect("fragment tree must contain a fragment for the element with the expected id")
}

/// The `Line` fragments directly under the element with `id`, in document order.
fn lines_of<'a>(tree: &'a FragmentTree, doc: &Document, id: &str) -> Vec<&'a Fragment> {
    let parent = fragment_by_id(tree, doc, id);
    parent
        .children
        .iter()
        .filter_map(|c| tree.get(*c))
        .filter(|f| f.kind == FragmentKind::Line)
        .collect()
}

/// Every [`GlyphRun`] under `line`, in visual order.
fn runs_of<'a>(tree: &'a FragmentTree, line: &'a Fragment) -> Vec<&'a GlyphRun> {
    line.children
        .iter()
        .filter_map(|c| tree.get(*c))
        .filter_map(|f| match &f.kind {
            FragmentKind::Text { runs } => Some(runs),
            _ => None,
        })
        .flatten()
        .collect()
}

/// Every glyph under `line`, in visual order.
fn glyph_count(tree: &FragmentTree, line: &Fragment) -> usize {
    runs_of(tree, line).iter().map(|r| r.glyphs.len()).sum()
}

/// The inline extent the text of `line` actually occupies: `(left edge, width)`, taken from
/// the runs' own origins and advances.
///
/// This is *not* the `Line` fragment's own width — a line box is as wide as its containing
/// block (CSS 2.1 §9.4.2) and `text-align` moves the content inside it, so the occupied
/// extent has to be read off the runs.
fn line_extent(tree: &FragmentTree, line: &Fragment) -> (Au, Au) {
    let runs = runs_of(tree, line);
    let (Some(first), Some(last)) = (runs.first(), runs.last()) else {
        return (line.border_box.origin.x, Au::ZERO);
    };
    let end = last
        .glyphs
        .iter()
        .fold(last.origin.x, |x, g| Au(x.0 + g.advance.0));
    (first.origin.x, Au(end.0 - first.origin.x.0))
}

/// Asserts that `line`'s own rect spans the content box of the element with `id`, as CSS 2.1
/// §9.4.2 requires of every line box.
fn assert_line_spans_container(tree: &FragmentTree, doc: &Document, id: &str, line: &Fragment) {
    let container = fragment_by_id(tree, doc, id);
    assert_eq!(
        line.border_box.origin.x, container.content_box.origin.x,
        "a line box starts at its containing block's content-box left edge"
    );
    assert_eq!(
        line.border_box.size.w, container.content_box.size.w,
        "a line box is as wide as its containing block (CSS 2.1 §9.4.2)"
    );
    assert_eq!(
        line.border_box, line.content_box,
        "a line box has no box model of its own"
    );
}

/// The style block every Ahem fixture shares: no UA margins anywhere, Ahem at 16px.
const AHEM: &str = "body, p, div, pre { margin: 0; padding: 0 } \
                    #t { font-family: Ahem; font-size: 16px }";

/// One Ahem glyph's advance at `font-size: 16px`: one em = 16 CSS px = 960 app units.
const AHEM_ADVANCE: Au = Au(960);

/// A single short word fits on one line: `"hello"` in Ahem at 16px is 5 × 16 = 80px wide,
/// far inside the 800px viewport, so exactly one `Line` fragment with one run of 5 glyphs
/// comes out.
#[test]
fn short_text_should_produce_one_line() {
    let html = format!("<style>{AHEM}</style><body><p id=\"t\">hello</p></body>");
    let (tree, styled) = common::layout_html(&html);
    let lines = lines_of(&tree, styled.document(), "t");
    assert_eq!(lines.len(), 1, "one line box");
    let line = lines.first().expect("one line");
    assert_eq!(glyph_count(&tree, line), 5, "one glyph per character");
    assert_eq!(
        line_extent(&tree, line),
        (Au::ZERO, Au(960 * 5)),
        "the text occupies five Ahem ems from the content-box left edge"
    );
    assert_line_spans_container(&tree, styled.document(), "t", line);
}

/// Line breaking happens at the container's *content-box* width: `"xxxxx xxxxx xxxxx"` in
/// Ahem at 16px is three 80px words separated by 16px spaces, so in a 100px-wide container
/// each word takes its own line — three lines, none wider than 100px, the first holding
/// exactly the five glyphs of the first word (its trailing collapsible space hangs and
/// neither counts toward the line's width nor produces a glyph, CSS Text 3 §4.1.1).
#[test]
fn long_text_should_wrap_at_available_width() {
    let html = format!(
        "<style>{AHEM} #t {{ width: 100px }}</style><body><div id=\"t\">xxxxx xxxxx xxxxx</div></body>"
    );
    let (tree, styled) = common::layout_html(&html);
    let lines = lines_of(&tree, styled.document(), "t");
    assert_eq!(lines.len(), 3, "three line boxes");
    for line in &lines {
        let (_, width) = line_extent(&tree, line);
        assert!(
            width <= Au::from_px(100.0),
            "line content wider than the 100px container: {width:?}"
        );
        assert_line_spans_container(&tree, styled.document(), "t", line);
    }
    let first = lines.first().expect("three lines");
    assert_eq!(glyph_count(&tree, first), 5, "first line holds one word");
    assert_eq!(line_extent(&tree, first), (Au::ZERO, Au(960 * 5)));
}

/// `<br>` forces a line break wherever it falls, even though the text either side would
/// easily have fit on one line — and contributes no glyph of its own.
#[test]
fn br_should_force_line_break() {
    let html = format!("<style>{AHEM}</style><body><p id=\"t\">a<br>b</p></body>");
    let (tree, styled) = common::layout_html(&html);
    let lines = lines_of(&tree, styled.document(), "t");
    assert_eq!(lines.len(), 2, "one line either side of the <br>");
    for line in &lines {
        assert_eq!(glyph_count(&tree, line), 1, "one glyph per line");
        assert_eq!(line_extent(&tree, line), (Au::ZERO, AHEM_ADVANCE));
    }
}

/// `white-space: pre` preserves every space and breaks at every `\n`: `"a  b\ncd"` becomes
/// two lines, the first four glyphs wide (`a`, two spaces, `b` — Ahem's space is a full em
/// like every other glyph) and the second two.
#[test]
fn white_space_pre_should_preserve_spaces_and_newlines() {
    let html = format!("<style>{AHEM}</style><body><pre id=\"t\">a  b\ncd</pre></body>");
    let (tree, styled) = common::layout_html(&html);
    let lines = lines_of(&tree, styled.document(), "t");
    assert_eq!(lines.len(), 2, "the embedded newline breaks the line");
    let first = lines.first().expect("two lines");
    let second = lines.get(1).expect("two lines");
    assert_eq!(glyph_count(&tree, first), 4, "a, space, space, b");
    assert_eq!(line_extent(&tree, first), (Au::ZERO, Au(960 * 4)));
    assert_eq!(glyph_count(&tree, second), 2, "c, d");
    assert_eq!(line_extent(&tree, second), (Au::ZERO, Au(960 * 2)));
}

/// `white-space: pre` has no soft-wrap opportunities at all, so a `pre` line wider than its
/// containing block *overflows* instead of wrapping (CSS Text 3 §3, §5): ten Ahem glyphs at
/// 16px are 160px of text in a 100px box, and they stay on one line.
#[test]
fn white_space_pre_should_not_wrap_at_available_width() {
    // The unbreakable case first: ten `x`s have no break opportunity to take even if wrapping
    // were on, so this pins the overflow geometry.
    let html = format!(
        "<style>{AHEM} #t {{ width: 100px }}</style><body><pre id=\"t\">xxxxxxxxxx</pre></body>"
    );
    let (tree, styled) = common::layout_html(&html);
    let lines = lines_of(&tree, styled.document(), "t");
    assert_eq!(lines.len(), 1, "pre never soft-wraps");
    let line = lines.first().expect("one line");
    assert_eq!(glyph_count(&tree, line), 10);
    assert_eq!(
        line_extent(&tree, line),
        (Au::ZERO, Au(960 * 10)),
        "160px of text overflowing a 100px box"
    );
    assert_line_spans_container(&tree, styled.document(), "t", line);

    // And the case that actually needs `TextWrapMode::NoWrap`: with spaces there *are* break
    // opportunities, and `white-space: normal` would take them.
    let html = format!(
        "<style>{AHEM} #t {{ width: 100px }}</style><body><pre id=\"t\">xxxxx xxxxx</pre></body>"
    );
    let (tree, styled) = common::layout_html(&html);
    let lines = lines_of(&tree, styled.document(), "t");
    assert_eq!(lines.len(), 1, "pre must not break at its spaces");
    let line = lines.first().expect("one line");
    assert_eq!(glyph_count(&tree, line), 11, "five x, space, five x");
    assert_eq!(line_extent(&tree, line), (Au::ZERO, Au(960 * 11)));
}

/// A forced break at the *end* of the content adds no line box (CSS 2.1 §9.4.2): `parley`
/// reports one more, empty line there — a caret position — and it must not become a `Line`
/// fragment or add a line height to the block.
#[test]
fn trailing_br_should_not_add_a_line_box() {
    let html = format!("<style>{AHEM}</style><body><p id=\"t\">a<br></p></body>");
    let (tree, styled) = common::layout_html(&html);
    let lines = lines_of(&tree, styled.document(), "t");
    assert_eq!(
        lines.len(),
        1,
        "the trailing <br> ends the line, it adds none"
    );
    assert_eq!(glyph_count(&tree, lines.first().expect("one line")), 1);
    let target = fragment_by_id(&tree, styled.document(), "t");
    assert_eq!(
        target.content_box.size.h,
        Au::from_px(19.2),
        "one line height tall (1.2 x 16px), not two"
    );
}

/// The mirror of [`trailing_br_should_not_add_a_line_box`] for `white-space: pre`: a trailing
/// `\n` is the same forced break and behaves the same way.
#[test]
fn trailing_newline_in_pre_should_not_add_a_line_box() {
    let html = format!("<style>{AHEM}</style><body><pre id=\"t\">a\n</pre></body>");
    let (tree, styled) = common::layout_html(&html);
    let lines = lines_of(&tree, styled.document(), "t");
    assert_eq!(lines.len(), 1, "the trailing newline adds no line box");
    assert_eq!(glyph_count(&tree, lines.first().expect("one line")), 1);
}

/// A forced break in the *middle* is a different matter: two adjacent breaks leave a genuinely
/// empty line box between them, which must survive (only the *terminal* empty line is dropped).
#[test]
fn double_br_in_the_middle_should_add_an_empty_line() {
    let html = format!("<style>{AHEM}</style><body><p id=\"t\">a<br><br>b</p></body>");
    let (tree, styled) = common::layout_html(&html);
    let lines = lines_of(&tree, styled.document(), "t");
    assert_eq!(lines.len(), 3, "a, an empty line, b");
    assert_eq!(glyph_count(&tree, lines.first().expect("three lines")), 1);
    assert_eq!(
        glyph_count(&tree, lines.get(1).expect("three lines")),
        0,
        "the middle line box is empty but real"
    );
    assert_eq!(glyph_count(&tree, lines.get(2).expect("three lines")), 1);

    // The `pre` mirror: `a\n\nb` is the same three lines.
    let html = format!("<style>{AHEM}</style><body><pre id=\"t\">a\n\nb</pre></body>");
    let (tree, styled) = common::layout_html(&html);
    let lines = lines_of(&tree, styled.document(), "t");
    assert_eq!(lines.len(), 3, "a, an empty line, b");
    assert_eq!(glyph_count(&tree, lines.get(1).expect("three lines")), 0);
}

/// `white-space: normal` collapses every run of spaces, tabs and newlines to a single space
/// (CSS Text 3 §4.1.1), so `"a   b"` shapes as `"a b"`: three glyphs, three ems wide.
#[test]
fn white_space_normal_should_collapse_runs_of_spaces() {
    let html = format!("<style>{AHEM}</style><body><p id=\"t\">a   b</p></body>");
    let (tree, styled) = common::layout_html(&html);
    let lines = lines_of(&tree, styled.document(), "t");
    assert_eq!(lines.len(), 1);
    let line = lines.first().expect("one line");
    assert_eq!(glyph_count(&tree, line), 3, "a, one space, b");
    assert_eq!(line_extent(&tree, line), (Au::ZERO, Au(960 * 3)));
}

/// `text-align: center` offsets each line by half its free space: `"xx"` is 32px wide in a
/// 100px container, so the line box — and with it the run's origin — starts at
/// `(100 − 32) / 2 = 34px`.
#[test]
fn text_align_center_should_offset_runs() {
    let html = format!(
        "<style>{AHEM} #t {{ width: 100px; text-align: center }}</style>\
         <body><div id=\"t\">xx</div></body>"
    );
    let (tree, styled) = common::layout_html(&html);
    let lines = lines_of(&tree, styled.document(), "t");
    assert_eq!(lines.len(), 1);
    let line = lines.first().expect("one line");
    // The line *box* still spans the whole 100px container; the centring shows up in the
    // content's own extent and in the run origin.
    assert_line_spans_container(&tree, styled.document(), "t", line);
    assert_eq!(
        line_extent(&tree, line),
        (Au::from_px(34.0), Au(960 * 2)),
        "(100 - 32) / 2 = 34px of free space to the left of the text"
    );
    let runs = runs_of(&tree, line);
    let run = runs.first().expect("one run");
    assert_eq!(
        run.origin.x,
        Au::from_px(34.0),
        "run origin is centered too"
    );
}

/// `line-height` sets the line box height, independently of the font's own metrics: at
/// `line-height: 40px` every line box is exactly 40px = `Au(2400)` tall, and the second line
/// starts exactly one line height below the first.
#[test]
fn line_height_should_set_line_box_height() {
    let html = format!(
        "<style>{AHEM} #t {{ line-height: 40px; width: 100px }}</style>\
         <body><div id=\"t\">xxxxx xxxxx</div></body>"
    );
    let (tree, styled) = common::layout_html(&html);
    let lines = lines_of(&tree, styled.document(), "t");
    assert_eq!(lines.len(), 2);
    for line in &lines {
        assert_eq!(line.border_box.size.h, Au(2400));
    }
    let first = lines.first().expect("two lines");
    let second = lines.get(1).expect("two lines");
    assert_eq!(first.border_box.origin.y, Au::ZERO);
    assert_eq!(second.border_box.origin.y, Au(2400));
    let target = fragment_by_id(&tree, styled.document(), "t");
    assert_eq!(target.content_box.size.h, Au(4800), "two line boxes tall");
}

/// Ahem's defining property, straight off the fragment tree: every glyph advance at
/// `font-size: 16px` is exactly one em, `Au(960)`, and each glyph sits one advance further
/// along the run than the one before it.
#[test]
fn ahem_glyph_advance_should_equal_font_size() {
    let html = format!("<style>{AHEM}</style><body><p id=\"t\">xxx</p></body>");
    let (tree, styled) = common::layout_html(&html);
    let lines = lines_of(&tree, styled.document(), "t");
    let line = lines.first().expect("one line");
    let runs = runs_of(&tree, line);
    let run = runs.first().expect("one run");
    assert_eq!(run.size, Au::from_px(16.0));
    assert_eq!(run.glyphs.len(), 3);
    for (i, glyph) in run.glyphs.iter().enumerate() {
        assert_eq!(glyph.advance, AHEM_ADVANCE, "glyph {i} advance");
        assert_eq!(
            glyph.x,
            Au(960 * i32::try_from(i).expect("small index")),
            "glyph {i} x"
        );
        assert_eq!(glyph.y, Au::ZERO, "glyph {i} sits on the baseline");
    }
}

/// The same wrapping, with the default proportional font and the real UA geometry rather
/// than Ahem in a narrow box: a long paragraph in an 800px viewport breaks against
/// `<body>`'s content width, `800 − 2 × 8px` of UA margin = 784px. Asserted as bounds rather
/// than exact widths — Noto Sans' advances are a property of the bundled font file, not of
/// this crate — but a first line narrower than 700px would mean line breaking was measuring
/// against something other than the container.
#[test]
fn noto_sans_text_should_wrap_at_the_body_content_width() {
    let word = "wrapping ";
    let html = format!("<body><p id=\"t\">{}</p></body>", word.repeat(60));
    let (tree, styled) = common::layout_html(&html);
    let lines = lines_of(&tree, styled.document(), "t");
    assert!(lines.len() > 1, "a 60-word paragraph must wrap");
    for line in &lines {
        let (left, width) = line_extent(&tree, line);
        assert!(
            width <= Au::from_px(784.0),
            "line content wider than <body>'s 784px content box: {width:?}"
        );
        assert_eq!(left, Au::from_px(8.0), "left aligned");
        assert_line_spans_container(&tree, styled.document(), "t", line);
    }
    let (_, first_width) = line_extent(&tree, lines.first().expect("at least two lines"));
    assert!(
        first_width > Au::from_px(700.0),
        "the first line must be filled close to the 784px limit, was {first_width:?}"
    );
}

/// Inline content is attacker-controlled: an absurd `font-size`, a zero-width container, a
/// multi-megabyte text node and a `<br>`-only paragraph must all lay out without panicking
/// (clamped or saturated — see `cl_layout::block::layout`'s totality contract).
#[test]
fn hostile_inline_content_should_not_panic() {
    let cases = [
        String::from(
            "<style>body{margin:0} #t{font-size:1e9px}</style><body><div id=\"t\">x y</div></body>",
        ),
        String::from(
            "<style>body{margin:0} #t{width:0}</style><body><div id=\"t\">wrap me</div></body>",
        ),
        String::from("<body><p id=\"t\"><br><br><br></p></body>"),
        format!(
            "<style>body{{margin:0}}</style><body><div id=\"t\">{}</div></body>",
            "word ".repeat(200_000)
        ),
    ];
    for html in &cases {
        let (tree, styled) = common::layout_html(html);
        // Reached at all = no panic; the fragment must still exist and be well formed.
        let target = fragment_by_id(&tree, styled.document(), "t");
        assert!(target.border_box.size.w >= Au::ZERO);
        assert!(target.border_box.size.h >= Au::ZERO);
    }
}
