# ADR-0016: M1b — the GPU process as a raster service, the browser presents the frame; display lists and frames over shared memory with copy-then-validate

**Status:** Accepted
**Date:** 2026-09-28
**Deciders:** project owner
**Amends:** ADR-0006 (where and how a frame is rasterised in M1–M3), ADR-0008 (the phrase "egui on the same wgpu device as the compositor")

## Context

M1b splits the single-process M1a pipeline into three processes: browser (privileged), renderer (HTML → display list, sandboxed in M1c) and gpu. Three accepted documents pull in different directions:

- ADR-0006: the compositor and vello/wgpu live in the GPU process; the renderer writes the display list into shm, and the GPU process validates it — that is the trust boundary.
- ADR-0008: the shell UI is egui (`egui-wgpu`) in the browser process, "the same wgpu device as the compositor". If the compositor is in the GPU process and egui in the browser, "the same device" is impossible without moving egui meshes into the GPU process.
- `docs/ARCHITECTURE.md` §2 item 5: the GPU process receives display lists over shm, validates everything, and its crash does not lose tabs.

Facts verified 2026-09-28 and re-verified against `ipc-channel` 0.23.1 on 2026-11-03 (sources: the `ipc-channel` crate sources in the cargo registry, docs.rs, Chromium documentation, GitHub advisories):

1. `ipc-channel` 0.23 has no read-only shared memory on any OS: after `send` the sender keeps a writable mapping (`IpcSharedMemory` is `Clone`, and there is an `unsafe fn deref_mut`). A hostile sender can change the bytes while the receiver reads them (TOCTOU / double fetch). Chromium requires copy-then-validate for this case (`docs/security/mojo.md`, the rule for `BigBuffer`) and has a separate `ReadOnlySharedMemoryRegion`.
2. In `ipc-channel` 0.23 deserialising an `IpcSharedMemory` with a wrong index returns an error (`src/ipc.rs:578-603`), but deserialising an `IpcSender`/`IpcReceiver` with a wrong index **panics** (`src/ipc.rs:1025-1027`, `:1054-1056`, the author's FIXME; unchanged in 0.23.1). The ChromeLight release profile is `panic = "abort"`.
3. The `ipc-channel` receive path is **not robust against a hostile peer** (0.23.1; upstream issue servo/ipc-channel#457 "remote-peer can trigger UB/panic in ipc-channel", open since 2026-09-30):
   - Linux: `platform/unix/mod.rs` takes `fragment_size`/`total_size` from the wire (`:1071-1078`); a datagram of 8–15 bytes whose `fragment_size` equals its length makes `set_len(bytes_read - 16)` underflow (`:1125`, `:1133-1134` — a panic in debug, UB in release); `channels.pop().unwrap()` (`:1150`) panics when no fd is attached; `reserve_exact(total_size - len)` (`:1154`) allocates a peer-chosen size; the shm is an unsealed memfd (`:1212-1214`, mapped `MAP_SHARED`), so a sender that shrinks it during the receiver's copy causes SIGBUS.
   - Windows: `platform/windows/mod.rs:217-218` `expect`s on undecodable out-of-band data; `:969-976` `unwrap`s while mapping a peer-supplied shm handle; `:981-982`, `:1048` reserve a peer-chosen size; `:272-273` opens a process id named by the peer with `PROCESS_DUP_HANDLE` and duplicates handles *from* it into the receiver; `:1349` sending shm or ports requires the sender to hold `PROCESS_DUP_HANDLE` on the receiver — a sandbox-escape primitive once Windows children are sandboxed.
   - All OSes: `IpcBytesReceiver::recv` (`src/ipc.rs:928-935`) drops any channel ends attached to a byte message, and `Drop for OsOpaqueIpcChannel` is `debug_assert!`-only (`platform/macos/mod.rs:599-604`, `platform/unix/mod.rs:665`): one extra port/fd panics the receiver in debug and `release-dev` builds and leaks a port/fd per message in release. No backend caps the receive size before allocating (macOS copies a peer-sized out-of-line payload, `platform/macos/mod.rs:815-817`), so our own size checks run after the allocation.
   - macOS: shm is sent as `MACH_MSG_VIRTUAL_COPY` (a snapshot — no TOCTOU on macOS). 0.23.0 trusted the descriptor layout of a received message (GHSA-fj85-w6rj-rxrc, high: a sender could make the receiver read and deallocate arbitrary memory); 0.23.1 parses and rejects a malformed message. A forged `MACH_NOTIFY_NO_SENDERS` id (`:786-788`) still fakes a disconnect and leaks the message. The workspace requires 0.23.1 or newer.

   These do not block M1b: no child is sandboxed yet and Gate S0 keeps untrusted content out until it is. They are **known gaps for every child → browser channel**: from M1c a sandboxed Linux child (renderer or gpu) could crash or corrupt the privileged browser through them, and the Windows items apply once Windows children are sandboxed. They are tracked in `docs/SECURITY.md` ("Known gaps") and must be closed — upstream fixes, a patched transport, or our own upward transport, decided in a separate ADR — before Gate S0 (M2), i.e. before the first untrusted content reaches a renderer (owner decision, 2026-11-03: a memory corruption in the browser is not contained by any sandbox, so the public-alpha gate S1 is too late).
4. `winit` 0.30.13 is the current stable (0.31 is in beta), `softbuffer` 0.4.8 does CPU presentation without a GPU driver, `wgpu` 30.0.1, `egui` 0.36.2 — all compatible through `raw-window-handle` 0.6.
5. Chromium without a GPU presents frames as a `SharedBitmap` in shared memory (software compositing); Firefox in some configurations composites on a parent-process thread without a GPU process. Cross-process presentation (macOS `CALayerHost` + `IOSurface`, Windows DirectComposition/child HWND, Linux X11/Wayland) is platform work on every OS.

The owner chose on 2026-09-28 (AskUserQuestion): the browser puts the frame into the window via softbuffer; the GPU process rasterises on the CPU in M1b (tiny-skia, the existing `cl-gfx`); M1b includes resize, GPU-process restart, renderer-crash handling, and per-process RSS.

## Decision

1. **In M1–M3 the GPU process is a raster service.** It receives the display list from the renderer, copies the bytes out of shm into its own buffer, decodes them with limits, validates them with `cl_paint::validate` against the surface size **received from the browser** (not from the renderer), and rasterises with `cl_gfx::cpu::rasterize`. No process loads a GPU driver in M1b. vello/wgpu and the compositor arrive in M4, as in ADR-0006; a separate ADR for M4 decides zero-copy (IOSurface / DXGI shared handle / dmabuf) and moving presentation into the GPU process.
2. **The browser presents the frame.** The browser owns the window (winit 0.30) and the softbuffer surface. The GPU process hands over pixels via shm; the browser checks the frame size against the requested one and copies the pixels into the window buffer. The browser does not parse pixels — it only copies and converts the format. Until M4, ADR-0008's "the same wgpu device as the compositor" reads as: in M1d egui draws in the browser process; where the device lives and who presents after M4 is decided by the M4 ADR.
3. **Transport.** Upward messages (child → browser, untrusted sender) are `postcard` over `ipc-channel` byte channels (as in M0) with `Validate` at the receiver. Downward messages (browser → child, trusted sender) travel on one typed `ipc-channel` channel per child and may carry strings, vectors, shm and channel ends. Upward bulk data (the display list renderer → gpu, frame pixels gpu → browser) is `IpcSharedMemory` carried by a typed message that holds only `IpcSharedMemory` and fixed-size fields; downward bulk data (document and subresource bytes) is `IpcSharedMemory` inside the downward messages.
4. **Copy-then-validate.** For any shm from a less trusted process (renderer → gpu, gpu → browser) the receiver first checks the length against a limit, then copies it into its own memory, and only then decodes or uses it. Validating against the mapping is forbidden.
5. **Channel ends only travel downward.** `IpcSender`, `IpcReceiver`, `IpcBytesSender`, `IpcBytesReceiver` values are sent only browser → child. A less trusted process never sends a typed message containing channel ends (fact 2: panic → abort). The only exception is the bootstrap message a freshly spawned child sends before it has loaded any untrusted content. **Channel ownership:** the process that receives shm on a channel creates that channel — the renderer creates its downward channel; the gpu creates the display-list channel (sent up in its bootstrap, relayed down to the renderer by the browser); the browser creates the frame channel and sends its sender down to the gpu.
6. **Subresources through the browser.** The renderer does not read files: `<link rel=stylesheet>` and other subresources of the document are requested from the browser in one batch (one round trip per document); the browser applies the policy and answers through shm. The renderer waits for the answer with a timeout; the browser answers asynchronously and never waits on the renderer. This is the only renderer → browser path where the renderer waits for an answer; it is bounded by the timeout and the request count, and in M2 it is replaced by the network process with a `FetchHandle` (ADR-0005). **Policy:** a subresource URL is served only if its scheme is `file:`, it has no remote host, its canonical path lies under the canonical directory of the top-level document, and it is a regular file; at most 64 URLs per document, each URL at most 8 KiB, at most 64 MiB in total per document; one batch per `LoadDocument`; reads run on a worker thread. A refusal is a warning in the page, never a render error.
7. **The browser sets the frame size.** The browser tells the GPU process the surface size and the frame number (`seq`) directly. The GPU process drops a display list with a stale `seq`, holds at most one future one (latest wins), and validates against its own size; the renderer cannot make the GPU process allocate a pixmap of arbitrary size.
8. **HiDPI in M1b.** The page is rasterised at DPR 1 in logical window pixels; the browser scales the frame by an integer factor (nearest) for the window's scale factor. Real DPR raster is a separate task after M1b.
9. **Bad-message policy (owner decision, 2026-09-28).** A malformed or hostile message from the renderer to the browser, or a malformed/invalid display list at the gpu (which reports the reason to the browser), makes the **browser kill the renderer**: an error fill in the window and a logged `BadMessageReason` code (stable numbers, never reused; modelled on Chromium `content/browser/bad_message.h`). A bad frame or message from the gpu to the browser → the browser kills and restarts the gpu. Benign races (a stale `seq`, a message for a dead gpu generation, a channel closing) are dropped silently. There is no strike counter. Evidence (code read on each engine's main branch, 2026-09-28):

   | Engine / receiver | On a bad message | Source |
   |---|---|---|
   | Chromium browser | kills the renderer (`RESULT_CODE_KILLED_BAD_MESSAGE`), never crashes itself; reason in UMA and a crash key | [bad_message.cc](https://github.com/chromium/chromium/blob/main/content/browser/bad_message.cc), [mojo.md](https://chromium.googlesource.com/chromium/src/+/main/docs/security/mojo.md) |
   | Chromium GPU (Viz) | resets the frame-sink connection; the renderer stays alive and reconnects | [compositor_frame_sink_support.cc](https://github.com/chromium/chromium/blob/main/components/viz/service/frame_sinks/compositor_frame_sink_support.cc) |
   | Firefox parent | `KillHard(reason)` on `IPC_FAIL`; only `MsgDropped` is ignored | [ContentParent.cpp](https://github.com/mozilla-firefox/firefox/blob/main/dom/ipc/ContentParent.cpp), [MessageChannel.cpp](https://github.com/mozilla-firefox/firefox/blob/main/ipc/glue/MessageChannel.cpp) |
   | WebKit UI / GPU process | terminates the WebContent process; the GPU process asks the UI process to do it | [WebProcessProxy.cpp](https://github.com/WebKit/WebKit/blob/main/Source/WebKit/UIProcess/WebProcessProxy.cpp), [GPUConnectionToWebProcess.cpp](https://github.com/WebKit/WebKit/blob/main/Source/WebKit/GPUProcess/GPUConnectionToWebProcess.cpp) |
   | Servo constellation | logs and drops | [constellation.rs](https://github.com/servo/servo/blob/main/components/constellation/constellation.rs) |

   We take the Chromium-browser / Firefox / WebKit position (assume the sender is compromised, kill it, never crash the privileged side) and the WebKit GPU-process route (the gpu reports, the browser kills), because in ChromeLight only the browser may kill processes.

## Options Considered

### Option A: the GPU process rasterises on the CPU, the browser presents via softbuffer (chosen)
| Dimension | Assessment |
|---|---|
| Complexity | Low–Med: a winit window + softbuffer, shm over `ipc-channel`, the existing `cl-paint`/`cl-gfx` |
| Cost (time) | the lowest of the options with a window |
| Memory/perf | one frame copy over shm per commit (1.9 MB at 800×600); unnoticeable for static pages |
| Security | no process loads a GPU driver; a driver-free GPU process gets a renderer-level sandbox in M1c; the browser receives only fixed-size pixels |
| Compat | determinism: a 3-process render can be compared byte for byte with the single-process one |

**Pros:** deterministic parity with the testshell, no driver in the browser before M1d, minimal platform code. **Cons:** frame copies; softbuffer gets replaced (or stays as a fallback) when egui on wgpu arrives in M1d.

### Option B: the same, but the browser presents via wgpu
**Pros:** M1d adds egui on the same device. **Cons:** a GPU driver in the most privileged process already in M1b; more code and CI risk around surfaces.

### Option C: the GPU process presents (the Chromium Viz model)
**Pros:** the driver is never in the browser; the GPU process draws egui meshes too. **Cons:** cross-process window embedding on three OSes (macOS — `CALayerHost`/`IOSurface`, Wayland — subsurfaces), weeks of work before the first frame.

### Option D: no window in M1b, headless PNG only
**Pros:** less work now. **Cons:** M1b does not show the page on screen; the window and resize move to M1d together with egui.

### Shared memory: `IpcSharedMemory` + copy-then-validate (chosen) vs our own read-only shm in `cl-platform`
Our own shm (a sealed memfd on Linux, a `VM_PROT_READ` copy of the Mach entry on macOS, a `FILE_MAP_READ` duplicate on Windows) gives the receiver a read-only mapping and reusable buffers, but needs `unsafe` and platform code on three OSes. For M1b's static pages the copy is cheaper than that work; for M4 (compositor, animations, 60 fps) the question comes back as a separate ADR.

## Trade-off Analysis

Option A's main gain is security and verifiability at minimal size: no process loads a GPU driver, the GPU process becomes an ordinary consumer of untrusted data with the same sandbox profile as the renderer, and parity with the single-process pipeline is checked byte for byte on all 22 reftest pairs (44 pages). The price is extra frame copies and deferred performance (GPU raster, zero-copy), which static M1 pages do not need and which are planned for M4.

## Consequences

- Easier: M1c gives the GPU process the same sandbox as the renderer (no files, no network, no driver). Parity between the 3-process and the single-process render is a strong regression test for the whole IPC chain.
- Harder: M1d must decide how egui is drawn (wgpu in the browser per ADR-0008 — a driver in the browser) and how the page frame gets into the egui window (a texture from the frame pixels). M4 reworks frame transport to zero-copy.
- Revisit when: animation or compositor scrolling arrives (M3–M4), GPU raster (vello), or `ipc-channel` gets read-only shm and panic-free channel deserialisation.

## Action Items

1. [x] Plan `docs/superpowers/plans/2026-09-28-m1b-renderer-gpu-processes.md` following this ADR and the spec `docs/superpowers/specs/2026-09-28-m1b-renderer-gpu-processes-design.md`.
2. [x] ADR-0006 and ADR-0008: status line "Amended by ADR-0016" (decision text untouched).
3. [ ] `docs/SECURITY.md` §3: rows for the renderer → gpu and gpu → browser boundaries with copy-then-validate and the "channel ends only travel downward" rule (M1b Task 14).
4. [ ] `docs/ARCHITECTURE.md` §2–4: the GPU process in M1–M3 as a raster service, presentation in the browser (M1b Task 14).
5. [ ] For the owner to decide: whether to report the panic on a bad channel index (`src/ipc.rs:1025`, `:1054`, still present in 0.23.1) upstream to `ipc-channel`, proposing `get()` and an error. An external action — only with the owner's consent.
