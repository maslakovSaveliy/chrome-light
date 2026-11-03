//! Main for every non-browser role: apply the sandbox, connect, handshake, then serve the role's
//! protocol. Until the renderer and gpu process loops land (M1b Tasks 8 and 9) both roles only
//! answer pings and exit on Shutdown.

use cl_ipc::{
    bootstrap::connect_child,
    handshake,
    proto::{
        gpu::{FromGpu, GpuProtocol, ToGpu},
        renderer::{FromRenderer, RendererProtocol, ToRenderer},
    },
};
use cl_platform::ProcessType;
use cl_process::{Sandbox, SandboxPolicy};
use tracing::{error, info};

use crate::{args::Args, exit::EXIT_SANDBOX_UNAVAILABLE};

/// Returns the process exit code.
pub fn run(args: &Args) -> i32 {
    let policy: Box<dyn SandboxPolicy> = choose_policy(args.no_sandbox);
    let sandbox = match Sandbox::new().apply(policy.as_ref()) {
        Ok(s) => s,
        Err(e) => {
            error!(%e, "sandbox unavailable for {}", args.process_type);
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

/// `--ignore-shutdown` exists only in debug builds.
fn ignores_shutdown(args: &Args) -> bool {
    #[cfg(debug_assertions)]
    if args.ignore_shutdown {
        return true;
    }
    let _ = args;
    false
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
    match args.process_type {
        ProcessType::Renderer => serve_renderer(args, name, sandbox),
        ProcessType::Gpu => serve_gpu(args, name, sandbox),
        other => anyhow::bail!("no child protocol for role {other}"),
    }
}

/// Returns `true` when the child should exit on this `Shutdown`.
fn honour_shutdown(args: &Args) -> bool {
    if ignores_shutdown(args) {
        info!("ignoring Shutdown (--ignore-shutdown)");
        return false;
    }
    true
}

fn serve_renderer(
    args: &Args,
    name: &str,
    sandbox: &Sandbox<cl_process::Applied>,
) -> anyhow::Result<()> {
    let ep = connect_child::<RendererProtocol>(name, None)?;
    handshake::child_side(&ep, ProcessType::Renderer, std::process::id())?;
    info!(role = %ProcessType::Renderer, policy = sandbox.policy_name(), "child ready");
    loop {
        match ep.recv()? {
            ToRenderer::Ping(n) => ep.send(&FromRenderer::Pong(n))?,
            ToRenderer::Shutdown => {
                if honour_shutdown(args) {
                    return Ok(());
                }
            }
            ToRenderer::HelloAck { .. } => anyhow::bail!("duplicate HelloAck"),
            // M1b Task 8 serves the document messages.
            other => anyhow::bail!("renderer cannot serve {other:?} yet"),
        }
    }
}

fn serve_gpu(
    args: &Args,
    name: &str,
    sandbox: &Sandbox<cl_process::Applied>,
) -> anyhow::Result<()> {
    // M1b Task 9 creates the display-list channel here and sends its sender up.
    let ep = connect_child::<GpuProtocol>(name, None)?;
    handshake::child_side(&ep, ProcessType::Gpu, std::process::id())?;
    info!(role = %ProcessType::Gpu, policy = sandbox.policy_name(), "child ready");
    loop {
        match ep.recv()? {
            ToGpu::Ping(n) => ep.send(&FromGpu::Pong(n))?,
            ToGpu::Shutdown => {
                if honour_shutdown(args) {
                    return Ok(());
                }
            }
            ToGpu::HelloAck { .. } => anyhow::bail!("duplicate HelloAck"),
            other => anyhow::bail!("gpu cannot serve {other:?} yet"),
        }
    }
}
