//! End-to-end: the real binary spawns a renderer child of itself and completes the handshake.
#![allow(clippy::expect_used, reason = "test assertions on subprocess output")]

use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_chromelight"))
}

#[test]
fn browser_should_spawn_renderer_and_complete_handshake() {
    let out = bin()
        .args(["--exit-after-handshake", "--no-sandbox"])
        .output()
        .expect("run binary");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "exit={:?}\nstdout:\n{stdout}\nstderr:\n{stderr}",
        out.status.code()
    );
    assert!(
        stdout.contains("handshake ok renderer pid="),
        "stdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

#[test]
fn renderer_without_sandbox_flag_should_exit_with_config_error() {
    // A child started directly with no bootstrap name and no --no-sandbox must refuse to run.
    let out = bin()
        .args(["--type=renderer", "--ipc-bootstrap=does-not-exist"])
        .output()
        .expect("run binary");
    assert_eq!(
        out.status.code(),
        Some(78),
        "stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn trace_out_should_write_a_chrome_trace_file() {
    let dir = std::env::temp_dir().join(format!("cl-trace-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let path = dir.join("trace.json");
    let out = bin()
        .args(["--exit-after-handshake", "--no-sandbox", "--trace-out"])
        .arg(&path)
        .output()
        .expect("run binary");
    assert!(
        out.status.success(),
        "stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let meta = std::fs::metadata(&path).expect("trace file exists");
    assert!(meta.len() > 2, "trace file should not be empty");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn browser_failure_after_handshake_should_kill_renderer_on_drop() {
    let out = bin()
        .args(["--no-sandbox", "--browser-fail-after-handshake"])
        .env("RUST_LOG", "warn")
        .output()
        .expect("run binary");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "stderr:\n{stderr}");
    assert!(
        stderr.contains("child still running at drop; killing"),
        "stderr:\n{stderr}"
    );
}
