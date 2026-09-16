//! Task 22 smoke tests: pathological-scale or malformed input must never panic, and a
//! generously-sized ordinary document must render within the M1a plan's time budget: 5s
//! locally, 15s in CI (`.github/workflows/ci.yml`'s test job sets `CL_SMOKE_SLOW=1` for
//! exactly this — see [`budget`]).
#![allow(
    clippy::expect_used,
    reason = "a failed setup step in a test should abort that test, loudly"
)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use cl_net::Url;
use cl_testshell::{RenderOptions, render_bytes, render_file};

/// One flat repeating unit — never nested — so N copies make a *wide* document, not a deep
/// one; a deep one would test the DOM/box/fragment tree's iterative-walk guarantees (already
/// covered by their own crates' fuzz targets) rather than this crate's raw throughput.
const UNIT: &str = "<div><p>lorem ipsum dolor sit amet consectetur adipiscing elit</p></div>\n";

/// Builds a `<!doctype html><body>` document of repeated [`UNIT`]s at least `1_000_000` bytes
/// long.
fn generate_1mb_html() -> String {
    let mut html = String::from("<!doctype html><body>\n");
    while html.len() < 1_000_000 {
        html.push_str(UNIT);
    }
    html
}

/// A unique path under the system temp dir (flat, per the task brief — not nested under a
/// per-test subdirectory), following `crates/net/src/file.rs`'s pid-plus-tag convention.
fn temp_html_path(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "cl-testshell-smoke-{}-{tag}.html",
        std::process::id()
    ))
}

/// `< 5s` normally; `< 15s` under `CL_SMOKE_SLOW=1` for a slower/shared CI runner (the task
/// brief's own escape hatch, matching `crates/layout/tests/block.rs`'s identical convention
/// for its own deep-chain smoke test). `.github/workflows/ci.yml`'s test-matrix job sets
/// `CL_SMOKE_SLOW=1` on its `cargo nextest run --workspace --locked` step; a plain local
/// `cargo test` still gets the tighter 5s bound.
#[allow(
    clippy::disallowed_methods,
    reason = "`std::env::var` is restricted to `cl-platform` in production code so env access \
              stays centralized/testable there; this is a test-only smoke-test escape hatch \
              matching the plan's own documented convention for this exact env var, not \
              production configuration"
)]
fn budget() -> Duration {
    if std::env::var("CL_SMOKE_SLOW").as_deref() == Ok("1") {
        Duration::from_secs(15)
    } else {
        Duration::from_secs(5)
    }
}

#[test]
fn render_should_finish_a_1mb_document_within_the_time_budget() {
    let html = generate_1mb_html();
    assert!(html.len() >= 1_000_000, "fixture must be at least 1MB");
    let path = temp_html_path("1mb");
    std::fs::write(&path, &html).expect("write fixture");

    let start = Instant::now();
    let result = render_file(&path, &RenderOptions::default());
    let elapsed = start.elapsed();
    let _ = std::fs::remove_file(&path);

    // `UNIT` repeated ~14000 times to cross 1MB is several hundred thousand CSS pixels tall
    // (each `<p>` contributes its own line plus the UA sheet's `1em` top/bottom margin) — far
    // past the viewport. `cl_paint::build` culls any item that does not intersect the
    // viewport widened by `cl_paint::OFFSCREEN_MARGIN_PX` (controller ruling, Task 22 review:
    // a page of any height must still render at viewport size — see
    // `crates/paint/src/build.rs`'s "Culling offscreen items"), so this must succeed, not
    // merely fail gracefully at the raster step.
    result.expect(
        "a 1MB document must render successfully: cl_paint::build culls everything outside \
         the viewport's offscreen margin, so no page height should fail cl_gfx's validation",
    );
    let budget = budget();
    assert!(
        elapsed <= budget,
        "render took {elapsed:?}, budget is {budget:?}"
    );
}

#[test]
fn render_should_accept_a_document_truncated_to_100_bytes() {
    // The html5ever tree builder recovers from a document cut off mid-tag/mid-element the
    // same way it recovers from any other malformed markup (HTML's parse-error-recovery
    // algorithm) — this must stay `Ok`, not become a hard failure.
    let html = generate_1mb_html();
    let truncated = html
        .as_bytes()
        .get(..100.min(html.len()))
        .expect("100.min(len) is always a valid end bound");
    let base = Url::parse("file:///smoke/truncated.html").expect("base url");

    render_bytes(truncated, &base, &RenderOptions::default())
        .expect("a truncated-but-otherwise-valid document must still render");
}

#[test]
fn render_should_not_panic_on_64kib_of_non_utf8_garbage() {
    // A minimal LCG (linear congruential generator), fixed seed: no `rand` dependency (this
    // workspace only pulls it in behind fuzz targets), reproducible across runs and
    // platforms — unlike anything seeded from the environment or wall clock.
    let mut state: u64 = 0x5EED_5EED_5EED_5EED;
    let mut bytes = Vec::with_capacity(64 * 1024);
    for _ in 0..64 * 1024 {
        state = state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        #[allow(
            clippy::cast_possible_truncation,
            reason = "only the LCG's high byte is wanted, truncation is the point"
        )]
        let byte = (state >> 56) as u8;
        bytes.push(byte);
    }
    // The bytes must actually be invalid UTF-8 for this test to mean anything; an LCG could
    // in principle produce a valid sequence, but for this fixed seed and length it does not.
    assert!(
        std::str::from_utf8(&bytes).is_err(),
        "fixture must be non-UTF-8 for this test to exercise the intended path"
    );

    let base = Url::parse("file:///smoke/garbage.html").expect("base url");
    // `Ok` or `Err` are both acceptable outcomes for 64KiB of non-UTF-8 noise interpreted as
    // HTML — the only forbidden outcome is a panic, which `catch_unwind` would report as a
    // process abort well before this assertion, since `#[test]` itself already catches an
    // ordinary panic and reports it as a failure. No further assertion is needed beyond
    // calling the function to completion.
    let _ = render_bytes(&bytes, &base, &RenderOptions::default());
}
