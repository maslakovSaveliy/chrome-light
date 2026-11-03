//! Exit codes of the `chromelight` process. Tests and CI assert on these numbers, so they are
//! stable. M1b adds the renderer-crash / no-frame / bad-message codes when the browser can
//! produce them.

/// Normal exit.
pub const EXIT_OK: i32 = 0;

/// `EX_CONFIG` from sysexits.h: the environment cannot run this role safely. A child exits with
/// it when its sandbox cannot be applied; the browser exits with it when a child it spawned did.
pub const EXIT_SANDBOX_UNAVAILABLE: i32 = 78;
