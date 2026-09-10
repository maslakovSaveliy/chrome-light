//! Golden computed-style dumps for five documents, plus the "never panic" cases.
//!
//! Each test runs the whole M1a static style pipeline through the crate's *public* API —
//! `cl_html::parse_document_str` → `StyleEngine::{add_ua_sheet, collect_document_sheets,
//! resolve}` → `cl_style::dump::computed_style_dump` — and snapshots the result with
//! `insta`. The fixtures live in `tests/golden/*.html` so a rule can be edited without
//! touching Rust, and the accepted snapshots in `tests/snapshots/`.
#![allow(
    clippy::expect_used,
    reason = "a failed setup step in a golden test should abort that test, loudly"
)]

use std::fmt::Write as _;

use cl_net::{NetError, Url};
use cl_style::StyleEngine;
use cl_style::dump::computed_style_dump;

/// The base URL every fixture is parsed and resolved against.
const BASE_URL: &str = "file:///golden/test.html";

/// The viewport the goldens are computed for. Fixed, because `em`/percentage resolution and
/// any future viewport unit would otherwise make the snapshots depend on the machine.
const VIEWPORT: (f32, f32) = (800.0, 600.0);

/// Runs the full pipeline over `html` and returns its computed-style dump.
///
/// The `load` callback handed to `collect_document_sheets` always fails: no fixture uses a
/// `<link>`, and a golden that silently started reading a file off disk would stop being
/// reproducible. A `<link>` appearing in a fixture therefore shows up as a missing sheet in
/// the dump rather than as a hidden dependency.
fn dump(html: &str) -> String {
    let base = Url::parse(BASE_URL).expect("base url");
    let doc = cl_html::parse_document_str(html, &base)
        .expect("parse")
        .document;

    let mut engine = StyleEngine::new(VIEWPORT, 1.0).expect("engine");
    engine.add_ua_sheet().expect("ua sheet");
    let warnings = engine
        .collect_document_sheets(&doc, &|_url: &Url| -> Result<Vec<u8>, NetError> {
            Err(NetError::UnsupportedScheme(
                "golden fixtures load no external sheets".into(),
            ))
        })
        .expect("collect sheets");
    assert!(
        warnings.is_empty(),
        "golden fixtures must not depend on external stylesheets: {warnings:?}"
    );

    let styled = engine.resolve(doc).expect("resolve");
    computed_style_dump(&styled)
}

/// The smallest real document: doctype, `<head>` (`display: none`, so its `<title>` is
/// never traversed) and one `<p>` styled entirely by the UA sheet.
#[test]
fn minimal_document() {
    insta::assert_snapshot!(dump(include_str!("golden/minimal.html")));
}

/// Nested block boxes with author margins, padding, borders and an explicit width — the
/// geometry longhands `cl-layout` reads first.
#[test]
fn nested_divs_with_margins() {
    insta::assert_snapshot!(dump(include_str!("golden/nested-divs.html")));
}

/// Inline `<span>`s inside a styled `<p>`: proves inheritance (`color`, `font-size`,
/// `line-height` reach both spans) and non-inheritance (`background-color` does not).
#[test]
fn inline_spans() {
    insta::assert_snapshot!(dump(include_str!("golden/inline-spans.html")));
}

/// `<pre>`: the one UA rule that changes white-space handling, so the dump's
/// `white-space-collapse` / `text-wrap-mode` pair is `preserve` / `nowrap` rather than the
/// inherited `collapse` / `wrap`.
#[test]
fn preformatted_text() {
    insta::assert_snapshot!(dump(include_str!("golden/pre.html")));
}

/// Two `<style>` sheets whose rules collide. The dump pins all three tie-breakers at once:
/// source order between equal-specificity rules (`p { color }`, second sheet wins),
/// specificity over source order (`p.pick` beats the later `p`, `#target` beats the later
/// `p.pick`), and the `style` attribute over every author rule (`font-size`).
#[test]
fn conflicting_sheets_resolve_by_specificity_then_order() {
    insta::assert_snapshot!(dump(include_str!("golden/cascade-conflict.html")));
}

/// A document far larger than any fixture must still resolve, not blow the stack: the
/// traversal is iterative (stylo's `style_trees` works off a `VecDeque`), and so is the
/// dump's walk.
#[test]
fn ten_thousand_elements_should_resolve() {
    let mut html = String::from("<body>");
    for i in 0..10_000 {
        let _ = write!(html, "<div class=\"c{i}\">x</div>");
    }
    html.push_str("</body>");

    let base = Url::parse(BASE_URL).expect("base url");
    let doc = cl_html::parse_document_str(&html, &base)
        .expect("parse")
        .document;
    let mut engine = StyleEngine::new(VIEWPORT, 1.0).expect("engine");
    engine.add_ua_sheet().expect("ua sheet");
    engine
        .add_author_sheet("div { color: red }", BASE_URL)
        .expect("author sheet");

    let styled = engine.resolve(doc).expect("resolve");
    let styled_elements = styled
        .document()
        .descendants(styled.document().root())
        .filter(|id| styled.computed(*id).is_some())
        .count();
    // <html>, <head>, <body> and the 10 000 divs.
    assert_eq!(styled_elements, 10_003);
}

/// A pathologically long selector must be matched (or rejected) without crashing. A
/// thousand descendant combinators is well past anything a real page contains, and the
/// depth of the match is bounded by the document, not by the selector.
#[test]
fn a_thousand_compound_selector_should_not_crash() {
    let selector = vec!["div"; 1000].join(" ");
    let css = format!("{selector} {{ color: red }}");

    let base = Url::parse(BASE_URL).expect("base url");
    let doc = cl_html::parse_document_str("<div><div><p>hi</p></div></div>", &base)
        .expect("parse")
        .document;
    let mut engine = StyleEngine::new(VIEWPORT, 1.0).expect("engine");
    engine.add_ua_sheet().expect("ua sheet");
    engine
        .add_author_sheet(&css, BASE_URL)
        .expect("author sheet");

    let styled = engine.resolve(doc).expect("resolve");
    assert!(
        styled
            .document()
            .descendants(styled.document().root())
            .any(|id| styled.computed(id).is_some()),
        "the document still resolves with a 1000-selector rule attached"
    );
}

/// Garbage in a `style` attribute is dropped declaration by declaration, never surfaced as
/// an error and never a panic: the element keeps whatever the stylesheets gave it, plus any
/// declaration in the attribute that did parse.
#[test]
fn malformed_style_attributes_should_not_crash() {
    let html = concat!(
        "<p style=\"color\">a</p>",
        "<p style=\"color:\">b</p>",
        "<p style=\";;;:::\">c</p>",
        "<p style=\"color: notacolor; font-size: 12px\">d</p>",
        "<p style=\"}{&quot;\">e</p>",
        "<p style=\"\">f</p>",
    );

    let base = Url::parse(BASE_URL).expect("base url");
    let doc = cl_html::parse_document_str(html, &base)
        .expect("parse")
        .document;
    let mut engine = StyleEngine::new(VIEWPORT, 1.0).expect("engine");
    engine.add_ua_sheet().expect("ua sheet");
    engine
        .add_author_sheet("p { color: red }", BASE_URL)
        .expect("author sheet");

    let styled = engine.resolve(doc).expect("resolve");
    let paragraphs: Vec<_> = styled
        .document()
        .descendants(styled.document().root())
        .filter(|id| {
            styled
                .document()
                .element(*id)
                .is_some_and(|el| el.name.local == cl_dom::local_name!("p"))
        })
        .collect();
    assert_eq!(paragraphs.len(), 6);
    for id in paragraphs {
        assert!(
            styled.computed(id).is_some(),
            "every <p> is styled despite its attribute"
        );
    }
}
