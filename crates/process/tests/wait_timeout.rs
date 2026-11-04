//! `wait_timeout` must bound the browser's wait on a child that never exits: kill it at the
//! deadline and reap it. The child is this test binary relaunched into [`hang_helper`], so the
//! test needs no platform-specific command and runs the same on every OS.
#![allow(clippy::expect_used, reason = "test assertions")]

use std::{
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

use cl_process::spawn::wait_timeout;

/// Marker argument that turns [`hang_helper`] from a no-op into a process that never exits.
const HANG_MARKER: &str = "cl-process-hang-helper";

/// Kills the child when the test ends, so a failing assertion does not leave a parked helper
/// running (its stdio is null, so the test runner would not notice it). Killing an already reaped
/// child is a no-op.
struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// A no-op under a normal test run. Relaunched with [`HANG_MARKER`] it parks forever, standing in
/// for a child that ignores `Shutdown`.
#[test]
fn hang_helper() {
    if !std::env::args().any(|a| a == HANG_MARKER) {
        return;
    }
    loop {
        std::thread::park();
    }
}

#[test]
fn wait_timeout_should_kill_and_reap_a_child_that_never_exits() {
    let exe = std::env::current_exe().expect("current exe");
    let mut guard = KillOnDrop(
        Command::new(exe)
            .args(["--exact", "hang_helper", HANG_MARKER, "--nocapture"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn hanging child"),
    );
    let child = &mut guard.0;

    let start = Instant::now();
    let outcome = wait_timeout(child, Duration::from_millis(500)).expect("wait_timeout");
    let elapsed = start.elapsed();

    assert!(outcome.killed, "a child that never exits must be killed");
    assert!(!outcome.status.success(), "status: {:?}", outcome.status);
    assert!(
        elapsed >= Duration::from_millis(500),
        "returned before the deadline: {elapsed:?}"
    );
    assert!(
        elapsed < Duration::from_secs(5),
        "the deadline was not honoured: {elapsed:?}"
    );
    // Reaped: the status is already collected, so try_wait reports it again without blocking.
    assert!(
        child.try_wait().expect("try_wait").is_some(),
        "the killed child must already be reaped"
    );
}

#[test]
fn wait_timeout_should_return_the_status_of_a_child_that_exits_in_time() {
    let exe = std::env::current_exe().expect("current exe");
    // Without the marker the helper returns at once, so the relaunched binary exits 0.
    let mut child = Command::new(exe)
        .args(["--exact", "hang_helper"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn child");

    let outcome = wait_timeout(&mut child, Duration::from_secs(30)).expect("wait_timeout");

    assert!(
        !outcome.killed,
        "a child that exits on its own must not be killed"
    );
    assert!(outcome.status.success(), "status: {:?}", outcome.status);
}

#[test]
fn wait_timeout_should_not_panic_on_an_unrepresentable_deadline() {
    let exe = std::env::current_exe().expect("current exe");
    let mut guard = KillOnDrop(
        Command::new(exe)
            .args(["--exact", "hang_helper"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn child"),
    );

    // `Instant::now() + Duration::MAX` would overflow and panic; this must mean "no deadline".
    let outcome = wait_timeout(&mut guard.0, Duration::MAX).expect("wait_timeout");

    assert!(!outcome.killed);
    assert!(outcome.status.success(), "status: {:?}", outcome.status);
}
