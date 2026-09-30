//! html5lib-tests tree-construction conformance harness for `cl_html`/`cl_dom`.
//!
//! The corpus (`tools/conformance/html5lib/tree-construction/*.dat`) is vendored at the commit
//! recorded in `tools/conformance/html5lib/PINNED_COMMIT` — see
//! `tools/conformance/html5lib/README.md` for how that commit was chosen (html5lib-tests
//! `master` no longer contains `tree-construction/`; it was migrated into web-platform-tests)
//! and how to refresh it. Known failures are tracked in
//! `tools/conformance/html5lib/expectations.txt`; this test asserts both directions: every case
//! that is not conformant is listed there, and every case listed there is actually still
//! failing (a stale entry would silently hide a future regression). It then asserts the overall
//! pass rate meets the M1a gate, and that not a single case panics the parser or serializer.
//!
//! For each non-skipped case: `cl_html::parse_document_str(data, base)` builds a
//! [`cl_dom::Document`], `cl_dom::serialize::html5lib_tree` renders it in the corpus's own
//! `#document` dump format, and the result is compared line-for-line against the case's
//! `#document` block (see [`normalize`] for the one intentional bit of slack: trailing
//! whitespace per line). `#document-fragment` cases (fragment parsing is not implemented) and
//! `#script-on` cases (this crate always parses with scripting disabled — see
//! `cl_html::run_parser`'s doc comment) are skipped, not failed; every `#script-on` case in this
//! corpus has a `#script-off` twin with the same `#data` that IS run.
#![allow(clippy::expect_used, clippy::panic)]
// The whole point of this harness is the CI-readable summary line (same convention as
// crates/net/tests/urltestdata.rs); `tracing` isn't wired into `#[test]` output capture.
#![allow(clippy::print_stdout)]

use std::collections::HashSet;
use std::fmt::Write as _;
use std::panic::{self, AssertUnwindSafe};

use cl_dom::serialize::html5lib_tree;
use cl_html::parse_document_str;
use cl_net::Url;

/// Gate from task-9 of the M1a plan: `docs/superpowers/plans/2026-09-07-m1a-static-pipeline.md`.
const MIN_PASS_RATE: f64 = 0.90;

const EXPECTATIONS: &str = include_str!("../../../tools/conformance/html5lib/expectations.txt");

/// `(file name, raw .dat source)` for every vendored tree-construction file — see
/// `tools/conformance/html5lib/fetch.sh` for how these were chosen (the exact list from the
/// M1a plan's Task 9) and vendored.
const FILES: &[(&str, &str)] = &[
    (
        "tests1.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/tests1.dat"),
    ),
    (
        "tests2.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/tests2.dat"),
    ),
    (
        "tests3.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/tests3.dat"),
    ),
    (
        "tests4.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/tests4.dat"),
    ),
    (
        "tests5.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/tests5.dat"),
    ),
    (
        "tests6.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/tests6.dat"),
    ),
    (
        "tests7.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/tests7.dat"),
    ),
    (
        "tests8.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/tests8.dat"),
    ),
    (
        "tests9.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/tests9.dat"),
    ),
    (
        "tests10.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/tests10.dat"),
    ),
    (
        "tests11.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/tests11.dat"),
    ),
    (
        "tests12.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/tests12.dat"),
    ),
    // tests13.dat has never existed in this corpus (skipped by upstream, not a gap here).
    (
        "tests14.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/tests14.dat"),
    ),
    (
        "tests15.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/tests15.dat"),
    ),
    (
        "tests16.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/tests16.dat"),
    ),
    (
        "tests17.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/tests17.dat"),
    ),
    (
        "tests18.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/tests18.dat"),
    ),
    (
        "tests19.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/tests19.dat"),
    ),
    (
        "tests20.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/tests20.dat"),
    ),
    (
        "tests21.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/tests21.dat"),
    ),
    (
        "tests22.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/tests22.dat"),
    ),
    (
        "tests23.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/tests23.dat"),
    ),
    (
        "tests24.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/tests24.dat"),
    ),
    (
        "tests25.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/tests25.dat"),
    ),
    (
        "tests26.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/tests26.dat"),
    ),
    (
        "doctype01.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/doctype01.dat"),
    ),
    (
        "entities01.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/entities01.dat"),
    ),
    (
        "entities02.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/entities02.dat"),
    ),
    (
        "comments01.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/comments01.dat"),
    ),
    (
        "adoption01.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/adoption01.dat"),
    ),
    (
        "adoption02.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/adoption02.dat"),
    ),
    (
        "tables01.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/tables01.dat"),
    ),
    (
        "template.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/template.dat"),
    ),
    (
        "tricky01.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/tricky01.dat"),
    ),
    (
        "webkit01.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/webkit01.dat"),
    ),
    (
        "webkit02.dat",
        include_str!("../../../tools/conformance/html5lib/tree-construction/webkit02.dat"),
    ),
];

/// One parsed `.dat` test case. See [`parse_dat`] for the file format.
struct Case {
    /// The `#data` section: the HTML source to parse, exactly as written. May be empty, and
    /// (per the corpus format) may itself contain embedded blank lines.
    data: String,
    /// The `#document` section: the expected `html5lib_tree` dump.
    document: String,
    /// Whether this case carries a `#document-fragment` section. Fragment parsing is not
    /// implemented (M1a scope), so these cases are always skipped, never failed.
    is_fragment: bool,
    /// Whether this case carries a `#script-on` section. `cl_html` always parses with
    /// scripting disabled, so these are always skipped; every `#script-on` case in this corpus
    /// has a `#script-off` (or unmarked, which defaults to the same behaviour) twin that is
    /// run instead.
    is_script_on: bool,
}

/// Which section of a `.dat` case a content line currently belongs to. `Discard` covers every
/// section this harness does not need the text of: `#errors`, `#new-errors` (we don't check
/// error diagnostics, only the tree), the `#document-fragment` context line (fragment cases are
/// skipped outright), and `#script-on`/`#script-off` (which never carry content lines anyway).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Section {
    Data,
    Document,
    Discard,
}

/// Accumulates one `.dat` case's sections while [`parse_dat`] walks the file line by line.
#[derive(Default)]
struct CaseBuilder {
    section: Option<Section>,
    data_lines: Vec<String>,
    document_lines: Vec<String>,
    is_fragment: bool,
    is_script_on: bool,
}

impl CaseBuilder {
    /// Switches the section currently being accumulated into, in response to a `#<heading>`
    /// line. Panics on a heading this format does not define — a malformed corpus file must
    /// fail loudly here, not silently misparse.
    fn set_section(&mut self, file: &str, heading: &str, line_no: usize) {
        self.section = Some(match heading {
            "data" => Section::Data,
            "document" => Section::Document,
            "document-fragment" => {
                self.is_fragment = true;
                Section::Discard
            }
            "script-on" => {
                self.is_script_on = true;
                Section::Discard
            }
            "script-off" | "errors" | "new-errors" => Section::Discard,
            other => panic!("{file}:{line_no}: unrecognized .dat section heading '#{other}'"),
        });
    }

    /// Appends one content line (a line that is not itself a `#`-prefixed heading) to whichever
    /// section is currently active.
    fn push_line(&mut self, line: &str) {
        match self.section {
            Some(Section::Data) => self.data_lines.push(line.to_string()),
            Some(Section::Document) => self.document_lines.push(line.to_string()),
            Some(Section::Discard) | None => {}
        }
    }

    /// Finalizes the accumulated lines into a [`Case`].
    ///
    /// The corpus always separates consecutive cases with exactly one blank line (verified
    /// against every file vendored here when this harness was written), and because
    /// `#document` is always a case's last section, that blank line lands as a trailing empty
    /// string in `document_lines` for every case but the file's last. It is dropped here: a
    /// genuine `#document` block never contains a blank line of its own (every real line is
    /// prefixed `| `, per `cl_dom::serialize`'s format), so this can never discard real
    /// content — only the separator. `data_lines` gets no such treatment: `#data` is followed
    /// immediately by `#errors`, never by a case-separating blank line, so an embedded blank
    /// line there (the corpus has several) is always genuine `#data` content, e.g. a case whose
    /// entire input is the empty string.
    fn finish(mut self) -> Case {
        if self.document_lines.last().is_some_and(String::is_empty) {
            self.document_lines.pop();
        }
        Case {
            data: self.data_lines.join("\n"),
            document: self.document_lines.join("\n"),
            is_fragment: self.is_fragment,
            is_script_on: self.is_script_on,
        }
    }
}

/// Parses `source` (the contents of one `tree-construction/*.dat` file, whose name is `file`,
/// used only for panic messages) into its test cases.
///
/// Format: cases are separated by a blank line before the next `#data` heading. Within a case,
/// `#data` (required, first) is followed by `#errors`, optionally `#new-errors`, optionally
/// `#document-fragment <context>`, optionally `#script-on`/`#script-off`, then `#document`
/// (required, last). Every line starting with `#` is treated as a new section heading — that
/// matches the reference `html5lib` Python test runner (`html5lib/tests/support.py`'s
/// `TestData.isSectionHeading`) exactly, including its willingness to treat *any* `#`-prefixed
/// line this way rather than only recognized keywords; none of the vendored files here ever put
/// a literal `#`-prefixed line inside `#data` content, so the two behaviours coincide in
/// practice, but matching the reference behaviour rather than a hand-picked keyword list is
/// what makes this a faithful reimplementation of the corpus's own format rather than a
/// close-enough guess.
fn parse_dat(file: &str, source: &str) -> Vec<Case> {
    let mut cases = Vec::new();
    let mut current: Option<CaseBuilder> = None;
    for (zero_based_line, line) in source.lines().enumerate() {
        let line_no = zero_based_line + 1;
        if let Some(heading) = line.strip_prefix('#') {
            let heading = heading.trim();
            if heading == "data" {
                if let Some(builder) = current.take() {
                    cases.push(builder.finish());
                }
                current = Some(CaseBuilder::default());
            }
            match current.as_mut() {
                Some(builder) => builder.set_section(file, heading, line_no),
                None => panic!(
                    "{file}:{line_no}: section heading '#{heading}' before the file's first '#data'"
                ),
            }
        } else if let Some(builder) = current.as_mut() {
            builder.push_line(line);
        }
        // A non-heading line before the very first `#data` would only be a leading blank line;
        // every vendored file starts directly with `#data` (verified when vendoring), so this
        // branch is unreachable on real corpus files and is silently ignored rather than
        // panicking over a merely-cosmetic leading blank line.
    }
    if let Some(builder) = current.take() {
        cases.push(builder.finish());
    }
    cases
}

/// The one intentional bit of slack in the tree comparison: trailing whitespace is trimmed from
/// every line (both the actual and expected output), then lines are rejoined with `\n`. Nothing
/// else is normalized — no line reordering, no ignoring blank lines, no case folding — because
/// over-normalizing would inflate the measured pass rate above what the parser and serializer
/// actually produce. `cl_dom::serialize::html5lib_tree`'s documented format never emits trailing
/// whitespace on a line, so in practice this normalization is a no-op for our own output; it
/// exists purely so a hypothetical trailing space in a vendored `.dat` file's `#document` block
/// does not register as a spurious mismatch.
fn normalize(text: &str) -> String {
    text.lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
}

/// Outcome of running one non-skipped case.
enum Outcome {
    Pass,
    Fail(String),
    /// `parse_document_str` or `html5lib_tree` panicked. Tracked separately from `Fail` per the
    /// task-9 gate: a panic fails the whole harness regardless of the measured pass rate, since
    /// it means the parser can be made to crash on corpus input, not just mis-parse it.
    Panic(String),
}

fn run_case(case: &Case, base: &Url) -> Outcome {
    let result = panic::catch_unwind(AssertUnwindSafe(|| {
        let output = parse_document_str(&case.data, base)
            .expect("parse_document_str is infallible (HtmlError is uninhabited)");
        html5lib_tree(&output.document)
    }));

    let actual = match result {
        Ok(tree) => tree,
        Err(payload) => return Outcome::Panic(panic_message(&payload)),
    };

    // `html5lib_tree` includes the leading `#document` line in its output (see its doc
    // comment); the corpus's own `#document` *section* does not (that line is the section
    // heading, consumed as such by `parse_dat`, never stored as content) — so strip it here
    // before comparing against `case.document`, rather than storing it redundantly on every
    // case.
    let actual_body = actual
        .strip_prefix("#document")
        .map_or(actual.as_str(), |rest| {
            rest.strip_prefix('\n').unwrap_or(rest)
        });

    if normalize(actual_body) == normalize(&case.document) {
        Outcome::Pass
    } else {
        Outcome::Fail(format!(
            "tree mismatch:\n--- expected ---\n{}\n--- actual ---\n{actual_body}",
            case.document
        ))
    }
}

/// Extracts a printable message from a `catch_unwind` panic payload, which is almost always a
/// `&'static str` (a `panic!("literal")`) or `String` (a `panic!("{}", ...)`) but is typed
/// `Box<dyn Any + Send>` because `std::panic::catch_unwind` cannot know that ahead of time.
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

/// Parse `expectations.txt`. Lines are `<file>:<index>\t# reason`; blank lines and full-line
/// `#` comments are ignored. Panics with a precise, file-relative message on any malformed
/// line — a broken expectations file must fail loudly, not silently accept garbage.
fn load_expectations(text: &str) -> HashSet<(String, usize)> {
    let mut out = HashSet::new();
    for (zero_based_line, raw_line) in text.lines().enumerate() {
        let line_no = zero_based_line + 1;
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        assert!(
            trimmed.contains('#'),
            "expectations.txt:{line_no}: entry missing a `# reason` comment: {trimmed:?}"
        );
        let key_field = trimmed
            .split_whitespace()
            .next()
            .expect("non-empty trimmed line has at least one whitespace-separated token");
        let (file, index_str) = key_field.split_once(':').unwrap_or_else(|| {
            panic!("expectations.txt:{line_no}: entry {key_field:?} is not `<file>:<index>`")
        });
        let index: usize = index_str.parse().unwrap_or_else(|e| {
            panic!(
                "expectations.txt:{line_no}: case index {index_str:?} is not a plain integer: {e}"
            )
        });
        assert!(
            out.insert((file.to_string(), index)),
            "expectations.txt:{line_no}: duplicate entry for case {key_field}"
        );
    }
    out
}

/// Running totals across every vendored file.
#[derive(Default)]
struct Totals {
    passed: usize,
    expected_failures: usize,
    unexpected_failures: Vec<(String, usize, String)>,
    panics: Vec<(String, usize, String)>,
    skipped_fragment: usize,
    skipped_script_on: usize,
}

#[allow(clippy::cast_precision_loss)] // diagnostic percentage over test counts, far below f64's exact-integer range
fn print_summary_line(prefix: &str, passed: usize, non_skipped: usize, suffix: &str) {
    let pct = if non_skipped == 0 {
        0.0
    } else {
        100.0 * passed as f64 / non_skipped as f64
    };
    println!("{prefix}{passed}/{non_skipped} ({pct:.1}%) passed{suffix}");
}

/// Runs every case in one `.dat` file, folding the outcomes into `totals`/`seen_expected`, and
/// prints that file's one-line summary. Split out of the `#[test]` fn purely to keep it under
/// clippy's `too_many_lines` threshold — this is still a straight-line per-file loop body, not
/// an independently reusable unit.
fn evaluate_file(
    file: &'static str,
    source: &'static str,
    base: &Url,
    expectations: &HashSet<(String, usize)>,
    seen_expected: &mut HashSet<(String, usize)>,
    totals: &mut Totals,
) {
    let cases = parse_dat(file, source);
    let mut file_passed = 0usize;
    let mut file_failed = 0usize;
    let mut file_skipped = 0usize;

    for (index, case) in cases.iter().enumerate() {
        if case.is_fragment {
            totals.skipped_fragment += 1;
            file_skipped += 1;
            continue;
        }
        if case.is_script_on {
            totals.skipped_script_on += 1;
            file_skipped += 1;
            continue;
        }

        match run_case(case, base) {
            Outcome::Pass => {
                totals.passed += 1;
                file_passed += 1;
            }
            Outcome::Fail(reason) => {
                file_failed += 1;
                let key = (file.to_string(), index);
                if expectations.contains(&key) {
                    totals.expected_failures += 1;
                    seen_expected.insert(key);
                } else {
                    totals
                        .unexpected_failures
                        .push((file.to_string(), index, reason));
                }
            }
            Outcome::Panic(message) => {
                file_failed += 1;
                totals.panics.push((file.to_string(), index, message));
            }
        }
    }

    print_summary_line(
        &format!("  {file}: "),
        file_passed,
        file_passed + file_failed,
        &format!(", {file_skipped} skipped"),
    );
}

/// Prints the overall summary line and enforces the task-9 gate: zero panics, zero un-triaged
/// (“unexpected”) failures, zero stale `expectations.txt` entries, and a pass rate over
/// non-skipped cases at or above [`MIN_PASS_RATE`]. Split out of the `#[test]` fn purely to
/// keep it under clippy's `too_many_lines` threshold.
#[allow(clippy::cast_precision_loss)] // diagnostic percentage over test counts, far below f64's exact-integer range
fn report_and_assert(totals: &Totals, stale: &[(String, usize)]) {
    let total_skipped = totals.skipped_fragment + totals.skipped_script_on;
    let non_skipped = totals.passed
        + totals.expected_failures
        + totals.unexpected_failures.len()
        + totals.panics.len();
    print_summary_line(
        "html5lib tree-construction: ",
        totals.passed,
        non_skipped,
        &format!(
            " — {} expected failures, {} unexpected, {total_skipped} skipped ({} \
             #document-fragment, {} #script-on), {} panics",
            totals.expected_failures,
            totals.unexpected_failures.len(),
            totals.skipped_fragment,
            totals.skipped_script_on,
            totals.panics.len()
        ),
    );

    if !totals.panics.is_empty() {
        let mut msg = String::new();
        for (file, index, message) in &totals.panics {
            let _ = writeln!(msg, "  {file}:{index}: {message}");
        }
        panic!(
            "{} case(s) panicked the parser or serializer — this fails the harness \
             unconditionally, regardless of pass rate:\n{msg}",
            totals.panics.len()
        );
    }

    if !totals.unexpected_failures.is_empty() {
        let mut msg = String::new();
        for (file, index, reason) in &totals.unexpected_failures {
            let _ = writeln!(msg, "  {file}:{index}: {reason}");
        }
        panic!(
            "{} case(s) fail html5lib tree-construction conformance and are not listed in \
             expectations.txt:\n{msg}\
             (either fix the parser/DOM/serializer, or add `<file>:<index>\\t# reason` lines to \
             expectations.txt)",
            totals.unexpected_failures.len()
        );
    }

    assert!(
        stale.is_empty(),
        "expectations.txt lists case(s) that actually pass now — stale entries, remove them: {stale:?}"
    );

    assert!(non_skipped > 0, "corpus produced zero comparable cases");
    let pass_rate = totals.passed as f64 / non_skipped as f64;
    assert!(
        pass_rate >= MIN_PASS_RATE,
        "pass rate {pass_rate:.4} ({}/{non_skipped}) is below the {MIN_PASS_RATE} gate",
        totals.passed
    );
}

#[test]
fn html5lib_tree_construction_conformance() {
    let base = Url::parse("http://example.test/").expect("static url");
    let expectations = load_expectations(EXPECTATIONS);
    let mut seen_expected: HashSet<(String, usize)> = HashSet::new();
    let mut totals = Totals::default();

    println!("html5lib tree-construction conformance, per file:");
    for &(file, source) in FILES {
        evaluate_file(
            file,
            source,
            &base,
            &expectations,
            &mut seen_expected,
            &mut totals,
        );
    }

    let stale: Vec<(String, usize)> = {
        let mut v: Vec<(String, usize)> =
            expectations.difference(&seen_expected).cloned().collect();
        v.sort();
        v
    };

    report_and_assert(&totals, &stale);
}
