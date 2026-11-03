//! [`BrowserCore`]: the browser's frame, restart and subresource policy as a pure state
//! machine (M1b Task 7) — events in, actions out, no IO. The IO layer (Task 10) turns every
//! [`Action`] into a wire message, a spawn, a kill or a present, and every child message,
//! exit, frame and window event into an [`Event`].
//!
//! Policy (ADR-0016 §7, §9; spec §5):
//!
//! * A bad message from the renderer, or a display list the gpu rejected, kills the renderer
//!   (stable [`BadMessageReason`] code); the gpu stays. A renderer exit the browser did not
//!   cause is a crash. Neither is restarted in M1b (tab restart is M2).
//! * A bad message or a bad frame from the gpu, or a gpu exit, kills (if alive) and respawns
//!   the gpu under [`RestartPolicy`], with a new generation; events of an older generation are
//!   ignored. When the budget is spent there is no frame.
//! * The browser decides every `seq` and surface size. At most one `seq` is in flight; resizes
//!   during it coalesce into the latest size, requested when its frame lands.
//! * One subresource batch per document; a renderer message for a document or `seq` the
//!   browser never issued is a protocol violation.
//! * Benign races are dropped silently: a stale display list, a repeated frame for a `seq`
//!   already issued, a message from a child already killed, a failure report for a superseded
//!   `seq`, a reply for another document. A frame arriving after the renderer died is never
//!   presented.
//!
//! With [`CoreOptions::fatal_errors`] (headless, `--exit-after-first-present`) every failure
//! ends in [`Action::Exit`] with its code from [`crate::exit`]; in a window the error is shown
//! and the browser stays. Pings and their timeouts are the IO layer's (plan Task 10).

use std::time::{Duration, Instant};

use cl_ipc::bad_message::BadMessageReason;
use cl_ipc::proto::gpu::FromGpu;
use cl_ipc::proto::renderer::FromRenderer;
use cl_ipc::proto::{InvalidViewport, MAX_SURFACE_DIMENSION, Viewport};

use crate::browser::frames::{FrameMeta, check_frame};
use crate::browser::present::surface_for;
use crate::browser::restart::RestartPolicy;
use crate::browser::subresources::LoadedSubresource;
use crate::exit::{EXIT_NO_FRAME, EXIT_RENDERER_CRASHED, EXIT_RENDERER_KILLED_BAD_MESSAGE};

/// How long a fatal-errors browser waits for its first frame before giving up (no frame).
pub const FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(30);

/// The one document of M1b.
const DOC_ID: u64 = 1;

/// The generation of the gpu the IO layer spawns before [`BrowserCore::new`].
const FIRST_GPU_GENERATION: u64 = 1;

/// How the browser runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreOptions {
    /// Every failure exits with its code (headless, `--exit-after-first-present`); otherwise
    /// the error is shown in the window and the browser stays.
    pub fatal_errors: bool,
}

/// Something that happened, as the IO layer reports it.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// A renderer message that decoded and validated.
    RendererMsg(FromRenderer),
    /// A renderer message that did not decode or validate.
    RendererBad(BadMessageReason),
    /// The renderer process exited (its code, if it had one).
    RendererExited(Option<i32>),
    /// A gpu message that decoded and validated.
    GpuMsg {
        /// The gpu it came from.
        generation: u64,
        /// The message.
        msg: FromGpu,
    },
    /// A gpu message that did not decode or validate.
    GpuBad {
        /// The gpu it came from.
        generation: u64,
        /// Why it was rejected.
        reason: BadMessageReason,
    },
    /// A gpu process exited.
    GpuExited {
        /// Which gpu.
        generation: u64,
        /// Its exit code, if it had one.
        code: Option<i32>,
    },
    /// A frame, its pixels already copied out of shared memory.
    Frame {
        /// The gpu it came from.
        generation: u64,
        /// Its integer fields.
        meta: FrameMeta,
        /// The copied pixels.
        pixels: Vec<u8>,
    },
    /// A frame message that could not even be copied (its shared memory too large).
    FrameBad {
        /// The gpu it came from.
        generation: u64,
        /// Why it was rejected.
        reason: BadMessageReason,
    },
    /// The window's size or scale factor changed.
    Resized {
        /// The new size in physical pixels.
        physical: (u32, u32),
        /// The window's scale factor.
        scale: f64,
    },
    /// The answer to an [`Action::LoadSubresources`].
    SubresourcesLoaded {
        /// The document it was for.
        doc_id: u64,
        /// One answer per requested URL.
        replies: Vec<LoadedSubresource>,
    },
    /// Time passed (the IO layer ticks regularly).
    Tick,
}

/// Browser → renderer, as plain data; the IO layer builds the wire message (and the shared
/// memory for document and subresource bytes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToRendererCmd {
    /// Load the document (the IO layer attaches its bytes and base URL).
    LoadDocument {
        /// The document.
        doc_id: u64,
        /// The surface to lay out for.
        viewport: Viewport,
        /// The frame number.
        seq: u64,
    },
    /// The answer to the document's subresource request.
    Subresources {
        /// The document.
        doc_id: u64,
        /// One answer per requested URL.
        replies: Vec<LoadedSubresource>,
    },
    /// Lay out for a new surface size.
    Resize {
        /// The new size.
        viewport: Viewport,
        /// The frame number.
        seq: u64,
    },
    /// Send the current display list again with a new frame number.
    Redraw {
        /// The frame number.
        seq: u64,
    },
    /// Liveness probe.
    Ping(u64),
    /// Orderly exit.
    Shutdown,
}

/// Browser → gpu, as plain data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToGpuCmd {
    /// The size to raster at and the frame number to expect next.
    SetSurface {
        /// The surface size.
        viewport: Viewport,
        /// The frame number.
        seq: u64,
    },
    /// Liveness probe.
    Ping(u64),
    /// Orderly exit.
    Shutdown,
}

/// A checked frame to put in the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentFrame {
    /// Its frame number.
    pub seq: u64,
    /// Width in CSS pixels.
    pub width: u32,
    /// Height in CSS pixels.
    pub height: u32,
    /// The integer scale to blit it at ([`crate::browser::present::blit`]).
    pub k: u32,
    /// Premultiplied RGBA8, `width × height × 4` bytes.
    pub pixels: Vec<u8>,
}

/// What the window shows instead of the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// The renderer exited without the browser killing it.
    RendererCrashed,
    /// The browser killed the renderer for a bad message.
    RendererKilled,
    /// No frame: the gpu restart budget is spent, or the first frame never came.
    NoFrame,
    /// The renderer reported that it could not render the document.
    RenderFailed,
}

impl ErrorKind {
    /// The browser's exit code for this failure when failures are fatal.
    pub fn exit_code(self) -> i32 {
        match self {
            ErrorKind::RendererCrashed => EXIT_RENDERER_CRASHED,
            ErrorKind::RendererKilled => EXIT_RENDERER_KILLED_BAD_MESSAGE,
            ErrorKind::NoFrame | ErrorKind::RenderFailed => EXIT_NO_FRAME,
        }
    }
}

/// What the IO layer must do, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Send to the renderer.
    SendRenderer(ToRendererCmd),
    /// Send to the current gpu.
    SendGpu(ToGpuCmd),
    /// Spawn a gpu of this generation and wire it: its display-list sender down to the
    /// renderer (`DisplayListSender { generation }`), a new frame channel's sender down to it.
    SpawnGpu {
        /// Its generation; every event from it carries this number.
        generation: u64,
    },
    /// Kill the renderer and log the reason.
    KillRenderer {
        /// Why.
        reason: BadMessageReason,
    },
    /// Kill the gpu of this generation and log the reason.
    KillGpu {
        /// Which gpu.
        generation: u64,
        /// Why.
        reason: BadMessageReason,
    },
    /// Load these URLs under the subresource policy, off the event loop, and report
    /// [`Event::SubresourcesLoaded`].
    LoadSubresources {
        /// The document.
        doc_id: u64,
        /// The URLs, as the renderer sent them (already validated for count and length).
        urls: Vec<String>,
    },
    /// Put this frame in the window (or the PNG, headless).
    Present(PresentFrame),
    /// Report the document's warnings (stderr; the window's console later), control
    /// characters already escaped.
    Warnings(Vec<String>),
    /// Show this error instead of the page.
    ShowError(ErrorKind),
    /// Shut everything down and exit with this code.
    Exit(i32),
}

/// A surface the browser asks for: its CSS size and the blit scale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Surface {
    viewport: Viewport,
    k: u32,
}

/// The one frame the browser is waiting for.
#[derive(Debug, Clone, Copy)]
struct InFlight {
    seq: u64,
    surface: Surface,
}

/// What the browser knows about the renderer and its document.
#[derive(Debug, Clone, Copy)]
struct RendererState {
    alive: bool,
    /// The document's one subresource batch was asked for.
    subresources_requested: bool,
    /// The document's warnings were reported (with its first `DocumentReady`).
    warnings_reported: bool,
}

/// The browser's state between events. See the module docs for the policy.
#[derive(Debug)]
pub struct BrowserCore {
    options: CoreOptions,
    started: Instant,
    /// An [`Action::Exit`] was emitted; every later event is ignored.
    finished: bool,
    /// The last frame number issued.
    seq: u64,
    in_flight: Option<InFlight>,
    /// The latest size the window wants.
    target: Surface,
    /// The viewport the renderer last laid out for.
    renderer_viewport: Viewport,
    renderer: RendererState,
    presented: bool,
    gpu_generation: u64,
    gpu_alive: bool,
    restarts: RestartPolicy,
}

impl BrowserCore {
    /// The core for a window of `physical` pixels at `scale`, after the IO layer has spawned
    /// the renderer and the first gpu (generation 1), with the first actions: the gpu's
    /// surface, then the document load, both for `seq` 1.
    ///
    /// # Errors
    /// [`InvalidViewport`] for a zero-sized window; a larger one than
    /// [`MAX_SURFACE_DIMENSION`] is clamped.
    pub fn new(
        options: CoreOptions,
        physical: (u32, u32),
        scale: f64,
        now: Instant,
    ) -> Result<(Self, Vec<Action>), InvalidViewport> {
        let target = surface(physical, scale)?;
        let core = BrowserCore {
            options,
            started: now,
            finished: false,
            seq: 1,
            in_flight: Some(InFlight {
                seq: 1,
                surface: target,
            }),
            target,
            renderer_viewport: target.viewport,
            renderer: RendererState {
                alive: true,
                subresources_requested: false,
                warnings_reported: false,
            },
            presented: false,
            gpu_generation: FIRST_GPU_GENERATION,
            gpu_alive: true,
            restarts: RestartPolicy::default(),
        };
        let actions = vec![
            Action::SendGpu(ToGpuCmd::SetSurface {
                viewport: target.viewport,
                seq: core.seq,
            }),
            Action::SendRenderer(ToRendererCmd::LoadDocument {
                doc_id: DOC_ID,
                viewport: target.viewport,
                seq: core.seq,
            }),
        ];
        Ok((core, actions))
    }

    /// Handles one event at `now` and returns what to do, in order.
    pub fn on_event(&mut self, event: Event, now: Instant) -> Vec<Action> {
        if self.finished {
            return Vec::new();
        }
        let mut out = Vec::new();
        match event {
            Event::RendererMsg(msg) => self.on_renderer_msg(msg, &mut out),
            Event::RendererBad(reason) => self.kill_renderer(reason, &mut out),
            Event::RendererExited(_) => {
                if self.renderer.alive {
                    self.renderer.alive = false;
                    self.fail(ErrorKind::RendererCrashed, &mut out);
                }
            }
            Event::GpuMsg { generation, msg } if self.is_current_gpu(generation) => match msg {
                FromGpu::Pong(_) | FromGpu::Stale { .. } => {}
                FromGpu::BadDisplayList { reason, .. } => self.kill_renderer(reason, &mut out),
            },
            Event::GpuBad { generation, reason } | Event::FrameBad { generation, reason }
                if self.is_current_gpu(generation) =>
            {
                self.fail_gpu(Some(reason), now, &mut out);
            }
            Event::GpuExited { generation, .. } if self.is_current_gpu(generation) => {
                self.fail_gpu(None, now, &mut out);
            }
            Event::Frame {
                generation,
                meta,
                pixels,
            } if self.is_current_gpu(generation) => self.on_frame(&meta, pixels, now, &mut out),
            Event::GpuMsg { .. }
            | Event::GpuBad { .. }
            | Event::FrameBad { .. }
            | Event::GpuExited { .. }
            | Event::Frame { .. } => {}
            Event::Resized { physical, scale } => self.on_resized(physical, scale, &mut out),
            Event::SubresourcesLoaded { doc_id, replies } => {
                if doc_id == DOC_ID && self.renderer.alive {
                    out.push(Action::SendRenderer(ToRendererCmd::Subresources {
                        doc_id,
                        replies,
                    }));
                }
            }
            Event::Tick => {
                let waited = now.saturating_duration_since(self.started);
                if self.options.fatal_errors && !self.presented && waited >= FIRST_FRAME_TIMEOUT {
                    self.fail(ErrorKind::NoFrame, &mut out);
                }
            }
        }
        out
    }

    fn is_current_gpu(&self, generation: u64) -> bool {
        self.gpu_alive && generation == self.gpu_generation
    }

    fn on_renderer_msg(&mut self, msg: FromRenderer, out: &mut Vec<Action>) {
        if !self.renderer.alive {
            return;
        }
        let unexpected = BadMessageReason::RendererUnexpectedMessage;
        match msg {
            FromRenderer::Pong(_) => {}
            FromRenderer::NeedSubresources { doc_id, urls } => {
                if doc_id != DOC_ID || self.renderer.subresources_requested {
                    self.kill_renderer(unexpected, out);
                } else {
                    self.renderer.subresources_requested = true;
                    out.push(Action::LoadSubresources { doc_id, urls });
                }
            }
            FromRenderer::DocumentReady { seq, warnings } => {
                if seq > self.seq {
                    self.kill_renderer(unexpected, out);
                } else if !self.renderer.warnings_reported {
                    self.renderer.warnings_reported = true;
                    if !warnings.is_empty() {
                        out.push(Action::Warnings(
                            warnings.iter().map(|w| escape_controls(w)).collect(),
                        ));
                    }
                }
            }
            FromRenderer::RenderFailed { seq, .. } => {
                if seq > self.seq {
                    self.kill_renderer(unexpected, out);
                    return;
                }
                // Only the seq in flight counts; a failure of a superseded one is old news.
                let Some(failed) = self.in_flight.filter(|f| f.seq == seq) else {
                    return;
                };
                self.fail(ErrorKind::RenderFailed, out);
                // In a window: the failed seq frees the slot; a different size wanted since
                // gets its chance (the same size would only fail again).
                self.in_flight = None;
                if !self.finished && self.target != failed.surface {
                    self.request(out);
                }
            }
        }
    }

    fn on_frame(&mut self, meta: &FrameMeta, pixels: Vec<u8>, now: Instant, out: &mut Vec<Action>) {
        // A frame still in flight when the renderer died carries its content: never paint it
        // over the error.
        if !self.renderer.alive {
            return;
        }
        let Some(expected) = self.in_flight.filter(|f| f.seq == meta.seq) else {
            // A seq the browser issued earlier is a benign duplicate: the renderer holds the
            // display-list sender and can send one list twice, and the gpu rasters both. Only
            // a seq never issued is the gpu's own fault.
            if meta.seq > self.seq {
                self.fail_gpu(Some(BadMessageReason::FrameUnexpectedSeq), now, out);
            }
            return;
        };
        let size = (
            expected.surface.viewport.width(),
            expected.surface.viewport.height(),
        );
        if let Err(reason) = check_frame(meta, pixels.len(), expected.seq, size) {
            self.fail_gpu(Some(reason), now, out);
            return;
        }
        self.presented = true;
        self.in_flight = None;
        out.push(Action::Present(PresentFrame {
            seq: meta.seq,
            width: meta.width,
            height: meta.height,
            k: expected.surface.k,
            pixels,
        }));
        if self.target != expected.surface {
            self.request(out);
        }
    }

    fn on_resized(&mut self, physical: (u32, u32), scale: f64, out: &mut Vec<Action>) {
        // A minimised window reports 0 × 0: keep the last size.
        let Ok(target) = surface(physical, scale) else {
            return;
        };
        if target == self.target {
            return;
        }
        self.target = target;
        if self.in_flight.is_none() {
            self.request(out);
        }
    }

    /// Asks for a frame of the target size with a new `seq`, if there is a renderer and a gpu
    /// to make it.
    fn request(&mut self, out: &mut Vec<Action>) {
        if !self.renderer.alive || !self.gpu_alive {
            return;
        }
        self.seq += 1;
        let seq = self.seq;
        let surface = self.target;
        self.in_flight = Some(InFlight { seq, surface });
        out.push(Action::SendGpu(ToGpuCmd::SetSurface {
            viewport: surface.viewport,
            seq,
        }));
        let to_renderer = if surface.viewport == self.renderer_viewport {
            ToRendererCmd::Redraw { seq }
        } else {
            self.renderer_viewport = surface.viewport;
            ToRendererCmd::Resize {
                viewport: surface.viewport,
                seq,
            }
        };
        out.push(Action::SendRenderer(to_renderer));
    }

    fn kill_renderer(&mut self, reason: BadMessageReason, out: &mut Vec<Action>) {
        if !self.renderer.alive {
            return;
        }
        self.renderer.alive = false;
        out.push(Action::KillRenderer { reason });
        self.fail(ErrorKind::RendererKilled, out);
    }

    /// The current gpu failed: `reason` is why the browser kills it, `None` if it already
    /// exited. Respawns it under the restart budget; without a renderer there is nothing to
    /// draw, so it is not respawned.
    fn fail_gpu(&mut self, reason: Option<BadMessageReason>, now: Instant, out: &mut Vec<Action>) {
        if let Some(reason) = reason {
            out.push(Action::KillGpu {
                generation: self.gpu_generation,
                reason,
            });
        }
        self.gpu_alive = false;
        self.in_flight = None;
        if !self.renderer.alive {
            return;
        }
        if !self.restarts.allow(now) {
            self.fail(ErrorKind::NoFrame, out);
            return;
        }
        self.gpu_generation += 1;
        self.gpu_alive = true;
        out.push(Action::SpawnGpu {
            generation: self.gpu_generation,
        });
        self.request(out);
    }

    fn fail(&mut self, kind: ErrorKind, out: &mut Vec<Action>) {
        out.push(Action::ShowError(kind));
        if self.options.fatal_errors {
            self.finished = true;
            out.push(Action::Exit(kind.exit_code()));
        }
    }
}

/// `text` with every control character escaped (`\u{1b}`, `\n` …): renderer-chosen text goes
/// to a terminal, where an escape sequence could set the title or write the clipboard (OSC 52).
fn escape_controls(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() {
                c.escape_default().to_string()
            } else {
                c.to_string()
            }
        })
        .collect()
}

/// The surface for a window: [`surface_for`], each edge clamped to
/// [`MAX_SURFACE_DIMENSION`].
///
/// # Errors
/// [`InvalidViewport`] for a zero edge.
fn surface(physical: (u32, u32), scale: f64) -> Result<Surface, InvalidViewport> {
    let (width, height, k) = surface_for(physical, scale);
    let viewport = Viewport::new(
        width.min(MAX_SURFACE_DIMENSION),
        height.min(MAX_SURFACE_DIMENSION),
    )?;
    Ok(Surface { viewport, k })
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "a failed setup step should abort the test, loudly"
)]
mod tests {
    use std::time::{Duration, Instant};

    use cl_ipc::bad_message::BadMessageReason;
    use cl_ipc::proto::Viewport;
    use cl_ipc::proto::gpu::FromGpu;
    use cl_ipc::proto::renderer::{FromRenderer, RenderFailureKind, SubresourceRefusal};

    use super::{
        Action, BrowserCore, CoreOptions, ErrorKind, Event, PresentFrame, ToGpuCmd, ToRendererCmd,
    };
    use crate::browser::frames::FrameMeta;
    use crate::browser::subresources::LoadedSubresource;
    use crate::exit::{EXIT_NO_FRAME, EXIT_RENDERER_CRASHED, EXIT_RENDERER_KILLED_BAD_MESSAGE};

    const FATAL: CoreOptions = CoreOptions { fatal_errors: true };
    const WINDOW: CoreOptions = CoreOptions {
        fatal_errors: false,
    };

    fn vp(width: u32, height: u32) -> Viewport {
        Viewport::new(width, height).expect("viewport")
    }

    /// A core started at 800×600, scale 1, with the initial actions checked.
    fn start(options: CoreOptions) -> (BrowserCore, Instant) {
        let t0 = Instant::now();
        let (core, actions) = BrowserCore::new(options, (800, 600), 1.0, t0).expect("core");
        assert_eq!(
            actions,
            [
                Action::SendGpu(ToGpuCmd::SetSurface {
                    viewport: vp(800, 600),
                    seq: 1
                }),
                Action::SendRenderer(ToRendererCmd::LoadDocument {
                    doc_id: 1,
                    viewport: vp(800, 600),
                    seq: 1
                }),
            ]
        );
        (core, t0)
    }

    fn frame(generation: u64, seq: u64, width: u32, height: u32) -> Event {
        let len = usize::try_from(width * height * 4).expect("small");
        Event::Frame {
            generation,
            meta: FrameMeta { seq, width, height },
            pixels: vec![255; len],
        }
    }

    fn presented(seq: u64, width: u32, height: u32, k: u32) -> Action {
        let len = usize::try_from(width * height * 4).expect("small");
        Action::Present(PresentFrame {
            seq,
            width,
            height,
            k,
            pixels: vec![255; len],
        })
    }

    fn at(t0: Instant, secs: u64) -> Instant {
        t0 + Duration::from_secs(secs)
    }

    #[test]
    fn the_first_frame_is_presented() {
        let (mut core, t0) = start(FATAL);
        assert_eq!(
            core.on_event(frame(1, 1, 800, 600), t0),
            [presented(1, 800, 600, 1)]
        );
    }

    #[test]
    fn bad_display_list_kills_renderer() {
        let (mut core, t0) = start(FATAL);
        let actions = core.on_event(
            Event::GpuMsg {
                generation: 1,
                msg: FromGpu::BadDisplayList {
                    seq: 1,
                    reason: BadMessageReason::DisplayListInvalid,
                },
            },
            t0,
        );
        assert_eq!(
            actions,
            [
                Action::KillRenderer {
                    reason: BadMessageReason::DisplayListInvalid
                },
                Action::ShowError(ErrorKind::RendererKilled),
                Action::Exit(EXIT_RENDERER_KILLED_BAD_MESSAGE),
            ]
        );
    }

    #[test]
    fn renderer_validate_failure_kills_renderer() {
        let (mut core, t0) = start(FATAL);
        let actions = core.on_event(
            Event::RendererBad(BadMessageReason::RendererInvalidField),
            t0,
        );
        assert_eq!(
            actions,
            [
                Action::KillRenderer {
                    reason: BadMessageReason::RendererInvalidField
                },
                Action::ShowError(ErrorKind::RendererKilled),
                Action::Exit(EXIT_RENDERER_KILLED_BAD_MESSAGE),
            ]
        );
    }

    #[test]
    fn in_a_window_a_killed_renderer_shows_the_error_and_the_browser_stays() {
        let (mut core, t0) = start(WINDOW);
        let actions = core.on_event(
            Event::RendererBad(BadMessageReason::RendererUndecodable),
            t0,
        );
        assert_eq!(
            actions,
            [
                Action::KillRenderer {
                    reason: BadMessageReason::RendererUndecodable
                },
                Action::ShowError(ErrorKind::RendererKilled),
            ]
        );
        // Its exit is the kill's, not a crash; later renderer traffic is ignored.
        assert_eq!(core.on_event(Event::RendererExited(Some(9)), t0), []);
        assert_eq!(
            core.on_event(
                Event::RendererBad(BadMessageReason::RendererUndecodable),
                t0
            ),
            []
        );
        // A resize has no renderer to go to; the gpu is left alone.
        assert_eq!(
            core.on_event(
                Event::Resized {
                    physical: (500, 400),
                    scale: 1.0
                },
                t0
            ),
            []
        );
    }

    #[test]
    fn a_renderer_crash_exits_3() {
        let (mut core, t0) = start(FATAL);
        assert_eq!(
            core.on_event(Event::RendererExited(Some(1)), t0),
            [
                Action::ShowError(ErrorKind::RendererCrashed),
                Action::Exit(EXIT_RENDERER_CRASHED),
            ]
        );
    }

    #[test]
    fn nothing_happens_after_exit() {
        let (mut core, t0) = start(FATAL);
        let _ = core.on_event(Event::RendererExited(None), t0);
        assert_eq!(core.on_event(frame(1, 1, 800, 600), t0), []);
        assert_eq!(core.on_event(Event::Tick, at(t0, 100)), []);
    }

    #[test]
    fn stale_is_dropped_silently() {
        let (mut core, t0) = start(FATAL);
        assert_eq!(
            core.on_event(
                Event::GpuMsg {
                    generation: 1,
                    msg: FromGpu::Stale { seq: 0 }
                },
                t0
            ),
            []
        );
        assert_eq!(
            core.on_event(
                Event::GpuMsg {
                    generation: 1,
                    msg: FromGpu::Pong(7)
                },
                t0
            ),
            []
        );
    }

    /// The actions of a gpu restart into `generation` that redraws at `seq` and `viewport`.
    fn restart(generation: u64, seq: u64, viewport: Viewport) -> [Action; 3] {
        [
            Action::SpawnGpu { generation },
            Action::SendGpu(ToGpuCmd::SetSurface { viewport, seq }),
            Action::SendRenderer(ToRendererCmd::Redraw { seq }),
        ]
    }

    #[test]
    fn bad_frame_restarts_gpu() {
        let (mut core, t0) = start(FATAL);
        let actions = core.on_event(
            Event::FrameBad {
                generation: 1,
                reason: BadMessageReason::FrameShortPixels,
            },
            t0,
        );
        let mut expected = vec![Action::KillGpu {
            generation: 1,
            reason: BadMessageReason::FrameShortPixels,
        }];
        expected.extend(restart(2, 2, vp(800, 600)));
        assert_eq!(actions, expected);
        // The new generation's frame for the new seq is presented.
        assert_eq!(
            core.on_event(frame(2, 2, 800, 600), t0),
            [presented(2, 800, 600, 1)]
        );
    }

    #[test]
    fn a_frame_failing_the_check_restarts_gpu() {
        let (mut core, t0) = start(FATAL);
        let actions = core.on_event(frame(1, 1, 801, 600), t0);
        assert_eq!(
            actions.first(),
            Some(&Action::KillGpu {
                generation: 1,
                reason: BadMessageReason::FrameWrongSize
            })
        );
        let (mut core, t0) = start(FATAL);
        let actions = core.on_event(frame(1, 9, 800, 600), t0);
        assert_eq!(
            actions.first(),
            Some(&Action::KillGpu {
                generation: 1,
                reason: BadMessageReason::FrameUnexpectedSeq
            })
        );
    }

    #[test]
    fn a_bad_gpu_message_restarts_gpu() {
        let (mut core, t0) = start(FATAL);
        let actions = core.on_event(
            Event::GpuBad {
                generation: 1,
                reason: BadMessageReason::GpuUndecodable,
            },
            t0,
        );
        assert_eq!(
            actions.first(),
            Some(&Action::KillGpu {
                generation: 1,
                reason: BadMessageReason::GpuUndecodable
            })
        );
    }

    #[test]
    fn a_gpu_crash_respawns_without_a_kill() {
        let (mut core, t0) = start(FATAL);
        assert_eq!(
            core.on_event(
                Event::GpuExited {
                    generation: 1,
                    code: None
                },
                t0
            ),
            restart(2, 2, vp(800, 600))
        );
    }

    #[test]
    fn old_generation_frame_ignored() {
        let (mut core, t0) = start(FATAL);
        let _ = core.on_event(
            Event::GpuExited {
                generation: 1,
                code: None,
            },
            t0,
        );
        // Everything from generation 1 is now noise: its frame (for the seq now in flight, so
        // only the generation rejects it), its exit, its bad messages.
        assert_eq!(core.on_event(frame(1, 2, 800, 600), t0), []);
        assert_eq!(
            core.on_event(
                Event::GpuExited {
                    generation: 1,
                    code: Some(1)
                },
                t0
            ),
            []
        );
        assert_eq!(
            core.on_event(
                Event::GpuBad {
                    generation: 1,
                    reason: BadMessageReason::GpuUndecodable
                },
                t0
            ),
            []
        );
        assert_eq!(
            core.on_event(
                Event::GpuMsg {
                    generation: 1,
                    msg: FromGpu::BadDisplayList {
                        seq: 1,
                        reason: BadMessageReason::DisplayListInvalid
                    }
                },
                t0
            ),
            []
        );
    }

    #[test]
    fn fourth_restart_in_60s_errors() {
        let (mut core, t0) = start(FATAL);
        for generation in 1..=3 {
            let actions = core.on_event(
                Event::GpuExited {
                    generation,
                    code: None,
                },
                at(t0, generation),
            );
            assert_eq!(
                actions.first(),
                Some(&Action::SpawnGpu {
                    generation: generation + 1
                })
            );
        }
        assert_eq!(
            core.on_event(
                Event::GpuExited {
                    generation: 4,
                    code: None
                },
                at(t0, 4)
            ),
            [
                Action::ShowError(ErrorKind::NoFrame),
                Action::Exit(EXIT_NO_FRAME)
            ]
        );
    }

    #[test]
    fn in_a_window_an_exhausted_gpu_budget_shows_the_error_only() {
        let (mut core, t0) = start(WINDOW);
        for generation in 1..=3 {
            let _ = core.on_event(
                Event::GpuExited {
                    generation,
                    code: None,
                },
                t0,
            );
        }
        assert_eq!(
            core.on_event(
                Event::GpuExited {
                    generation: 4,
                    code: None
                },
                t0
            ),
            [Action::ShowError(ErrorKind::NoFrame)]
        );
        // No gpu: a resize goes nowhere.
        assert_eq!(
            core.on_event(
                Event::Resized {
                    physical: (500, 400),
                    scale: 1.0
                },
                t0
            ),
            []
        );
    }

    #[test]
    fn one_seq_in_flight_resizes_coalesce() {
        let (mut core, t0) = start(FATAL);
        for physical in [(500, 700), (400, 300)] {
            assert_eq!(
                core.on_event(
                    Event::Resized {
                        physical,
                        scale: 1.0
                    },
                    t0
                ),
                []
            );
        }
        // The pending frame lands; only the latest size is asked for next.
        assert_eq!(
            core.on_event(frame(1, 1, 800, 600), t0),
            [
                presented(1, 800, 600, 1),
                Action::SendGpu(ToGpuCmd::SetSurface {
                    viewport: vp(400, 300),
                    seq: 2
                }),
                Action::SendRenderer(ToRendererCmd::Resize {
                    viewport: vp(400, 300),
                    seq: 2
                }),
            ]
        );
        assert_eq!(
            core.on_event(frame(1, 2, 400, 300), t0),
            [presented(2, 400, 300, 1)]
        );
    }

    #[test]
    fn a_resize_while_idle_is_sent_at_once_with_the_scale_factor() {
        let (mut core, t0) = start(FATAL);
        let _ = core.on_event(frame(1, 1, 800, 600), t0);
        assert_eq!(
            core.on_event(
                Event::Resized {
                    physical: (1000, 800),
                    scale: 2.0
                },
                t0
            ),
            [
                Action::SendGpu(ToGpuCmd::SetSurface {
                    viewport: vp(500, 400),
                    seq: 2
                }),
                Action::SendRenderer(ToRendererCmd::Resize {
                    viewport: vp(500, 400),
                    seq: 2
                }),
            ]
        );
        assert_eq!(
            core.on_event(frame(1, 2, 500, 400), t0),
            [presented(2, 500, 400, 2)]
        );
    }

    #[test]
    fn a_resize_to_the_same_size_or_to_zero_does_nothing() {
        let (mut core, t0) = start(FATAL);
        let _ = core.on_event(frame(1, 1, 800, 600), t0);
        for physical in [(800, 600), (0, 0)] {
            assert_eq!(
                core.on_event(
                    Event::Resized {
                        physical,
                        scale: 1.0
                    },
                    t0
                ),
                []
            );
        }
    }

    #[test]
    fn a_gpu_restart_carries_a_pending_resize() {
        let (mut core, t0) = start(FATAL);
        let _ = core.on_event(
            Event::Resized {
                physical: (400, 300),
                scale: 1.0,
            },
            t0,
        );
        assert_eq!(
            core.on_event(
                Event::GpuExited {
                    generation: 1,
                    code: None
                },
                t0
            ),
            [
                Action::SpawnGpu { generation: 2 },
                Action::SendGpu(ToGpuCmd::SetSurface {
                    viewport: vp(400, 300),
                    seq: 2
                }),
                Action::SendRenderer(ToRendererCmd::Resize {
                    viewport: vp(400, 300),
                    seq: 2
                }),
            ]
        );
    }

    #[test]
    fn subresources_are_loaded_once_per_document() {
        let (mut core, t0) = start(FATAL);
        let urls = vec!["file:///doc/a.css".to_owned()];
        assert_eq!(
            core.on_event(
                Event::RendererMsg(FromRenderer::NeedSubresources {
                    doc_id: 1,
                    urls: urls.clone()
                }),
                t0
            ),
            [Action::LoadSubresources {
                doc_id: 1,
                urls: urls.clone()
            }]
        );
        let replies = vec![LoadedSubresource {
            index: 0,
            result: Err(SubresourceRefusal::NotRegularFile),
        }];
        assert_eq!(
            core.on_event(
                Event::SubresourcesLoaded {
                    doc_id: 1,
                    replies: replies.clone()
                },
                t0
            ),
            [Action::SendRenderer(ToRendererCmd::Subresources {
                doc_id: 1,
                replies
            })]
        );
        // A second batch for the same document is a protocol violation.
        assert_eq!(
            core.on_event(
                Event::RendererMsg(FromRenderer::NeedSubresources { doc_id: 1, urls }),
                t0
            ),
            [
                Action::KillRenderer {
                    reason: BadMessageReason::RendererUnexpectedMessage
                },
                Action::ShowError(ErrorKind::RendererKilled),
                Action::Exit(EXIT_RENDERER_KILLED_BAD_MESSAGE),
            ]
        );
    }

    #[test]
    fn subresources_for_an_unknown_document_kill_the_renderer() {
        let (mut core, t0) = start(FATAL);
        let actions = core.on_event(
            Event::RendererMsg(FromRenderer::NeedSubresources {
                doc_id: 2,
                urls: vec![],
            }),
            t0,
        );
        assert_eq!(
            actions.first(),
            Some(&Action::KillRenderer {
                reason: BadMessageReason::RendererUnexpectedMessage
            })
        );
    }

    #[test]
    fn a_reply_for_another_document_is_dropped() {
        let (mut core, t0) = start(FATAL);
        assert_eq!(
            core.on_event(
                Event::SubresourcesLoaded {
                    doc_id: 7,
                    replies: vec![]
                },
                t0
            ),
            []
        );
    }

    #[test]
    fn document_ready_reports_its_warnings_once() {
        let (mut core, t0) = start(FATAL);
        let warnings = vec!["file:///x.css: outside the document directory".to_owned()];
        let ready = || {
            Event::RendererMsg(FromRenderer::DocumentReady {
                seq: 1,
                warnings: warnings.clone(),
            })
        };
        assert_eq!(
            core.on_event(ready(), t0),
            [Action::Warnings(warnings.clone())]
        );
        assert_eq!(core.on_event(ready(), t0), []);
    }

    #[test]
    fn a_message_for_a_seq_never_issued_kills_the_renderer() {
        for msg in [
            FromRenderer::DocumentReady {
                seq: 2,
                warnings: vec![],
            },
            FromRenderer::RenderFailed {
                seq: 2,
                kind: RenderFailureKind::Layout,
            },
        ] {
            let (mut core, t0) = start(FATAL);
            let actions = core.on_event(Event::RendererMsg(msg), t0);
            assert_eq!(
                actions.first(),
                Some(&Action::KillRenderer {
                    reason: BadMessageReason::RendererUnexpectedMessage
                })
            );
        }
    }

    #[test]
    fn a_failed_render_is_no_frame() {
        let (mut core, t0) = start(FATAL);
        assert_eq!(
            core.on_event(
                Event::RendererMsg(FromRenderer::RenderFailed {
                    seq: 1,
                    kind: RenderFailureKind::Layout
                }),
                t0
            ),
            [
                Action::ShowError(ErrorKind::RenderFailed),
                Action::Exit(EXIT_NO_FRAME)
            ]
        );
    }

    #[test]
    fn in_a_window_a_failed_render_frees_the_seq_for_the_next_resize() {
        let (mut core, t0) = start(WINDOW);
        let _ = core.on_event(
            Event::Resized {
                physical: (400, 300),
                scale: 1.0,
            },
            t0,
        );
        assert_eq!(
            core.on_event(
                Event::RendererMsg(FromRenderer::RenderFailed {
                    seq: 1,
                    kind: RenderFailureKind::Layout
                }),
                t0
            ),
            [
                Action::ShowError(ErrorKind::RenderFailed),
                Action::SendGpu(ToGpuCmd::SetSurface {
                    viewport: vp(400, 300),
                    seq: 2
                }),
                Action::SendRenderer(ToRendererCmd::Resize {
                    viewport: vp(400, 300),
                    seq: 2
                }),
            ]
        );
    }

    #[test]
    fn no_frame_within_the_first_frame_timeout_is_no_frame() {
        let (mut core, t0) = start(FATAL);
        assert_eq!(core.on_event(Event::Tick, at(t0, 29)), []);
        assert_eq!(
            core.on_event(Event::Tick, at(t0, 30)),
            [
                Action::ShowError(ErrorKind::NoFrame),
                Action::Exit(EXIT_NO_FRAME)
            ]
        );
        // In a window the browser keeps waiting; after a frame the watchdog is off.
        let (mut core, t0) = start(WINDOW);
        assert_eq!(core.on_event(Event::Tick, at(t0, 31)), []);
        let (mut core, t0) = start(FATAL);
        let _ = core.on_event(frame(1, 1, 800, 600), t0);
        assert_eq!(core.on_event(Event::Tick, at(t0, 60)), []);
    }

    #[test]
    fn a_repeated_frame_for_an_issued_seq_is_dropped_not_blamed_on_the_gpu() {
        // The renderer holds the display-list sender: it can send seq 1 twice, and the gpu
        // rasters both. The second frame is a benign duplicate, not a gpu fault.
        let (mut core, t0) = start(FATAL);
        let _ = core.on_event(frame(1, 1, 800, 600), t0);
        assert_eq!(core.on_event(frame(1, 1, 800, 600), t0), []);
        let _ = core.on_event(
            Event::Resized {
                physical: (400, 300),
                scale: 1.0,
            },
            t0,
        );
        // Seq 2 in flight: an old seq 1 frame is still dropped, and seq 2 still lands.
        assert_eq!(core.on_event(frame(1, 1, 800, 600), t0), []);
        assert_eq!(
            core.on_event(frame(1, 2, 400, 300), t0),
            [presented(2, 400, 300, 1)]
        );
    }

    #[test]
    fn a_frame_for_a_seq_never_issued_restarts_gpu() {
        let (mut core, t0) = start(FATAL);
        let _ = core.on_event(frame(1, 1, 800, 600), t0);
        let actions = core.on_event(frame(1, 5, 800, 600), t0);
        assert_eq!(
            actions.first(),
            Some(&Action::KillGpu {
                generation: 1,
                reason: BadMessageReason::FrameUnexpectedSeq
            })
        );
    }

    #[test]
    fn a_frame_after_the_renderer_died_is_not_presented() {
        let (mut core, t0) = start(WINDOW);
        let _ = core.on_event(
            Event::RendererBad(BadMessageReason::RendererUndecodable),
            t0,
        );
        assert_eq!(core.on_event(frame(1, 1, 800, 600), t0), []);
    }

    #[test]
    fn a_gpu_failure_after_the_renderer_died_is_not_respawned() {
        let (mut core, t0) = start(WINDOW);
        let _ = core.on_event(Event::RendererExited(Some(1)), t0);
        assert_eq!(
            core.on_event(
                Event::GpuExited {
                    generation: 1,
                    code: None
                },
                t0
            ),
            []
        );
    }

    #[test]
    fn render_failed_for_a_superseded_seq_is_ignored() {
        let (mut core, t0) = start(WINDOW);
        let _ = core.on_event(frame(1, 1, 800, 600), t0);
        let _ = core.on_event(
            Event::Resized {
                physical: (400, 300),
                scale: 1.0,
            },
            t0,
        );
        assert_eq!(
            core.on_event(
                Event::RendererMsg(FromRenderer::RenderFailed {
                    seq: 1,
                    kind: RenderFailureKind::Layout
                }),
                t0
            ),
            []
        );
    }

    #[test]
    fn warnings_reach_the_terminal_with_control_characters_escaped() {
        // OSC 52 (clipboard write) and a terminal-title sequence from a hostile renderer.
        let (mut core, t0) = start(FATAL);
        let actions = core.on_event(
            Event::RendererMsg(FromRenderer::DocumentReady {
                seq: 1,
                warnings: vec!["a\u{1b}]52;c;eA==\u{7}b\n".to_owned()],
            }),
            t0,
        );
        assert_eq!(
            actions,
            [Action::Warnings(vec![
                "a\\u{1b}]52;c;eA==\\u{7}b\\n".to_owned()
            ])]
        );
    }

    #[test]
    fn a_zero_initial_size_is_refused_and_a_huge_one_clamped() {
        assert!(BrowserCore::new(FATAL, (0, 600), 1.0, Instant::now()).is_err());
        let (_core, actions) =
            BrowserCore::new(FATAL, (100_000, 600), 1.0, Instant::now()).expect("core");
        assert_eq!(
            actions.first(),
            Some(&Action::SendGpu(ToGpuCmd::SetSurface {
                viewport: vp(16_384, 600),
                seq: 1
            }))
        );
    }

    #[test]
    fn the_initial_size_follows_the_scale_factor() {
        let (_core, actions) =
            BrowserCore::new(FATAL, (1600, 1200), 2.0, Instant::now()).expect("core");
        assert_eq!(
            actions.first(),
            Some(&Action::SendGpu(ToGpuCmd::SetSurface {
                viewport: vp(800, 600),
                seq: 1
            }))
        );
    }
}
