//! tracing subscriber: human logs to stderr (`RUST_LOG` filter) plus optional Chrome trace file.

use std::path::Path;

use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

/// Keep alive until process exit so the trace file is flushed.
pub struct TraceGuard {
    _chrome: Option<tracing_chrome::FlushGuard>,
}

/// Install the global subscriber. Call once, first thing in `main`.
pub fn init(trace_out: Option<&Path>) -> anyhow::Result<TraceGuard> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_target(true);

    let (chrome_layer, guard) = match trace_out {
        Some(path) => {
            let (layer, guard) = tracing_chrome::ChromeLayerBuilder::new()
                .file(path)
                .include_args(true)
                .build();
            (Some(layer), Some(guard))
        }
        None => (None, None),
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .with(chrome_layer)
        .try_init()?;
    Ok(TraceGuard { _chrome: guard })
}
