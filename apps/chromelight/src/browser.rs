//! Browser process main (M0: spawn one renderer, ping, optionally idle, shut down).

use std::time::Duration;

use cl_ipc::message::{ToBrowser, ToChild};
use cl_platform::ProcessType;
use cl_process::{ProcessError, SpawnOptions, spawn};
use tracing::{error, info, warn};

use crate::{
    args::Args,
    exit::{EXIT_OK, EXIT_SANDBOX_UNAVAILABLE},
};

const BOOTSTRAP_TIMEOUT: Duration = Duration::from_secs(15);
const PING_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a child gets to exit after `Shutdown` before the browser kills it.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

/// Runs the browser process and returns its exit code. `Err` is an unexpected failure (exit 1).
pub fn run(args: &Args) -> anyhow::Result<i32> {
    let exe = std::env::current_exe()?;
    let opts = SpawnOptions {
        extra_args: renderer_args(args),
        timeout: BOOTSTRAP_TIMEOUT,
        trace_path: args.trace_out.clone(),
    };

    let mut renderer = match spawn(&exe, ProcessType::Renderer, &opts) {
        Ok(renderer) => renderer,
        Err(ProcessError::ExitedDuringBootstrap {
            code: Some(EXIT_SANDBOX_UNAVAILABLE),
        }) => {
            error!(
                "renderer exited during bootstrap with {EXIT_SANDBOX_UNAVAILABLE}: \
                 sandbox unavailable for renderer"
            );
            return Ok(EXIT_SANDBOX_UNAVAILABLE);
        }
        Err(e) => return Err(e.into()),
    };
    info!(pid = renderer.pid(), "renderer handshake complete");

    renderer.endpoint.send(&ToChild::Ping(1))?;
    match renderer.endpoint.recv_timeout(PING_TIMEOUT)? {
        ToBrowser::Pong(1) => {}
        other => anyhow::bail!("expected Pong(1), got {other:?}"),
    }
    println!("handshake ok renderer pid={}", renderer.pid());

    #[cfg(debug_assertions)]
    if args.browser_fail_after_handshake {
        anyhow::bail!("simulated browser failure after handshake (dev flag)");
    }

    if let Some(secs) = args.idle_seconds {
        info!(secs, "idling for benchmark");
        idle(Duration::from_secs(secs));
    }

    renderer.endpoint.send(&ToChild::Shutdown)?;
    let outcome = renderer.wait_timeout(SHUTDOWN_TIMEOUT)?;
    // A child that exited cleanly in the race between the last poll and the kill is not an error.
    if outcome.killed && !outcome.status.success() {
        warn!(
            pid = renderer.pid(),
            "renderer did not exit after Shutdown; killed after {SHUTDOWN_TIMEOUT:?}"
        );
        anyhow::bail!("renderer did not exit after Shutdown; killed");
    }
    anyhow::ensure!(
        outcome.status.success(),
        "renderer exited with {}",
        outcome.status
    );
    // M0-ONLY: no UI yet; a plain launch behaves like --exit-after-handshake.
    Ok(EXIT_OK)
}

/// Arguments the browser passes on to the renderer it spawns.
fn renderer_args(args: &Args) -> Vec<String> {
    let mut extra = Vec::new();
    if args.no_sandbox {
        extra.push("--no-sandbox".to_owned());
    }
    #[cfg(debug_assertions)]
    if args.renderer_ignore_shutdown {
        extra.push("--ignore-shutdown".to_owned());
    }
    extra
}

#[expect(
    clippy::disallowed_methods,
    reason = "benchmark idle in the binary, not engine code"
)]
fn idle(d: Duration) {
    std::thread::sleep(d);
}
