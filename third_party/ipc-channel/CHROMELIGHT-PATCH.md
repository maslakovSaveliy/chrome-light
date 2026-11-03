# ChromeLight patch to ipc-channel 0.23.1

This directory is `ipc-channel` 0.23.1 as published on crates.io (upstream commit
`7448f0ef5cf2754043518f40eacdc9c3eb00c728`, "Publish 0.23.1. (#458)"), used through
`[patch.crates-io]` in the root `Cargo.toml` and in `tools/fuzz/Cargo.toml`. Licenses:
`LICENSE-APACHE`, `LICENSE-MIT` (unchanged). Removed from the copy: `.github/`, `.gitignore`,
`Cargo.lock`, `.cargo_vcs_info.json`, `.cargo-ok`.

## The one change

`src/platform/windows/mod.rs`, `OutOfBandMessage::recover_handles`: return early when the
message's `target_process_id` is the current process.

Why: on send, `OsIpcSender::send` duplicates every shared-memory handle (`dup_handle_to_process`)
and moves every channel handle (`move_handle_to_process`) into the pipe server's process — the
receiver — and records that pid as `target_process_id`. On receive, `recover_handles` opened
that pid (its own) with `PROCESS_DUP_HANDLE` and called `DuplicateHandle` from itself to itself
without `DUPLICATE_CLOSE_SOURCE`, overwriting the handle value: the copy the sender made was
never closed. Every received shared-memory region, channel end and fragmented-message
(big-data) receiver leaked one handle. Upstream meant to guard this: the PR that added
`recover_handles` (servo/ipc-channel#388, merged 2025-04-10) says it is invoked "if the target
process ID (PID) and the current PID differ", but the merged code calls it unconditionally.
Found by
`crates/ipc/tests/xproc.rs::repeated_shm_sends_should_not_leak` on windows-2022 CI (browser
handles 137 → 637 over 500 frames, 2026-11-03).

Not changed: the case where the target is another process (the receiving end moved before the
message was read) still duplicates without closing the source in that process. Possibly affected
and not covered by a test: a channel whose receiving end was created in another process and
moved here (our upward bootstrap byte channel is created in the child) — if Windows reports the
creating process as the pipe server, sends on it target the child, the early return does not
apply there, and a fragmented (> ~64 KiB) message or an attached region would leak in the child.

Consequence (security, accepted until the Gate S0 transport work): the out-of-band block is
decoded from the wire, so a hostile sender chooses both `target_process_id` and the raw handle
values. Upstream, a forged message naming the receiver's own pid made it duplicate an arbitrary
value of its own handle table into a harmless alias. With the early return the receiver adopts
the value directly and closes it when the message is dropped — it closes one of its own handles
(a later close by the real owner then trips `WinHandle::drop`'s `assert!`, or a reused value
closes an unrelated object). Not exploitable while Windows children are unsandboxed and Gate S0
keeps untrusted content out; recorded in `docs/SECURITY.md` "Known gaps". Trusting handle values
from the wire is inherent in `ipc-channel`'s Windows design, which the Gate S0 transport task
replaces or fixes.

## Removing this copy

Drop the directory and both `[patch.crates-io]` entries once an upstream release contains an
equivalent fix (upstream report: https://github.com/servo/ipc-channel/issues/461; our fix: https://github.com/servo/ipc-channel/pull/462, which puts the same check at the only call site, `Message::oob_data`, so `recover_handles`' own unit tests keep calling it with the current pid). Until then any
upgrade of `ipc-channel` must re-apply the change to the new version.
