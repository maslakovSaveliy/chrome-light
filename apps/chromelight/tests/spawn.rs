//! End-to-end: the real binary spawns a renderer child of itself and completes the handshake.
#![allow(clippy::expect_used, reason = "test assertions on subprocess output")]

use std::{
    io::Read,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_chromelight"))
}

/// Runs `cmd` to completion, but kills it after `limit` so a regression that makes the browser
/// wait forever fails the test instead of hanging the run. Returns the output and the wall time.
fn run_bounded(mut cmd: Command, limit: Duration) -> (Output, Duration) {
    let start = Instant::now();
    let mut child = cmd
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
        if start.elapsed() > limit {
            let _ = child.kill();
            break child.wait().expect("wait after kill");
        }
        std::thread::park_timeout(Duration::from_millis(10));
    };
    let elapsed = start.elapsed();
    let output = Output {
        status,
        stdout: out_reader.join().expect("stdout reader"),
        stderr: err_reader.join().expect("stderr reader"),
    };
    (output, elapsed)
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
fn trace_out_should_write_file_per_role() {
    let dir = std::env::temp_dir().join(format!("cl-trace-role-{}", std::process::id()));
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
    // `<stem>.<role>.json` next to the browser's own trace.
    let renderer = dir.join("trace.renderer.json");
    let meta = std::fs::metadata(&renderer).expect("renderer trace file exists");
    assert!(meta.len() > 2, "renderer trace file should not be empty");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn sandbox_refusal_should_fail_spawn_within_3s_with_78() {
    // Without --no-sandbox the renderer refuses to run (no sandbox policy is implemented yet) and
    // exits 78 before connecting to the bootstrap server. The browser must notice the exit
    // instead of waiting for its bootstrap timeout, and report it with the same code.
    let mut cmd = bin();
    cmd.args(["--exit-after-handshake"]);
    let (out, elapsed) = run_bounded(cmd, Duration::from_secs(30));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(78), "stderr:\n{stderr}");
    assert!(
        elapsed < Duration::from_secs(3),
        "the browser waited {elapsed:?} for a child that had already exited"
    );
    assert!(
        stderr.contains("renderer exited during bootstrap"),
        "stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("sandbox unavailable for renderer"),
        "stderr:\n{stderr}"
    );
}

#[test]
fn wait_timeout_should_kill_child_ignoring_shutdown() {
    // A renderer that ignores Shutdown must not keep the browser alive: the browser waits a
    // bounded time, then kills and reaps it.
    let mut cmd = bin();
    cmd.args([
        "--exit-after-handshake",
        "--no-sandbox",
        "--renderer-ignore-shutdown",
    ])
    .env("RUST_LOG", "warn");
    let (out, elapsed) = run_bounded(cmd, Duration::from_mins(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "stderr:\n{stderr}");
    assert!(
        stderr.contains("did not exit after Shutdown; killed"),
        "stderr:\n{stderr}"
    );
    assert!(
        elapsed < Duration::from_secs(15),
        "the browser waited {elapsed:?} on a child that ignores Shutdown"
    );
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
