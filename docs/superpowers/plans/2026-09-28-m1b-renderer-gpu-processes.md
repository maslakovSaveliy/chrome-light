# M1b Renderer + GPU Processes Implementation Plan

> **Execution:** the main agent implements every task itself, end to end (CLAUDE.md §3.5 — no implementer subagents); fresh read-only reviewers only. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> Approved by the owner 2026-09-28 (plan mode); execution rules re-approved 2026-11-03 (see "Execution" below). Executed from this file.

## Context

M1a (tag `m1a`) delivered a deterministic single-process static pipeline: `cl-html` → `cl-dom` → `cl-style` (stylo) → `cl-layout` → `cl-paint` (display list + `validate`) → `cl-gfx` (CPU raster) → `cl-testshell`, 22 reftest pairs, 6 fuzz targets. M1b splits it across processes: the **renderer** turns document bytes into a display list, the **gpu** process (in M1b a CPU raster service) validates and rasterises it, and the privileged **browser** presents the frame in a window. Owner decisions 2026-09-28: the browser presents via **softbuffer** (CPU blit); the gpu rasterises on the **CPU** (`cl-gfx` as is); M1b also covers resize → relayout, gpu crash → restart, renderer crash → error fill, per-process RSS. **Bad-message policy:** a malformed/hostile message from the renderer, or an invalid display list at the gpu (reported to the browser), makes the browser **kill the renderer**; a bad frame from the gpu makes the browser kill and restart the gpu; benign races (stale `seq`, dead generation, channel closing) are dropped silently; no strike counter.

**Goal:** `chromelight --no-sandbox file:///page.html` shows the page in a window, rendered by renderer + gpu processes over validated shared memory, pixel-identical to `cl-testshell render`.

**Architecture:** Downward traffic (browser → child, trusted sender) travels on one typed `ipc-channel` channel per child; upward traffic (child → browser, untrusted) is `postcard` over byte channels with `Validate` on receipt, as in M0. Bulk data is `IpcSharedMemory` that every receiver copies before use. The process that receives shm on a channel creates that channel; the only upward transfer of channel ends is the M0 bootstrap, before any untrusted content is loaded. Processes are pure state machines (`on(event) -> actions`) behind thin IO shells so that logic is unit-tested without spawning.

**Tech Stack:** Rust 1.95 stable, `ipc-channel` 0.23.1 (minimum; 0.23.0 has GHSA-fj85-w6rj-rxrc), `postcard` 1, `winit` 0.30.13, `softbuffer` 0.4.8, `tiny-skia` 0.12 (existing), `cargo-nextest` 0.9.

**Spec:** `docs/superpowers/specs/2026-09-28-m1b-renderer-gpu-processes-design.md`; decisions in `docs/adr/0016-m1b-raster-service-and-frame-transport.md` (Accepted in Task 1), ADR-0005, ADR-0006, ADR-0008, ADR-0012. Research: `.superpowers/m1b-drafts/research-bad-message-policy-2026-09-28.md`, `.superpowers/m1b-drafts/m1b-facts-2026-09-28.md`.

## Global Constraints

Everything from the M0 and M1a plans still holds (toolchain pin, clippy `-D warnings` with the workspace lint set incl. `indexing_slicing`, no `unwrap`/`expect` outside tests, no panic on untrusted input, no recursion in tree walks, platform cfg only in crates/{platform,process,gfx}, stylo types only in crates/style/** and crates/layout/src/style_adapt.rs, `git add` explicit paths only, gate chained with `&&`, no AI attribution in commits or PRs (owner, 2026-11-04 — the trailer earlier versions of this plan asked for is not added any more; pushed commits are not rewritten), docs in English — CLAUDE.md §3.4). Delta for M1b:

- **Channel direction.** `IpcSender`/`IpcReceiver`/`IpcBytesSender`/`IpcBytesReceiver` values travel only browser → child. Sole exception: `BootstrapMsg`, sent by a freshly spawned child before it has loaded any untrusted content. Reason: `ipc-channel` 0.23 panics on a bad channel index while deserialising (`src/ipc.rs:1025`, `:1054`; unchanged in 0.23.1) and release is `panic = "abort"`.
- **Channel ownership.** The process that receives shm on a channel creates it: the renderer creates its downward channel; the gpu creates the display-list channel (sent up in its bootstrap, relayed down to the renderer by the browser); the browser creates the frame channel and sends its sender down to the gpu. Task 3's Windows leak test confirms this rule or produces a ruling.
- **Copy-then-validate.** Any `IpcSharedMemory` received from a less-trusted process: check length against a limit → copy into owned memory → decode/validate the copy. Never parse the mapping in place.
- **Typed upward messages** (`DisplayListMsg`, `FrameMsg`) contain only `IpcSharedMemory` and fixed-size integer fields — no `Vec`, `String`, channel types.
- **Browser authority.** Surface size and frame `seq` come from the browser. The browser never blocks on a child: receive on reader threads, send on writer threads with bounded queues, wait for exit with `try_wait`/`wait_timeout`.
- **Subresource policy.** A subresource URL is served only if: scheme `file:`, no remote host, the canonical path lies under the canonical directory of the top-level document, it is a regular file; ≤ 64 URLs per document, each URL ≤ 8 KiB, ≤ 64 MiB total per document; one batch per `LoadDocument`; reads run on a worker thread. A refusal is a warning in the page, never an error.
- **Limits** (names are normative): `MAX_SUBRESOURCES_PER_DOCUMENT = 64`, `MAX_URL_BYTES = 8 * 1024`, `MAX_SUBRESOURCE_BYTES_PER_DOCUMENT = 64 * 1024 * 1024`, `SUBRESOURCE_TIMEOUT = 5 s`, `MAX_WARNINGS = 64`, `MAX_WARNING_BYTES = 1024`, `MAX_SURFACE_DIMENSION = 16_384`, `GPU_RESTARTS_PER_MINUTE = 3`, `CHILD_PING_TIMEOUT = 5 s`, `FIRST_FRAME_TIMEOUT = 30 s` (headless and `--exit-after-first-present`: no frame by then → exit 4); `MAX_DISPLAY_LIST_BYTES = 16 * 1024 * 1024`, `MAX_TOTAL_GLYPHS = 1 << 20` and `MAX_ITEMS = 1 << 18` (lowered from M1a's 1 000 000) — set by Task 6 from a measurement, owner decision 2026-11-04 (smoke page: 49 749 bytes, 133 items, 7 182 glyphs).
- **Exit codes of the browser process:** 0 ok, 3 renderer crashed, 4 no frame (gpu restart budget exhausted or first-frame watchdog), 5 renderer killed for a bad message, 78 sandbox unavailable.
- **Debug-only flags.** `--renderer-test-mode`, `--gpu-test-mode`, `--crash-gpu-after-frames`, `--crash-renderer-after-load` exist only under `cfg(debug_assertions)`; the release CLI rejects them (clap exit 2).
- **Timing tests.** Any test asserting on wall-clock time is added to the exact-name list in `.config/nextest.toml` (`threads-required = "num-cpus"`).
- **New dependencies:** `winit` 0.30.13 (Apache-2.0), `softbuffer` 0.4.8 (MIT OR Apache-2.0); each with a `docs/DEPENDENCIES.md` row and `cargo deny check` green. No other new third-party crates.


## Execution (owner decisions 2026-11-03)

- **Task 0 (done before Task 1):** `ipc-channel` 0.23.0 → 0.23.1 (`deps: bump ipc-channel to 0.23.1 (GHSA-fj85-w6rj-rxrc)`): 0.23.0 lets a macOS sender make the receiver read and deallocate arbitrary memory; the advisory is not in RustSec, so `cargo deny` does not flag it.
- **One PR for the whole of M1b** (like M1a): one commit per task, a draft PR after Task 1, a final whole-branch review, merge and tag `m1b` by the owner.
- **Risk accepted once for the plan.** Every task is `risky` by CLAUDE.md §3.5 (IPC, process lifecycle, parsing untrusted input). Each task's brief in the ledger (`.superpowers/sdd/2026-11-03-m1b-renderer-gpu/progress.md`) states the goal, current and expected behaviour, non-goals, the threat, the rollback, and acceptance cases including hostile ones.
- **Per task:** brief → failing test first (show the RED run) → implementation by the main agent → the full CLAUDE.md §5 gate chained with `&&` (under heavy external load re-run on a quiet machine; never commit red; new wall-clock tests go into `.config/nextest.toml`) → commit and push → a fresh read-only **opus** reviewer without cargo, from the brief and the task diff (sonnet + a `Ruling:` on HTTP 429) → every finding verified, fixed by the main agent, scoped re-review of the fix; at most 5 rounds, then a `Ruling:`.
- **CI** is read after Tasks 3, 10, 13 and at the end. A Task 3 failure only on windows-2022 stops the plan until the engine is fixed or the owner decides.

---

## File Structure

```
.config/nextest.toml                   + new timing tests (Tasks 10–12)
crates/process/src/spawn.rs            try_wait, wait_timeout, ExitedDuringBootstrap, SpawnOptions, generic over ChildProtocol
crates/process/src/error.rs            + ExitedDuringBootstrap
crates/ipc/src/bootstrap.rs            BootstrapMsg<D> (typed downward channel + optional display-list sender)
crates/ipc/src/protocol.rs             trait ChildProtocol { ROLE, Down, Up, hello_ack() }
crates/ipc/src/transfer.rs             DisplayListMsg, FrameMsg (typed upward, shm + ints only)
crates/ipc/src/shm.rs                  copy_bounded
crates/ipc/src/pump.rs                 reader threads → PumpEvent<T>
crates/ipc/src/proto/{mod,renderer,gpu}.rs   role protocols + Validate
crates/ipc/src/bad_message.rs          BadMessageReason
crates/ipc/src/codec.rs                + decode_bounded
crates/ipc/tests/xproc.rs              self-relaunching 3-process transport tests
crates/paint/src/wire.rs               encode/decode display list with in-decode limits
crates/style/src/sheets.rs             + stylesheet_requests
crates/renderer/                       NEW cl-renderer: ParsedDocument / LoadedDocument / Rendered
crates/testshell/src/pipeline.rs       uses cl-renderer
apps/chromelight/src/exit.rs           exit codes
apps/chromelight/src/browser/{mod,core,frames,present,subresources,restart,children,headless,window}.rs
apps/chromelight/src/renderer.rs       renderer process (state machine + IO shell)
apps/chromelight/src/gpu.rs            gpu process (state machine + IO shell)
apps/chromelight/tests/{parity,crash,hostile,window}.rs
tools/fuzz/fuzz_targets/display_list_decode.rs, ipc_decode.rs (extended)
tools/bench/mem.sh                     3 processes, empty + smoke page
.github/workflows/ci.yml               Gate S0 strengthened, window step, fuzz loop
docs/adr/0016-*.md, docs/superpowers/specs/2026-09-28-*.md, docs/{SECURITY,ARCHITECTURE,DEPENDENCIES,FEATURE_MATRIX,SPEC_REGISTRY,PLAN,RESEARCH-2026-09}.md, MEMORY.md
```

Dependency DAG stays acyclic: `cl-renderer` → html, dom, style, net, layout, paint, fonts; `cl-ipc` → platform, ipc-channel, postcard, serde (no engine crates); `apps/chromelight` → ipc, process, renderer, paint(serde), gfx, fonts, net, winit, softbuffer; `cl-testshell` → renderer, gfx.

---

### Task 1: ADR-0016, spec, research, docs

**Files:** `docs/adr/0016-m1b-raster-service-and-frame-transport.md`, `docs/superpowers/specs/2026-09-28-m1b-renderer-gpu-processes-design.md` (from `.superpowers/m1b-drafts/docs/…`), `docs/adr/0006-graphics-stack.md` and `docs/adr/0008-shell-ui.md` (status line only), `docs/adr/README.md`, `docs/RESEARCH-2026-09.md`, `docs/PLAN.md`, `docs/history/perf-m1a-2026-09.md`.

- [x] **Step 1:** Copy the ADR and spec drafts into `docs/`, translated to English (they were drafted in Russian before the English-docs rule). ADR-0016 status → `Accepted`, date 2026-09-28. Amend the draft: (a) Decision §5 gains the bootstrap exception and the channel-ownership rule from Global Constraints; (b) a new Decision item "Bad-message policy" with the owner's decision and a short evidence table (Chromium browser / Viz, Firefox parent, WebKit UI / GPU process, Servo) citing the URLs in `.superpowers/m1b-drafts/research-bad-message-policy-2026-09-28.md`; (c) Context gains the ipc-channel OS-layer facts: Windows `platform/windows/mod.rs:218` `expect` on bad OOB data, `:976` `unwrap` mapping shm, `:273` handle duplication from a peer-chosen process id, `:1349` shm sent upward requires the sender to hold `PROCESS_DUP_HANDLE` on the receiver (a sandbox-escape primitive for gpu → browser frames once Windows sandboxing lands); Linux `platform/unix/mod.rs:1154` peer-chosen `reserve_exact`, `:1212-1214` unsealed memfd → SIGBUS if the sender shrinks it during `copy_bounded`; macOS `platform/macos/mod.rs:819` `expect` on a malformed message (0.23.0; 0.23.1 rejects malformed messages — GHSA-fj85-w6rj-rxrc), shm sent as `MACH_MSG_VIRTUAL_COPY` (snapshot — no TOCTOU on macOS). State plainly: these block a Windows gpu sandbox later, not M1b. (d) Decision gains the subresource policy (Global Constraints). Spec §5 table rewritten for the kill policy and exit codes 3/4/5/78.
- [x] **Step 2:** ADR-0006 and ADR-0008: add `**Amended by:** ADR-0016 (2026-09-28)` under the status line; no other change. `docs/adr/README.md` index row for 0016.
- [x] **Step 3:** `docs/RESEARCH-2026-09.md`: append `## Re-check 2026-09-28` with the CHANGED/UNCHANGED table from `.superpowers/m1b-drafts/m1b-facts-2026-09-28.md` (stylo 0.21, cssparser 0.38, html5ever 0.40.1, Servo 0.6.0, Chrome 154; winit 0.30.13 current, softbuffer 0.4.8, wgpu 30.0.1, egui 0.36.2, accesskit conflict for M1d) and the note that the stylo-family bump is a separate whole-family PR (ADR-0015 §4); plus a 2026-11-03 line (ipc-channel 0.23.1 + advisory, stylo 0.23, winit 0.31 still beta).
- [x] **Step 4:** `docs/PLAN.md` M1b row → link to this plan; `docs/history/perf-m1a-2026-09.md` fuzz-target count (already corrected to 6 in M1a).
- [x] **Step 5:** `bash scripts/check-agents-md.sh`; commit `docs: ADR-0016 raster service and frame transport, M1b spec`.

---

### Task 2: Process lifecycle and a real Gate S0

**Files:** `crates/process/src/{spawn,error,lib}.rs`, `apps/chromelight/src/{browser,args,exit,main}.rs`, `apps/chromelight/tests/spawn.rs`, `.github/workflows/ci.yml`.

**Interfaces — Produces:**
```rust
pub struct SpawnOptions { pub extra_args: Vec<String>, pub timeout: Duration, pub trace_path: Option<PathBuf> }
impl ChildProcess {
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>, ProcessError>;
    pub fn wait_timeout(&mut self, deadline: Duration) -> Result<ExitStatus, ProcessError>; // kills at the deadline
}
pub enum ProcessError { /* existing */ ExitedDuringBootstrap { code: Option<i32> } }
// apps/chromelight/src/exit.rs
pub const EXIT_OK: i32 = 0; pub const EXIT_RENDERER_CRASHED: i32 = 3; pub const EXIT_NO_FRAME: i32 = 4;
pub const EXIT_RENDERER_KILLED_BAD_MESSAGE: i32 = 5; pub const EXIT_SANDBOX_UNAVAILABLE: i32 = 78;
```
Today the release Gate S0 step passes because the browser waits the full 15 s bootstrap timeout, not because it sees the refusal (`spawn.rs:57`). `spawn` must poll the child's `try_wait` while waiting for the bootstrap connection (the accept already runs on a helper thread; poll every 10 ms with a deadline) and return `ExitedDuringBootstrap{code}` as soon as the child exits. The browser maps a child that exits 78 during bootstrap to its own exit 78 with stderr `sandbox unavailable for <role>`. The trace file per child is `<stem>.<role>.json` (fixes the hard-coded `renderer.json`, `browser.rs:24`).

- [x] **Step 1: Failing tests** in `apps/chromelight/tests/spawn.rs` and `crates/process`: `sandbox_refusal_should_fail_spawn_within_3s_with_78` (debug binary, no `--no-sandbox`: `--exit-after-handshake` exits 78 in < 3 s, stderr contains `sandbox unavailable for renderer`), `wait_timeout_should_kill_child_ignoring_shutdown` (a child spawned with a hidden debug flag `--ignore-shutdown` is killed at the deadline and reaped), `trace_out_should_write_file_per_role`. Existing M0 spawn tests keep passing (`browser_failure_after_handshake_should_kill_renderer_on_drop` still sees its log line).
- [x] **Step 2: Implement** `try_wait`, `wait_timeout`, bootstrap exit polling, `SpawnOptions`, `exit.rs`; move `EXIT_SANDBOX_UNAVAILABLE` from `child.rs` to `exit.rs`.
- [x] **Step 3: CI Gate S0** (`.github/workflows/ci.yml`, the "release build refuses unsandboxed renderer" step): assert exit code `== 78` (not `!= 0`) and stderr contains `sandbox unavailable`, for `--exit-after-handshake`; assert `chromelight --no-crash-flag-exists` style probes: each debug-only flag (`--no-sandbox` stays accepted but inert in release) that exists after this task is rejected by the release binary with exit 2. The `--headless --png` variant is added to this step by Task 10.
- [x] **Step 4: Gate + commit** `process: detect a child that exits during bootstrap, bounded waits, per-role traces`.

---

### Task 3: ipc-channel transport spike (three real processes, all three OSes)

**Files:** `crates/ipc/src/{bootstrap,protocol,transfer,shm,pump,lib,error}.rs`, `crates/ipc/tests/xproc.rs`, `crates/process/src/spawn.rs` (generic over the protocol), `apps/chromelight/src/{browser,child}.rs` (compile against the generic bootstrap; M0 behaviour unchanged).

**Interfaces — Produces:**
```rust
// protocol.rs
pub trait ChildProtocol: 'static {
    const ROLE: ProcessType;
    /// Browser → child, typed ipc-channel message (trusted sender).
    type Down: Serialize + DeserializeOwned + Send + 'static;
    /// Child → browser, postcard bytes validated on receipt.
    type Up: Serialize + DeserializeOwned + Validate + Send + 'static;
    fn hello_ack(accepted: bool) -> Self::Down;
}
// bootstrap.rs
pub struct BootstrapMsg<D> { pub to_browser: IpcBytesReceiver, pub to_child: IpcSender<D>, pub display_lists: Option<IpcSender<DisplayListMsg>> }
pub struct BrowserEndpoint<P: ChildProtocol> { /* send(&P::Down), try-free receive only via pump */ }
pub struct ChildEndpoint<P: ChildProtocol> { /* send(&P::Up), recv() -> P::Down, recv_timeout(d) via pump */ }
pub fn connect_child<P: ChildProtocol>(server_name: &str, display_lists: Option<IpcSender<DisplayListMsg>>) -> Result<(ChildEndpoint<P>, IpcReceiver<P::Down>), IpcError>;
// transfer.rs — typed upward, shm + fixed-size fields only
pub struct DisplayListMsg { pub seq: u64, pub bytes: IpcSharedMemory }
pub struct FrameMsg { pub seq: u64, pub width: u32, pub height: u32, pub pixels: IpcSharedMemory }
// shm.rs
pub enum ShmError { TooLarge { size: usize, max: usize } }
pub fn copy_bounded(shm: &IpcSharedMemory, max: usize) -> Result<Vec<u8>, ShmError>;
// pump.rs
pub enum PumpEvent<T> { Message(T), Closed, Error(String) }
pub fn spawn_reader<T, R, F>(name: &str, rx: R, sink: F) -> std::io::Result<JoinHandle<()>>
    where R: PumpSource<T> + Send + 'static, F: FnMut(PumpEvent<T>) -> bool + Send + 'static; // sink returns false to stop
pub trait PumpSource<T> { fn recv_blocking(&self) -> Result<T, PumpRecvError>; } // impl for IpcReceiver<T> and for (IpcBytesReceiver + postcard decode)
```
`copy_bounded` checks `shm.len()` before touching bytes. The Hello handshake stays the first exchange: upward `Hello` over the byte channel (M0 `message::ToBrowser::Hello`), downward `P::hello_ack(accepted)`.

- [x] **Step 1: Failing tests** in `crates/ipc/tests/xproc.rs`. The test binary re-launches itself (`std::env::current_exe()` with an env var naming the role) so three REAL processes take part: a "browser" (the test), a "gpu" child that creates the display-list channel and sends its sender in the bootstrap, a "renderer" child that receives it from the browser (relayed downward) and sends shm on it. Tests: `dl_shm_should_cross_brokered_channel_3proc` (renderer sends 1 MiB of patterned bytes, gpu `copy_bounded`s and echoes a checksum up to the browser), `frame_shm_should_reach_browser` (browser creates the frame channel, sends the sender down to gpu, gpu sends a 800×600×4 frame; browser copies and checks the pattern), `foreign_payload_on_typed_channel_should_err` (send a different type through `OpaqueIpcSender::to::<Other>()` → receiver gets an error, no panic), `peer_exit_should_surface_closed` (child exits; the pump reports `Closed`), `empty_shm_should_roundtrip`, `copy_bounded_should_reject_oversize_before_copy`, `repeated_shm_sends_should_not_leak` (500 × 4 MiB sends; RSS of browser and gpu measured before/after with a platform-independent helper that shells out to `ps`/`tasklist` via `std::process::Command` and parses digits — growth < 64 MiB; runs on all three OSes; if Windows shows growth in the browser, record it in the report and propose the fix).
- [x] **Step 2: Implement** the generic bootstrap, protocol trait, transfer types, `copy_bounded`, pump. Migrate `cl-process::spawn` and `apps/chromelight` to the generic API with a transitional `M0Protocol` so all existing tests stay green.
- [ ] **Step 3: Push the branch and read the CI result for windows-2022 before Task 4.** If a transport test fails only on Windows, fix the engine (not the test) or stop and report — every later task depends on this.
- [ ] **Step 4: Ruling in the ledger** on channel ownership from the leak test; commit `ipc: generic bootstrap, typed downward channel, shm transfer, reader pumps`.

---

### Task 4: Protocol v2 with limits and bad-message reasons

**Files:** `crates/ipc/src/{proto/mod,proto/renderer,proto/gpu,bad_message,codec,validate,message,handshake,lib}.rs`, `apps/chromelight/src/child.rs`, `tools/fuzz/fuzz_targets/ipc_decode.rs`.

**Interfaces — Produces:**
```rust
pub const PROTOCOL_VERSION: u32 = 2;
pub struct Viewport { pub width: u32, pub height: u32 }           // 1..=MAX_SURFACE_DIMENSION, validated
// proto/renderer.rs
pub enum ToRenderer {                                               // typed, downward
    HelloAck { accepted: bool },
    LoadDocument { doc_id: u64, base_url: String, viewport: Viewport, seq: u64, bytes: IpcSharedMemory },
    Subresources { doc_id: u64, replies: Vec<SubresourceReply> },
    Resize { viewport: Viewport, seq: u64 },
    DisplayListSender { generation: u64, sender: IpcSender<DisplayListMsg> },
    Redraw { seq: u64 }, Ping(u64), Shutdown,
}
pub struct SubresourceReply { pub index: u32, pub result: Result<IpcSharedMemory, SubresourceRefusal> }
pub enum SubresourceRefusal { NotFile, OutsideDocumentDirectory, NotRegularFile, TooLarge, TooMany, Io }
pub enum FromRenderer {                                             // postcard, upward, Validate
    Pong(u64),
    NeedSubresources { doc_id: u64, urls: Vec<String> },            // ≤ MAX_SUBRESOURCES_PER_DOCUMENT, each ≤ MAX_URL_BYTES
    DocumentReady { seq: u64, warnings: Vec<String> },              // ≤ MAX_WARNINGS × MAX_WARNING_BYTES
    RenderFailed { seq: u64, kind: RenderFailureKind },
}
pub struct RendererProtocol;  // impl ChildProtocol { ROLE = Renderer, Down = ToRenderer, Up = FromRenderer }
// proto/gpu.rs
pub enum ToGpu { HelloAck { accepted: bool }, SetSurface { viewport: Viewport, seq: u64 }, FrameSender { sender: IpcSender<FrameMsg> }, Ping(u64), Shutdown }
pub enum FromGpu { Pong(u64), Stale { seq: u64 }, BadDisplayList { seq: u64, reason: BadMessageReason } }
pub struct GpuProtocol;
// bad_message.rs — modelled on Chromium content/browser/bad_message.h; stable numeric codes, never reused
#[repr(u16)] pub enum BadMessageReason {
    RendererUndecodable = 1, RendererInvalidField = 2, RendererUnexpectedMessage = 3, RendererTooManySubresources = 4,
    DisplayListTooLarge = 20, DisplayListUndecodable = 21, DisplayListInvalid = 22, DisplayListUnknownFont = 23, DisplayListLimitsExceeded = 24,
    FrameWrongSize = 40, FrameShortPixels = 41, FrameUnexpectedSeq = 42, GpuUndecodable = 43, GpuUnexpectedMessage = 44,
}
// codec.rs
pub fn decode_bounded<T: DeserializeOwned>(bytes: &[u8], max: usize) -> Result<T, CodecError>;
```
`Validate` for `FromRenderer`/`FromGpu` enforces every limit above and maps a failure to a `BadMessageReason`. `message.rs` keeps only `Hello` (upward) — Ping/Pong/Shutdown move into the role protocols; `child.rs` serves `Ping`/`Shutdown` per role. A v1 `Hello` is rejected (`VersionMismatch`).

- [x] **Step 1: Failing tests:** one test per limit (`need_subresources_over_64_urls_is_invalid`, `url_over_8k_is_invalid`, `warning_over_1k_is_invalid`, `viewport_zero_or_over_16384_is_invalid`), `v1_hello_rejected`, `bad_message_reason_codes_are_stable` (asserts every numeric value), proptest round-trip of `FromRenderer`/`FromGpu` through `codec::encode`/`decode_bounded`.
- [x] **Step 2: Implement.** Extend `tools/fuzz/fuzz_targets/ipc_decode.rs` to decode `FromRenderer` and `FromGpu` with `decode_bounded` and run `validate`; run it 60 s (`cargo +nightly fuzz run --fuzz-dir tools/fuzz --target aarch64-apple-darwin ipc_decode -- -max_total_time=60`).
- [x] **Step 3: Gate + commit** `ipc: protocol v2 for renderer and gpu, limits and bad-message reasons`.

---

### Task 6: Display-list wire format with in-decode limits

**Files:** `crates/paint/src/{wire,validate,lib}.rs`, `crates/paint/Cargo.toml` (`postcard` under the `serde` feature), `crates/layout/src/text.rs` (if `GlyphRun` needs a custom bounded `Deserialize`), `crates/paint/tests/wire.rs`, `tools/fuzz/fuzz_targets/display_list_decode.rs`, `tools/fuzz/Cargo.toml`, `.github/workflows/ci.yml` (fuzz loop).

**Interfaces — Produces:**
```rust
// cl_paint::wire (feature "serde")
pub const MAX_DISPLAY_LIST_BYTES: usize;      // from the measurement in Step 1, with the justification in the doc comment
pub const MAX_TOTAL_GLYPHS: usize;            // also enforced by validate()
pub enum WireError { TooLarge { size: usize, max: usize }, Undecodable(String), LimitsExceeded, Trailing(usize) }
pub fn encode_display_list(dl: &DisplayList) -> Result<Vec<u8>, WireError>;
pub fn decode_display_list(bytes: &[u8], max: usize) -> Result<DisplayList, WireError>;
```
A postcard `PopClip` is ~1 byte on the wire and ~56 bytes in memory, so a size cap alone lets 64 MiB of input become ~3.5 GiB. Decoding must count items and glyphs as it goes (custom `Deserialize` for the item sequence and glyph vectors: reject a declared length above the remaining budget before allocating; preallocate at most `min(declared, budget)`), and fail with `LimitsExceeded` the moment `MAX_ITEMS`, `MAX_GLYPHS_PER_RUN` or `MAX_TOTAL_GLYPHS` would be exceeded. `validate()` gains the `MAX_TOTAL_GLYPHS` check.

As built (deviations recorded in the ledger): a declared length within the limit is preallocated exactly (postcard reports a length only when the bytes left can hold it), otherwise nothing is preallocated and growth never requests capacity past the limit; `encode_display_list` refuses a list over the limits (`LimitsExceeded`) so a renderer learns before sending; the 44-page and smoke round-trip lives in `crates/testshell/tests/wire_roundtrip.rs` (a `cl-paint` → `cl-testshell` dev-dependency would be a cycle); `MAX_TOTAL_GLYPHS` lives in `validate.rs` so `validate()` does not depend on the `serde` feature.

- [x] **Step 1: Measure** the encoded size, item count and total glyph count of the display list of the 1 MB smoke page and of every reftest page; set `MAX_DISPLAY_LIST_BYTES` ≥ 4 × the smoke page's encoded size and `MAX_TOTAL_GLYPHS` ≥ 4 × its glyph count; record the numbers in the doc comments.
- [x] **Step 2: Failing tests:** `roundtrip_reftest_lists` (encode → decode equals the original for all 44 pages), `popclip_flood_fails_during_decode` (a crafted postcard buffer declaring 10⁹ items fails with `LimitsExceeded` and allocates < 16 MiB — assert via a capacity check on a bounded helper, not via a global allocator), `glyph_vector_over_limit_fails_during_decode`, `trailing_bytes_fail`, `oversize_input_fails_before_decode`.
- [x] **Step 3: Implement**; fuzz target `display_list_decode`: `fuzz_target!(|data: &[u8]| { if let Ok(dl) = cl_paint::wire::decode_display_list(data, cl_paint::wire::MAX_DISPLAY_LIST_BYTES) { let _ = cl_paint::validate(&dl, cl_layout::Rect::from_px(0.0, 0.0, 800.0, 600.0)); } });`, run 60 s, add to the CI loop.
- [x] **Step 4: Gate + commit** `paint: display-list wire format with limits enforced during decode`.

---

### Task 5: Subresource listing, `cl-renderer`, testshell migration

**Files:** `crates/style/src/sheets.rs`, new `crates/renderer/{Cargo.toml,src/lib.rs,src/document.rs,src/error.rs,tests/render.rs}`, `crates/testshell/src/{pipeline,reftest}.rs`, root `Cargo.toml` (workspace dep `cl-renderer`).

**Interfaces — Produces:**
```rust
// cl_style::sheets
pub fn stylesheet_requests(doc: &Document) -> (Vec<Url>, Vec<SheetWarning>);   // document order, rel tokens as collect_link
// cl_renderer
pub struct ParsedDocument { /* Document, base Url, parse warnings */ }
impl ParsedDocument {
    pub fn parse(bytes: &[u8], base: &Url) -> Result<ParsedDocument, RenderError>;
    pub fn subresource_requests(&self) -> &[Url];
    pub fn attach(self, subresources: SubresourceMap) -> LoadedDocument;
}
pub struct SubresourceMap { /* Url -> Result<Vec<u8>, String> */ }
impl SubresourceMap { pub fn new() -> Self; pub fn insert_ok(&mut self, url: Url, bytes: Vec<u8>); pub fn insert_err(&mut self, url: Url, reason: String); }
pub struct LoadedDocument { /* DOM + cached sheet bytes; rebuilds StyleEngine per render */ }
impl LoadedDocument {
    pub fn render(&mut self, viewport: (u32, u32), fonts: &mut FontDb, dumps: StageDumps) -> Result<Rendered, RenderError>;
}
pub struct Rendered { pub display_list: DisplayList, pub warnings: Vec<String>, pub stages: Option<Stages> }
```
As built (ledger): `collect_document_sheets`' `load` returns `Result<Vec<u8>, String>`; `subresource_requests` is deduplicated; only a failed cascade (`StyleEngine::resolve`) loses the document (`RenderError::DocumentLost`, unreachable today); every other failure gives it back. The testshell keeps its public API; `render_bytes` becomes: `ParsedDocument::parse` → load each requested URL with `cl_net::load_file` into a `SubresourceMap` → `attach` → `render` → `cl_gfx::cpu::rasterize`. `reftest::run_pair` unchanged in behaviour. `StageDumps`/`Stages` move to `cl-renderer` (re-exported by the testshell) — keep whatever opt-in shape debt D1 established.

- [x] **Step 1: Failing tests:** `requests_should_follow_document_order_and_rel_tokens` (cl-style), `rerender_should_equal_fresh_render_all_44_pages` (render at 800×600, then `render` again at 500×700 on the same `LoadedDocument`; the second display list equals a fresh `ParsedDocument` rendered at 500×700), `missing_subresource_should_warn_not_fail`.
- [x] **Step 2: Implement**; all goldens unchanged, 22/22 reftests.
- [x] **Step 3: Gate + commit** `renderer: cl-renderer document pipeline shared by the renderer process and the testshell`.

---

### Task 7: Browser core logic (no IO)

**Files:** `apps/chromelight/src/browser/{mod,core,frames,present,subresources,restart}.rs`.

**Interfaces — Produces:**
```rust
pub enum Event { RendererMsg(FromRenderer), RendererBad(BadMessageReason), RendererExited(Option<i32>),
                 GpuMsg { generation: u64, msg: FromGpu }, GpuBad { generation: u64, reason: BadMessageReason }, GpuExited { generation: u64, code: Option<i32> },
                 Frame { generation: u64, meta: FrameMeta, pixels: Vec<u8> }, FrameBad { generation: u64, reason: BadMessageReason },
                 Resized { physical: (u32, u32), scale: f64 }, SubresourcesLoaded { doc_id: u64, replies: Vec<LoadedSubresource> }, Tick }
// Commands carry plain bytes; the IO layer (Task 10) turns them into wire messages (creating IpcSharedMemory)
// and wires channels on spawn (frame sender down to the gpu, the gpu's display-list sender down to the renderer).
pub enum ToRendererCmd { LoadDocument { doc_id: u64, viewport: Viewport, seq: u64 }, Subresources { doc_id: u64, replies: Vec<LoadedSubresource> },
                         Resize { viewport: Viewport, seq: u64 }, Redraw { seq: u64 }, Ping(u64), Shutdown }
pub enum ToGpuCmd { SetSurface { viewport: Viewport, seq: u64 }, Ping(u64), Shutdown }
pub struct LoadedSubresource { pub index: u32, pub result: Result<Vec<u8>, SubresourceRefusal> }
pub enum Action { SendRenderer(ToRendererCmd), SendGpu(ToGpuCmd), SpawnGpu, KillRenderer { reason: BadMessageReason },
                  KillGpu { reason: BadMessageReason }, LoadSubresources { doc_id: u64, urls: Vec<String> }, Present(PresentFrame), ShowError(ErrorKind), Exit(i32) }
pub struct BrowserCore { /* seq, generations, restart policy, doc state */ }
impl BrowserCore { pub fn new(doc: DocumentSource, initial: (u32, u32), scale: f64, now: Instant) -> (Self, Vec<Action>); pub fn on_event(&mut self, ev: Event, now: Instant) -> Vec<Action>; }
pub struct FrameMeta { pub seq: u64, pub width: u32, pub height: u32 }
pub fn check_frame(meta: &FrameMeta, pixels_len: usize, expected_seq: u64, expected: (u32, u32)) -> Result<(), BadMessageReason>;
pub fn surface_for(physical: (u32, u32), scale: f64) -> (u32, u32, u32);   // (css w, css h, k = max(1, floor(scale)))
pub fn blit(src: &[u8], src_size: (u32, u32), k: u32, dst: &mut [u32], dst_size: (u32, u32), background: u32);  // premul RGBA opaque → 0RGB, nearest ×k, crop, pad
pub fn resolve_subresource(doc_dir: &Path, url: &str) -> Result<PathBuf, SubresourceRefusal>;
pub struct RestartPolicy; impl RestartPolicy { pub fn allow(&mut self, now: Instant) -> bool; } // ≤ GPU_RESTARTS_PER_MINUTE
```
`FromRenderer`/`FromGpu` messages that fail decoding or `Validate` arrive as `RendererBad` / `GpuBad` events from the IO layer (Task 10). As built (ledger): `BrowserCore::new(options, physical, scale, now) -> Result<(Self, Vec<Action>), InvalidViewport>` with `CoreOptions { fatal_errors }` (headless / `--exit-after-first-present` exit with the failure's code, a window shows it); `Action::SpawnGpu { generation }`, `Action::KillGpu { generation, reason }`, `Action::Warnings`; `ErrorKind::RenderFailed` (exit 4); `load_subresources(doc_dir, urls, max_total_bytes)` holds the count/byte budget; the surface's CSS edge is `ceil(physical / k)`; a gpu failure with no renderer left is not respawned; pings stay in the IO layer. Policy: `FromGpu::BadDisplayList` or `RendererBad` → `KillRenderer` + `ShowError(RendererKilled)` + exit 5 in headless; `FromGpu::Stale` → nothing; `GpuBad`, `FrameBad` or a frame failing `check_frame` → `KillGpu` + respawn per `RestartPolicy`; any event whose `generation` is older than the current gpu's → ignored. At most one `seq` in flight: resizes arriving while a frame is pending coalesce into the latest size.

- [x] **Step 1: Failing tests:** `bad_display_list_kills_renderer`, `renderer_validate_failure_kills_renderer`, `stale_is_dropped_silently`, `bad_frame_restarts_gpu`, `old_generation_frame_ignored`, `fourth_restart_in_60s_errors` (fake clock via `Instant` arithmetic), `one_seq_in_flight_resizes_coalesce`, `path_escape_and_fifo_refused` (`../`, symlink out of the directory — create in `std::env::temp_dir()`, skip the FIFO part on platforms where `std` cannot create one without platform cfg: create it via `std::process::Command::new("mkfifo")` only if that binary exists, else assert the non-regular-file check with a directory), `blit_k2_nearest_and_crop`, `fractional_scale_floors_k`, `frame_with_wrong_size_or_short_pixels_is_bad`.
- [x] **Step 2: Implement; Gate + commit** `browser: frame, restart and subresource policy as a pure state machine`.

---

### Task 8: Renderer process

**Files:** `apps/chromelight/src/renderer.rs`, `apps/chromelight/src/{main,child,args}.rs`.

**Interfaces:** `pub struct RendererState; impl RendererState { pub fn on(&mut self, input: RendererInput, now: Instant) -> Vec<RendererOutput>; }` with inputs `Down(ToRenderer)`, `Tick`, and outputs `Up(FromRenderer)`, `SendDisplayList { seq: u64, bytes: Vec<u8> }`, `Exit`. IO shell: pump for the downward channel, `cl_renderer` for the pipeline, `cl_paint::wire::encode_display_list` → `IpcSharedMemory::from_bytes` → `DisplayListMsg`. A failed display-list send means "the gpu is gone — wait for `DisplayListSender` of a newer generation and resend the latest list". As built (ledger): inputs are plain data (`RendererInput::{Load, Subresources, Resize, Redraw, DisplayListSender{generation}, DisplayListSendFailed, Ping, Shutdown, Tick}`), the IO shell (`child::serve_renderer`) copies shm and keeps the sender; `RendererState::new(max_list_bytes)`; requests and warnings are cut to the protocol limits; a parse failure answers every seq with `RenderFailed{Parse}`.

- [x] **Step 1: Failing tests:** `load_requests_subresources_then_renders`, `shutdown_honored_while_awaiting_subresources`, `subresource_timeout_renders_with_warnings` (5 s via injected `Instant`), `resize_during_await_renders_latest_only`, `new_display_list_sender_triggers_resend`, `redraw_during_load_sets_the_first_seq`, `redraw_after_a_failed_render_renders_again`, `redraw_after_a_failed_resize_lays_out_for_the_latest_viewport` (every `Redraw`/`Resize` seq is answered with a list for the latest viewport or with `RenderFailed` for that seq — `ToRenderer::Redraw` docs; Task 7 review).
- [x] **Step 2: Implement; Gate + commit** `renderer: renderer process loop over cl-renderer`.

---

### Task 9: GPU process

**Files:** `apps/chromelight/src/gpu.rs`, `apps/chromelight/src/{main,child}.rs`, `apps/chromelight/tests/gpu_process.rs`.

**Interfaces:** `pub struct GpuState { surface: Option<(Viewport, u64)>, pending: Option<(u64, Vec<u8>)> }` with `on(GpuInput) -> Vec<GpuOutput>`; inputs `Down(ToGpu)`, `DisplayList(DisplayListMsg)`; outputs `Up(FromGpu)`, `Frame(FrameMsg)`. Pipeline per display list, in this order: length > `MAX_DISPLAY_LIST_BYTES` → `BadDisplayList{DisplayListTooLarge}` before any copy; `seq < surface.seq` → `Stale`; `copy_bounded` (so `pending` only ever holds an owned, size-checked copy); `seq > surface.seq` → hold the copy in `pending` (latest wins); otherwise → `decode_display_list` → `cl_gfx::cpu::rasterize(&dl, surface.w, surface.h, &fonts)` → `FrameMsg`. `GfxError::Invalid` → `DisplayListInvalid`, `FontNotBundled` → `DisplayListUnknownFont`. As built (ledger): stale checks run before the copy too; a held list is dropped as `Stale` when a later list or a higher surface seq arrives (latest wins by arrival — Task 9 review: by seq, a `u64::MAX` list would pin the slot); the gpu queues at most 4 display lists (browser messages are never held behind them — Task 9 re-review); each seq is rastered once; frames are plain data (`GpuOutput::Frame`) until the shell builds the `FrameMsg`, and wait for `FrameSender`; any other `GfxError` is the gpu's own failure (`GpuOutput::Fail`, exit 1, the browser restarts it); end-to-end test `apps/chromelight/tests/gpu_process.rs` (the real binary as a gpu child).

- [x] **Step 1: Failing tests:** `dl_before_set_surface_held_then_rastered`, `stale_seq_rejected_as_stale`, `oversize_shm_rejected_before_copy`, `undecodable_bytes_reported_as_bad`, `unknown_font_rejected_gpu_alive`, `raster_size_comes_from_surface_not_display_list`, `held_list_below_the_new_surface_seq_is_stale` (a list held before `SetSurface` whose seq is below the surface seq is dropped as `Stale`, not rastered — Task 8 re-review: a new gpu can get the renderer's previous list before the browser's `SetSurface`), `a_seq_is_rastered_once` (after rastering seq N, a list with seq ≤ N is `Stale` — Task 7 review; the browser also drops a repeated frame for an issued seq).
- [x] **Step 2: Implement; Gate + commit** `gpu: raster service process with pending-seq slot and bad-list reporting`.

---

### Task 10: Headless browser, CLI, parity — first end-to-end

**Files:** `apps/chromelight/src/browser/{children,headless}.rs`, `apps/chromelight/src/{args,main}.rs`, `apps/chromelight/Cargo.toml` (dev-dependency `cl-testshell`), `apps/chromelight/tests/parity.rs`, `.config/nextest.toml`, `.github/workflows/ci.yml` (Gate S0 `--headless` variant).

CLI: `chromelight [--no-sandbox] [--headless --png <out>] [--viewport WxH] [--resize-to WxH] <path|file-url>`; M0 flags (`--exit-after-handshake`, `--idle-seconds`, `--trace-out`) keep working. `children.rs` owns the real IO: spawn renderer + gpu (Task 2 options), reader pumps into one `mpsc` of `Event`s, a writer thread per child with a bounded queue (overflow kills that child), `try_wait` polling on `Tick`, `wait_timeout` on shutdown, ping watchdog (`CHILD_PING_TIMEOUT`). Subresource reads on a worker thread. The browser's event queue must not grow with a child's flood: one `FromGpu::Stale` per flooded list arrives, cheap for the core, but bound or coalesce the reader's queue (Task 9 re-review). Before acting on a gpu's exit, drain what its up channel already holds (a `BadDisplayList` sent just before exiting must still kill the renderer, not be dropped as an old generation — Task 9 review). Frame shm is copied with `copy_bounded` against the size of the seq in flight (`w × h × 4`), never against `MAX_SURFACE_DIMENSION²` (Task 7 review: a hostile gpu must not make the browser copy 1 GiB per message). Headless drives `BrowserCore` until the first `Present` (or the `--resize-to` second one), writes the PNG from the frame bytes via `tiny_skia::Pixmap`, exits with the core's code. As built (ledger): one bounded event queue (256) for all readers; frames refused unread above the largest surface asked of that gpu, at most 2 copies queued (permits); writer queues of 64; a child's exit is reported when its upward channel closes (in order after what it sent), or 1 s after `try_wait` saw it; pings every 1 s, `CHILD_PING_TIMEOUT` → kill; a failed gpu spawn counts as a gpu exit; `ToRendererCmd`/`ToGpuCmd` lost `Ping`/`Shutdown` (the IO layer's own); `cl-process` gained `ChildProcess::into_parts` + `ChildHandle` (kill-on-drop kept) so the endpoint can be split; a gpu respawn still blocks the loop up to the bootstrap timeout, the renderer's watchdog is postponed by that time (T13 note); children answer `Ping` on their reader thread (liveness, not progress — Task 10 review); a `--resize-to` that keeps the surface writes the first frame, and the resized frame has its own `FIRST_FRAME_TIMEOUT`.

- [x] **Step 1: Failing tests** (`apps/chromelight/tests/parity.rs`, debug binary via `CARGO_BIN_EXE_chromelight` with `--no-sandbox`): `parity_all_44_pages` (for every page on both sides of the 22 pairs in `crates/testshell/tests/ref/`, the 3-process PNG's pixels equal `cl_testshell::render_file`'s — assert the page count is exactly 44), `resize_parity_matches_testshell` (`--resize-to 500x700` second frame equals the testshell at 500×700), `link_outside_doc_dir_warns_and_renders` (a page linking `../outside.css` renders, the stylesheet is not applied, stderr carries the warning).
- [x] **Step 2: Implement.** Extend the CI Gate S0 step: the release binary with `--headless --png out.png <page>` exits 78 within 3 s and `out.png` does not exist.
- [ ] **Step 3: Gate + push, read CI on all three OSes; commit** `browser: headless three-process rendering with pixel parity to the testshell`.

---

### Task 11: Crash handling

**Files:** `apps/chromelight/src/{renderer,gpu,args}.rs`, `apps/chromelight/tests/crash.rs`.

Debug-only flags `--crash-gpu-after-frames=N` (the gpu calls `std::process::abort()` after sending N frames) and `--crash-renderer-after-load` (abort after `DocumentReady`), forwarded by the browser to the child it spawns (the first instance only, so a respawned gpu does not crash again unless `--crash-gpu-always` is also given).

Also from the Task 10 review (IO-layer guarantees not yet tested end to end): a gpu that sends `BadDisplayList` and exits at once still makes the browser exit 5 (the report is read before the exit); the renderer is killed (exit 3) when it stops reading (writer queue full) or goes silent (no Pong within `CHILD_PING_TIMEOUT` — pings are answered on the child's reader thread, so only a silent process trips it); the renderer is killed when the browser fails after `into_parts`.

- [ ] **Step 1: Failing tests:** `gpu_crash_respawns_png_matches` (`--crash-gpu-after-frames=0` then the respawned gpu delivers a frame; PNG equals the testshell's), `gpu_crash_loop_exits_4` (`--crash-gpu-always`: after the restart budget, exit 4, all children reaped), `renderer_crash_exits_3_children_reaped`.
- [ ] **Step 2: Implement; Gate + commit** `browser: gpu restart with generations, renderer crash handling`.

---

### Task 12: Hostile renderer and hostile gpu

**Files:** `apps/chromelight/src/{renderer,gpu,args}.rs`, `apps/chromelight/tests/hostile.rs`, `.config/nextest.toml`.

Debug-only `--renderer-test-mode=<garbage|oversized|wrong-seq|future-seq-flood|out-of-bounds|bad-font|popclip-flood|bad-up-message>` and `--gpu-test-mode=<wrong-size|short-pixels|future-seq|flood>` make the respective child misbehave in that one way.

- [ ] **Step 1: Failing tests,** one per mode, each asserting: renderer modes — the gpu process is still alive and answers `Ping` (checked by a browser-side log line), stderr carries the expected `BadMessageReason` name, the renderer was killed, exit 5; except `wrong-seq` (→ `Stale`, silent, the correct frame still arrives, exit 0) and `future-seq-flood` (the gpu keeps only the latest pending list — its RSS stays under a bound, it answers `Ping`, the renderer is not killed). Gpu modes — the gpu is killed and respawned, the browser stays alive, the final PNG is correct or exit 4 when the budget is exhausted.
- [ ] **Step 2: Implement; Gate + commit** `browser: hostile renderer and gpu modes with the kill policy`.

---

### Task 13: Window (winit + softbuffer)

**Files:** `apps/chromelight/src/browser/window.rs`, `apps/chromelight/Cargo.toml`, root `Cargo.toml` (workspace deps), `apps/chromelight/tests/window.rs`, `.github/workflows/ci.yml`, `docs/DEPENDENCIES.md`.

`winit = { version = "0.30.13", default-features = false, features = ["rwh_06", "x11", "wayland", "wayland-dlopen", "wayland-csd-adwaita-notitle"] }`, `softbuffer = "0.4.8"` (verify both feature sets in the registry source before pinning). `ApplicationHandler<Event>`; the window is created in `resumed`; reader threads post `Event`s through `EventLoopProxy`; frames go into a latest-wins slot and are blitted on the main thread with `browser::present::blit`; the title is the document URL, or `ChromeLight — renderer crashed` / `— renderer killed` on error with a solid error fill; `WindowEvent::Resized` → `Event::Resized{physical, scale}`. `--exit-after-first-present` exits 0 after the first `present()`, with a 30 s watchdog (exit 4). The event loop is created only in `browser::run` on the main thread; headless never creates one.

- [ ] **Step 1: Failing test** `apps/chromelight/tests/window.rs::first_present_should_exit_zero` — `#[ignore]` by default (needs a display); CI runs it explicitly.
- [ ] **Step 2: Implement.** CI: a step in the `test` matrix after nextest: Linux `sudo apt-get install -y xvfb libxkbcommon-x11-0` and `env -u WAYLAND_DISPLAY xvfb-run -a cargo test -p chromelight --test window -- --ignored`; macOS and Windows run the same command without xvfb. If a runner cannot open a window, record a ruling with the error and keep the manual check for that OS.
- [ ] **Step 3: Manual check (owner, macOS):** window shows `crates/testshell/tests/ref/text-basic.html`, resize relayouts, Retina scale is crisp (k = 2).
- [ ] **Step 4: Gate + commit** `browser: window presentation with winit and softbuffer`.

---

### Task 14: RSS per process, docs, close-out

**Files:** `tools/bench/mem.sh`, `docs/history/bench-2026-09.md` (or a new `docs/history/bench-m1b-2026-10.md`), `docs/{SECURITY,ARCHITECTURE,DEPENDENCIES,FEATURE_MATRIX,SPEC_REGISTRY,PLAN}.md`, `MEMORY.md`.

- [ ] **Step 1:** `mem.sh` runs `chromelight --no-sandbox --headless --idle-seconds 5 <page>` (add `--idle-seconds` to headless: hold after the first frame) for an empty page and for the 1 MB smoke page, labels each PID by its `--type=`, and records browser/renderer/gpu RSS. Note that renderer and gpu now link winit (AppKit on macOS) — record that cost.
- [ ] **Step 2:** `SECURITY.md` §3 rows renderer → gpu and gpu → browser (copy-then-validate, channel direction, kill policy) and a "known gaps" list (ipc-channel OS-layer facts from ADR-0016); `ARCHITECTURE.md` §2–4 (gpu as raster service in M1–M3, browser presents); `FEATURE_MATRIX`, `SPEC_REGISTRY` (nothing spec-facing changes; note the DPR-1 + integer upscale deviation), `PLAN.md` M1b exit checkboxes with numbers, `MEMORY.md` state/debts/session log.
- [ ] **Step 3:** Full verification: the gate, `cargo nextest run --workspace --locked` 3× green, fuzz `ipc_decode` and `display_list_decode` 60 s each, CI 5/5 + window step green.
- [ ] **Step 4:** Commit `docs: close M1b renderer and gpu processes`; final whole-branch review; owner decides merge and tag `m1b`.

---

## Verification (end-to-end)

1. `cargo test -p chromelight --test parity` — 44/44 pages pixel-equal to `cl-testshell`, plus the resize case, on macOS/Linux/Windows CI.
2. `cargo test -p chromelight --test hostile --test crash` — every mode behaves per the kill policy; gpu restarts deliver a frame; renderer crash → exit 3 with children reaped.
3. `cargo test -p chromelight --test window -- --ignored` green on the three CI runners (Linux under `xvfb-run`); owner's manual macOS check.
4. Release Gate S0 in CI: `--exit-after-handshake` and `--headless --png` → exit 78 within 3 s, no PNG.
5. Fuzz `ipc_decode` and `display_list_decode`: 60 s each, no crash.
6. `tools/bench/mem.sh` numbers for browser/renderer/gpu in `docs/history/`.
7. Full gate green; ADR-0016 Accepted; SECURITY/ARCHITECTURE/MEMORY updated.

## Risks and stop conditions

| Risk | Signal | Action |
|---|---|---|
| ipc-channel shm/handles misbehave on Windows | Task 3 xproc tests red only on windows-2022 | fix the engine or stop before Task 4; never skip Windows |
| Browser handle leak on Windows | `repeated_shm_sends_should_not_leak` growth in the browser | channel-ownership ruling (receiver creates) or own shm in cl-platform (separate ADR). **Happened 2026-11-03** (+1 handle per shm, an ipc-channel bug independent of channel ownership): owner chose a vendored one-line patch (`third_party/ipc-channel`) + an upstream report |
| winit cannot open a window on a CI runner | Task 13 CI step red on one OS | runner setup; else ruling + manual check for that OS |
| Parity breaks | any of 44 pages differs | a transport/decoding bug — never loosen the comparison |
| Deadlock between processes | a test hangs | writer threads, bounded queues, ping watchdog; the browser never blocks |
| Timing tests flaky under load | nextest red only on wall-clock tests | add the test to `.config/nextest.toml`; never widen a bound silently |

## Self-review

- **Spec coverage:** processes and roles → Tasks 7–10; channels and messages → 3, 4; copy-then-validate → 3, 6, 9, 10; subresources → 5, 7, 10; resize → 5, 7, 10; crash handling → 11; hostile modes and kill policy → 4, 7, 12; window → 13; fuzz → 4, 6; RSS → 14; docs → 1, 14. Exit criteria 1–7 ↔ Verification 1–7.
- **Placeholders:** `MAX_DISPLAY_LIST_BYTES` / `MAX_TOTAL_GLYPHS` values come from Task 6's measurement (the method is specified); winit feature names are verified by Task 13 against the registry source.
- **Type consistency:** `Viewport`, `ToRenderer`, `FromRenderer`, `ToGpu`, `FromGpu`, `DisplayListMsg`, `FrameMsg`, `BadMessageReason`, `SubresourceReply`, `SubresourceRefusal`, `ChildProtocol`, `PumpEvent`, `copy_bounded`, `decode_bounded`, `decode_display_list`, `ParsedDocument`/`LoadedDocument`/`Rendered`, `BrowserCore::on_event`, exit codes — defined once, used with the same names in later tasks.
- **Order:** 1 → 2 → 3 (CI on Windows before continuing) → 4 → 6 → 5 → 7 → 8 → 9 → 10 (first end-to-end, CI) → 11 → 12 → 13 → 14.
