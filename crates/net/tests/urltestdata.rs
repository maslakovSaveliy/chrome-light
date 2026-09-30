//! WPT `url/resources/urltestdata.json` conformance harness for `cl_net::Url`.
//!
//! The corpus (`tools/conformance/url/urltestdata.json`) is vendored at the commit recorded in
//! `tools/conformance/url/PINNED_COMMIT` — see `tools/conformance/url/README.md` to refresh it.
//! Known failures are tracked in `tools/conformance/url/expectations.txt`; this test asserts
//! both directions: every case that is not `Url`-conformant is listed there, and every case
//! listed there is actually still failing (a stale entry would silently hide a future
//! regression). It then asserts the overall pass rate meets the M1a gate.
#![allow(clippy::expect_used, clippy::panic)]
// The whole point of this harness is the CI-readable summary line (task spec: "Print a summary
// line ... so CI logs are useful"); `tracing` isn't wired into `#[test]` output capture.
#![allow(clippy::print_stdout)]

use std::collections::HashSet;
use std::fmt::Write as _;

use cl_net::Url;
use serde::Deserialize;

/// Gate from task-6 of the M1a plan: `docs/superpowers/plans/2026-09-07-m1a-static-pipeline.md`.
const MIN_PASS_RATE: f64 = 0.92;

const CORPUS: &str = include_str!("../../../tools/conformance/url/urltestdata.json");
const EXPECTATIONS: &str = include_str!("../../../tools/conformance/url/expectations.txt");

/// One entry of the WPT `urltestdata.json` array: either a `string` comment (skipped) or a
/// test-case object. See `tools/conformance/url/README.md` for the field meanings.
#[derive(Deserialize)]
#[serde(untagged)]
enum Entry {
    // Only matched to skip comment strings; never read.
    #[allow(dead_code)]
    Comment(String),
    Case(Case),
}

#[derive(Deserialize)]
struct Case {
    input: String,
    base: Option<String>,
    #[serde(default)]
    failure: bool,
    href: Option<String>,
}

/// Result of running the WHATWG URL parser on one WPT case.
enum Outcome {
    /// The result matched what the case expected (`failure` or `href`).
    Pass,
    /// The result did not match what the case expected; carries a short diagnostic.
    Fail(String),
    /// The case's own `base` failed to parse against `cl_net::Url`. A harness-level skip, not a
    /// pass or fail: the corpus assumes a conformant parser for `base` too, and every case in
    /// this corpus is expected to have a `base` that `cl_net::Url` also accepts, but we count
    /// this separately (per the task spec) rather than silently folding it into "fail".
    HarnessSkip,
}

fn run_case(case: &Case) -> Outcome {
    let result = match &case.base {
        None => Url::parse(&case.input),
        Some(base_str) => match Url::parse(base_str) {
            Ok(base) => Url::parse_with_base(&case.input, &base),
            Err(_) => return Outcome::HarnessSkip,
        },
    };

    if case.failure {
        return match result {
            Err(_) => Outcome::Pass,
            Ok(u) => Outcome::Fail(format!("expected failure, parsed as {:?}", u.as_str())),
        };
    }

    let expected_href = case
        .href
        .as_deref()
        .expect("corpus case without `failure: true` must carry `href` (malformed corpus entry)");
    match result {
        Ok(u) if u.as_str() == expected_href => Outcome::Pass,
        Ok(u) => Outcome::Fail(format!(
            "href mismatch: expected {expected_href:?}, got {:?}",
            u.as_str()
        )),
        Err(e) => Outcome::Fail(format!(
            "expected success ({expected_href:?}), got error: {e}"
        )),
    }
}

/// Parse `expectations.txt`. Lines are `<index>\t# reason`; blank lines and lines starting with
/// `#` are ignored. Panics with a precise, file-relative message on any malformed line — a
/// broken expectations file must fail loudly, not silently accept garbage or index-panic.
fn load_expectations(text: &str) -> HashSet<usize> {
    let mut out = HashSet::new();
    for (line_no, raw_line) in text.lines().enumerate() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        assert!(
            trimmed.contains('#'),
            "expectations.txt:{}: entry missing a `# reason` comment: {trimmed:?}",
            line_no + 1
        );
        let index_field = trimmed
            .split_whitespace()
            .next()
            .expect("non-empty trimmed line has at least one whitespace-separated token");
        let index: usize = match index_field.parse() {
            Ok(i) => i,
            Err(e) => panic!(
                "expectations.txt:{}: case index {index_field:?} is not a plain integer: {e}",
                line_no + 1
            ),
        };
        assert!(
            out.insert(index),
            "expectations.txt:{}: duplicate entry for case {index}",
            line_no + 1
        );
    }
    out
}

#[test]
#[allow(clippy::cast_precision_loss)] // diagnostic percentage over test counts, far below f64's exact-integer range
fn wpt_url_conformance() {
    let entries: Vec<Entry> = serde_json::from_str(CORPUS)
        .expect("tools/conformance/url/urltestdata.json must be valid JSON matching the WPT urltestdata.json schema");
    let expectations = load_expectations(EXPECTATIONS);

    let mut total = 0usize;
    let mut pass = 0usize;
    let mut harness_skips = 0usize;
    let mut unexpected_failures: Vec<(usize, String)> = Vec::new();
    let mut seen_expected: HashSet<usize> = HashSet::new();

    for (index, entry) in entries.iter().enumerate() {
        let Entry::Case(case) = entry else { continue };
        match run_case(case) {
            Outcome::HarnessSkip => harness_skips += 1,
            Outcome::Pass => {
                total += 1;
                pass += 1;
            }
            Outcome::Fail(reason) => {
                total += 1;
                if expectations.contains(&index) {
                    seen_expected.insert(index);
                } else {
                    unexpected_failures.push((index, reason));
                }
            }
        }
    }

    let stale: Vec<usize> = {
        let mut v: Vec<usize> = expectations.difference(&seen_expected).copied().collect();
        v.sort_unstable();
        v
    };

    let pct = if total == 0 {
        0.0
    } else {
        100.0 * pass as f64 / total as f64
    };
    println!(
        "url conformance: {pass}/{total} ({pct:.1}%) — {} expected failures, {} unexpected ({harness_skips} harness skips: base failed to parse)",
        seen_expected.len(),
        unexpected_failures.len()
    );

    if !unexpected_failures.is_empty() {
        let mut msg = String::new();
        for (index, reason) in &unexpected_failures {
            let _ = writeln!(msg, "  case {index}: {reason}");
        }
        panic!(
            "{} case(s) fail WPT url conformance and are not listed in expectations.txt:\n{msg}\
             (either fix the parser, or add `<index>\\t# reason` lines to expectations.txt)",
            unexpected_failures.len()
        );
    }

    assert!(
        stale.is_empty(),
        "expectations.txt lists case(s) that actually pass now — stale entries, remove them: {stale:?}"
    );

    assert!(total > 0, "corpus produced zero comparable cases");
    let pass_rate = pass as f64 / total as f64;
    assert!(
        pass_rate >= MIN_PASS_RATE,
        "pass rate {pass_rate:.4} ({pass}/{total}) is below the {MIN_PASS_RATE} gate"
    );
}
