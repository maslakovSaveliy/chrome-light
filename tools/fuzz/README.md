# Fuzz targets

Nightly-only workspace (excluded from root). Run:

    rustup toolchain install nightly
    cargo +nightly fuzz run --fuzz-dir tools/fuzz ipc_decode -- -max_total_time=60   # from the repo root
    cargo +nightly fuzz run --fuzz-dir tools/fuzz url_parse -- -max_total_time=60

Targets: `ipc_decode` (cl-ipc codec, both directions), `url_parse` (cl-net `Url::parse` and
`Url::parse_with_base`). Every new parser/decoder adds a target in the same PR (CLAUDE.md §3.1).
