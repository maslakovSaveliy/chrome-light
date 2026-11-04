//! End-to-end, three processes: the real binary renders a page headless — browser, renderer and
//! gpu children — and its PNG must equal `cl_testshell::render_file`'s pixels for the same page
//! and viewport (M1b plan Task 10). The single-process shell is the reference: both run the same
//! `cl-renderer` pipeline and the same CPU rasteriser, so any difference is the process split's.
#![allow(clippy::expect_used, reason = "test assertions on subprocess output")]

use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

use cl_gfx::Pixmap;
use cl_testshell::{
    RenderOptions,
    reftest::{default_dir, discover},
};

/// Pages on both sides of the M1a reftest pairs (22 pairs).
const EXPECTED_PAGES: usize = 44;

/// Only keeps a hung browser from hanging the run; not a timing assertion.
const RUN_LIMIT: Duration = Duration::from_mins(2);

/// A scratch directory for one test, empty at the start.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("cl-parity-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// Runs the binary with `args`, killing it after [`RUN_LIMIT`].
fn run_bounded(args: &[&str]) -> Output {
    let start = Instant::now();
    let mut child = Command::new(env!("CARGO_BIN_EXE_chromelight"))
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn binary");
    let mut stdout = child.stdout.take().expect("stdout pipe");
    let mut stderr = child.stderr.take().expect("stderr pipe");
    let out_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let err_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        buf
    });
    let status = loop {
        if let Some(status) = child.try_wait().expect("try_wait") {
            break status;
        }
        if start.elapsed() > RUN_LIMIT {
            let _ = child.kill();
            break child.wait().expect("wait after kill");
        }
        std::thread::park_timeout(Duration::from_millis(10));
    };
    Output {
        status,
        stdout: out_reader.join().expect("stdout reader"),
        stderr: err_reader.join().expect("stderr reader"),
    }
}

/// Renders `page` headless into `png`; `extra` goes before the page. Panics with the browser's
/// stderr if it does not exit 0.
fn headless(page: &Path, png: &Path, extra: &[&str]) -> Output {
    let page = page.to_str().expect("utf-8 page path");
    let png_arg = png.to_str().expect("utf-8 png path");
    let mut args = vec!["--no-sandbox", "--headless", "--png", png_arg];
    args.extend_from_slice(extra);
    args.push(page);
    let out = run_bounded(&args);
    assert!(
        out.status.success(),
        "{page}: browser exited with {}:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    out
}

/// The single-process reference for `page` at `viewport`, through a PNG round trip like the
/// browser's output (PNG stores straight alpha; both sides go through the same conversion).
fn reference(page: &Path, viewport: (u32, u32)) -> Pixmap {
    let opts = RenderOptions {
        viewport,
        ..RenderOptions::default()
    };
    let out = cl_testshell::render_file(page, &opts).expect("testshell render");
    let png = out.pixmap.encode_png().expect("encode reference");
    Pixmap::decode_png(&png).expect("decode reference")
}

/// Asserts that the PNG at `png` has exactly `expected`'s size and pixels.
fn assert_same_pixels(what: &str, png: &Path, expected: &Pixmap) {
    let actual = Pixmap::load_png(png).expect("browser png");
    assert_eq!(
        (actual.width(), actual.height()),
        (expected.width(), expected.height()),
        "{what}: size"
    );
    let differing = actual
        .pixels()
        .iter()
        .zip(expected.pixels())
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(differing, 0, "{what}: {differing} pixels differ");
}

/// Every `.html` file under the reftest directory: tests and references.
fn reftest_pages() -> Vec<PathBuf> {
    let dir = default_dir();
    let pairs = discover(&dir).expect("discover reftest pairs");
    let mut pages = Vec::new();
    for name in pairs {
        pages.push(dir.join(format!("{name}.html")));
        pages.push(dir.join(format!("{name}-ref.html")));
    }
    pages
}

#[test]
fn parity_all_44_pages() {
    let pages = reftest_pages();
    assert_eq!(pages.len(), EXPECTED_PAGES, "{pages:?}");
    let out_dir = scratch("all");
    // Four browsers at a time: each is three processes, and the pages are independent.
    std::thread::scope(|scope| {
        for chunk in pages.chunks(pages.len().div_ceil(4)) {
            let out_dir = &out_dir;
            scope.spawn(move || {
                for page in chunk {
                    let stem = page.file_stem().expect("stem").to_string_lossy();
                    let png = out_dir.join(format!("{stem}.png"));
                    headless(page, &png, &[]);
                    assert_same_pixels(&stem, &png, &reference(page, (800, 600)));
                }
            });
        }
    });
}

#[test]
fn resize_parity_matches_testshell() {
    let page = default_dir().join("block-percent-width.html");
    let png = scratch("resize").join("resized.png");
    headless(
        &page,
        &png,
        &["--viewport", "800x600", "--resize-to", "500x700"],
    );
    assert_same_pixels("resized", &png, &reference(&page, (500, 700)));
}

#[test]
fn resize_to_the_same_size_writes_the_first_frame() {
    // No second frame is ever asked for when the surface does not change: the first one is
    // the answer (Task 10 review: the browser waited for it forever).
    let page = default_dir().join("block-percent-width.html");
    let png = scratch("same-size").join("same.png");
    headless(
        &page,
        &png,
        &["--viewport", "640x480", "--resize-to", "640x480"],
    );
    assert_same_pixels("same size", &png, &reference(&page, (640, 480)));
}

#[test]
fn link_outside_doc_dir_warns_and_renders() {
    let dir = scratch("outside");
    let doc_dir = dir.join("doc");
    std::fs::create_dir_all(&doc_dir).expect("doc dir");
    std::fs::write(
        dir.join("outside.css"),
        "html { background: rgb(255, 0, 0) }",
    )
    .expect("outside.css");
    let body = "<body><div style=\"width: 100px; height: 50px; background: blue\"></div></body>";
    let page = doc_dir.join("page.html");
    std::fs::write(
        &page,
        format!("<!doctype html><link rel=\"stylesheet\" href=\"../outside.css\">{body}"),
    )
    .expect("page");
    let expected = doc_dir.join("expected.html");
    std::fs::write(&expected, format!("<!doctype html>{body}")).expect("expected page");

    let png = dir.join("page.png");
    let out = headless(&page, &png, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("refused: outside the document directory"),
        "no refusal warning on stderr:\n{stderr}"
    );
    assert_same_pixels("outside link", &png, &reference(&expected, (800, 600)));
}
