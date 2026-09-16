//! The M1a reftest harness (Task 23): discovers `<name>.html` + `<name>-ref.html` pairs under
//! a directory, renders both through the real pipeline at a fixed 800×600 viewport, and
//! diffs the resulting pixmaps in memory (reusing [`crate::compare_pixmaps`] — the same
//! comparison [`crate::compare_png`] runs on two files on disk, just without the PNG
//! round-trip a reftest has no reason to take).
//!
//! # Known-fail pairs
//!
//! A page may carry an HTML comment `<!-- KNOWN-FAIL: <reason> -->` marking a pair whose
//! *ref* is correct but whose *engine output* has a real bug — never the other way around
//! (a ref is never adjusted to match a wrong engine output; see the module's task brief).
//! Such a pair still mismatches like any other failure (`differing_pixels > 0`) and its
//! artifacts are still written, but [`PairStatus::KnownFail`] carries the marker's reason so
//! callers can report it distinctly. It is still a failure for [`PairStatus::is_failure`]'s
//! purposes — the suite is not green until the bug is fixed and the marker removed.
//!
//! # No panics, no recursion
//!
//! Every per-pair failure mode (a missing ref, a render error, a pixel mismatch, a failed
//! artifact write) becomes a [`PairStatus`] variant, never a panic — this module has no
//! `unwrap`/`expect` outside its own tests. There is no recursion: [`discover`] walks one
//! flat directory (`tests/ref` is never expected to have subdirectories) and every other
//! function here is a straight-line pass over a pair or a pixel buffer.

use std::fs;
use std::path::{Path, PathBuf};

use tiny_skia::{Pixmap, PremultipliedColorU8};

use crate::pipeline::{RenderOptions, render_file};
use crate::{ShellError, compare_pixmaps};

/// The fixed viewport every reftest pair renders at.
const VIEWPORT: (u32, u32) = (800, 600);

/// One `<name>.html` + `<name>-ref.html` pair's outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairResult {
    /// The pair's shared name (`text-basic` for `text-basic.html`/`text-basic-ref.html`).
    pub name: String,
    /// What happened.
    pub status: PairStatus,
}

/// A [`PairResult`]'s outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairStatus {
    /// The page and its ref rendered pixel-identically.
    Pass,
    /// The page and its ref did not render identically (or could not both be rendered), for
    /// the given human-readable reason.
    Fail(String),
    /// Like [`PairStatus::Fail`], but the page carries a `KNOWN-FAIL` marker recording an
    /// already-identified engine bug — the ref is trusted, the engine output is not. Still a
    /// failure (see [`PairResult::is_failure`]); this variant only changes how it is
    /// reported.
    KnownFail(String),
}

impl PairResult {
    /// Whether this pair did not pass — [`PairStatus::Fail`] and [`PairStatus::KnownFail`]
    /// both count, so a suite-level assertion treats a known bug exactly as failing as any
    /// other, per the task brief ("the controller decides", not the runner).
    #[must_use]
    pub fn is_failure(&self) -> bool {
        !matches!(self.status, PairStatus::Pass)
    }

    /// A one-line human-readable rendering: `PASS <name>`, `FAIL <name> (<reason>)`, or
    /// `FAIL (known) <name> (<reason>)`.
    #[must_use]
    pub fn line(&self) -> String {
        match &self.status {
            PairStatus::Pass => format!("PASS {}", self.name),
            PairStatus::Fail(reason) => format!("FAIL {} ({reason})", self.name),
            PairStatus::KnownFail(reason) => format!("FAIL (known) {} ({reason})", self.name),
        }
    }
}

/// This crate's own bundled fixtures — the default `--dir` for the CLI's `reftest` command,
/// embedded at compile time (`env!("CARGO_MANIFEST_DIR")`) so the default works regardless of
/// the caller's current directory, matching every other fixture path in this crate's own
/// tests (`tests/determinism.rs`, `tests/cli.rs`).
#[must_use]
pub fn default_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/ref")
}

/// Where a failing pair's `test.png`/`ref.png`/`diff.png` are written: `target/reftest-failures`
/// at the workspace root, two directories above this crate's own manifest directory
/// (`crates/testshell/../../target`).
#[must_use]
pub fn default_failures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("target")
        .join("reftest-failures")
}

/// Discovers every reftest pair under `dir`: every `<name>.html` that is not itself a
/// `<name>-ref.html` and has a sibling `<name>-ref.html`. Names are returned sorted, for a
/// deterministic run order and deterministic printed output.
///
/// A page with no matching ref is *not* silently skipped here — [`discover`] still returns
/// pairs it can find rules for; a page whose ref is truly missing surfaces later, as a
/// [`PairStatus::Fail`] from [`run_pair`], never dropped from the count.
///
/// # Errors
/// [`ShellError::Io`] if `dir` cannot be read.
pub fn discover(dir: &Path) -> Result<Vec<String>, ShellError> {
    let mut names = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("html") {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        if stem.ends_with("-ref") {
            continue;
        }
        names.push(stem.to_owned());
    }
    names.sort();
    Ok(names)
}

/// Runs every pair `discover(dir)` finds whose name contains `filter` (a plain substring
/// match; `None` runs everything), writing failure artifacts for any mismatch under
/// `failures_dir`. Results are in the same sorted order [`discover`] returns.
///
/// # Errors
/// [`ShellError::Io`] if `dir` cannot be read (a per-pair render/compare failure is instead
/// folded into that pair's [`PairStatus::Fail`], never propagated here).
pub fn run(
    dir: &Path,
    filter: Option<&str>,
    failures_dir: &Path,
) -> Result<Vec<PairResult>, ShellError> {
    let names = discover(dir)?;
    Ok(names
        .into_iter()
        .filter(|name| filter.is_none_or(|f| name.contains(f)))
        .map(|name| run_pair(dir, &name, failures_dir))
        .collect())
}

/// Renders `<name>.html` and `<name>-ref.html` under `dir` at the fixed reftest viewport and
/// compares them, writing `test.png`/`ref.png`/`diff.png` under `failures_dir/<name>/` on any
/// mismatch. Never panics: a missing ref, a render error on either side, or a failed artifact
/// write all become part of the returned [`PairResult`] rather than propagating.
#[must_use]
pub fn run_pair(dir: &Path, name: &str, failures_dir: &Path) -> PairResult {
    let page = dir.join(format!("{name}.html"));
    let ref_page = dir.join(format!("{name}-ref.html"));
    let known_fail = known_fail_marker(&page);

    if !ref_page.is_file() {
        return finish(
            name,
            known_fail,
            format!("missing ref {}", ref_page.display()),
        );
    }

    let opts = RenderOptions { viewport: VIEWPORT };
    let test_pixmap = match render_file(&page, &opts) {
        Ok(output) => output.pixmap,
        Err(e) => return finish(name, known_fail, format!("render {name}.html: {e}")),
    };
    let ref_pixmap = match render_file(&ref_page, &opts) {
        Ok(output) => output.pixmap,
        Err(e) => return finish(name, known_fail, format!("render {name}-ref.html: {e}")),
    };

    let diff = match compare_pixmaps(&test_pixmap, &ref_pixmap) {
        Ok(diff) => diff,
        Err(e) => return finish(name, known_fail, e.to_string()),
    };

    if diff.differing_pixels == 0 {
        return PairResult {
            name: name.to_owned(),
            status: PairStatus::Pass,
        };
    }

    write_failure_artifacts(failures_dir, name, &test_pixmap, &ref_pixmap);
    finish(
        name,
        known_fail,
        format!("{} differing", diff.differing_pixels),
    )
}

/// Builds the [`PairResult`] for a failing pair: [`PairStatus::KnownFail`] with the marker's
/// reason appended when `known_fail` is `Some`, [`PairStatus::Fail`] with just `reason`
/// otherwise.
fn finish(name: &str, known_fail: Option<String>, reason: String) -> PairResult {
    let status = match known_fail {
        Some(marker) => PairStatus::KnownFail(format!("{reason}: {marker}")),
        None => PairStatus::Fail(reason),
    };
    PairResult {
        name: name.to_owned(),
        status,
    }
}

/// Reads `path` looking for a `<!-- KNOWN-FAIL: <reason> -->` comment, returning its trimmed
/// reason text. `None` for an unreadable file (the render step below reports that failure
/// itself) or one with no such marker — the overwhelmingly common case, so this is a plain
/// substring search rather than a full HTML-comment parse.
fn known_fail_marker(path: &Path) -> Option<String> {
    let text = fs::read_to_string(path).ok()?;
    let marker = "KNOWN-FAIL:";
    let start = text.find(marker)?;
    let rest = text.get(start.saturating_add(marker.len())..)?;
    let end = rest.find("-->")?;
    let reason = rest.get(..end)?.trim();
    if reason.is_empty() {
        None
    } else {
        Some(reason.to_owned())
    }
}

/// Best-effort diagnostics for a human investigating a failure: `test.png` (the page's own
/// render), `ref.png` (the ref's render), and `diff.png` (differing pixels in red on a white
/// ground) under `failures_dir/<name>/`. Never panics and never fails the caller — a reftest
/// failure is already reported through the [`PairResult`] the caller builds regardless of
/// whether these files could be written (a read-only `target/`, say, must not turn a
/// reporting nicety into a harder failure than the mismatch itself).
fn write_failure_artifacts(failures_dir: &Path, name: &str, test: &Pixmap, reference: &Pixmap) {
    let dir = failures_dir.join(name);
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let _ = test.save_png(dir.join("test.png"));
    let _ = reference.save_png(dir.join("ref.png"));
    if let Some(diff) = diff_pixmap(test, reference) {
        let _ = diff.save_png(dir.join("diff.png"));
    }
}

/// Builds a same-size pixmap: white wherever `a`/`b` agree, opaque red wherever they differ.
/// `None` only if `a`/`b` disagree in size (should not happen — both are rendered at the same
/// fixed [`VIEWPORT`] — or if `Pixmap::new`/color construction somehow fails on a zero
/// dimension) or on any zero dimension, which [`Pixmap::new`] itself refuses.
fn diff_pixmap(a: &Pixmap, b: &Pixmap) -> Option<Pixmap> {
    if (a.width(), a.height()) != (b.width(), b.height()) {
        return None;
    }
    let mut out = Pixmap::new(a.width(), a.height())?;
    let white = PremultipliedColorU8::from_rgba(255, 255, 255, 255)?;
    let red = PremultipliedColorU8::from_rgba(255, 0, 0, 255)?;
    let pixels = out.pixels_mut();
    for (i, (x, y)) in a.pixels().iter().zip(b.pixels()).enumerate() {
        if let Some(px) = pixels.get_mut(i) {
            *px = if x == y { white } else { red };
        }
    }
    Some(out)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use super::*;

    /// A fresh scratch directory under the system temp dir, unique per test invocation (pid
    /// plus a caller-chosen tag), matching this crate's other tests' temp-dir convention.
    fn scratch_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cl-reftest-{}-{tag}", std::process::id()));
        fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    /// A minimal document that paints a `width`×`height` black square at the origin —
    /// enough to build matching/mismatching pairs without depending on the real fixture set.
    fn square_html(width: u32, height: u32) -> String {
        format!(
            "<!doctype html><style>body{{margin:0}}div{{width:{width}px;height:{height}px;\
             background:#000}}</style><body><div></div></body>"
        )
    }

    #[test]
    fn discover_should_find_pairs_and_ignore_refs_and_non_html() {
        let dir = scratch_dir("discover");
        fs::write(dir.join("a.html"), square_html(1, 1)).expect("write");
        fs::write(dir.join("a-ref.html"), square_html(1, 1)).expect("write");
        fs::write(dir.join("b.html"), square_html(1, 1)).expect("write");
        fs::write(dir.join("b-ref.html"), square_html(1, 1)).expect("write");
        fs::write(dir.join("notes.txt"), "not a fixture").expect("write");

        let names = discover(&dir).expect("discover");
        assert_eq!(names, vec!["a".to_owned(), "b".to_owned()]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn run_pair_should_pass_a_matching_pair() {
        let dir = scratch_dir("pass");
        fs::write(dir.join("match.html"), square_html(20, 20)).expect("write");
        fs::write(dir.join("match-ref.html"), square_html(20, 20)).expect("write");
        let failures = scratch_dir("pass-failures");

        let result = run_pair(&dir, "match", &failures);
        assert_eq!(result.status, PairStatus::Pass);
        assert!(!failures.join("match").exists(), "no artifacts on a pass");

        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&failures);
    }

    #[test]
    fn run_pair_should_fail_a_mismatching_pair_and_write_a_diff() {
        let dir = scratch_dir("mismatch");
        fs::write(dir.join("bad.html"), square_html(20, 20)).expect("write");
        fs::write(dir.join("bad-ref.html"), square_html(40, 40)).expect("write");
        let failures = scratch_dir("mismatch-failures");

        let result = run_pair(&dir, "bad", &failures);
        assert!(result.is_failure());
        assert!(matches!(result.status, PairStatus::Fail(_)));
        assert!(failures.join("bad/test.png").is_file());
        assert!(failures.join("bad/ref.png").is_file());
        assert!(failures.join("bad/diff.png").is_file());

        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&failures);
    }

    #[test]
    fn run_pair_should_fail_when_the_ref_is_missing() {
        let dir = scratch_dir("noref");
        fs::write(dir.join("lonely.html"), square_html(20, 20)).expect("write");
        let failures = scratch_dir("noref-failures");

        let result = run_pair(&dir, "lonely", &failures);
        assert!(result.is_failure());
        assert!(result.line().contains("missing ref"));

        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&failures);
    }

    #[test]
    fn run_pair_should_report_known_fail_for_a_marked_mismatch() {
        let dir = scratch_dir("known");
        let marked = format!(
            "<!-- KNOWN-FAIL: engine bug placeholder -->{}",
            square_html(20, 20)
        );
        fs::write(dir.join("marked.html"), marked).expect("write");
        fs::write(dir.join("marked-ref.html"), square_html(40, 40)).expect("write");
        let failures = scratch_dir("known-failures");

        let result = run_pair(&dir, "marked", &failures);
        assert!(result.is_failure(), "a known fail is still a failure");
        assert!(matches!(result.status, PairStatus::KnownFail(_)));
        assert!(result.line().starts_with("FAIL (known) marked"));
        assert!(result.line().contains("engine bug placeholder"));

        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&failures);
    }

    #[test]
    fn run_should_apply_the_filter_as_a_substring_match() {
        let dir = scratch_dir("filter");
        fs::write(dir.join("apple.html"), square_html(1, 1)).expect("write");
        fs::write(dir.join("apple-ref.html"), square_html(1, 1)).expect("write");
        fs::write(dir.join("banana.html"), square_html(1, 1)).expect("write");
        fs::write(dir.join("banana-ref.html"), square_html(1, 1)).expect("write");
        let failures = scratch_dir("filter-failures");

        let results = run(&dir, Some("app"), &failures).expect("run");
        assert_eq!(results.len(), 1);
        assert_eq!(
            results.first().expect("just asserted len() == 1").name,
            "apple"
        );

        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&failures);
    }

    #[test]
    fn default_dir_should_point_at_this_crates_own_tests_ref() {
        assert!(default_dir().ends_with(Path::new("tests/ref")));
    }
}
