//! M1b Task 6: every display list the real pipeline produces — all 44 reftest pages and the
//! 1 MB smoke page — survives the renderer → gpu wire format unchanged and fits its limits
//! with room to spare. Lives here, not in `cl-paint`, because it needs the whole pipeline.
#![allow(
    clippy::expect_used,
    reason = "a failed setup step in a test should abort that test, loudly"
)]

#[path = "common/mod.rs"]
mod common;

use cl_net::Url;
use cl_paint::wire::{MAX_DISPLAY_LIST_BYTES, decode_display_list, encode_display_list};
use cl_paint::{DisplayItem, DisplayList, MAX_ITEMS, MAX_TOTAL_GLYPHS};
use cl_testshell::reftest::{default_dir, discover};
use cl_testshell::{RenderOptions, display_list_for_bytes};

/// Both pages of each of the 22 reftest pairs.
const EXPECTED_PAGE_COUNT: usize = 44;

fn roundtrip(dl: &DisplayList) -> usize {
    let bytes = encode_display_list(dl).expect("encode");
    let decoded = decode_display_list(&bytes, MAX_DISPLAY_LIST_BYTES).expect("decode");
    assert_eq!(&decoded, dl);
    bytes.len()
}

fn glyph_count(dl: &DisplayList) -> usize {
    dl.items
        .iter()
        .map(|item| match item {
            DisplayItem::Text { run } => run.glyphs.len(),
            _ => 0,
        })
        .sum()
}

#[test]
fn every_reftest_page_list_should_roundtrip() {
    let dir = default_dir();
    let mut pages = 0;
    for name in discover(&dir).expect("discover reftest pairs") {
        for file in [format!("{name}.html"), format!("{name}-ref.html")] {
            let path = dir.join(&file).canonicalize().expect("canonical page path");
            let base = Url::from_file_path(&path).expect("file url");
            let bytes = std::fs::read(&path).expect("read page");
            let dl = display_list_for_bytes(&bytes, &base, &RenderOptions::default())
                .expect("display list");
            roundtrip(&dl);
            pages += 1;
        }
    }
    assert_eq!(pages, EXPECTED_PAGE_COUNT);
}

#[test]
fn smoke_page_list_should_roundtrip_within_a_quarter_of_every_limit() {
    let html = common::generate_1mb_html();
    let base = Url::parse("file:///smoke.html").expect("base url");
    let dl = display_list_for_bytes(html.as_bytes(), &base, &RenderOptions::default())
        .expect("display list");
    let encoded = roundtrip(&dl);
    // The limits were set at ≥ 4× this page's numbers (cl_paint::wire's doc comments).
    assert!(encoded * 4 <= MAX_DISPLAY_LIST_BYTES, "{encoded} bytes");
    assert!(dl.items.len() * 4 <= MAX_ITEMS, "{} items", dl.items.len());
    let glyphs = glyph_count(&dl);
    assert!(glyphs * 4 <= MAX_TOTAL_GLYPHS, "{glyphs} glyphs");
}
