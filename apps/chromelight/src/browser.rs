//! Browser process main (M0: spawn one renderer, ping, optionally idle, shut down).

use std::time::Duration;

use cl_ipc::message::{ToBrowser, ToChild};
use cl_platform::ProcessType;
use cl_process::spawn;
use tracing::info;

use crate::args::Args;

const BOOTSTRAP_TIMEOUT: Duration = Duration::from_secs(15);

pub fn run(args: &Args) -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let mut extra = Vec::new();
    if args.no_sandbox {
        extra.push("--no-sandbox".to_owned());
    }
    if let Some(p) = &args.trace_out {
        // Child traces go next to the browser's trace, suffixed by role.
        let mut child_path = p.clone();
        child_path.set_extension("renderer.json");
        extra.push(format!("--trace-out={}", child_path.display()));
    }

    let mut renderer = spawn(&exe, ProcessType::Renderer, &extra, BOOTSTRAP_TIMEOUT)?;
    info!(pid = renderer.pid(), "renderer handshake complete");

    renderer.endpoint.send(&ToChild::Ping(1))?;
    match renderer.endpoint.recv()? {
        ToBrowser::Pong(1) => {}
        other => anyhow::bail!("expected Pong(1), got {other:?}"),
    }
    println!("handshake ok renderer pid={}", renderer.pid());

    if let Some(secs) = args.idle_seconds {
        info!(secs, "idling for benchmark");
        idle(Duration::from_secs(secs));
    }

    renderer.endpoint.send(&ToChild::Shutdown)?;
    let status = renderer.wait()?;
    anyhow::ensure!(status.success(), "renderer exited with {status}");
    if args.exit_after_handshake || args.idle_seconds.is_some() {
        return Ok(());
    }
    // M0-ONLY: no UI yet; a plain launch behaves like --exit-after-handshake.
    Ok(())
}

#[expect(
    clippy::disallowed_methods,
    reason = "benchmark idle in the binary, not engine code"
)]
fn idle(d: Duration) {
    std::thread::sleep(d);
}
