# chrome-light architecture

Document version: 2026-09-07. Source of decisions — `docs/adr/`. This is the coherent picture.

## 1. Boundaries and terminology

- **Product (browser)** — windows, tabs, omnibox, profiles, history/bookmarks/passwords, downloads, permissions, sync, import from Chrome, extensions, DevTools, updates.
- **Engine** — everything from URL to pixels: network, HTML/DOM, CSS/style, layout, paint, compositor, GPU, Web IDL bindings, Web API.
- **JS/Wasm VM** — V8 (crate `v8`), wrapped by `cl-js`. DOM is not part of the VM: bindings belong to the engine.
- **Platform services** — OS abstraction, processes, sandbox, IPC, storage, accessibility.

Rule: Chromium ≠ Blink ≠ V8. For us: chrome-light ≠ cl-engine ≠ V8.

## 2. Process model (ADR-0005)

```
┌──────────────────────────────── browser process (privileged) ────────────────────────────────┐
│ cl-browser  cl-shell-ui  cl-storage  cl-chrome-import  cl-sync  cl-extensions(host)  cl-devtools │
│ cl-process (spawn, sandbox policy, crash recovery)   cl-ipc (broker, capability handles)         │
└───────┬─────────────────────┬──────────────────────────┬──────────────────────┬────────────────┘
        │                     │                          │                      │
 ┌──────▼──────┐       ┌──────▼──────┐            ┌──────▼──────┐        ┌──────▼──────┐
 │ renderer    │  ...  │ renderer    │            │ network     │        │ gpu         │
 │ site A      │       │ site B      │            │ cl-net      │        │ cl-gfx      │
 │ (sandboxed) │       │ (sandboxed) │            │ (sandboxed, │        │ cl-compositor│
 │ cl-renderer │       │             │            │  weaker)    │        │ (sandboxed, │
 └─────────────┘       └─────────────┘            └─────────────┘        │  weaker)    │
        utility processes: image decode, font parse, media demux/decode  └─────────────┘
```

**Principles:**

1. The browser process does not parse untrusted bytes. Ever. Even a favicon is decoded in a utility process.
2. Renderer — one per **site** (scheme + eTLD+1) within a profile; cross-site iframe — out-of-process (OOPIF) starting with M4. Before M4 a cross-site iframe renders in the same process, but this is a documented limitation and a blocker for the public beta.
3. The renderer has no: sockets, files, keychain, clipboard-write without user gesture, access to other renderers. Everything goes through capability handles from the broker.
4. The network process holds session TLS keys, cookies, cache. The renderer gets only a `FetchHandle` for a specific request, with CORS/CSP/cookie policies already applied and checked in the browser process.
5. The GPU process receives display lists / command buffers via shared memory; validates everything; GPU process crash → restart without losing tabs.
6. Any child may crash: the browser process shows "page crashed" and restarts it. Crash dump without sensitive data (ADR-0005 §crash).

**Sandbox per platform** (cl-process):

| OS | Mechanism | Notes |
|---|---|---|
| macOS | `sandbox_init` (seatbelt profiles, SBPL) + separate user-less process, entitlements | profiles per process type in `cl-process/sandbox/macos/*.sb` |
| Linux | user+pid+net namespaces, seccomp-bpf allowlist, `no_new_privs`, chroot-into-empty-dir | fallback without user-ns → refuse to load untrusted content, not a "silent" mode |
| Windows | restricted token + job object + AppContainer + Win32k lockdown (`ProcessSystemCallDisablePolicy`) | the hardest one; a separate owner-milestone |

## 3. IPC (cl-ipc)

- Transport: `ipc-channel` (unix domain sockets / named pipes) for messages; shared memory (`cl-platform::shm`) for frames, display lists, large resources.
- Schema: messages — Rust enums with `serde` + `postcard`; **every** type has `validate(&self, ctx: &ReceiverCtx) -> Result<(), IpcViolation>`; protocol version in the handshake.
- Capabilities: non-transferable `Handle<T>` are issued by the broker; the renderer cannot "construct" a handle. A handle carries origin/site and a lifetime.
- Async only. The only permitted sync channel — renderer→browser for `window.alert`-class modal operations, and even that one has a timeout.
- IPC decoders — fuzz-target from day one (`fuzz/ipc_*`).

## 4. Document pipeline (renderer process)

```
FetchHandle bytes ──► encoding sniff ──► cl-html (html5ever tokenizer/tree builder)
        │                                           │ sink
        │                                           ▼
        │                                        cl-dom (Node arena, events, shadow DOM, mutation records)
        │                                           ▲ ▼ generated bindings (cl-bindings, Web IDL → V8 glue)
        │                                        cl-js (V8 isolate per renderer, context per Window, event loop, tasks/microtasks)
        │                                           │
        ▼                                           ▼
   cl-style: stylo (cascade, computed values, invalidation) ──► ComputedStyle per element
                                                    │
                                                    ▼
   cl-layout: box tree → fragment tree (immutable per pass, LayoutNG-style)
              block/inline (own), flex/grid math (taffy), tables (own), text (parley/swash), fragmentation later
                                                    │
                                                    ▼
   cl-paint: display list (own format), stacking contexts, clips, transforms, effects
                                                    │ commit (shared memory)
                                                    ▼
   ─────────────── gpu process ───────────────
   cl-compositor: property trees (transform/clip/effect/scroll), layerization, tiles, damage, async scroll/animation
   cl-gfx: vello scene → wgpu; CPU fallback (vello_cpu/tiny-skia) for headless/reftests/no GPU
                                                    │
                                                    ▼
   present → window (winit surface) / PNG (testshell)
```

**Invalidation:** style → layout → paint → raster mark only the affected subtrees. A full redraw is allowed only in M1 and is marked `// M1-ONLY: full relayout`.

**Event loop (cl-js + cl-dom):** implementation of HTML §event loop: task sources, microtask checkpoint, rendering opportunity, `requestAnimationFrame`, timers with throttling for background tabs. Workers — separate threads with their own isolates inside the same renderer.

## 5. Network (network process, cl-net)

- URL: crate `url` (WHATWG). DNS: our own async resolver on top of `hickory-resolver` (see DEPENDENCIES). TLS: `rustls` + `rustls-platform-verifier` (system root stores). HTTP/1.1, HTTP/2: `hyper`. HTTP/3: `quinn` + `h3`.
- Fetch (WHATWG): implemented **in the network process** as a state machine: redirects, CORS (including preflight), credentials mode, referrer policy, CSP `connect-src`, mixed content, service worker (later), ORB-like body filtering for `no-cors`.
- HTTP cache: RFC 9111, disk (`cl-storage` cache backend) + memory; the key includes the top-level site (partitioning).
- Cookies: RFC 6265bis: `Secure`, `HttpOnly`, `SameSite`, `__Host-`/`__Secure-` prefixes, CHIPS `Partitioned`. Storage — SQLite in the profile dir, encrypted with a platform key.
- Downloads: in the browser process (policy, UI); bytes — via the network process into a file with a quarantine attribute (macOS `com.apple.quarantine`, Windows MOTW).

## 6. Storage (cl-storage)

A single `StorageKey = (origin, top-level site, ancestor-bit)` as in WHATWG Storage. Backends:

| Data | Backend | Process |
|---|---|---|
| history, bookmarks, prefs, permissions, site data index | SQLite | browser |
| cookies, HTTP cache index | SQLite | network |
| localStorage | SQLite (per StorageKey, async commit; sync API in the renderer via a local cache + IPC) | browser (storage service) |
| sessionStorage | browser process memory, namespace per tab | browser |
| IndexedDB | SQLite (one DB per StorageKey), transactions — RFC-style journal | browser (storage service) |
| Cache Storage | files + SQLite index | browser |
| OPFS | directories in the profile dir with a quota | browser |

Quotas and eviction — per WHATWG Storage. Private mode — memory-only backends with the same interface.

## 7. Product (browser process)

- **cl-browser:** `Tab`, `NavigationController` (history entries, back/forward, bfcache — later), `SiteInstance` assignment, permissions (`Permission` state machine + UI prompts), downloads, session restore, crash recovery, memory pressure policy (freeze → discard → hibernate-to-disk, ADR-0012).
- **cl-shell-ui:** egui on wgpu: tab strip, omnibox (with exact origin display, security state), dialogs, settings. Later — privileged web UI on our own engine (ADR-0008).
- **cl-chrome-import (ADR-0007):** reads the on-device Chrome profile (read-only): `Bookmarks` (JSON), `History` (SQLite, copy under lock), `Login Data` (SQLite + decryption: macOS Keychain "Chrome Safe Storage", Linux libsecret/kwallet/basic, Windows DPAPI for legacy `v10`, while `v20` App-Bound — only via a user CSV export), `Preferences`, `Extensions/` (manifests + CRX id → reinstall from the Web Store), `Web Data` (autofill). "Mirror" mode: file watcher + periodic diff, Chrome → us only.
- **cl-sync (ADR-0007):** Chromium sync protocol client (`components/sync/protocol/*.proto` → `prost`), types: bookmarks, history, passwords, preferences, tabs, extensions. Server `cl-sync-server` (axum + SQLite/Postgres), self-hosted; encryption — passphrase-derived key on the client (like Brave/custom passphrase in Chrome).
- **cl-extensions (ADR-0011):** MV3: manifest parse, permissions model, service worker background, content scripts in an isolated world (separate V8 context in the renderer), `chrome.*` API host-side in the browser process with validation; declarativeNetRequest in the network process; CRX3 verify + install from the Chrome Web Store update URL.
- **cl-devtools:** CDP-compatible server (domains Runtime, Debugger via V8 Inspector, DOM, CSS, Network, Page, Log, Target). Frontend — Chrome DevTools frontend (BSD) as a separate download, or our own minimal one.
- **updater:** signed packages (ed25519 + OS code signing), staged rollout, rollback. Without it — no public beta.

## 8. Crate map

```
Cargo.toml (workspace, [workspace.lints], [workspace.dependencies])
crates/
  platform/      cl-platform     threads, clocks, files, shm, keychain, quarantine, fonts discovery (unsafe allowed)
  process/       cl-process      spawn, sandbox policies, crash handling (unsafe allowed)
  ipc/           cl-ipc          schema, validation, capability broker
  net/           cl-net          fetch state machine, http stack, cache, cookies
  html/          cl-html         html5ever integration → DOM sink, encoding sniffing
  dom/           cl-dom          nodes, events, ranges, shadow DOM, mutation, custom elements
  style/         cl-style        stylo integration, invalidation, computed style access
  layout/        cl-layout       box/fragment trees, formatting contexts, text, tables
  paint/         cl-paint        display list format and builder, hit testing
  compositor/    cl-compositor   property trees, layers, tiles, damage, async scroll
  gfx/           cl-gfx          vello/wgpu backend, CPU backend, gpu process main (unsafe allowed)
  js/            cl-js           V8 embedding, JsRuntime trait, event loop (unsafe allowed)
  bindings/      cl-bindings     Web IDL parser + codegen (build-time), runtime glue
  webapi/        cl-webapi       Fetch API, Storage APIs, Workers, Canvas, timers… (feature-gated modules)
  storage/       cl-storage      SQLite backends, quota, StorageKey
  a11y/          cl-a11y         DOM → accesskit tree
  renderer/      cl-renderer     renderer process main
  browser/       cl-browser      browser process core
  shell-ui/      cl-shell-ui     egui chrome UI
  chrome-import/ cl-chrome-import
  sync/          cl-sync         sync client
  sync-server/   cl-sync-server  self-hosted server binary
  extensions/    cl-extensions   MV3 runtime
  devtools/      cl-devtools     CDP server
  testshell/     cl-testshell    headless deterministic shell (WPT product, reftests, PNG)
apps/
  chromelight/                   main binary (browser process entry; child processes — same binary with `--type=`)
tools/
  wpt/           wptrunner product adapter, expectations metadata
  test262/       harness
  bench/         memory/perf harness, corpus
  fuzz/          cargo-fuzz targets
  chrome-corpus/ curated site list for compat acceptance
docs/
```

**Status as of 2026-09-07 (M0):** existing: `cl-platform`, `cl-ipc`, `cl-process`, `cl-testshell`, `apps/chromelight` binary, `tools/fuzz`, `tools/bench`; the remaining crates — per plan, M1+.

Inter-crate dependency rule (checked by `cargo deny` bans + `tools/check-deps.sh`): `cl-dom` does not depend on `cl-layout`; `cl-layout` does not depend on `cl-js`; nothing in the renderer depends on `cl-browser`; `cl-platform` — a leaf.

## 9. Memory budgets (ADR-0012, hypotheses pending measurement)

| Metric | v1 target | Chrome 140+ reference (from public sources, 2026) |
|---|---|---|
| Empty browser, 1 empty tab | ≤ 120 MB RSS total across processes | ~300–400 MB |
| Typical news page, active | ≤ 70 MB per renderer | 150–300 MB |
| 10 active tabs | ≤ 600 MB total | ~1.4 GB |
| Background tab after freeze | ≤ 15 MB (heap snapshot to disk, V8 isolate disposed) | "up to 80% less" after discard |
| Time to first frame (cold start) | ≤ 400 ms on M1 | — |

Tactics: one isolate per renderer with lazy context; V8 flags for background (`--lazy`, `--optimize-for-size`, no sparkplug/turbofan in background); shared glyph/image caches in the GPU process; hibernate tabs to disk (DOM+state serialization); no redundant service processes (network in-process is **forbidden** — but GPU and network — one each per profile, not per window).

Measurement: `tools/bench` — fixed corpus, RSS/PSS per process, `cargo bench` + CI gate on regression >5%.

## 10. Observability

`tracing` in all crates; structured events; `tracing-chrome` export for Perfetto UI; crash handling — minidump via `minidumper`/`crash-handler` (Rust), without page contents; feature flags via `cl-browser::flags` (compile-time + runtime toggles).

## 11. What we revisit as we grow

- OOPIF and Site Isolation granularity (origin vs site) — M4.
- Own JS VM — not before v2, and only if V8 constrains the memory budget or the sandbox.
- Privileged web UI instead of egui — M5.
- Service workers, bfcache, WebGPU, WebRTC — per feature matrix.
