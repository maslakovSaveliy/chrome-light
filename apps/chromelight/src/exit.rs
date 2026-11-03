//! Exit codes of the `chromelight` process. Tests and CI assert on these numbers, so they are
//! stable (spec §5). The browser exits with 3/4/5 only when failures are fatal (headless, or
//! `--exit-after-first-present`); in a window it shows the error and stays open.

/// Normal exit.
pub const EXIT_OK: i32 = 0;

/// The renderer crashed (exited without being killed by the browser).
#[allow(dead_code, reason = "driven by the IO layer of M1b Task 10")]
pub const EXIT_RENDERER_CRASHED: i32 = 3;

/// No frame: the gpu restart budget is exhausted, the render failed, or no frame arrived within
/// the first-frame timeout.
#[allow(dead_code, reason = "driven by the IO layer of M1b Task 10")]
pub const EXIT_NO_FRAME: i32 = 4;

/// The browser killed the renderer for a bad message (its own, or a display list the gpu
/// rejected).
#[allow(dead_code, reason = "driven by the IO layer of M1b Task 10")]
pub const EXIT_RENDERER_KILLED_BAD_MESSAGE: i32 = 5;

/// `EX_CONFIG` from sysexits.h: the environment cannot run this role safely. A child exits with
/// it when its sandbox cannot be applied; the browser exits with it when a child it spawned did.
pub const EXIT_SANDBOX_UNAVAILABLE: i32 = 78;
