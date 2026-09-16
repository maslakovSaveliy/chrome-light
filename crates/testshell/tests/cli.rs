//! Integration tests for the CLI.
#![allow(clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_cl-testshell"))
}

fn ref_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/ref")
        .join(name)
}

#[test]
fn render_then_compare_should_round_trip_through_cli() {
    let dir = std::env::temp_dir().join(format!("cl-tscli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let html = ref_path("text-basic.html");
    let a = dir.join("a.png");
    let b = dir.join("b.png");

    let r = bin()
        .arg("render")
        .arg(&html)
        .arg("--png")
        .arg(&a)
        .status()
        .expect("run");
    assert!(r.success());
    let r = bin()
        .arg("render")
        .arg(&html)
        .arg("--png")
        .arg(&b)
        .status()
        .expect("run");
    assert!(r.success());

    let output = bin().arg("compare").arg(&a).arg(&b).output().expect("run");
    assert_eq!(output.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("differing_pixels=0"),
        "rendering the same page twice must produce byte-identical PNGs, got: {stdout}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn render_should_fail_when_input_missing() {
    // The output path is never written (the input does not exist, so the render fails first),
    // but it still has to be a *plausible* path on every platform this suite runs on — `/tmp`
    // is not one on Windows. `std::env::temp_dir()` is, everywhere.
    let out = std::env::temp_dir().join("cl-tscli-never-written.png");
    let r = bin()
        .args([
            "render".as_ref(),
            "/definitely/missing.html".as_ref(),
            "--png".as_ref(),
            out.as_os_str(),
        ])
        .status()
        .expect("run");
    assert!(!r.success());
    assert!(
        !out.exists(),
        "a failed render must not leave an output file behind"
    );
}

#[test]
fn dump_should_print_stage() {
    let html = ref_path("text-basic.html");
    let expect_token = |stage: &str, token: &str| {
        let output = bin()
            .arg("dump")
            .arg(&html)
            .arg("--stage")
            .arg(stage)
            .output()
            .expect("run");
        assert!(output.status.success(), "dump --stage {stage} failed");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(!stdout.trim().is_empty(), "dump --stage {stage} was empty");
        assert!(
            stdout.contains(token),
            "dump --stage {stage} expected to contain {token:?}, got:\n{stdout}"
        );
    };

    expect_token("dom", "#document");
    expect_token("style", "display");
    expect_token("box-tree", "Block");
    expect_token("fragments", "Block");
    expect_token("display-list", "DisplayList");
}

#[test]
fn dump_should_reject_an_unknown_stage() {
    let html = ref_path("text-basic.html");
    let r = bin()
        .arg("dump")
        .arg(&html)
        .arg("--stage")
        .arg("nonsense")
        .status()
        .expect("run");
    assert!(!r.success());
}
