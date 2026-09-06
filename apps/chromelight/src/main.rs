//! `ChromeLight` entry point. One binary; `--type` selects the role (ADR-0005).
#![forbid(unsafe_code)]
// reason: the CLI success line (`handshake ok renderer pid=...`) is consumed by tests and
// tooling; it lives only in browser.rs, so a crate-level #[expect] is not reliably fulfilled.
#![allow(clippy::print_stdout)]

mod args;
mod browser;
mod child;
mod telemetry;

use cl_platform::ProcessType;
use clap::Parser;

#[expect(clippy::disallowed_methods, reason = "binary entry point")]
fn main() {
    let args = args::Args::parse();
    let guard = match telemetry::init(args.trace_out.as_deref()) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("chromelight: failed to init telemetry: {e}");
            std::process::exit(1);
        }
    };

    let code = match args.process_type {
        ProcessType::Browser => match browser::run(&args) {
            Ok(()) => 0,
            Err(e) => {
                tracing::error!(%e, "browser process failed");
                1
            }
        },
        // `ProcessType` is `#[non_exhaustive]`; every non-browser role behaves identically in M0.
        _ => child::run(&args),
    };
    drop(guard);
    std::process::exit(code);
}
