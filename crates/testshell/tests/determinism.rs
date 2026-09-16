//! Task 22: the M1a pipeline must be deterministic — same document, same pixels, regardless
//! of which call or which thread rendered it. Each [`render_file`] call builds its own
//! [`cl_style::StyleEngine`]/[`cl_fonts::FontDb`]/etc. from scratch (see
//! `crates/testshell/src/pipeline.rs`), so nothing is shared across the two renders compared
//! here — a real cross-process guarantee, not just "the same in-memory state twice".
#![allow(
    clippy::expect_used,
    reason = "a failed setup step in a test should abort that test, loudly"
)]

use std::path::{Path, PathBuf};

use cl_testshell::{RenderOptions, render_file};

fn ref_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ref/text-basic.html")
}

#[test]
fn render_should_be_deterministic_across_two_calls() {
    let opts = RenderOptions::default();
    let path = ref_path();

    let a = render_file(&path, &opts).expect("render a").pixmap;
    let b = render_file(&path, &opts).expect("render b").pixmap;

    assert_eq!(a.width(), b.width());
    assert_eq!(a.height(), b.height());
    assert_eq!(
        a.data(),
        b.data(),
        "identical input must produce identical pixels"
    );
}

#[test]
fn render_should_be_deterministic_across_threads() {
    let path = ref_path();

    let main_thread_pixmap = render_file(&path, &RenderOptions::default())
        .expect("render on the test's own thread")
        .pixmap;

    let handle = std::thread::spawn(move || {
        render_file(&path, &RenderOptions::default())
            .expect("render on a second thread")
            .pixmap
    });
    let second_thread_pixmap = handle.join().expect("rendering thread must not panic");

    assert_eq!(
        main_thread_pixmap.data(),
        second_thread_pixmap.data(),
        "the same document must render identically regardless of which thread runs the \
         pipeline — each render_file call owns its whole pipeline, nothing is shared"
    );
}
