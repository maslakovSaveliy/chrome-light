//! Shared setup for `cl-layout`'s integration tests: parses and styles a document through
//! the same public-API path `crates/style/tests/computed_style_goldens.rs` uses, so a
//! `cl_style::StyledDocument` here is built exactly the way the rest of the pipeline builds
//! one — no test-only shortcut around the cascade.
//!
//! This file lives at `tests/common/mod.rs` rather than `tests/common.rs` so cargo does not
//! treat it as its own test binary (only files directly under `tests/` become separate test
//! crates); each test file pulls it in with `#[path = "common/mod.rs"] mod common;`.
#![allow(
    clippy::expect_used,
    reason = "a failed setup step in a test should abort that test, loudly"
)]

use cl_dom::{Document, Element, NodeId};
use cl_net::{NetError, Url};
use cl_style::{StyleEngine, StyledDocument};

/// The base URL every fixture is parsed and resolved against.
pub const BASE_URL: &str = "file:///layout/test.html";

/// The viewport fixtures are styled against, matching `cl-style`'s own goldens so a document
/// shared between the two crates' tests resolves identically.
pub const VIEWPORT: (f32, f32) = (800.0, 600.0);

/// Parses `html`, collects every `<style>` sheet it contains, and resolves it — the same
/// three-step pipeline (`cl_html::parse_document_str` → `StyleEngine::add_ua_sheet` +
/// `collect_document_sheets` → `StyleEngine::resolve`) `cl-style`'s golden tests run.
///
/// `load` always fails, exactly as in `cl-style`'s goldens: no fixture here uses a `<link>`,
/// so a fixture that started reading a file off disk would silently stop being reproducible.
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
                "layout test fixtures load no external sheets".into(),
            ))
        })
        .expect("collect sheets");
    assert!(
        warnings.is_empty(),
        "layout test fixtures must not depend on external stylesheets: {warnings:?}"
    );

    engine.resolve(doc).expect("resolve")
}

/// The first node in document order whose element matches `predicate`.
///
/// Not every test binary that includes this module calls this (each `tests/*.rs` file is
/// compiled as its own crate), hence `allow(dead_code)` rather than leaving one binary warn.
#[allow(dead_code)]
pub fn find_element(doc: &Document, predicate: impl Fn(&Element) -> bool) -> Option<NodeId> {
    doc.descendants(doc.root())
        .find(|id| doc.element(*id).is_some_and(&predicate))
}
