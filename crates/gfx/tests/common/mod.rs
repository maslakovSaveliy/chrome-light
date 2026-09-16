//! Shared setup for `cl-gfx`'s integration tests: parses and styles a document, lays it
//! out, and returns the resulting fragment tree plus the `StyledDocument` it was built
//! from — the same three-stage pipeline `crates/layout/tests/common/mod.rs` sets up.
//!
//! Copied rather than reused: `cl-gfx`'s tests must not depend on `cl-layout`'s test files
//! (Task 21's controller ruling), so this is a second, independent copy of the same
//! public-API pipeline `crates/style/tests/computed_style_goldens.rs` and
//! `crates/layout/tests/common/mod.rs` already set up, not a shortcut around any of it.
#![allow(
    clippy::expect_used,
    reason = "a failed setup step in a test should abort that test, loudly"
)]

use cl_fonts::FontDb;
use cl_layout::{FragmentTree, Viewport};
use cl_net::{NetError, Url};
use cl_style::{StyleEngine, StyledDocument};

/// The base URL every fixture is parsed and resolved against.
pub const BASE_URL: &str = "file:///gfx/test.html";

/// The viewport fixtures are styled and laid out against, matching `cl-layout`'s and
/// `cl-style`'s own goldens so a document shared between the three crates' tests resolves
/// identically.
pub const VIEWPORT: (f32, f32) = (800.0, 600.0);

/// Parses `html`, collects every `<style>` sheet it contains, and resolves it — the same
/// three-step pipeline (`cl_html::parse_document_str` -> `StyleEngine::add_ua_sheet` +
/// `collect_document_sheets` -> `StyleEngine::resolve`) `cl-layout`'s and `cl-style`'s
/// goldens run.
///
/// `load` always fails: no fixture here uses a `<link>`, so a fixture that started reading a
/// file off disk would silently stop being reproducible.
pub fn styled_document(html: &str) -> StyledDocument {
    let base = Url::parse(BASE_URL).expect("base url");
    let doc = cl_html::parse_document_str(html, &base)
        .expect("parse")
        .document;

    let mut engine = StyleEngine::new(VIEWPORT, 1.0).expect("engine");
    engine.add_ua_sheet().expect("ua sheet");
    let warnings = engine
        .collect_document_sheets(&doc, &|_url: &Url| -> Result<Vec<u8>, NetError> {
            Err(NetError::UnsupportedScheme(
                "gfx test fixtures load no external sheets".into(),
            ))
        })
        .expect("collect sheets");
    assert!(
        warnings.is_empty(),
        "gfx test fixtures must not depend on external stylesheets: {warnings:?}"
    );

    engine.resolve(doc).expect("resolve")
}

/// Runs the full pipeline (parse, style, build the box tree, lay it out) over `html` against
/// [`VIEWPORT`] and returns both the resulting fragment tree and the [`StyledDocument`] it
/// was built from — callers need the latter to call [`cl_paint::build`], which takes a
/// `&cl_dom::Document` (via `StyledDocument::document()`).
pub fn layout_html(html: &str) -> (FragmentTree, StyledDocument) {
    let styled = styled_document(html);
    let mut fonts = FontDb::bundled().expect("bundled font db");
    let viewport = Viewport::new(VIEWPORT.0, VIEWPORT.1);
    let tree = cl_layout::layout(&styled, viewport, &mut fonts).expect("layout");
    (tree, styled)
}
