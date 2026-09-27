//! Tree-structural selectors over a very wide sibling list (debt D2.c).
//!
//! `selectors` evaluates `:nth-child`/`:nth-last-child`/`+`/`~` by stepping through
//! `Element::{prev,next}_sibling_element`, so how much one step costs is multiplied by the
//! number of siblings each match visits. A page controls its sibling count, so that step has
//! to be O(1): these tests run the whole public pipeline (`cl_html::parse_document_str` →
//! `StyleEngine::resolve`) over one parent with tens of thousands of element children and
//! check both the time and the answer — a fast wrong colour is as much a failure as a slow
//! right one.
#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a failed setup step or a missing fixture node in a test should abort that test, \
              loudly"
)]

use cl_dom::{Document, NodeId, local_name};
use cl_net::Url;
use cl_style::StyleEngine;
use style::color::AbsoluteColor;

/// The base URL every fixture is parsed and resolved against.
const BASE_URL: &str = "file:///style/siblings.html";

/// The viewport, matching `tests/computed_style_goldens.rs`.
const VIEWPORT: (f32, f32) = (800.0, 600.0);

/// The wall-clock budget for a time-bounded test here: 5s normally, 15s when
/// `CL_SMOKE_SLOW=1` is set — the escape hatch every smoke test in this repo uses for a slow
/// or loaded CI runner (`.github/workflows/ci.yml` sets it).
#[allow(
    clippy::disallowed_methods,
    reason = "`std::env::var` is restricted to `cl-platform` in production code so env access \
              stays centralized/testable there; this is a test-only smoke-test escape hatch \
              matching the plan's own documented convention for this exact env var, not \
              production configuration"
)]
fn smoke_time_limit() -> std::time::Duration {
    if std::env::var("CL_SMOKE_SLOW").as_deref() == Ok("1") {
        std::time::Duration::from_secs(15)
    } else {
        std::time::Duration::from_secs(5)
    }
}

/// Every `<p>` element of `doc`, in document order.
fn paragraphs(doc: &Document) -> Vec<NodeId> {
    doc.descendants(doc.root())
        .filter(|id| {
            doc.element(*id)
                .is_some_and(|el| el.name.local == local_name!("p"))
        })
        .collect()
}

/// A `<div>` with 20 000 `<p>` children and `p:nth-child(2n+1) { color: red }`.
///
/// The children are separated by whitespace text nodes, as in any hand-written page, so the
/// sibling-element step also has to skip a non-element each time. Only `resolve` is timed —
/// parsing is `cl-html`'s cost. Child 1 and child 19 999 are odd (red); child 2 and child
/// 20 000 are even (the UA's black).
#[test]
fn nth_child_over_twenty_thousand_siblings_should_resolve_under_the_time_bound() {
    const CHILDREN: usize = 20_000;

    let mut html = String::from("<body><div>\n");
    for _ in 0..CHILDREN {
        html.push_str("<p></p>\n");
    }
    html.push_str("</div></body>");

    let base = Url::parse(BASE_URL).expect("base url");
    let doc = cl_html::parse_document_str(&html, &base)
        .expect("parse")
        .document;
    let mut engine = StyleEngine::new(VIEWPORT, 1.0).expect("engine");
    engine.add_ua_sheet().expect("ua sheet");
    engine
        .add_author_sheet("p:nth-child(2n+1) { color: red }", BASE_URL)
        .expect("author sheet");

    let start = std::time::Instant::now();
    let styled = engine.resolve(doc).expect("resolve");
    let elapsed = start.elapsed();

    let limit = smoke_time_limit();
    assert!(
        elapsed < limit,
        "resolving `p:nth-child(2n+1)` over {CHILDREN} siblings took {elapsed:?}, expected \
         well under {limit:?} — this smells like a sibling-element step regressed from a \
         precomputed link to a walk over the sibling list. Set CL_SMOKE_SLOW=1 to raise the \
         bound to 15s on a slow/loaded machine."
    );

    let ps = paragraphs(styled.document());
    assert_eq!(
        ps.len(),
        CHILDREN,
        "every <p> must be a child of the one <div>"
    );
    let red = AbsoluteColor::srgb_legacy(255, 0, 0, 1.0);
    let color = |child: usize| {
        styled
            .computed(ps[child - 1])
            .expect("every <p> is styled")
            .clone_color()
    };
    assert_eq!(color(1), red, "child 1 is odd");
    assert_ne!(color(2), red, "child 2 is even");
    assert_eq!(color(19_999), red, "child 19 999 is odd");
    assert_ne!(color(20_000), red, "child 20 000 is even");
}
