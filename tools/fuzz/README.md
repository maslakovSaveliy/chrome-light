# Fuzz targets

Nightly-only workspace (excluded from root). Run:

    rustup toolchain install nightly
    cd tools/fuzz && cargo +nightly fuzz run ipc_decode -- -max_total_time=60

Targets: `ipc_decode` (cl-ipc codec, both directions). Every new parser/decoder adds a target in the same PR (CLAUDE.md §3.1).
