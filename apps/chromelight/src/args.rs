//! Command line. Children get `--type` and `--ipc-bootstrap` from the browser (see
//! `cl_process::spawn::child_args`); everything else is for humans and tests.

use std::path::PathBuf;

use cl_platform::ProcessType;
use clap::Parser;

/// `ChromeLight` process entry arguments.
#[derive(Debug, Parser)]
#[command(name = "chromelight", version, about)]
#[cfg_attr(
    debug_assertions,
    expect(
        clippy::struct_excessive_bools,
        reason = "CLI switches are booleans by nature; the debug-only test flags push the count \
                  over the lint's limit in debug builds only"
    )
)]
pub struct Args {
    /// Process role. Omit for the browser process.
    #[arg(long = "type", value_parser = parse_process_type, default_value = "browser")]
    pub process_type: ProcessType,

    /// Bootstrap server name handed to a child by the browser.
    #[arg(long = "ipc-bootstrap")]
    pub ipc_bootstrap: Option<String>,

    /// Debug builds only: run children without a sandbox. Never loads untrusted content.
    #[arg(long = "no-sandbox")]
    pub no_sandbox: bool,

    /// Browser: spawn one renderer, ping it, shut it down, exit 0. Used by tests and CI.
    #[arg(long = "exit-after-handshake")]
    pub exit_after_handshake: bool,

    /// Browser: stay alive this many seconds after handshake (benchmarks), then shut down.
    #[arg(long = "idle-seconds")]
    pub idle_seconds: Option<u64>,

    /// Write a Chrome-trace-format JSON of all tracing spans to this path.
    #[arg(long = "trace-out")]
    pub trace_out: Option<PathBuf>,

    /// Debug builds only: return an error right after the handshake without shutting the
    /// renderer down, to exercise `ChildProcess`'s kill-on-drop safety net.
    #[cfg(debug_assertions)]
    #[arg(long = "browser-fail-after-handshake", hide = true)]
    pub browser_fail_after_handshake: bool,

    /// Debug builds only: start the renderer with `--ignore-shutdown`, to exercise the browser's
    /// bounded wait on a child that does not exit.
    #[cfg(debug_assertions)]
    #[arg(long = "renderer-ignore-shutdown", hide = true)]
    pub renderer_ignore_shutdown: bool,

    /// Debug builds only, child roles: keep serving after `Shutdown` instead of exiting.
    #[cfg(debug_assertions)]
    #[arg(long = "ignore-shutdown", hide = true)]
    pub ignore_shutdown: bool,
}

fn parse_process_type(s: &str) -> Result<ProcessType, String> {
    s.parse::<ProcessType>().map_err(|e| e.to_string())
}
