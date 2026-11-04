//! Headless browsing (M1b Task 10): the three processes render one document, the browser writes
//! the frame to a PNG and exits with [`BrowserCore`]'s code. Failures are fatal
//! ([`CoreOptions::fatal_errors`]): a crash, a kill or no frame exits 3, 5 or 4.
//!
//! With `--resize-to` the browser resizes the surface after the first frame (as a window resize
//! would) and writes the first frame of the new size; a resize that keeps the surface writes
//! the first frame. Which frame to write is decided by its size ([`on_present`]), never by
//! whether the core answered the resize: a resize can coalesce behind a frame already asked
//! for (a gpu restart), and a finished core answers nothing. An `Exit` already queued behind a
//! frame wins over writing it. The resized frame gets its own [`FIRST_FRAME_TIMEOUT`]: a
//! renderer or gpu that stays alive but never sends it ends in exit 4, not a browser that
//! waits forever.

use std::collections::VecDeque;
use std::path::Path;
use std::time::Instant;

use cl_gfx::Pixmap;
use cl_process::SpawnOptions;
use tracing::{error, info};

use super::children::{Children, DocumentSource, StartError};
use cl_ipc::proto::Viewport;

use super::core::{
    Action, BrowserCore, CoreOptions, Event, FIRST_FRAME_TIMEOUT, PresentFrame, surface_viewport,
};
use super::{BOOTSTRAP_TIMEOUT, SHUTDOWN_TIMEOUT, gpu_args, renderer_args};
use crate::args::Args;
use crate::exit::{EXIT_NO_FRAME, EXIT_OK, EXIT_SANDBOX_UNAVAILABLE};

/// What headless needs from the command line.
#[derive(Debug)]
pub struct HeadlessOptions<'a> {
    /// The document, a path or a `file:` URL.
    pub document: &'a Path,
    /// Where the frame goes.
    pub png: &'a Path,
    /// The surface's first size, in pixels (scale 1).
    pub viewport: (u32, u32),
    /// The size to resize to after the first frame; that frame is written instead.
    pub resize_to: Option<(u32, u32)>,
}

/// Renders the document and returns the browser's exit code.
///
/// # Errors
/// An unreadable document, a failed spawn other than a sandbox refusal, or a PNG that cannot
/// be written.
pub fn run(args: &Args, opts: &HeadlessOptions<'_>) -> anyhow::Result<i32> {
    let doc = DocumentSource::open(opts.document)?;
    let renderer = SpawnOptions {
        extra_args: renderer_args(args),
        timeout: BOOTSTRAP_TIMEOUT,
        trace_path: args.trace_out.clone(),
    };
    let gpu = SpawnOptions {
        extra_args: gpu_args(args),
        ..renderer.clone()
    };
    let mut children = match Children::start(std::env::current_exe()?, &renderer, gpu, doc) {
        Ok(children) => children,
        Err(StartError::SandboxUnavailable) => return Ok(EXIT_SANDBOX_UNAVAILABLE),
        Err(StartError::Other(e)) => return Err(e),
    };
    let options = CoreOptions { fatal_errors: true };
    let (mut core, actions) = BrowserCore::new(options, opts.viewport, 1.0, Instant::now())?;
    let mut pending: VecDeque<Action> = actions.into();
    let mut goal = Goal {
        resize_to: opts
            .resize_to
            .map(|p| surface_viewport(p, 1.0))
            .transpose()?,
        resize_sent: false,
    };
    // Set once the resize was sent; the core's own timeout covers the first frame.
    let mut resized_deadline: Option<Instant> = None;

    loop {
        while let Some(action) = pending.pop_front() {
            match action {
                Action::Present(frame) => {
                    let exit_queued = pending.iter().any(|a| matches!(a, Action::Exit(_)));
                    match on_present((frame.width, frame.height), &mut goal, exit_queued) {
                        OnPresent::Write => {
                            write_png(&frame, opts.png)?;
                            info!(seq = frame.seq, png = %opts.png.display(), "frame written");
                            children.shutdown(SHUTDOWN_TIMEOUT);
                            return Ok(EXIT_OK);
                        }
                        OnPresent::Resize => {
                            let Some(physical) = opts.resize_to else {
                                continue;
                            };
                            info!(seq = frame.seq, ?physical, "first frame; resizing");
                            let now = Instant::now();
                            resized_deadline = now.checked_add(FIRST_FRAME_TIMEOUT);
                            let resized = Event::Resized {
                                physical,
                                scale: 1.0,
                            };
                            pending.extend(core.on_event(resized, now));
                        }
                        OnPresent::Skip => {}
                    }
                }
                Action::Warnings(warnings) => {
                    for warning in warnings {
                        eprintln!("chromelight: warning: {warning}");
                    }
                }
                Action::ShowError(kind) => error!(?kind, "no page to show"),
                Action::Exit(exit_code) => {
                    children.kill_all();
                    return Ok(exit_code);
                }
                io => children.execute(io),
            }
        }
        for event in children.next_events() {
            pending.extend(core.on_event(event, Instant::now()));
        }
        if pending.is_empty() && resized_deadline.is_some_and(|d| Instant::now() >= d) {
            error!("no frame for the resized surface within {FIRST_FRAME_TIMEOUT:?}");
            children.kill_all();
            return Ok(EXIT_NO_FRAME);
        }
    }
}

/// What `--resize-to` still waits for.
#[derive(Debug, Clone, Copy)]
struct Goal {
    /// The surface to write a frame of, if not the first frame's.
    resize_to: Option<Viewport>,
    /// The resize was sent to the core.
    resize_sent: bool,
}

/// What to do with a presented frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OnPresent {
    /// It is the frame asked for: write it and exit 0.
    Write,
    /// Resize the surface now, and wait for a frame of the new size.
    Resize,
    /// Not the frame asked for (or an exit is already queued): keep going.
    Skip,
}

/// Decides by the frame's size (`(width, height)`): with no resize asked for, the first frame
/// is written; otherwise only a frame of the resized surface, the resize sent once on the
/// first frame of another size. An `Exit` already queued (`exit_queued`) wins.
fn on_present(frame: (u32, u32), goal: &mut Goal, exit_queued: bool) -> OnPresent {
    if exit_queued {
        return OnPresent::Skip;
    }
    let Some(target) = goal.resize_to else {
        return OnPresent::Write;
    };
    if frame == (target.width(), target.height()) {
        return OnPresent::Write;
    }
    if goal.resize_sent {
        return OnPresent::Skip;
    }
    goal.resize_sent = true;
    OnPresent::Resize
}

/// Writes a checked frame (premultiplied RGBA8, `width × height × 4`) as a PNG.
fn write_png(frame: &PresentFrame, path: &Path) -> anyhow::Result<()> {
    let mut pixmap = Pixmap::new(frame.width, frame.height)
        .ok_or_else(|| anyhow::anyhow!("no pixmap of {}×{}", frame.width, frame.height))?;
    anyhow::ensure!(
        pixmap.data().len() == frame.pixels.len(),
        "frame of {} bytes for a {}×{} pixmap",
        frame.pixels.len(),
        frame.width,
        frame.height
    );
    pixmap.data_mut().copy_from_slice(&frame.pixels);
    pixmap.save_png(path)?;
    Ok(())
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "a failed setup step should abort the test, loudly"
)]
mod tests {
    use cl_ipc::proto::Viewport;

    use super::{Goal, OnPresent, on_present};

    fn goal(resize_to: Option<(u32, u32)>) -> Goal {
        Goal {
            resize_to: resize_to.map(|(w, h)| Viewport::new(w, h).expect("viewport")),
            resize_sent: false,
        }
    }

    #[test]
    fn without_a_resize_the_first_frame_is_written() {
        assert_eq!(
            on_present((800, 600), &mut goal(None), false),
            OnPresent::Write
        );
    }

    #[test]
    fn a_resize_waits_for_a_frame_of_the_new_size() {
        let mut g = goal(Some((500, 700)));
        assert_eq!(on_present((800, 600), &mut g, false), OnPresent::Resize);
        // The resize coalesced behind a frame already asked for (a gpu restart): its frame is
        // of the old size and is not the answer (Task 10 re-review).
        assert_eq!(on_present((800, 600), &mut g, false), OnPresent::Skip);
        assert_eq!(on_present((500, 700), &mut g, false), OnPresent::Write);
    }

    #[test]
    fn a_resize_to_the_same_surface_writes_the_first_frame() {
        assert_eq!(
            on_present((640, 480), &mut goal(Some((640, 480))), false),
            OnPresent::Write
        );
    }

    #[test]
    fn a_queued_exit_wins_over_any_frame() {
        // A frame and the renderer's crash in one batch: exit 3, not a PNG and 0.
        assert_eq!(
            on_present((800, 600), &mut goal(None), true),
            OnPresent::Skip
        );
        assert_eq!(
            on_present((500, 700), &mut goal(Some((500, 700))), true),
            OnPresent::Skip
        );
    }
}
