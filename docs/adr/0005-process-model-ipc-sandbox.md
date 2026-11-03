# ADR-0005: Multi-process, typed IPC, sandbox from the first milestone

**Status:** Accepted
**Date:** 2026-09-07
**Deciders:** project owner

## Context

Reference: "features first, security later" = late redesign of ownership and APIs. The renderer processes hostile bytes; V8 is C++. The only protection of the OS when the renderer is compromised is the sandbox; the only protection of other sites is process isolation; the only protection of the browser process is IPC validation.

## Decision

1. **Processes from M1:** browser (privileged) + renderer per site + network + gpu + utility (decoders). One binary, `--type=`. `--single-process` only in dev builds.
2. **Gate S0:** an untrusted URL is not loaded without `Sandbox<Applied>` (type-state) in the renderer. Until the sandbox is ready on a platform — only `file://` test fixtures and localhost.
3. **Sandbox:** macOS seatbelt SBPL profiles; Linux user/pid/net namespaces + seccomp-bpf allowlist + `no_new_privs`; Windows restricted token + job + AppContainer + Win32k lockdown. Network/GPU — weaker, but separate profiles.
4. **IPC (`cl-ipc`):** transport `ipc-channel` + shm; messages — serde enums via `postcard`; `validate()` on every type on the receiver side; version in the handshake; capability handles from the broker; **async only** (the only sync path — modal dialogs renderer→browser with a timeout).
5. **Site assignment** in the browser process from M2 (site-per-process), OOPIF — M4.
6. **Crash:** minidump in the child; the browser restarts it; "tab crashed" UI; dumps without page contents.

## Options Considered

- **Single-process until M3, then split** — rejected: reworking ownership (DOM ↔ network ↔ storage) costs more than doing it right away; the reference and the Chromium/Firefox/WebKit experience confirm this.
- **Thread isolation instead of processes** — does not protect against V8/FFI memory bugs and Spectre.
- **Mojo-like IDL with codegen** — later, if enum+serde becomes a bottleneck; overkill for now.

## Consequences

- Easier: the security model is clear from the first commit; tests are multiprocess from the start and catch IPC bugs.
- Harder: M1 slower by ~a month (spawn/IPC/shm infrastructure); the Windows sandbox is a separate complexity.
- Revisit when: the cost of processes hits the memory budget → renderer reuse policy (ADR-0012), not abandoning isolation.

## Action Items

1. [ ] `cl-process` spawn + handshake on 3 OSes (M0/M1).
2. [ ] Renderer sandbox profiles on macOS and Linux (M1), Windows (M2).
3. [ ] `fuzz/ipc_*` targets in the PR that introduces the first message.
4. [ ] `tests/security/sandbox_*`: from the renderer `open("/etc/passwd")` → EPERM on all OSes.
