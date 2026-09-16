//! Task 23: runs every reftest pair under `tests/ref` as a single cargo test, asserting the
//! full M1a suite is 20/20 `PASS` (a `FAIL (known)` still counts as a failure here — see
//! `cl_testshell::reftest`'s module docs; this suite is not green until a known bug is fixed
//! and its marker removed).
#![allow(
    clippy::expect_used,
    reason = "a failed setup step in a test should abort that test, loudly"
)]

use cl_testshell::reftest::{self, default_dir, default_failures_dir};

/// The M1a plan's exact reftest pair count (Task 23's brief lists all twenty by name).
const EXPECTED_PAIR_COUNT: usize = 20;

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
