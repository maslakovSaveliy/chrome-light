//! Spawn a child of the same executable with `--type=` and `--ipc-bootstrap=`, then run the
//! validated handshake. Any failure kills the child: a process that did not prove its identity
//! must not exist.

use std::{
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    time::Duration,
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

/// Spawn `exe` as `process_type`, wait up to `timeout` for bootstrap, run the handshake.
pub fn spawn(
    exe: &Path,
    process_type: ProcessType,
    extra_args: &[String],
    timeout: Duration,
) -> Result<ChildProcess, ProcessError> {
    let server = BootstrapServer::new()?;
    let args = child_args(process_type, server.name());
    let mut child = Command::new(exe)
        .args(args)
        .args(extra_args)
        .stdin(Stdio::null())
        .spawn()
        .map_err(ProcessError::Spawn)?;
    let pid = child.id();
    tracing::info!(%process_type, pid, "spawned child");

    let endpoint = match server.accept_with_timeout(timeout) {
        Ok(ep) => ep,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e.into());
        }
    };
    let ctx = ReceiverCtx {
        expected_process_type: process_type,
        expected_pid: Some(pid),
    };
    let hello = match handshake::browser_side(&endpoint, &ctx) {
        Ok(h) => h,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
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
                let _ = self.child.kill();
                let _ = self.child.wait();
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

    /// Wait for exit.
    pub fn wait(&mut self) -> Result<ExitStatus, ProcessError> {
        self.child.wait().map_err(ProcessError::Child)
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
