//! Integration tests for the CLI.
#![allow(clippy::expect_used)]

use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_cl-testshell"))
}

#[test]
fn render_then_compare_should_round_trip_through_cli() {
    let dir = std::env::temp_dir().join(format!("cl-tscli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let html = dir.join("page.html");
    std::fs::write(&html, "<!doctype html><p>hi</p>").expect("write");
    let a = dir.join("a.png");
    let b = dir.join("b.png");

    let r = bin()
        .arg("render")
        .arg(&html)
        .arg("--png")
        .arg(&a)
        .arg("--viewport")
        .arg("16x8")
        .status()
        .expect("run");
    assert!(r.success());
    let r = bin()
        .arg("render")
        .arg(&html)
        .arg("--png")
        .arg(&b)
        .arg("--viewport")
        .arg("16x8")
        .status()
        .expect("run");
    assert!(r.success());

    let r = bin().arg("compare").arg(&a).arg(&b).status().expect("run");
    assert_eq!(r.code(), Some(0));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn render_should_fail_when_input_missing() {
    let r = bin()
        .args(["render", "/definitely/missing.html", "--png", "/tmp/x.png"])
        .status()
        .expect("run");
    assert!(!r.success());
}
