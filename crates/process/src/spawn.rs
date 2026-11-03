//! Spawn a child of the same executable with `--type=` and `--ipc-bootstrap=`, then run the
//! validated handshake. Any failure kills the child: a process that did not prove its identity
//! must not exist.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::{Child, Command, ExitStatus, Stdio},
    time::{Duration, Instant},
};

use cl_ipc::{
    ReceiverCtx,
    bootstrap::{BootstrapServer, BrowserEndpoint},
    handshake,
    message::Hello,
};
use cl_platform::ProcessType;

use crate::error::ProcessError;

/// A live, handshaken child.
#[derive(Debug)]
pub struct ChildProcess {
    /// What we spawned it as.
    pub process_type: ProcessType,
    /// Channel to the child.
    pub endpoint: BrowserEndpoint,
    /// The validated hello.
    pub hello: Hello,
    child: Child,
}

/// Command-line arguments a child receives. Public so tests and the binary agree on the format.
#[must_use]
pub fn child_args(process_type: ProcessType, bootstrap_name: &str) -> [String; 2] {
    [
        format!("--type={process_type}"),
        format!("--ipc-bootstrap={bootstrap_name}"),
    ]
}

/// How often [`spawn`] checks whether a child that has not connected yet has already exited, and
/// how often [`wait_timeout`] polls a child's exit status.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// What [`spawn`] needs besides the executable and the role.
#[derive(Debug, Clone)]
pub struct SpawnOptions {
    /// Extra command-line arguments for the child, after `--type` and `--ipc-bootstrap`.
    pub extra_args: Vec<String>,
    /// Upper bound for each of the two waits: the child connecting to the bootstrap server, and
    /// the handshake.
    pub timeout: Duration,
    /// The browser's own `--trace-out` path, if any. The child then writes its trace next to it
    /// as `<stem>.<role>.json` (see [`child_trace_path`]).
    pub trace_path: Option<PathBuf>,
}

/// The trace file a child of `role` writes when the browser traces to `browser_trace`:
/// `trace.json` → `trace.renderer.json`, `trace` → `trace.renderer.json`. A path with no file
/// name (`..`, `/`) is treated as a directory: `../renderer.json`.
#[must_use]
pub fn child_trace_path(browser_trace: &Path, role: ProcessType) -> PathBuf {
    match browser_trace.file_name() {
        Some(_) => browser_trace.with_extension(format!("{role}.json")),
        None => browser_trace.join(format!("{role}.json")),
    }
}

/// Spawn `exe` as `process_type`, wait up to `opts.timeout` for bootstrap, run the handshake.
///
/// While waiting for the child to connect, the child's exit status is polled every
/// [`POLL_INTERVAL`]: a child that exits first (for example because it refuses to run without a
/// sandbox) ends the wait at once with [`ProcessError::ExitedDuringBootstrap`] instead of
/// running out the timeout.
pub fn spawn(
    exe: &Path,
    process_type: ProcessType,
    opts: &SpawnOptions,
) -> Result<ChildProcess, ProcessError> {
    let server = BootstrapServer::new()?;
    let args = child_args(process_type, server.name());
    let mut cmd = Command::new(exe);
    cmd.args(args).args(&opts.extra_args).stdin(Stdio::null());
    if let Some(trace) = &opts.trace_path {
        // Built as an OsString so a non-UTF-8 path reaches the child unchanged.
        let mut arg = OsString::from("--trace-out=");
        arg.push(child_trace_path(trace, process_type));
        cmd.arg(arg);
    }
    let mut child = cmd.spawn().map_err(ProcessError::Spawn)?;
    let pid = child.id();
    tracing::info!(%process_type, pid, "spawned child");

    let endpoint = match await_bootstrap(server, &mut child, opts.timeout) {
        Ok(ep) => ep,
        Err(e) => {
            kill_and_reap(&mut child);
            return Err(e);
        }
    };
    let ctx = ReceiverCtx {
        expected_process_type: process_type,
        expected_pid: Some(pid),
    };
    let hello = match handshake::browser_side(&endpoint, &ctx, opts.timeout) {
        Ok(h) => h,
        Err(e) => {
            kill_and_reap(&mut child);
            return Err(e.into());
        }
    };
    Ok(ChildProcess {
        process_type,
        endpoint,
        hello,
        child,
    })
}

/// Waits for the child to connect to `server`, polling `child` in between so that an early exit
/// is reported at once. The caller kills and reaps the child on any error (for an exited child
/// that is a no-op: its status has already been collected here).
///
/// A child that connects and exits within the same poll slice is reported as
/// [`ProcessError::ExitedDuringBootstrap`] too; its connection is dropped unused.
fn await_bootstrap(
    server: BootstrapServer,
    child: &mut Child,
    timeout: Duration,
) -> Result<BrowserEndpoint, ProcessError> {
    let deadline = deadline_after(timeout);
    let pending = server.start_accept()?;
    loop {
        if let Some(endpoint) = pending.poll(POLL_INTERVAL)? {
            return Ok(endpoint);
        }
        if let Some(status) = child.try_wait().map_err(ProcessError::Child)? {
            return Err(ProcessError::ExitedDuringBootstrap {
                code: status.code(),
            });
        }
        if deadline.is_some_and(|d| Instant::now() >= d) {
            return Err(ProcessError::Ipc(cl_ipc::IpcError::Transport(
                std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "child did not connect to bootstrap server in time",
                ),
            )));
        }
    }
}

/// `now + timeout`, or `None` (no deadline) when that instant is not representable — a caller
/// passing `Duration::MAX` gets an unbounded wait instead of an overflow panic.
fn deadline_after(timeout: Duration) -> Option<Instant> {
    Instant::now().checked_add(timeout)
}

/// How a bounded wait on a child ended (see [`wait_timeout`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WaitOutcome {
    /// The child's exit status. After a kill this normally reports the kill; if the child exited
    /// on its own between the last poll and the kill, it is that natural status.
    pub status: ExitStatus,
    /// `true` when the child was still running at the last poll before the deadline and a kill
    /// was sent.
    pub killed: bool,
}

/// Waits up to `timeout` for `child` to exit; if it is still running then, kills it. Either way
/// the child is reaped before this returns, so no zombie is left behind and a later `try_wait`
/// reports the same status.
///
/// `std::process::Child` has no wait-with-timeout, so this polls `try_wait` every
/// [`POLL_INTERVAL`], parking the thread in between (`std::thread::sleep` is disallowed in this
/// workspace; a spurious unpark only means one extra poll).
///
/// A `timeout` too large to add to the current instant means no deadline.
pub fn wait_timeout(child: &mut Child, timeout: Duration) -> Result<WaitOutcome, ProcessError> {
    let deadline = deadline_after(timeout);
    loop {
        if let Some(status) = child.try_wait().map_err(ProcessError::Child)? {
            return Ok(WaitOutcome {
                status,
                killed: false,
            });
        }
        let now = Instant::now();
        let slice = match deadline {
            Some(d) if now >= d => {
                child.kill().map_err(ProcessError::Child)?;
                let status = child.wait().map_err(ProcessError::Child)?;
                return Ok(WaitOutcome {
                    status,
                    killed: true,
                });
            }
            Some(d) => POLL_INTERVAL.min(d - now),
            None => POLL_INTERVAL,
        };
        std::thread::park_timeout(slice);
    }
}

/// Kill a child and reap it, ignoring failures: this runs on error paths where the process may
/// already be gone, and there is nothing more useful to do with a second failure here.
fn kill_and_reap(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

impl Drop for ChildProcess {
    /// A child that is still running when its handle is dropped is killed and reaped. The
    /// browser owns every child's lifetime; nothing may outlive its handle (docs/SECURITY.md §3).
    fn drop(&mut self) {
        match self.child.try_wait() {
            Ok(Some(_)) => {}
            Ok(None) => {
                tracing::warn!(
                    pid = self.child.id(),
                    "child still running at drop; killing"
                );
                kill_and_reap(&mut self.child);
            }
            Err(e) => tracing::warn!(pid = self.child.id(), error = %e, "try_wait failed at drop"),
        }
    }
}

impl ChildProcess {
    /// OS pid.
    #[must_use]
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// The child's exit status if it has exited, without blocking.
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>, ProcessError> {
        self.child.try_wait().map_err(ProcessError::Child)
    }

    /// Wait up to `timeout` for exit, then kill; always reaps. See [`wait_timeout`].
    pub fn wait_timeout(&mut self, timeout: Duration) -> Result<WaitOutcome, ProcessError> {
        wait_timeout(&mut self.child, timeout)
    }

    /// Kill and reap.
    pub fn kill(&mut self) -> Result<(), ProcessError> {
        self.child.kill().map_err(ProcessError::Child)?;
        self.child.wait().map_err(ProcessError::Child)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_trace_path_should_insert_the_role_before_the_extension() {
        assert_eq!(
            child_trace_path(Path::new("out/trace.json"), ProcessType::Renderer),
            PathBuf::from("out/trace.renderer.json")
        );
        assert_eq!(
            child_trace_path(Path::new("out/trace"), ProcessType::Renderer),
            PathBuf::from("out/trace.renderer.json")
        );
    }

    #[test]
    fn child_trace_path_should_not_reuse_a_path_without_a_file_name() {
        // `with_extension` is a no-op on `..`; the child must not get the browser's own path.
        let path = child_trace_path(Path::new(".."), ProcessType::Renderer);
        assert_ne!(path, PathBuf::from(".."));
        assert_eq!(path, Path::new("..").join("renderer.json"));
    }

    #[test]
    fn child_args_should_encode_type_and_bootstrap_name() {
        assert_eq!(
            child_args(ProcessType::Renderer, "srv-1"),
            [
                "--type=renderer".to_owned(),
                "--ipc-bootstrap=srv-1".to_owned()
            ]
        );
    }
}
