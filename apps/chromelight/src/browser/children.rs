//! The browser's IO layer (M1b Task 10): spawns and wires the renderer and the gpu, turns
//! everything they send into [`Event`]s for [`BrowserCore`](super::core::BrowserCore), and
//! carries out its IO [`Action`]s. The core decides; this module only moves bytes and
//! processes.
//!
//! The browser never blocks on a child (ADR-0005, plan Global Constraints):
//!
//! * Every upward channel has its own reader thread. All of them feed **one bounded queue**
//!   ([`EVENT_QUEUE`]): a child that floods the browser blocks its own reader, which leaves its
//!   messages in its own channel and, in turn, blocks the child's sends — the browser's memory
//!   does not grow with the flood.
//! * Frames are copied out of shared memory on their reader thread, refused unread when larger
//!   than the largest surface the browser asked of that gpu (`width × height × 4`), and at most
//!   [`FRAME_PERMITS`] copies wait in the queue.
//! * Every child has a writer thread with a bounded queue ([`WRITER_QUEUE`]); a child that stops
//!   reading until its queue is full is killed.
//! * A child's exit is reported once its upward channel has drained and closed, so whatever it
//!   sent before dying (a `BadDisplayList` from a gpu) reaches the core first; if the channel
//!   does not close within [`EXIT_DRAIN_GRACE`] of the exit, the exit is reported anyway.
//! * A child that does not answer a ping within [`CHILD_PING_TIMEOUT`] is killed (its exit then
//!   reads as a crash: the renderer's is fatal, the gpu is restarted).
//! * Subresources are read on a worker thread.
//!
//! What still blocks: spawning a gpu (bounded by the bootstrap timeout) and reaping a child the
//! browser has just killed.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::time::{Duration, Instant};

use cl_ipc::ChildProtocol;
use cl_ipc::bad_message::BadMessageReason;
use cl_ipc::proto::Viewport;
use cl_ipc::proto::gpu::{FromGpu, GpuProtocol, ToGpu};
use cl_ipc::proto::renderer::{FromRenderer, RendererProtocol, SubresourceReply, ToRenderer};
use cl_ipc::pump::{self, PumpEvent};
use cl_ipc::shm::copy_bounded;
use cl_ipc::transfer::FrameMsg;
use cl_process::{ChildHandle, ProcessError, SpawnOptions, spawn};
use ipc_channel::ipc::{self, IpcSharedMemory};
use tracing::{info, warn};

use super::core::{Action, Event, ToGpuCmd, ToRendererCmd};
use super::frames::FrameMeta;
use super::subresources::{
    LoadedSubresource, MAX_SUBRESOURCE_BYTES_PER_DOCUMENT, load_subresources,
};
use crate::exit::EXIT_SANDBOX_UNAVAILABLE;

/// A child that has not answered a ping for this long is killed.
pub const CHILD_PING_TIMEOUT: Duration = Duration::from_secs(5);

/// How often each child is pinged.
const PING_INTERVAL: Duration = Duration::from_secs(1);

/// Events the reader threads may queue before they block (and, behind them, the children).
const EVENT_QUEUE: usize = 256;

/// Frame copies a gpu's frame reader may queue before it blocks.
const FRAME_PERMITS: usize = 2;

/// Messages a child's writer may hold before the child counts as hung and is killed.
const WRITER_QUEUE: usize = 64;

/// How long after a child's exit its upward channel may take to close before the exit is
/// reported anyway.
const EXIT_DRAIN_GRACE: Duration = Duration::from_secs(1);

/// How often the core gets an [`Event::Tick`], also under a flood.
const TICK: Duration = Duration::from_millis(50);

/// Events handled per [`Children::next_events`] call beyond the first.
const BATCH: usize = 64;

/// The top-level document, read by the browser.
#[derive(Debug)]
pub struct DocumentSource {
    /// Its `file:` URL (the base for its relative references).
    pub base_url: cl_net::Url,
    /// Its bytes.
    pub bytes: Vec<u8>,
    /// The canonical directory it lies in: the subresource policy's root.
    pub dir: PathBuf,
}

impl DocumentSource {
    /// Reads the document named by `arg`, a path or a `file:` URL.
    ///
    /// # Errors
    /// A URL that is not a local `file:` URL, a path that does not exist, a file over
    /// [`cl_net::load_file`]'s size cap or that cannot be read.
    pub fn open(arg: &Path) -> anyhow::Result<Self> {
        let path = match arg.to_str() {
            Some(s) if s.starts_with("file:") => cl_net::Url::parse(s)?
                .to_file_path()
                .ok_or_else(|| anyhow::anyhow!("not a local file: URL: {s}"))?,
            _ => arg.to_path_buf(),
        };
        let canonical = path.canonicalize()?;
        let dir = canonical
            .parent()
            .ok_or_else(|| anyhow::anyhow!("{} has no parent directory", canonical.display()))?
            .to_path_buf();
        let base_url = cl_net::Url::from_file_path(&canonical)?;
        let bytes = cl_net::load_file(&base_url)?;
        Ok(DocumentSource {
            base_url,
            bytes,
            dir,
        })
    }
}

/// Which child a reader thread speaks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChildId {
    Renderer,
    Gpu(u64),
}

/// What the threads report to the loop.
#[derive(Debug)]
enum IoEvent {
    /// For the core as is.
    Core(Event),
    /// A frame-channel event of a gpu; its permit goes back when the loop takes it.
    Frame { generation: u64, event: Event },
    /// A child answered a ping.
    Pong { child: ChildId, nonce: u64 },
    /// A child's upward channel is drained and closed (or broken).
    UpClosed { child: ChildId },
}

/// Ping bookkeeping for one child.
#[derive(Debug)]
struct Watchdog {
    next_nonce: u64,
    outstanding: Option<(u64, Instant)>,
    last_answer: Instant,
}

impl Watchdog {
    fn new(now: Instant) -> Self {
        Watchdog {
            next_nonce: 1,
            outstanding: None,
            last_answer: now,
        }
    }

    /// A nonce to send now, if a ping is due.
    fn due(&mut self, now: Instant) -> Option<u64> {
        if self.outstanding.is_some()
            || now.saturating_duration_since(self.last_answer) < PING_INTERVAL
        {
            return None;
        }
        let nonce = self.next_nonce;
        self.next_nonce = self.next_nonce.wrapping_add(1);
        self.outstanding = Some((nonce, now));
        Some(nonce)
    }

    fn answered(&mut self, nonce: u64, now: Instant) {
        if self.outstanding.is_some_and(|(n, _)| n == nonce) {
            self.outstanding = None;
            self.last_answer = now;
        }
    }

    /// Moves the outstanding ping and the last answer `by` later: time the browser spent
    /// blocked, not waiting for this child.
    fn postpone(&mut self, by: Duration) {
        if let Some((_, sent)) = &mut self.outstanding {
            *sent = sent.checked_add(by).unwrap_or(*sent);
        }
        self.last_answer = self.last_answer.checked_add(by).unwrap_or(self.last_answer);
    }

    fn overdue(&self, now: Instant) -> bool {
        self.outstanding
            .is_some_and(|(_, sent)| now.saturating_duration_since(sent) >= CHILD_PING_TIMEOUT)
    }
}

/// One child process and the browser's side of its channels.
#[derive(Debug)]
struct Slot<D> {
    handle: ChildHandle,
    /// `None` once the child is killed or its writer is gone.
    writer: Option<SyncSender<D>>,
    watchdog: Watchdog,
    up_closed: bool,
    exited_at: Option<Instant>,
    /// Its exit was reported to the core.
    reported: bool,
}

impl<D> Slot<D> {
    fn new(handle: ChildHandle, writer: SyncSender<D>, now: Instant) -> Self {
        Slot {
            handle,
            writer: Some(writer),
            watchdog: Watchdog::new(now),
            up_closed: false,
            exited_at: None,
            reported: false,
        }
    }

    /// Queues `msg` for the child. A full queue means the child stopped reading, a gone writer
    /// that its channel broke: either way it is killed, and its exit reaches the core like any
    /// other.
    fn send(&mut self, what: &str, msg: D) {
        let Some(writer) = &self.writer else {
            return;
        };
        match writer.try_send(msg) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                warn!(
                    pid = self.handle.pid(),
                    "{what} stopped reading its messages; killing it"
                );
                self.kill();
            }
            // The child's downward channel broke (its receiver gone) while it may still be
            // running: never leave a child up with a dead channel (cl_ipc::pump).
            Err(TrySendError::Disconnected(_)) => {
                // Usually the child already exited (a crash says so in its own line).
                if matches!(self.handle.try_wait(), Ok(Some(_))) {
                    info!(
                        pid = self.handle.pid(),
                        "{what} is gone; its channel closed"
                    );
                } else {
                    warn!(
                        pid = self.handle.pid(),
                        "{what}'s channel broke; killing it"
                    );
                }
                self.kill();
            }
        }
    }

    fn kill(&mut self) {
        self.writer = None;
        if let Err(e) = self.handle.kill() {
            // Already exited and reaped, most likely; the exit is reported either way.
            info!(pid = self.handle.pid(), error = %e, "kill failed");
        }
    }

    /// Kills the child if it is still running, and returns its exit code.
    fn reap(&mut self) -> Option<i32> {
        self.writer = None;
        if let Ok(Some(status)) = self.handle.try_wait() {
            return status.code();
        }
        self.kill();
        self.handle.try_wait().ok().flatten().and_then(|s| s.code())
    }
}

/// The current gpu: its slot, its generation and its frame channel's state.
#[derive(Debug)]
struct Gpu {
    generation: u64,
    slot: Slot<ToGpu>,
    /// The largest frame (bytes) the browser asked of this gpu; larger frames are refused unread.
    frame_limit: Arc<AtomicUsize>,
    /// Gives its frame reader back a permit.
    frame_permits: SyncSender<()>,
}

/// Why the children could not be started.
#[derive(Debug)]
pub enum StartError {
    /// A child exited with [`EXIT_SANDBOX_UNAVAILABLE`] during bootstrap.
    SandboxUnavailable,
    /// Anything else.
    Other(anyhow::Error),
}

impl From<anyhow::Error> for StartError {
    fn from(e: anyhow::Error) -> Self {
        StartError::Other(e)
    }
}

/// The renderer and the current gpu, with the threads that serve them.
#[derive(Debug)]
pub struct Children {
    exe: PathBuf,
    gpu_opts: SpawnOptions,
    doc: DocumentSource,
    events_tx: SyncSender<IoEvent>,
    events: Receiver<IoEvent>,
    renderer: Slot<ToRenderer>,
    gpu: Option<Gpu>,
    /// Events the IO layer made itself (a failed spawn), handed out first.
    pending: Vec<Event>,
    next_tick: Instant,
}

impl Children {
    /// Spawns the renderer and the first gpu (generation 1) and wires them: the gpu's
    /// display-list sender down to the renderer, a frame channel's sender down to the gpu.
    ///
    /// # Errors
    /// [`StartError::SandboxUnavailable`] if either child refused to run unsandboxed; anything
    /// else that failed to spawn or wire.
    pub fn start(
        exe: PathBuf,
        renderer_opts: &SpawnOptions,
        gpu_opts: SpawnOptions,
        doc: DocumentSource,
    ) -> Result<Self, StartError> {
        let now = Instant::now();
        let (events_tx, events) = mpsc::sync_channel(EVENT_QUEUE);
        let renderer = spawn_child::<RendererProtocol>(&exe, renderer_opts)?;
        let ctx = renderer.receiver_ctx();
        let pid = renderer.pid();
        let (endpoint, handle) = renderer.into_parts();
        let (down, up) = endpoint.split(ctx);
        let writer = spawn_writer("renderer-writer", move |msg| down.send(msg).is_ok())
            .map_err(anyhow::Error::from)?;
        let tx = events_tx.clone();
        pump::spawn_reader("renderer-up", up, move |event| {
            tx.send(renderer_event(event)).is_ok()
        })
        .map_err(anyhow::Error::from)?;
        info!(pid, "renderer started");
        let mut children = Children {
            exe,
            gpu_opts,
            doc,
            events_tx,
            events,
            renderer: Slot::new(handle, writer, now),
            gpu: None,
            pending: Vec::new(),
            next_tick: now,
        };
        match children.spawn_gpu(1) {
            Ok(()) => Ok(children),
            Err(SpawnFailure::Sandbox) => Err(StartError::SandboxUnavailable),
            Err(SpawnFailure::Other(e)) => Err(StartError::Other(e)),
        }
    }

    /// Carries out one IO action of the core. The presentation actions (`Present`,
    /// `Warnings`, `ShowError`, `Exit`) are the caller's and ignored here.
    pub fn execute(&mut self, action: Action) {
        match action {
            Action::SendRenderer(cmd) => {
                let msg = self.renderer_message(cmd);
                self.renderer.send("the renderer", msg);
            }
            Action::SendGpu(cmd) => self.send_gpu(cmd),
            Action::SpawnGpu { generation } => {
                let started = Instant::now();
                let result = self.spawn_gpu(generation);
                // The loop read nothing while the spawn blocked: the renderer's ping clock
                // stops for that time, so a Pong waiting in the queue is not taken for silence.
                self.renderer.watchdog.postpone(started.elapsed());
                if let Err(failure) = result {
                    warn!(generation, error = ?failure, "gpu spawn failed");
                    // A gpu that never started counts as one that exited: the core spends a
                    // restart on it.
                    self.pending.push(Event::GpuExited {
                        generation,
                        code: None,
                    });
                }
            }
            Action::KillRenderer { reason } => {
                warn!(
                    pid = self.renderer.handle.pid(),
                    %reason,
                    "killing the renderer for a bad message"
                );
                self.renderer.kill();
            }
            Action::KillGpu { generation, reason } => {
                if let Some(mut gpu) = self.gpu.take_if(|g| g.generation == generation) {
                    warn!(
                        generation,
                        %reason,
                        "killing the gpu for a bad message"
                    );
                    gpu.slot.kill();
                }
            }
            Action::LoadSubresources { doc_id, urls } => self.load_subresources(doc_id, urls),
            Action::Present(_) | Action::Warnings(_) | Action::ShowError(_) | Action::Exit(_) => {}
        }
    }

    /// Waits up to the next tick for something to happen and returns it as core events; an
    /// [`Event::Tick`] is among them whenever one is due.
    pub fn next_events(&mut self) -> Vec<Event> {
        let mut out = std::mem::take(&mut self.pending);
        // Wait only when there is nothing to hand out; the queue is read either way, so a Pong
        // that arrived while the loop was busy is seen before the watchdog runs.
        let first = if out.is_empty() {
            let wait = self.next_tick.saturating_duration_since(Instant::now());
            // The loop holds a sender itself, so the queue never disconnects.
            self.events.recv_timeout(wait).ok()
        } else {
            self.events.try_recv().ok()
        };
        let mut next = first;
        for _ in 0..=BATCH {
            let Some(event) = next.take() else {
                break;
            };
            // A frame ends the batch: its permit is back, and the core takes its pixels before
            // the next copy is let into the queue.
            let frame = matches!(event, IoEvent::Frame { .. });
            self.on_io(event, &mut out);
            if frame {
                break;
            }
            next = self.events.try_recv().ok();
        }
        let now = Instant::now();
        if now >= self.next_tick {
            self.tick(now, &mut out);
            out.push(Event::Tick);
            self.next_tick = now + TICK;
        }
        out
    }

    /// Asks both children to exit and waits up to `timeout` for each, killing what is left.
    pub fn shutdown(mut self, timeout: Duration) {
        self.renderer.send("the renderer", ToRenderer::Shutdown);
        if let Some(gpu) = &mut self.gpu {
            gpu.slot.send("the gpu", ToGpu::Shutdown);
        }
        wait_or_kill("renderer", &mut self.renderer.handle, timeout);
        if let Some(gpu) = &mut self.gpu {
            wait_or_kill("gpu", &mut gpu.slot.handle, timeout);
        }
    }

    /// Kills both children (dropping their handles would too; this logs nothing).
    pub fn kill_all(mut self) {
        self.renderer.kill();
        if let Some(gpu) = &mut self.gpu {
            gpu.slot.kill();
        }
    }

    fn renderer_message(&self, cmd: ToRendererCmd) -> ToRenderer {
        match cmd {
            ToRendererCmd::LoadDocument {
                doc_id,
                viewport,
                seq,
            } => ToRenderer::LoadDocument {
                doc_id,
                base_url: self.doc.base_url.to_string(),
                viewport,
                seq,
                bytes: IpcSharedMemory::from_bytes(&self.doc.bytes),
            },
            ToRendererCmd::Subresources { doc_id, replies } => ToRenderer::Subresources {
                doc_id,
                replies: replies.into_iter().map(subresource_reply).collect(),
            },
            ToRendererCmd::Resize { viewport, seq } => ToRenderer::Resize { viewport, seq },
            ToRendererCmd::Redraw { seq } => ToRenderer::Redraw { seq },
        }
    }

    fn send_gpu(&mut self, cmd: ToGpuCmd) {
        let Some(gpu) = &mut self.gpu else {
            return;
        };
        let ToGpuCmd::SetSurface { viewport, seq } = cmd;
        // Set before the request leaves, so the frame that answers it is never refused.
        gpu.frame_limit
            .fetch_max(frame_bytes(viewport), Ordering::AcqRel);
        gpu.slot
            .send("the gpu", ToGpu::SetSurface { viewport, seq });
    }

    /// Spawns a gpu of `generation`, makes it the current one, and wires it.
    fn spawn_gpu(&mut self, generation: u64) -> Result<(), SpawnFailure> {
        let mut gpu =
            spawn_child::<GpuProtocol>(&self.exe, &self.gpu_opts).map_err(|e| match e {
                StartError::SandboxUnavailable => SpawnFailure::Sandbox,
                StartError::Other(e) => SpawnFailure::Other(e),
            })?;
        let lists = gpu
            .endpoint
            .take_display_lists()
            .ok_or_else(|| anyhow::anyhow!("the gpu sent no display-list sender"))?;
        let (frames_tx, frames_rx) = ipc::channel::<FrameMsg>().map_err(anyhow::Error::from)?;
        let ctx = gpu.receiver_ctx();
        let pid = gpu.pid();
        let (endpoint, handle) = gpu.into_parts();
        let (down, up) = endpoint.split(ctx);
        let writer = spawn_writer(&format!("gpu-{generation}-writer"), move |msg| {
            down.send(msg).is_ok()
        })
        .map_err(anyhow::Error::from)?;

        let tx = self.events_tx.clone();
        pump::spawn_reader(&format!("gpu-{generation}-up"), up, move |event| {
            tx.send(gpu_event(generation, event)).is_ok()
        })
        .map_err(anyhow::Error::from)?;

        let frame_limit = Arc::new(AtomicUsize::new(0));
        let (permit_returns, permits) = mpsc::sync_channel::<()>(FRAME_PERMITS);
        for _ in 0..FRAME_PERMITS {
            permit_returns.send(()).map_err(anyhow::Error::from)?;
        }
        let limit = Arc::clone(&frame_limit);
        let tx = self.events_tx.clone();
        pump::spawn_reader(
            &format!("gpu-{generation}-frames"),
            frames_rx,
            move |event: PumpEvent<FrameMsg>| {
                // Taken before the copy: at most FRAME_PERMITS copies wait in the queue. Fails
                // once this gpu is gone (its permit sender dropped), which stops the reader.
                if permits.recv().is_err() {
                    return false;
                }
                let Some(event) = frame_event(generation, event, &limit) else {
                    return false;
                };
                tx.send(IoEvent::Frame { generation, event }).is_ok()
            },
        )
        .map_err(anyhow::Error::from)?;

        let mut slot = Slot::new(handle, writer, Instant::now());
        slot.send("the gpu", ToGpu::FrameSender { sender: frames_tx });
        self.renderer.send(
            "the renderer",
            ToRenderer::DisplayListSender {
                generation,
                sender: lists,
            },
        );
        info!(pid, generation, "gpu started");
        self.gpu = Some(Gpu {
            generation,
            slot,
            frame_limit,
            frame_permits: permit_returns,
        });
        Ok(())
    }

    fn load_subresources(&self, doc_id: u64, urls: Vec<String>) {
        let dir = self.doc.dir.clone();
        let tx = self.events_tx.clone();
        let worker = std::thread::Builder::new()
            .name("subresources".to_owned())
            .spawn(move || {
                let replies = load_subresources(&dir, &urls, MAX_SUBRESOURCE_BYTES_PER_DOCUMENT);
                let _ = tx.send(IoEvent::Core(Event::SubresourcesLoaded { doc_id, replies }));
            });
        if let Err(e) = worker {
            // Without the worker the renderer waits for its subresource timeout and renders
            // without them, with a warning per URL.
            warn!(error = %e, "could not start the subresource worker");
        }
    }

    fn on_io(&mut self, event: IoEvent, out: &mut Vec<Event>) {
        match event {
            IoEvent::Core(event) => out.push(event),
            IoEvent::Frame { generation, event } => {
                if let Some(gpu) = self.gpu.as_ref().filter(|g| g.generation == generation) {
                    let _ = gpu.frame_permits.try_send(());
                }
                out.push(event);
            }
            IoEvent::Pong { child, nonce } => {
                let now = Instant::now();
                match child {
                    ChildId::Renderer => self.renderer.watchdog.answered(nonce, now),
                    ChildId::Gpu(generation) => {
                        if let Some(gpu) = self.gpu.as_mut().filter(|g| g.generation == generation)
                        {
                            gpu.slot.watchdog.answered(nonce, now);
                        }
                    }
                }
            }
            IoEvent::UpClosed { child } => match child {
                ChildId::Renderer => {
                    self.renderer.up_closed = true;
                    self.report_renderer_exit(out);
                }
                ChildId::Gpu(generation) => {
                    if let Some(gpu) = self.gpu.as_mut().filter(|g| g.generation == generation) {
                        gpu.slot.up_closed = true;
                    }
                    self.report_gpu_exit(generation, out);
                }
            },
        }
    }

    fn report_renderer_exit(&mut self, out: &mut Vec<Event>) {
        if self.renderer.reported {
            return;
        }
        self.renderer.reported = true;
        let code = self.renderer.reap();
        info!(?code, "renderer exited");
        out.push(Event::RendererExited(code));
    }

    fn report_gpu_exit(&mut self, generation: u64, out: &mut Vec<Event>) {
        let Some(mut gpu) = self.gpu.take_if(|g| g.generation == generation) else {
            return;
        };
        let code = gpu.slot.reap();
        info!(generation, ?code, "gpu exited");
        out.push(Event::GpuExited { generation, code });
    }

    /// Exit polling and the ping watchdog.
    fn tick(&mut self, now: Instant, out: &mut Vec<Event>) {
        if !self.renderer.reported {
            if exited_long_enough(&mut self.renderer, now) {
                self.report_renderer_exit(out);
            } else {
                watch(&mut self.renderer, "the renderer", now, ToRenderer::Ping);
            }
        }
        if let Some(gpu) = &mut self.gpu {
            let generation = gpu.generation;
            if exited_long_enough(&mut gpu.slot, now) {
                self.report_gpu_exit(generation, out);
            } else {
                watch(&mut gpu.slot, "the gpu", now, ToGpu::Ping);
            }
        }
    }
}

/// Why a gpu did not start.
#[derive(Debug)]
enum SpawnFailure {
    Sandbox,
    Other(anyhow::Error),
}

impl From<anyhow::Error> for SpawnFailure {
    fn from(e: anyhow::Error) -> Self {
        SpawnFailure::Other(e)
    }
}

/// Whether the child exited (polled now) and its channel had [`EXIT_DRAIN_GRACE`] to close.
fn exited_long_enough<D>(slot: &mut Slot<D>, now: Instant) -> bool {
    if slot.exited_at.is_none() && matches!(slot.handle.try_wait(), Ok(Some(_))) {
        slot.exited_at = Some(now);
    }
    slot.exited_at
        .is_some_and(|at| now.saturating_duration_since(at) >= EXIT_DRAIN_GRACE)
}

/// Pings the child when due; kills it when its last ping is overdue.
fn watch<D>(slot: &mut Slot<D>, what: &str, now: Instant, ping: fn(u64) -> D) {
    if slot.writer.is_none() {
        return;
    }
    if slot.watchdog.overdue(now) {
        warn!(
            pid = slot.handle.pid(),
            "{what} did not answer a ping within {CHILD_PING_TIMEOUT:?}; killing it"
        );
        slot.kill();
        return;
    }
    if let Some(nonce) = slot.watchdog.due(now) {
        slot.send(what, ping(nonce));
    }
}

fn wait_or_kill(what: &str, handle: &mut ChildHandle, timeout: Duration) {
    match handle.wait_timeout(timeout) {
        Ok(outcome) if outcome.killed && !outcome.status.success() => {
            warn!(
                pid = handle.pid(),
                "{what} did not exit after Shutdown; killed after {timeout:?}"
            );
        }
        Ok(_) => {}
        Err(e) => warn!(pid = handle.pid(), error = %e, "waiting for the {what} failed"),
    }
}

/// Spawns a child of protocol `P`, mapping a refusal to run unsandboxed to its own error.
fn spawn_child<P: ChildProtocol>(
    exe: &Path,
    opts: &SpawnOptions,
) -> Result<cl_process::ChildProcess<P>, StartError> {
    match spawn::<P>(exe, opts) {
        Ok(child) => Ok(child),
        Err(ProcessError::ExitedDuringBootstrap {
            code: Some(EXIT_SANDBOX_UNAVAILABLE),
        }) => {
            tracing::error!(
                role = %P::ROLE,
                "{} exited during bootstrap with {EXIT_SANDBOX_UNAVAILABLE}: sandbox unavailable for {}",
                P::ROLE,
                P::ROLE
            );
            Err(StartError::SandboxUnavailable)
        }
        Err(e) => Err(StartError::Other(e.into())),
    }
}

/// A writer thread that hands each message to `send` (the child's channel), behind a queue of
/// [`WRITER_QUEUE`] messages. It ends when the queue's sender is dropped or `send` fails (the
/// child's channel broke).
fn spawn_writer<T: Send + 'static>(
    name: &str,
    send: impl Fn(T) -> bool + Send + 'static,
) -> std::io::Result<SyncSender<T>> {
    let (queue, inbox) = mpsc::sync_channel::<T>(WRITER_QUEUE);
    std::thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || {
            for msg in inbox {
                if !send(msg) {
                    return;
                }
            }
        })?;
    Ok(queue)
}

fn renderer_event(event: PumpEvent<FromRenderer>) -> IoEvent {
    match event {
        PumpEvent::Message(FromRenderer::Pong(nonce)) => IoEvent::Pong {
            child: ChildId::Renderer,
            nonce,
        },
        PumpEvent::Message(msg) => IoEvent::Core(Event::RendererMsg(msg)),
        PumpEvent::Error(bad) => {
            info!(detail = %bad.detail, "bad renderer message");
            IoEvent::Core(Event::RendererBad(
                bad.reason.unwrap_or(RendererProtocol::UNDECODABLE),
            ))
        }
        PumpEvent::Closed => IoEvent::UpClosed {
            child: ChildId::Renderer,
        },
        PumpEvent::Failed(e) => {
            warn!(error = %e, "the renderer's channel failed");
            IoEvent::UpClosed {
                child: ChildId::Renderer,
            }
        }
    }
}

fn gpu_event(generation: u64, event: PumpEvent<FromGpu>) -> IoEvent {
    match event {
        PumpEvent::Message(FromGpu::Pong(nonce)) => IoEvent::Pong {
            child: ChildId::Gpu(generation),
            nonce,
        },
        PumpEvent::Message(msg) => IoEvent::Core(Event::GpuMsg { generation, msg }),
        PumpEvent::Error(bad) => {
            info!(generation, detail = %bad.detail, "bad gpu message");
            IoEvent::Core(Event::GpuBad {
                generation,
                reason: bad.reason.unwrap_or(GpuProtocol::UNDECODABLE),
            })
        }
        PumpEvent::Closed => IoEvent::UpClosed {
            child: ChildId::Gpu(generation),
        },
        PumpEvent::Failed(e) => {
            warn!(generation, error = %e, "the gpu's channel failed");
            IoEvent::UpClosed {
                child: ChildId::Gpu(generation),
            }
        }
    }
}

/// A frame-channel event as a core event, the pixels copied if they fit `limit`; `None` once
/// the channel is closed (the gpu's exit arrives on its upward channel).
fn frame_event(generation: u64, event: PumpEvent<FrameMsg>, limit: &AtomicUsize) -> Option<Event> {
    match event {
        PumpEvent::Message(frame) => {
            let max = limit.load(Ordering::Acquire);
            Some(match copy_bounded(&frame.pixels, max) {
                Ok(pixels) => Event::Frame {
                    generation,
                    meta: FrameMeta {
                        seq: frame.seq,
                        width: frame.width,
                        height: frame.height,
                    },
                    pixels,
                },
                Err(e) => {
                    info!(generation, error = %e, "frame refused before copying");
                    Event::FrameBad {
                        generation,
                        reason: BadMessageReason::FrameWrongSize,
                    }
                }
            })
        }
        PumpEvent::Error(bad) => {
            info!(generation, detail = %bad.detail, "undecodable frame message");
            Some(Event::FrameBad {
                generation,
                reason: BadMessageReason::GpuUndecodable,
            })
        }
        PumpEvent::Failed(e) => {
            warn!(generation, error = %e, "the gpu's frame channel failed");
            Some(Event::FrameBad {
                generation,
                reason: BadMessageReason::GpuUndecodable,
            })
        }
        PumpEvent::Closed => None,
    }
}

/// `width × height × 4`, saturating.
fn frame_bytes(viewport: Viewport) -> usize {
    let bytes = u64::from(viewport.width())
        .saturating_mul(u64::from(viewport.height()))
        .saturating_mul(4);
    usize::try_from(bytes).unwrap_or(usize::MAX)
}

fn subresource_reply(loaded: LoadedSubresource) -> SubresourceReply {
    SubresourceReply {
        index: loaded.index,
        result: loaded
            .result
            .map(|bytes| IpcSharedMemory::from_bytes(&bytes)),
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "a failed setup step should abort the test, loudly"
)]
mod tests {
    use std::sync::atomic::AtomicUsize;
    use std::time::{Duration, Instant};

    use cl_ipc::bad_message::BadMessageReason;
    use cl_ipc::proto::Viewport;
    use cl_ipc::pump::PumpEvent;
    use cl_ipc::transfer::FrameMsg;
    use ipc_channel::ipc::IpcSharedMemory;

    use super::{CHILD_PING_TIMEOUT, PING_INTERVAL, Watchdog, frame_bytes, frame_event};
    use crate::browser::core::Event;
    use crate::browser::frames::FrameMeta;

    fn frame(len: usize) -> PumpEvent<FrameMsg> {
        PumpEvent::Message(FrameMsg {
            seq: 3,
            width: 2,
            height: 2,
            pixels: IpcSharedMemory::from_bytes(&vec![9; len]),
        })
    }

    #[test]
    fn a_frame_within_the_limit_is_copied() {
        let limit = AtomicUsize::new(16);
        assert_eq!(
            frame_event(7, frame(16), &limit),
            Some(Event::Frame {
                generation: 7,
                meta: FrameMeta {
                    seq: 3,
                    width: 2,
                    height: 2
                },
                pixels: vec![9; 16],
            })
        );
    }

    #[test]
    fn a_frame_over_the_limit_is_refused_unread() {
        // The gpu chose the size of its shared memory; the browser asked for at most 16 bytes.
        let limit = AtomicUsize::new(16);
        assert_eq!(
            frame_event(7, frame(17), &limit),
            Some(Event::FrameBad {
                generation: 7,
                reason: BadMessageReason::FrameWrongSize
            })
        );
        // Before any surface was asked of the gpu, no frame fits.
        assert_eq!(
            frame_event(7, frame(1), &AtomicUsize::new(0)),
            Some(Event::FrameBad {
                generation: 7,
                reason: BadMessageReason::FrameWrongSize
            })
        );
    }

    #[test]
    fn a_closed_frame_channel_ends_the_reader() {
        assert_eq!(
            frame_event(7, PumpEvent::Closed, &AtomicUsize::new(16)),
            None
        );
    }

    #[test]
    fn frame_bytes_is_the_surface_area_times_four() {
        let vp = Viewport::new(800, 600).expect("viewport");
        assert_eq!(frame_bytes(vp), 800 * 600 * 4);
    }

    #[test]
    fn the_watchdog_pings_once_per_interval_and_waits_for_the_answer() {
        let t0 = Instant::now();
        let mut dog = Watchdog::new(t0);
        assert_eq!(dog.due(t0), None, "no ping before the first interval");
        let first = dog.due(t0 + PING_INTERVAL).expect("a ping is due");
        assert_eq!(
            dog.due(t0 + PING_INTERVAL * 2),
            None,
            "one ping outstanding"
        );
        // A wrong nonce does not count as an answer.
        dog.answered(first.wrapping_add(1), t0 + PING_INTERVAL);
        assert!(dog.overdue(t0 + PING_INTERVAL + CHILD_PING_TIMEOUT));
        dog.answered(first, t0 + PING_INTERVAL * 2);
        assert!(!dog.overdue(t0 + PING_INTERVAL * 2 + CHILD_PING_TIMEOUT));
        let second = dog.due(t0 + PING_INTERVAL * 3).expect("the next ping");
        assert_ne!(first, second);
    }

    #[test]
    fn time_the_browser_was_blocked_does_not_count_against_a_child() {
        // A gpu spawn blocked the loop for 6 s right after a ping went out; its Pong waited in
        // the queue (Task 10 review).
        let t0 = Instant::now();
        let mut dog = Watchdog::new(t0);
        let sent = t0 + PING_INTERVAL;
        let _ = dog.due(sent).expect("a ping is due");
        dog.postpone(Duration::from_secs(6));
        let after_spawn = sent + Duration::from_secs(6);
        assert!(!dog.overdue(after_spawn));
        assert!(dog.overdue(after_spawn + CHILD_PING_TIMEOUT));
    }

    #[test]
    fn an_unanswered_ping_is_overdue_after_the_timeout() {
        let t0 = Instant::now();
        let mut dog = Watchdog::new(t0);
        let sent = t0 + PING_INTERVAL;
        let _ = dog.due(sent).expect("a ping is due");
        let just_before = (sent + CHILD_PING_TIMEOUT)
            .checked_sub(Duration::from_millis(1))
            .expect("representable");
        assert!(!dog.overdue(just_before));
        assert!(dog.overdue(sent + CHILD_PING_TIMEOUT));
    }
}
