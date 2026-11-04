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

    /// Browser: render the document without a window, write the frame to `--png`, exit with
    /// the browser's code (0 ok, 3/4/5 a failure, 78 no sandbox).
    #[arg(
        long = "headless",
        requires_all = ["png", "document"],
        conflicts_with_all = ["exit_after_handshake", "idle_seconds"]
    )]
    pub headless: bool,

    /// Browser, headless: where to write the frame.
    #[arg(long = "png", requires = "headless")]
    pub png: Option<PathBuf>,

    /// Browser: the window's size in physical pixels, `WIDTHxHEIGHT`.
    #[arg(long = "viewport", value_parser = parse_size, default_value = "800x600")]
    pub viewport: (u32, u32),

    /// Browser, headless: after the first frame, resize to `WIDTHxHEIGHT` and write the second
    /// frame instead.
    #[arg(long = "resize-to", value_parser = parse_size, requires = "headless")]
    pub resize_to: Option<(u32, u32)>,

    /// Browser: the document to show, a path or a `file:` URL.
    #[arg(value_name = "PATH|FILE-URL")]
    pub document: Option<PathBuf>,

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

/// `WIDTHxHEIGHT`, both at least 1.
fn parse_size(s: &str) -> Result<(u32, u32), String> {
    let (w, h) = s
        .split_once('x')
        .ok_or_else(|| format!("expected WIDTHxHEIGHT, got {s:?}"))?;
    let parse = |v: &str| match v.parse::<u32>() {
        Ok(0) | Err(_) => Err(format!("expected a positive integer, got {v:?}")),
        Ok(n) => Ok(n),
    };
    Ok((parse(w)?, parse(h)?))
}

fn parse_process_type(s: &str) -> Result<ProcessType, String> {
    s.parse::<ProcessType>().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use clap::{CommandFactory, Parser};

    use super::{Args, parse_size};

    #[test]
    fn the_cli_definition_is_consistent() {
        Args::command().debug_assert();
    }

    #[test]
    fn headless_needs_a_png_and_a_document_and_excludes_the_m0_modes() {
        let ok = ["chromelight", "--headless", "--png", "o.png", "page.html"];
        assert!(Args::try_parse_from(ok).is_ok());
        for bad in [
            &["chromelight", "--headless", "page.html"][..],
            &["chromelight", "--headless", "--png", "o.png"],
            &["chromelight", "--png", "o.png", "page.html"],
            &["chromelight", "--resize-to", "5x5", "page.html"],
            &[
                "chromelight",
                "--headless",
                "--png",
                "o.png",
                "--exit-after-handshake",
                "p.html",
            ],
            &[
                "chromelight",
                "--headless",
                "--png",
                "o.png",
                "--idle-seconds",
                "1",
                "p.html",
            ],
        ] {
            assert!(Args::try_parse_from(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn sizes_parse_and_reject_zero_or_garbage() {
        assert_eq!(parse_size("800x600"), Ok((800, 600)));
        for bad in ["800", "0x600", "800x0", "x", "-1x5", "800x600x1", ""] {
            assert!(parse_size(bad).is_err(), "{bad:?}");
        }
    }
}
