# Plan: ChromeLight milestone program

Version: 2026-09-07. This is the **program** (what, in which order, exit criteria). Detailed executable plans — one per milestone or sub-milestone in `docs/superpowers/plans/YYYY-MM-DD-<name>.md` (format: tasks → tests → commits). First: `2026-09-07-m0-foundation.md`.

Estimates — **order of magnitude in "agent-weeks"** (one week of the owner's work with AI agents), not promises. Each milestone closes only when **all** exit criteria are met on **three OSes**; exceptions are recorded in MEMORY.md.

## 0. Sequencing principles

1. Security and infrastructure before features: processes/IPC/sandbox/tests — M0–M2, before the open Web.
2. Every milestone ends with a working binary that can be launched and measured.
3. Correctness → measurement → optimization. The reference path (CPU raster, single-thread layout) is kept for differential tests.
4. Vertical slice first (one page end-to-end), then breadth (feature matrix).
5. Chrome interop (import, sync, extensions, DevTools) — after a stable core, but the API boundaries for it (StorageKey, capability handles, isolated worlds) are laid down earlier.

Python 3 is a build dependency from M1a on (stylo).

## 1. Milestone map

```
M0 Foundation ─► M1 Static pages ─► M2 Secure + JS ─► M3 Interactive platform
                                                            │
                                    M4 Isolation + Storage ◄┘
                                            │
                                    M5 Browser product ─► M6 Chrome interop ─► Alpha
```

| M | Name | Gist | Order of magnitude |
|---|---|---|---|
| M0 | Foundation | workspace, CI on 3 OSes, cl-platform, cl-ipc, cl-process (spawn+handshake, sandbox type-state), testshell PNG, bench script | 2–3 wk |
| M1 | Static pages | URL/encoding, html5ever→cl-dom, stylo→cl-style, block/inline layout, text, paint→display list, CPU raster in GPU-process, egui shell with one tab, `file://`+localhost, WPT adapter, sandbox macOS/Linux | 8–12 wk |
| M2 | Secure + JS | network process (rustls/hyper h1/h2, Fetch, cache, cookies), sandbox Windows, V8 via cl-js, Web IDL codegen, event loop, basic DOM API, `fetch`/XHR, site-per-process assignment, crash reporting, Gate S0 → first open HTTPS site | 10–14 wk |
| M3 | Interactive platform | events/forms/focus/IME/selection, flex/grid/tables, incremental invalidation, dedicated workers, Canvas 2D, SVG, images in utility process, a11y tree, WebSocket | 10–14 wk |
| M4 | Isolation + storage | OOPIF, compositor async scroll/animations, freeze/discard, localStorage/IndexedDB/Cache Storage/OPFS/quota, partitioning, HTTP/3, shared workers, memory budgets confirmed | 8–12 wk |
| M5 | Browser product | tabs/omnibox/history/bookmarks/downloads/permissions/cert UI, profiles, private mode, session restore, passwords/autofill, hibernate, signed updater, l10n en/ru, service workers, basic media playback | 10–14 wk |
| M6 | Chrome interop | Chrome profile import/mirror, sync client + server, MV3 runtime + Web Store install, CDP subset + DevTools frontend, WebDriver BiDi; Gate S1 → **Alpha** | 10–14 wk |

Total — on the order of **60–85 agent-weeks** to alpha. This is an optimistic order of magnitude; the compatibility long tail lives on after alpha indefinitely.

## 2. Milestones in detail

### M0 — Foundation

**Goal:** the skeleton all other crates build on; multiprocess from the first commit; CI on three OSes.

Scope: root workspace + lints + `deny.toml`; `cl-platform` (ProcessType, Clock, paths); `cl-ipc` (messages, postcard codec with limits, `Validate`, bootstrap via ipc-channel, handshake, fuzz `ipc_decode`); `cl-process` (spawn same-binary child, timeout, `Sandbox<Unapplied→Applied>`, `NotImplementedPolicy`/`DevNoSandbox`); `apps/chromelight` (`--type`, browser↔renderer ping/pong, tracing, `--trace-out`); `cl-testshell` (`render` → white PNG, `compare`); `tools/bench/mem.sh`; CI workflow; check scripts (AGENTS.md sync, platform cfg).

Exit criteria:
- [x] `cargo build/test/clippy -D warnings/fmt` green on macOS, Windows, Linux in CI; `deny`/`doc`/scripts — on Linux (platform-independent).
- [x] `chromelight --exit-after-handshake --no-sandbox` completes the browser↔renderer handshake on 3 OSes (integration test).
- [x] Release binary refuses to spawn a renderer without a sandbox (`NotImplementedPolicy`) — test.
- [x] `cargo fuzz run ipc_decode` runs 60 s without crashes.
- [x] `cl-testshell render` writes an 800×600 PNG; `compare` returns 0/1.
- [x] `tools/bench/mem.sh` writes JSON with RSS per process; in `docs/history/bench-2026-09.md` — benchmark script ready.
- [ ] Chrome 153 baseline on the owner's machine (manual measurement) — owner.

Detailed plan: `docs/superpowers/plans/2026-09-07-m0-foundation.md`.

### M1 — Static pages

**Goal:** `chromelight file:///page.html` renders a static page with CSS through the real pipeline in three processes; reftests and the WPT subset are live.

Scope: split into five sequential sub-plans (M1a–M1e), each tied to one critical system.

| Sub-plan | What it does | Detailed plan |
|---|---|---|
| **M1a** | Single-process static pipeline: encoding sniff → html5ever → arena DOM → stylo cascade → block/inline layout (parley) → display list → CPU raster (tiny-skia). JS-free conformance (html5lib-tests, WPT `urltestdata.json`), 22 reftest pairs, 6 fuzz targets, golden dumps. | `docs/superpowers/plans/2026-09-07-m1a-static-pipeline.md` |
| **M1b** | Renderer + GPU processes: display list over shm, validation on receipt, frame in a window. | written after M1a |
| **M1c** | Sandbox: macOS seatbelt, Linux Landlock + seccomp + `no_new_privs`; `--probe` tests. | written after M1b |
| **M1d** | egui shell: one tab, address bar, `file://` and `http://localhost` via `cl-net`. | written after M1c |
| **M1e** | Bench corpus, Chrome baseline, closing the M1 exit gates. | written after M1d |

M1 exit criteria (union of M1a–M1e):
- [x] M1a: single-process pipeline; html5lib tree-construction 1307/1313 = 99.5% over 36 vendored `.dat` files (0 panics, threshold was ≥ 90%; the SVG/MathML files are not vendored — see SPEC_REGISTRY), WPT `urltestdata.json` 828/893 = 92.7% (threshold ≥ 92%, the ceiling of the `url` crate, see SPEC_REGISTRY), 22/22 reftests (`cargo run -p cl-testshell -- reftest`; the 20 from the brief plus a regression pair for the whitespace anonymous-block bug fixed in Task 23 and `max-width-auto-margins` for the min/max clamp under `width:auto`, CSS 2.1 §10.4 — M1a final review), 6 fuzz targets (the plan named 4 M1a targets — `html_parse`, `css_stylesheet`, `url_parse`, `display_list_validate` — plus `ipc_decode` from M0 and `display_list_rasterize`, added by the final review; no crash in 60 s each), 5-stage golden dumps (dom/style/box-tree/fragments/display-list) — detailed criteria in `docs/superpowers/plans/2026-09-07-m1a-static-pipeline.md`.
  - [ ] Not met: peak RSS on the 1 MB smoke page ≈ 330 MiB against the "< 300 MB" criterion (`docs/history/perf-m1a-2026-09.md`) — do not tick, debt in MEMORY.md.
  - [ ] Not met: tests in the release profile (`cargo test --release`) were never run locally — the stylo LTO rebuild (`lto = "thin"`, `codegen-units = 1`) is too slow on the owner's machine — do not tick, debt in MEMORY.md.
- [ ] M1b: `chromelight file:///page.html` renders through renderer + GPU processes (display list over shm).
- [ ] M1c: the renderer sandbox is applied on macOS and Linux; `--probe` tests (open /etc/passwd, socket) → EPERM.
- [ ] M1d: egui shell with one tab, `file://` and `http://localhost`.
- [ ] M1e: budget ≤ 120 MB RSS (empty browser + empty tab), Chrome baseline on the corpus recorded.
- The wptrunner product adapter moved to M2 (testharness.js needs JS; the current wptrunner expects WebDriver classic from the product).

### M2 — Secure + JS

**Goal:** the first **arbitrary HTTPS site** opens behind the sandbox on all three OSes; JS works.

Scope: `cl-net` full network process (rustls + platform verifier, hyper h1/h2, Fetch state machine with redirects/CORS/credentials, HTTP cache RFC 9111 memory+disk, cookie store RFC 6265bis, mixed content, HSTS preload); `cl-js` (V8 isolate/context, JsRuntime trait, event loop tasks/microtasks/timers); `cl-bindings` (Web IDL parser → codegen; Node/Element/Document/Event/Window/console/setTimeout/fetch/XHR minimum); `cl-webapi::fetch`; DOM mutation from JS → style/layout invalidation (full redraw allowed, `M2-ONLY`); `cl-browser::site` (SiteInstance, site-per-process assignment); Windows sandbox; crash dumps (minidumper); Test262 qualification set; WPT `fetch`, `xhr`, `dom/events`, `html/webappapis`.

Exit criteria:
- [ ] Gate S0 met on 3 OSes; `--no-sandbox` does not exist in release.
- [ ] Open and render without first-level JS errors: example.com, wikipedia.org (article), news.ycombinator.com, mdn (a doc page), github.com README page — curated corpus v0 (5 sites) with a checklist.
- [ ] WPT: `fetch/api` ≥ 60%, `xhr` ≥ 60%, `dom/events` ≥ 70%, `html/webappapis/scripting` ≥ 60%.
- [ ] Test262 qualification set 100% (V8) — harness works.
- [ ] Renderer crash → tab restarts, browser process stays alive (integration test with `--crash-renderer-after=…`).
- [ ] Memory: active corpus v0 page ≤ 70 MB renderer (or revisit ADR-0012 with numbers).
- [ ] V8 updated to the current Chrome version via `tools/v8-bump.sh` at least once.

### M3 — Interactive platform

**Goal:** sites you can interact with: forms, clicks, scroll, scrolling, flex/grid layout of modern pages.

Scope: UI Events/Pointer Events, focus/tab order, forms (input/textarea/select/checkbox/radio, submit, validation), IME/composition via winit, selection/clipboard (copy with gesture), scrolling (main-thread), flex/grid via taffy + baseline/intrinsic sizing, tables, `position: sticky/fixed`, incremental style/layout/paint invalidation (removal of all `M2-ONLY`), dedicated workers, Canvas 2D (tiny-skia/vello), inline SVG (resvg approach + DOM), images AVIF/WebP in utility process, MutationObserver, Shadow DOM + custom elements, WebSocket, `cl-a11y` accesskit tree, WPT `css-flexbox/css-grid/css-position/css-text`, `pointerevents`, `uievents`, `html/semantics/forms`, `workers`, `custom-elements`, `shadow-dom`.

Exit criteria:
- [ ] Corpus v1 (15 sites, including login forms and React/Vue SPAs): "open, click, fill in, submit, scroll" scenarios pass per checklist.
- [ ] WPT: `css-flexbox` ≥ 70%, `css-grid` ≥ 50%, `html/semantics/forms` ≥ 50%, `workers` ≥ 60%, `shadow-dom` ≥ 70%.
- [ ] Incremental relayout: 1000 DOM mutations on a 10k-node page ≤ 16 ms/frame on average on M1 (bench).
- [ ] Screen reader (VoiceOver) reads headings/links/forms of a test page.

### M4 — Isolation + storage

**Goal:** security and memory at the level of the architectural promises: OOPIF, async compositor, freeze/discard, full storage.

Scope: OOPIF (RemoteFrame in the renderer, surface embedding in the GPU process, input routing), COOP/COEP/CORP, ORB, `cl-compositor` property trees/tiles/async scroll/transform-opacity animations, vello/wgpu main path + CPU fallback, freeze/discard policy, `cl-storage` (localStorage sync-API via local cache + async commit, IndexedDB on SQLite, Cache Storage, OPFS, quota/eviction, private mode), storage partitioning, shared workers, HTTP/3 (quinn/h3), WPT `storage`, `IndexedDB`, `webstorage`, `html/browsers/origin`, `cross-origin-*`, `css-transforms`, `css-animations` subset.

Exit criteria:
- [ ] Cross-site iframe lives in a separate renderer; `tests/security/site_isolation.rs` — renderer A does not receive site B's responses (ORB) and cannot access its storage.
- [ ] Long-page scroll at 60 fps with a busy main thread (bench with busy-loop JS).
- [ ] 10 corpus v1 tabs ≤ 600 MB total; background frozen ≤ 25 MB (ADR-0012).
- [ ] WPT `IndexedDB` ≥ 70%, `webstorage` ≥ 90%, `storage` ≥ 70%.
- [ ] Revisit ADR-0004 (V8 share of memory) and ADR-0006 (vello fps) — decisions recorded.

### M5 — Browser product

**Goal:** a browser fit for the owner's daily use (dogfooding) with signed updates.

Scope: `cl-browser` full (tabs, NavigationController, history, bookmarks, downloads with quarantine, permissions state machine + prompts, cert interstitials, session restore, profiles, private mode, memory pressure policy + hibernate), passwords/autofill (platform keystore encryption), `cl-shell-ui` full egui (tab strip, omnibox with origin display, settings, downloads, history/bookmarks manager), l10n en/ru, service workers (`cl-webapi::sw` + Cache Storage), basic `<video>/<audio>` (utility decode, MSE later), updater (ed25519 + OS signing, staged rollout, rollback), installers (dmg notarized, msi/msix, AppImage/deb), `tools/rename-checklist.md`, revisit ADR-0008 (privileged web UI).

Exit criteria:
- [ ] The owner uses ChromeLight as the main browser 5 days in a row; issue log ≤ 20 blockers.
- [ ] Updater drill: version N → N+1 → rollback passed on 3 OSes.
- [ ] Corpus v2 (30 sites, including a YouTube page without DRM, Gmail login up to 2FA, Google Docs viewing) — checklist.
- [ ] Hibernated tab ≤ 5 MB RAM and restores offline.
- [ ] WPT `service-workers` ≥ 50%.

### M6 — Chrome interop → Alpha

**Goal:** a Chrome user migrates without losses; extensions; DevTools; sync.

Scope: `cl-chrome-import` (profile parsers, decryption on macOS/Linux/Windows-v10, mirror via watcher), `cl-sync` + `cl-sync-server` (pinned sync.proto, E2E encryption, types bookmarks/history/passwords/preferences/tabs/extensions), `cl-extensions` (MV3 manifest/permissions, CRX3, Web Store install, background SW, content scripts isolated world, `chrome.runtime/storage/tabs/scripting/declarativeNetRequest/action/contextMenus/webNavigation/cookies/alarms/notifications`), `cl-devtools` (CDP: Runtime/Debugger via V8 Inspector, DOM, CSS, Network, Page, Log, Target; Chrome DevTools frontend), WebDriver BiDi endpoint, Gate S1 (external security review, IPC fuzz ≥ 30 days, disclosure policy).

Exit criteria:
- [ ] Import of the owner's Chrome profile: bookmarks/history/passwords (macOS)/settings/extension list — verified manually; fixture tests on 3 OSes.
- [ ] Sync between two of the owner's devices via the self-hosted server: bookmarks and tabs converge in ≤ 30 s.
- [ ] Extension corpus: uBlock Origin Lite, Bitwarden, Dark Reader, Vimium, React DevTools — work per checklist.
- [ ] Chrome DevTools frontend connects: Elements, Console, Sources (breakpoints), Network.
- [ ] Gate S1 closed → **Alpha** for technical users (macOS/Linux/Windows), disclaimer "not affiliated with Google".

## 3. Standing rituals (from M2)

| Ritual | Frequency | What |
|---|---|---|
| V8 bump | ≤ 7 days after a Chrome release (every 2 weeks) | `tools/v8-bump.sh`, Test262 qualification, bench |
| Upstream bump (stylo, wgpu, vello, parley, html5ever…) | monthly | separate PR, changelog links, WPT diff |
| Bench vs Chrome | monthly | `docs/history/bench-YYYY-MM.md` |
| WPT full run | nightly | dashboard, regressions into issues |
| Fuzz long | nightly | 30 min, all targets |
| ADR review | on milestone close | review of items marked "revisit" |
| MEMORY.md | every session | log, next step |

## 4. Program risks and stop conditions

| Risk | Signal | Action |
|---|---|---|
| stylo integration with the arena-DOM is not working out | M1 spike > 2 weeks without cascade | fallback = ADR-0015 Option B |
| V8 does not fit the memory budget | M2: idle renderer > 80 MB | `--jitless`/snapshot/lazy; ADR-0004 revisit earlier |
| Windows sandbox | M2 > 3 weeks | Windows moves to the nightly matrix until M3, Windows release is blocked |
| vello on weak GPUs | M4 fps < 30 | CPU tiles + GPU composite |
| Solo velocity below estimates | any milestone > 2× estimate | cut the milestone's feature matrix, not gate quality; record in MEMORY.md |
| ADR-0001 review after 12 mo | WPT focus < 50% or corpus < 30% | consider Servo as a fallback engine for incompatible tabs |

## 5. How a detailed milestone plan is born

1. Read this document, ARCHITECTURE, the subsystem ADRs.
2. Split the milestone into sub-plans per crate (each sub-plan — working, testable software).
3. Write it in the `superpowers:writing-plans` format: files, interfaces, TDD steps with code, commits.
4. Execute via `superpowers:subagent-driven-development` (fresh agent per task + review) or `executing-plans`.
5. On completion — update FEATURE_MATRIX, SPEC_REGISTRY, MEMORY.md; tag `mN`.
