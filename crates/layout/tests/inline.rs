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
        line.border_box.size.w,
        Au(960 * 5),
        "line width is five Ahem ems"
    );
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
        assert!(
            line.border_box.size.w <= Au::from_px(100.0),
            "line wider than the 100px container: {:?}",
            line.border_box.size.w
        );
    }
    let first = lines.first().expect("three lines");
    assert_eq!(glyph_count(&tree, first), 5, "first line holds one word");
    assert_eq!(first.border_box.size.w, Au(960 * 5));
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
        assert_eq!(line.border_box.size.w, AHEM_ADVANCE);
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
    assert_eq!(first.border_box.size.w, Au(960 * 4));
    assert_eq!(glyph_count(&tree, second), 2, "c, d");
    assert_eq!(second.border_box.size.w, Au(960 * 2));
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
    assert_eq!(line.border_box.size.w, Au(960 * 3));
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
    assert_eq!(line.border_box.origin.x, Au::from_px(34.0));
    assert_eq!(line.border_box.size.w, Au(960 * 2));
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
        assert!(
            line.border_box.size.w <= Au::from_px(784.0),
            "line wider than <body>'s 784px content box: {:?}",
            line.border_box.size.w
        );
        assert_eq!(line.border_box.origin.x, Au::from_px(8.0), "left aligned");
    }
    let first = lines.first().expect("at least two lines");
    assert!(
        first.border_box.size.w > Au::from_px(700.0),
        "the first line must be filled close to the 784px limit, was {:?}",
        first.border_box.size.w
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
