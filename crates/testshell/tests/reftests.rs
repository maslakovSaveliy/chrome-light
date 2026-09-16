//! Task 23: runs every reftest pair under `tests/ref` as a single cargo test, asserting the
//! full M1a suite is 22/22 `PASS` (a `FAIL (known)` still counts as a failure here — see
//! `cl_testshell::reftest`'s module docs; this suite is not green until a known bug is fixed
//! and its marker removed). 22, not the brief's original 20: `margin-collapse-siblings-whitespace`
//! was added as regression coverage for the box-generation fix in `crates/layout/src/box_tree.rs`
//! (CSS 2.1 §9.2.2.1) once that bug was found and fixed during this task, and
//! `max-width-auto-margins` as regression coverage for the M1a final review's B1 (CSS 2.1 §10.4's
//! min/max clamp was never applied to an `auto` width).
#![allow(
    clippy::expect_used,
    reason = "a failed setup step in a test should abort that test, loudly"
)]

use cl_testshell::reftest::{self, default_dir, default_failures_dir};

/// The M1a reftest pair count: the brief's original 20, plus
/// `margin-collapse-siblings-whitespace` (regression coverage for the box-generation fix in
/// `crates/layout/src/box_tree.rs`, CSS 2.1 §9.2.2.1) and `max-width-auto-margins` (regression
/// coverage for the min/max-width clamp on an `auto` width, CSS 2.1 §10.4).
const EXPECTED_PAIR_COUNT: usize = 22;

#[test]
fn all_m1a_reftest_pairs_should_pass() {
    let dir = default_dir();
    let names = reftest::discover(&dir).expect("discover reftest pairs");
    assert_eq!(
        names.len(),
        EXPECTED_PAIR_COUNT,
        "expected exactly {EXPECTED_PAIR_COUNT} reftest pairs under {dir:?}, found {}: {names:?}",
        names.len()
    );

    let results = reftest::run(&dir, None, &default_failures_dir()).expect("run reftest suite");
    let failures: Vec<String> = results
        .iter()
        .filter(|r| r.is_failure())
        .map(reftest::PairResult::line)
        .collect();

    assert!(
        failures.is_empty(),
        "{}/{} reftest pairs failed:\n{}",
        failures.len(),
        results.len(),
        failures.join("\n")
    );
}
