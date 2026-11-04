//! Main for every non-browser role: apply the sandbox, connect, handshake, then serve the role's
//! protocol: the renderer runs [`RendererState`] (M1b Task 8), the gpu [`GpuState`] (Task 9).

use std::collections::VecDeque;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use cl_ipc::{
    bad_message::BadMessageReason,
    bootstrap::connect_child,
    handshake,
    proto::{
        gpu::{FromGpu, GpuProtocol, ToGpu},
        renderer::{RendererProtocol, ToRenderer},
    },
    pump::{self, PumpEvent},
    transfer::{DisplayListMsg, FrameMsg},
};
use cl_paint::wire::MAX_DISPLAY_LIST_BYTES;
use cl_platform::ProcessType;
use cl_process::{Sandbox, SandboxPolicy};
use ipc_channel::ipc::{self, IpcSender, IpcSharedMemory};
use tracing::{error, info};

use crate::{
    args::Args,
    exit::EXIT_SANDBOX_UNAVAILABLE,
    gpu::{GpuInput, GpuOutput, GpuState},
    renderer::{RendererInput, RendererOutput, RendererState},
};

/// Display lists the gpu queues before its list reader blocks (browser messages are not
/// counted).
const GPU_LIST_PERMITS: usize = 4;

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
    let mut display_lists: Option<(u64, IpcSender<DisplayListMsg>)> = None;

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
                            .is_some_and(|(_, tx)| tx.send(msg).is_ok());
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
/// writable mapping), the display-list sender kept here — only a newer generation's replaces
/// the one held, exactly as [`RendererState`] counts them — and the state told its generation.
fn renderer_input(
    msg: ToRenderer,
    display_lists: &mut Option<(u64, IpcSender<DisplayListMsg>)>,
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
            if display_lists
                .as_ref()
                .is_none_or(|(held, _)| generation > *held)
            {
                *display_lists = Some((generation, sender));
            }
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
    // The gpu receives shm on the display-list channel, so it creates it (ADR-0016 §5) and
    // sends its sender up in the bootstrap — the one upward transfer of a channel end.
    let (lists_tx, lists_rx) = ipc::channel::<DisplayListMsg>()?;
    let ep = connect_child::<GpuProtocol>(name, Some(lists_tx))?;
    handshake::child_side(&ep, ProcessType::Gpu, std::process::id())?;
    info!(role = %ProcessType::Gpu, policy = sandbox.policy_name(), "child ready");

    let (up, down) = ep.split();
    // The browser's messages are never held back; display lists are. The list reader takes a
    // permit before queueing a list and the loop returns it once the list is handled, so at
    // most GPU_LIST_PERMITS lists (each holding a shared-memory mapping) wait at a time and a
    // renderer flood blocks the renderer's own sends — not the browser's pings behind it.
    let (events, inbox) = mpsc::channel();
    let (permit_returns, permits) = mpsc::sync_channel::<()>(GPU_LIST_PERMITS);
    for _ in 0..GPU_LIST_PERMITS {
        permit_returns.send(())?;
    }
    let down_events = events.clone();
    pump::spawn_reader("gpu-down", down, move |event| {
        down_events.send(GpuEvent::Down(event)).is_ok()
    })?;
    pump::spawn_reader("gpu-lists", lists_rx, move |event| {
        permits.recv().is_ok() && events.send(GpuEvent::List(event)).is_ok()
    })?;
    let mut state = GpuState::new()?;
    let mut frames: Option<IpcSender<FrameMsg>> = None;

    loop {
        let input = match inbox.recv() {
            Ok(GpuEvent::Down(PumpEvent::Message(msg))) => gpu_input(msg, &mut frames)?,
            // The browser is gone: nothing left to serve.
            Ok(GpuEvent::Down(PumpEvent::Closed)) | Err(mpsc::RecvError) => return Ok(()),
            Ok(GpuEvent::Down(PumpEvent::Failed(e))) => {
                anyhow::bail!("downward channel failed: {e}")
            }
            Ok(GpuEvent::Down(PumpEvent::Error(bad))) => {
                anyhow::bail!("undecodable browser message: {bad:?}")
            }
            Ok(GpuEvent::List(PumpEvent::Message(msg))) => {
                // The list leaves the queue here; its permit goes back for the next one.
                let _ = permit_returns.try_send(());
                GpuInput::DisplayList(msg)
            }
            // No renderer holds the display-list sender any more: keep serving the browser.
            Ok(GpuEvent::List(PumpEvent::Closed)) => continue,
            // A message on the renderer's channel that is not a DisplayListMsg: the renderer's
            // fault, reported; the browser kills it.
            Ok(GpuEvent::List(PumpEvent::Error(bad))) => {
                let _ = permit_returns.try_send(());
                info!(?bad, "undecodable display-list message");
                up.send(&FromGpu::BadDisplayList {
                    seq: 0,
                    reason: BadMessageReason::DisplayListUndecodable,
                })?;
                continue;
            }
            // A transport-level failure leaves the channel unusable (cl_ipc::pump): report it
            // against the renderer, then exit so the browser starts a fresh gpu.
            Ok(GpuEvent::List(PumpEvent::Failed(e))) => {
                let _ = up.send(&FromGpu::BadDisplayList {
                    seq: 0,
                    reason: BadMessageReason::DisplayListUndecodable,
                });
                anyhow::bail!("display-list channel failed: {e}");
            }
        };
        for output in state.on(input) {
            match output {
                GpuOutput::Up(msg) => up.send(&msg)?,
                GpuOutput::Frame {
                    seq,
                    width,
                    height,
                    pixels,
                } => {
                    let msg = FrameMsg {
                        seq,
                        width,
                        height,
                        pixels: IpcSharedMemory::from_bytes(&pixels),
                    };
                    // `GpuState` makes frames only once it was told of a sender.
                    let Some(tx) = frames.as_ref() else {
                        continue;
                    };
                    // A browser that stopped reading frames is gone.
                    if tx.send(msg).is_err() {
                        return Ok(());
                    }
                }
                GpuOutput::Exit => {
                    if honour_shutdown(args) {
                        return Ok(());
                    }
                }
                GpuOutput::Fail(reason) => anyhow::bail!(reason),
            }
        }
    }
}

/// The two channels the gpu reads, merged into one queue.
enum GpuEvent {
    Down(PumpEvent<ToGpu>),
    List(PumpEvent<DisplayListMsg>),
}

/// A downward message as [`GpuInput`]: the frame sender kept here, the state told it exists.
fn gpu_input(msg: ToGpu, frames: &mut Option<IpcSender<FrameMsg>>) -> anyhow::Result<GpuInput> {
    Ok(match msg {
        ToGpu::HelloAck { .. } => anyhow::bail!("duplicate HelloAck"),
        ToGpu::SetSurface { viewport, seq } => GpuInput::SetSurface { viewport, seq },
        ToGpu::FrameSender { sender } => {
            *frames = Some(sender);
            GpuInput::FrameSender
        }
        ToGpu::Ping(n) => GpuInput::Ping(n),
        ToGpu::Shutdown => GpuInput::Shutdown,
    })
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "a failed setup step should abort the test, loudly"
)]
mod tests {
    use cl_ipc::proto::renderer::ToRenderer;
    use cl_ipc::transfer::DisplayListMsg;
    use ipc_channel::ipc::{IpcSender, channel};

    use super::renderer_input;

    fn sender() -> IpcSender<DisplayListMsg> {
        channel::<DisplayListMsg>().expect("channel").0
    }

    #[test]
    fn only_a_newer_generation_replaces_the_display_list_sender() {
        let mut held = None;
        for (generation, expect_held) in [(2, 2), (1, 2), (2, 2), (3, 3)] {
            let msg = ToRenderer::DisplayListSender {
                generation,
                sender: sender(),
            };
            let _ = renderer_input(msg, &mut held).expect("input");
            assert_eq!(held.as_ref().map(|(g, _)| *g), Some(expect_held));
        }
    }
}
