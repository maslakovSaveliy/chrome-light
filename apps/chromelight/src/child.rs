//! Main for every non-browser role. In M0 all roles behave identically: apply sandbox, connect,
//! handshake, answer pings until Shutdown.

use cl_ipc::{
    bootstrap::connect_child,
    handshake,
    message::{ToBrowser, ToChild},
};
use cl_process::{Sandbox, SandboxError, SandboxPolicy};
use tracing::{error, info};

use crate::args::Args;

/// `EX_CONFIG` from sysexits.h: the environment cannot run this role safely.
pub const EXIT_SANDBOX_UNAVAILABLE: i32 = 78;

/// Returns the process exit code.
pub fn run(args: &Args) -> i32 {
    let policy: Box<dyn SandboxPolicy> = choose_policy(args.no_sandbox);
    let sandbox = match Sandbox::new().apply(policy.as_ref()) {
        Ok(s) => s,
        Err(e @ SandboxError::NotImplemented) => {
            error!(%e, "refusing to start {} without sandbox", args.process_type);
            return EXIT_SANDBOX_UNAVAILABLE;
        }
        Err(e) => {
            error!(%e, "sandbox failed");
            return EXIT_SANDBOX_UNAVAILABLE;
        }
    };
    match serve(args, &sandbox) {
        Ok(()) => 0,
        Err(e) => {
            error!(%e, "child exiting with error");
            1
        }
    }
}

fn choose_policy(no_sandbox: bool) -> Box<dyn SandboxPolicy> {
    #[cfg(debug_assertions)]
    if no_sandbox {
        return Box::new(cl_process::sandbox::DevNoSandbox);
    }
    let _ = no_sandbox;
    Box::new(cl_process::NotImplementedPolicy)
}

fn serve(args: &Args, sandbox: &Sandbox<cl_process::Applied>) -> anyhow::Result<()> {
    let name = args
        .ipc_bootstrap
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("--ipc-bootstrap is required"))?;
    let ep = connect_child(name)?;
    handshake::child_side(&ep, args.process_type, std::process::id())?;
    info!(role = %args.process_type, policy = sandbox.policy_name(), "child ready");
    loop {
        match ep.recv()? {
            ToChild::Ping(n) => ep.send(&ToBrowser::Pong(n))?,
            ToChild::Shutdown => return Ok(()),
            ToChild::HelloAck { .. } => anyhow::bail!("duplicate HelloAck"),
        }
    }
}
