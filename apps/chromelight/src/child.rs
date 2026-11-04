//! Main for every non-browser role: apply the sandbox, connect, handshake, then serve the role's
//! protocol. The renderer runs [`RendererState`] (M1b Task 8); until the gpu process loop lands
//! (Task 9) the gpu only answers pings and exits on Shutdown.

use std::collections::VecDeque;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use cl_ipc::{
    bootstrap::connect_child,
    handshake,
    proto::{
        gpu::{FromGpu, GpuProtocol, ToGpu},
        renderer::{RendererProtocol, ToRenderer},
    },
    pump::{self, PumpEvent},
    transfer::DisplayListMsg,
};
use cl_paint::wire::MAX_DISPLAY_LIST_BYTES;
use cl_platform::ProcessType;
use cl_process::{Sandbox, SandboxPolicy};
use ipc_channel::ipc::{IpcSender, IpcSharedMemory};
use tracing::{error, info};

use crate::{
    args::Args,
    exit::EXIT_SANDBOX_UNAVAILABLE,
    renderer::{RendererInput, RendererOutput, RendererState},
};

/// How often the renderer loop wakes without a message (the subresource deadline is checked
/// on these ticks).
const RENDERER_TICK: Duration = Duration::from_millis(100);

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

    let (up, down) = ep.split();
    let (events, inbox) = mpsc::channel();
    pump::spawn_reader("renderer-down", down, move |event| {
        events.send(event).is_ok()
    })?;
    let mut state = RendererState::new(MAX_DISPLAY_LIST_BYTES)?;
    let mut display_lists: Option<IpcSender<DisplayListMsg>> = None;

    loop {
        let input = match inbox.recv_timeout(RENDERER_TICK) {
            Ok(PumpEvent::Message(msg)) => renderer_input(msg, &mut display_lists)?,
            // The browser is gone, or its channel broke: nothing left to serve.
            Ok(PumpEvent::Closed) | Err(mpsc::RecvTimeoutError::Disconnected) => return Ok(()),
            Ok(PumpEvent::Failed(e)) => anyhow::bail!("downward channel failed: {e}"),
            Ok(PumpEvent::Error(bad)) => anyhow::bail!("undecodable browser message: {bad:?}"),
            Err(mpsc::RecvTimeoutError::Timeout) => RendererInput::Tick,
        };
        let mut queue = VecDeque::from([input]);
        while let Some(input) = queue.pop_front() {
            for output in state.on(input, Instant::now()) {
                match output {
                    RendererOutput::Up(msg) => up.send(&msg)?,
                    RendererOutput::SendDisplayList { seq, bytes } => {
                        let msg = DisplayListMsg {
                            seq,
                            bytes: IpcSharedMemory::from_bytes(&bytes),
                        };
                        let sent = display_lists
                            .as_ref()
                            .is_some_and(|tx| tx.send(msg).is_ok());
                        if !sent {
                            queue.push_back(RendererInput::DisplayListSendFailed);
                        }
                    }
                    RendererOutput::Exit => {
                        if honour_shutdown(args) {
                            return Ok(());
                        }
                    }
                }
            }
        }
    }
}

/// A downward message as [`RendererInput`]: shared memory copied out (the browser keeps a
/// writable mapping), the display-list sender kept here, the state told its generation.
fn renderer_input(
    msg: ToRenderer,
    display_lists: &mut Option<IpcSender<DisplayListMsg>>,
) -> anyhow::Result<RendererInput> {
    Ok(match msg {
        ToRenderer::HelloAck { .. } => anyhow::bail!("duplicate HelloAck"),
        ToRenderer::LoadDocument {
            doc_id,
            base_url,
            viewport,
            seq,
            bytes,
        } => RendererInput::Load {
            doc_id,
            base_url,
            viewport,
            seq,
            bytes: bytes.to_vec(),
        },
        ToRenderer::Subresources { doc_id, replies } => RendererInput::Subresources {
            doc_id,
            replies: replies
                .into_iter()
                .map(|reply| (reply.index, reply.result.map(|shm| shm.to_vec())))
                .collect(),
        },
        ToRenderer::Resize { viewport, seq } => RendererInput::Resize { viewport, seq },
        ToRenderer::Redraw { seq } => RendererInput::Redraw { seq },
        ToRenderer::DisplayListSender { generation, sender } => {
            *display_lists = Some(sender);
            RendererInput::DisplayListSender { generation }
        }
        ToRenderer::Ping(n) => RendererInput::Ping(n),
        ToRenderer::Shutdown => RendererInput::Shutdown,
    })
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
