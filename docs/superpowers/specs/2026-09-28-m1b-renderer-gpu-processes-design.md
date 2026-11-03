# M1b — renderer and GPU processes: design

**Date:** 2026-09-28. **Status:** approved by the owner (2026-09-28); updated 2026-11-03 for the bad-message policy, channel ownership and `ipc-channel` 0.23.1.
**Decisions:** ADR-0016 (Accepted), ADR-0005, ADR-0006, ADR-0008, ADR-0012, ADR-0015.
**Owner decisions 2026-09-28:** the browser presents the frame (softbuffer); the GPU process rasterises on the CPU (tiny-skia); the scope includes resize, GPU-process restart, renderer-crash handling, per-process RSS; a bad message from the renderer kills the renderer.
**Plan:** `docs/superpowers/plans/2026-09-28-m1b-renderer-gpu-processes.md` (interfaces, tests, order).

## 1. Goal and boundaries

`chromelight --no-sandbox file:///page.html` opens a window and shows the page, parsed and laid out by the **renderer process** and rasterised by the **GPU process**; the display list goes renderer → gpu over shared memory and is validated on receipt. The pixels are identical to the single-process `cl-testshell render`.

Not in M1b: the sandbox (M1c; until then children run only with `--no-sandbox` in debug/`release-dev`), egui and the address bar (M1d), the network and `http:` (M1d/M2), scrolling and input (M3), GPU raster and the compositor (M4), restarting a crashed tab (M2), DPR raster (after M1b), OOPIF (M4).

## 2. Processes and roles

| Process | Owns | Never does |
|---|---|---|
| browser | the window (winit 0.30), the softbuffer surface, spawning and supervising children, reading files (`cl_net::load_file`), the subresource policy, the surface size and frame number, killing children | does not parse HTML/CSS/display lists; does not wait on children without a timeout; does not receive channel ends from children (bootstrap excepted) |
| renderer | `cl-html` → `cl-style` → `cl-layout` → `cl-paint`; keeps the document between frames (resize) | does not read files, open sockets, or choose the pixmap size |
| gpu | copying the display list out of shm, decoding with limits, `cl_paint::validate` against the size from the browser, `cl_gfx::cpu::rasterize`, the frame into shm | loads no GPU driver in M1b, reads no files, does not trust `seq` or sizes from the renderer |

## 3. Channels and messages

Downward traffic (browser → child, trusted sender) is one typed `ipc-channel` channel per child. Upward traffic (child → browser, untrusted) is `postcard` over byte channels with `Validate` at the receiver, as in M0; `PROTOCOL_VERSION = 2`. Bulk data is `IpcSharedMemory`, which every receiver copies before use. The process that receives shm on a channel creates that channel (ADR-0016 §5).

```
browser ══typed══► renderer   ToRenderer: HelloAck, LoadDocument{doc_id, base_url, viewport, seq, bytes: shm},
                                          Subresources{doc_id, replies}, Resize{viewport, seq},
                                          DisplayListSender{generation, sender}, Redraw{seq}, Ping, Shutdown
browser ◄─bytes─── renderer   FromRenderer: Pong, NeedSubresources{doc_id, urls ≤ 64}, DocumentReady{seq, warnings},
                                            RenderFailed{seq, kind}
browser ══typed══► gpu        ToGpu: HelloAck, SetSurface{viewport, seq}, FrameSender{sender}, Ping, Shutdown
browser ◄─bytes─── gpu        FromGpu: Pong, Stale{seq}, BadDisplayList{seq, reason}
renderer ══dl════► gpu        DisplayListMsg{seq, bytes: shm}                      (typed, shm + integers only)
gpu ═════frames══► browser    FrameMsg{seq, width, height, pixels: shm}           (typed, shm + integers only)
```

Rules: channel ends travel only browser → child (ADR-0016 §5); the gpu creates the display-list channel and sends its sender up in its bootstrap, and the browser relays it down to the renderer in `DisplayListSender`; the browser creates the frame channel and sends its sender down to the gpu. `FrameMsg` and `DisplayListMsg` are the only typed messages from less trusted processes; they hold no `Vec`, `String` or channels. Limits (normative names in the plan's Global Constraints): `MAX_DISPLAY_LIST_BYTES` and `MAX_TOTAL_GLYPHS` are set by a measurement of the smoke page's display list, `MAX_SURFACE_DIMENSION = 16_384` (as `cl_gfx`), `MAX_SUBRESOURCES_PER_DOCUMENT = 64`, subresource answer timeout 5 s.

## 4. Data flow

1. **Start.** The browser spawns the renderer and the gpu (M0 bootstrap + typed downward channel). The gpu's bootstrap carries the display-list sender; the browser relays it to the renderer and sends the frame sender to the gpu. `SetSurface{viewport, seq=1}` → gpu.
2. **Document.** The browser reads the file (`cl_net::load_file`) and puts the bytes into shm → renderer with `LoadDocument{base_url, viewport, seq=1}`.
3. **Subresources.** The renderer parses the document, collects subresource URLs (`<link rel=stylesheet>`) without loading them, sends `NeedSubresources{doc_id, urls}` and waits ≤ 5 s for the answer. The browser checks each URL against the subresource policy (ADR-0016 §6), reads it, and answers with a status plus shm per loaded file. A policy refusal is a warning in `DocumentReady.warnings`, not a render error.
4. **Display list.** Renderer: style → layout → paint → wire encoding → shm → `DisplayListMsg{seq}` to the gpu.
5. **Raster.** Gpu: a `seq` older than the current `SetSurface` → `Stale`; newer → held in a single pending slot (latest wins); length ≤ `MAX_DISPLAY_LIST_BYTES` checked before any copy; copy out of shm; decode with in-decode limits; `validate` against `(0, 0, w, h)` from the browser; `rasterize(w, h)`; pixels → shm → `FrameMsg{seq, w, h}`. Any rejection → `BadDisplayList{seq, reason}` to the browser; the gpu stays alive.
6. **Present.** Browser: `seq` = the last one requested, `w×h` = the requested size, length = `w·h·4`; the pixels are copied into the softbuffer buffer (premultiplied RGBA → `0RGB`, the frame is always opaque — a white canvas), scaled by an integer factor for the scale factor, `present`. Before the first frame and after a failure — the last good frame or a background fill.
7. **Resize.** The browser coalesces resize events, `seq += 1`, `SetSurface` → gpu, `Resize{viewport, seq}` → renderer. The renderer re-runs style/layout/paint from the stored DOM (`M1-ONLY: full re-style on resize`) and sends a new display list. At most one `seq` is in flight.

The browser receives children's messages on background threads (one per channel) and forwards them into the winit event loop through `EventLoopProxy` (headless: an `mpsc`). This closes M0 debt 7 (the `recv_timeout` poll) for the new channels; waiting for a child to exit is `try_wait` with a deadline and a kill (M0 debt 8).

## 5. Errors and crashes

Policy: ADR-0016 §9 (kill on a bad message, no strike counter, benign races dropped). Browser exit codes (headless and `--exit-after-first-present`): 0 ok, 3 renderer crashed, 4 no frame (gpu restart budget exhausted or first-frame watchdog), 5 renderer killed for a bad message, 78 sandbox unavailable.

| Event | Reaction | Check |
|---|---|---|
| A renderer message fails decoding or `Validate` | the browser kills the renderer, logs the `BadMessageReason`, error fill + title "ChromeLight — renderer killed"; headless exit 5 | a test per `--renderer-test-mode` (`garbage`, `oversized`, `out-of-bounds`, `bad-font`, `popclip-flood`, `bad-up-message`) |
| The gpu reports `BadDisplayList` (oversize shm, undecodable, invalid, unknown `FontKey`, limits exceeded) | the browser kills the renderer as above; the gpu stays alive and answers `Ping` | same tests, plus a gpu liveness check |
| A display list with a stale `seq` | `Stale`, dropped silently; the correct frame still arrives | `--renderer-test-mode=wrong-seq` |
| A flood of future-`seq` display lists | the gpu keeps only the latest pending list; its RSS stays bounded; the renderer is not killed | `--renderer-test-mode=future-seq-flood` |
| The gpu crashed or closed its channel | the browser spawns a new gpu (≤ 3 per 60 s) with a new generation, relays the new display-list sender, `SetSurface`, `Redraw{seq}` → renderer; after the budget — error fill and exit 4 | `--crash-gpu-after-frames=N`, `--crash-gpu-always` |
| A frame of the wrong size or length, or undecodable gpu traffic | the browser kills and restarts the gpu (same budget) | `--gpu-test-mode=<wrong-size|short-pixels|future-seq|flood>` |
| Any event from an older gpu generation | ignored | unit test of the core state machine |
| The renderer crashed | the browser lives, error fill + title "ChromeLight — renderer crashed"; restarting the tab is M2; headless exit 3; all children reaped | `--crash-renderer-after-load` |
| A child is silent | timeouts on the handshake (M0), on subresources (5 s), on `Ping` (5 s), on the first frame (watchdog, exit 4) | unit tests of the state machines |

All test flags exist only under `cfg(debug_assertions)`, like `--no-sandbox`; the release CLI rejects them.

## 6. Code and crates

- `crates/renderer` (new `cl-renderer`): `ParsedDocument::parse` → `subresource_requests` → `attach(SubresourceMap)` → `LoadedDocument::render(viewport, fonts, dumps)` — shared by the renderer process and `cl-testshell` (the testshell moves onto it, so parity compares the same pipeline). Collecting subresource URLs without loading them lives here and in `cl_style::sheets::stylesheet_requests`.
- `crates/ipc`: a generic bootstrap with a typed downward channel, `ChildProtocol`, the role protocols `proto::{renderer, gpu}` (postcard + `Validate`), `BadMessageReason`, the typed upward transfer messages (`DisplayListMsg`, `FrameMsg` — shm and fixed fields only), `shm::copy_bounded(&IpcSharedMemory, max)`, reader pumps, `codec::decode_bounded`. `PROTOCOL_VERSION = 2`.
- `crates/paint`: `wire::{encode_display_list, decode_display_list}` with limits enforced during decoding, next to `validate`; the fuzz target `display_list_decode` (decode → validate).
- `apps/chromelight`: `browser/{mod, core, frames, present, subresources, restart, children, headless, window}.rs`, `renderer.rs`, `gpu.rs`, `exit.rs`. Frame state (`seq`, stale frames, restarts) is pure structures with unit tests, separate from winit.
- New dependencies: `winit` 0.30.13 (Apache-2.0), `softbuffer` 0.4.8 (MIT OR Apache-2.0); rows in `docs/DEPENDENCIES.md`, `cargo deny check`. No platform `cfg` (winit/softbuffer are cross-platform). `ipc-channel` ≥ 0.23.1 (ADR-0016 fact 3).

## 7. Testing

- **Parity (the main M1b test):** `chromelight --no-sandbox --headless --png out.png <page>` for all 44 pages of the 22 reftest pairs gives pixels identical to `cl-testshell render` of the same page, on three OSes in CI. Plus resize in headless (`--headless --resize-to WxH` after the first frame) against the testshell at the same viewport.
- **Hostile renderer and gpu:** a test per `--renderer-test-mode` / `--gpu-test-mode`, each asserting the §5 reaction.
- **Crashes:** gpu restart delivering a frame; a renderer crash does not take the browser down.
- **Unit:** `Validate` of every message, `copy_bounded`, the frame check, pixel conversion and scaling, the `seq` state machine, the restart policy, the subresource policy.
- **Fuzz:** `ipc_decode` extended to v2 messages; new `display_list_decode`; 60 s each without crashes; the CI loop updated.
- **Window:** `chromelight --no-sandbox --exit-after-first-present <page>` in CI on three OSes (Linux under `xvfb-run`); a manual check by the owner on macOS (window, resize, Retina).
- **Memory:** `tools/bench/mem.sh` records browser/renderer/gpu RSS on an empty page and on the smoke page; numbers in `docs/history/`.

## 8. M1b exit criteria

1. `chromelight --no-sandbox file:///page.html` shows the page in a window through the renderer + GPU processes; resize re-lays out the page (owner, macOS) and `--exit-after-first-present` is green on three OSes in CI.
2. Parity 44/44 reftest pages (and the resize case) with `cl-testshell` on three OSes in CI.
3. The display list goes over shm; the gpu does copy-then-validate; every hostile mode follows §5, the gpu stays alive.
4. Gpu crash → restart and a frame; renderer crash → the browser lives, error in the window.
5. Fuzz `ipc_decode` (v2) and `display_list_decode` 60 s each without crashes; the CI loop updated.
6. Per-process RSS recorded in `docs/history/`.
7. ADR-0016 Accepted; `SECURITY.md`, `ARCHITECTURE.md`, `SPEC_REGISTRY.md`, `DEPENDENCIES.md`, `MEMORY.md` updated.

## 9. Risks

| Risk | Signal | Action |
|---|---|---|
| A window cannot be created on a CI runner | `--exit-after-first-present` fails on one OS | xvfb/runner setup; if impossible — a ruling and a manual check on that OS, recorded in the ledger |
| `ipc-channel` behaves differently on Windows (shm/handles) | integration tests red only on windows-2022 | fix the engine, not the test (as `to_file_path` in M1a); stop if it cannot be fixed |
| Parity breaks through non-determinism | 3-process and single-process pixels differ | a bug in the engine or the transport; never loosen the comparison |
| Frequent resize `seq` changes flicker | manual check | coalescing resize events, showing the last good frame |
