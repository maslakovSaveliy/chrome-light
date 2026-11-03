# ChromeLight patch to ipc-channel 0.23.1

This directory is `ipc-channel` 0.23.1 as published on crates.io (upstream commit
`7448f0ef5cf2754043518f40eacdc9c3eb00c728`, "Publish 0.23.1. (#458)"), used through
`[patch.crates-io]` in the root `Cargo.toml` and in `tools/fuzz/Cargo.toml`. Licenses:
`LICENSE-APACHE`, `LICENSE-MIT` (unchanged). Removed from the copy: `.github/`, `.gitignore`,
`Cargo.lock`, `.cargo_vcs_info.json`.

## The one change

`src/platform/windows/mod.rs`, `OutOfBandMessage::recover_handles`: return early when the
message's `target_process_id` is the current process.

Why: on send, `OsIpcSender::send` duplicates every shared-memory handle (`dup_handle_to_process`)
and moves every channel handle (`move_handle_to_process`) into the pipe server's process — the
receiver — and records that pid as `target_process_id`. On receive, `recover_handles` opened
that pid (its own) with `PROCESS_DUP_HANDLE` and called `DuplicateHandle` from itself to itself
without `DUPLICATE_CLOSE_SOURCE`, overwriting the handle value: the copy the sender made was
never closed. Every received shared-memory region and channel end leaked one handle. Found by
`crates/ipc/tests/xproc.rs::repeated_shm_sends_should_not_leak` on windows-2022 CI (browser
handles 137 → 637 over 500 frames, 2026-11-03).

Not changed: the case where the target is another process (the receiving end moved before the
message was read) still duplicates without closing the source in that process.

## Removing this copy

Drop the directory and both `[patch.crates-io]` entries once an upstream release contains an
equivalent fix (upstream report: see `docs/DEPENDENCIES.md`, `ipc-channel` row). Until then any
upgrade of `ipc-channel` must re-apply the change to the new version.
