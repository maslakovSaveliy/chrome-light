# Feature Matrix

Statuses: **planned(Mx)** — in milestone x; **later** — after v1; **non-goal** — not doing it, reason given. Milestones are approximate: M0 foundation/tooling, M1 static pages, M2 sandbox + network + JS, M3 interactive platform, M4 site isolation + storage, M5 product, M6 interop (import/sync/extensions/DevTools). Detailed exit criteria — in the plan (next document).

## Engine

| Area | Feature | Status |
|---|---|---|
| URL/encoding | WHATWG URL, Encoding (UTF-8, legacy sniffing) | M1a: done (URL 828/893 WPT = 92.7%, the ceiling of the `url` crate; encoding — BOM/transport/`<meta>` prescan, UTF-8 default) / M1 |
| HTML | tokenizer/tree builder (html5ever), parser-script reentrancy, `document.write` | M1a: done (tree construction 1307/1313 = 99.5% over the **36 vendored `.dat` files** of the Task 9 list; `svg.dat`/`math.dat`/`plain-text-unsafe.dat`/`scriptdata01.dat` and others are not vendored, so SVG/MathML are not covered by conformance — see `docs/SPEC_REGISTRY.md`) / M3 (reentrancy, `document.write`) |
| DOM | Node tree, events (capture/bubble), MutationObserver, Range/Selection, Shadow DOM, custom elements | M1a: done (static Node tree, no events) / M1 / M3 |
| CSS | stylo: cascade, layers, custom properties, media/container queries, nesting | M1a: partial (the cascade works; `cl-layout` reads only the M1a subset of 22 longhands) / M1 |
| Layout | block, inline, positioned, floats | M1a: partial (block + inline + `position:relative` implemented and covered by 22/22 reftest pairs; no floats, no absolute/fixed) / M1 |
| Layout | flex, grid (taffy math), tables | M3 |
| Layout | multicol, fragmentation, writing modes vertical | later |
| Text | shaping, bidi, line breaking, font fallback, variable fonts, emoji | M1a: partial (Ahem/Noto Sans via parley/swash, LTR only, no per-inline line-height/vertical-align) / M1 basic / M3 full |
| Paint/compositor | display lists, stacking, clips, transforms, opacity, filters; async scroll; transform/opacity animations on the compositor | M1a: partial (display list + solid borders + backgrounds + text + `overflow:hidden` clip, CPU raster) / M1 / M4 |
| GPU | vello+wgpu; CPU fallback | M1a: done (CPU fallback — tiny-skia + swash, deterministic) / M1 (vello+wgpu) |
| Test infra | `cl-testshell`: render/dump/compare/reftest CLI, 22 reftest pairs (`crates/testshell/tests/ref`), 5-stage golden dumps, 6 fuzz targets | M1a: done |
| Images | PNG, JPEG, GIF, WebP, AVIF (utility process) | M1 / M3 (AVIF) |
| SVG | inline SVG rendering (usvg/resvg approach) | M3 |
| JS | V8, ES2025, modules, Wasm | M2 |
| Event loop | tasks/microtasks, timers, rAF, throttling | M2 |
| Fetch API, XHR | CORS, credentials, streams | M2 |
| Forms | inputs, submit, validation, IME | M3 |
| Storage | cookies, localStorage, sessionStorage | M2/M4 |
| Storage | IndexedDB, Cache Storage, OPFS, quota | M4 |
| Workers | dedicated, shared | M3 / M4 |
| Service workers | | M5 |
| Canvas 2D | | M3 |
| WebGL | | later |
| WebGPU | | later |
| Web Audio, `<audio>/<video>` (MSE) | | M5 / later |
| WebRTC | | non-goal v1 (scope + privacy surface) |
| EME/DRM | | non-goal v1 (CDM licensing) |
| WebXR | | non-goal |
| Accessibility | accesskit tree, focus/keyboard | M1 basic / M3 |
| Printing/PDF | | later |
| HTTP | 1.1, 2 | M2 |
| HTTP | 3/QUIC | M4 |
| WebSocket, WebTransport | | M3 / later |
| bfcache | | later |
| Site isolation | site-per-process | M2 (assignment) / M4 (OOPIF) |

## Product

| Feature | Status |
|---|---|
| Windows, tabs, omnibox, back/forward, history | M5 |
| Profiles, private mode, session restore | M5 |
| Bookmarks, downloads, permissions UI, cert UI | M5 |
| Passwords, autofill | M5 |
| Chrome profile import (bookmarks, history, passwords, settings, extension list) | M6 |
| Chrome profile mirror (periodic one-way diff) | M6 |
| Sync client over Chromium `sync.proto` + self-hosted server | M6 |
| Google Chrome Sync | **non-goal** (API closed by Google, 2021) |
| MV3 extensions, install from Chrome Web Store | M6 |
| MV2 extensions | **non-goal** (Chrome removed them) |
| DevTools: CDP subset + Chrome DevTools frontend | M6 |
| WebDriver BiDi | M6 |
| Signed auto-updates, staged rollout | M5 (beta blocker) |
| Crash reporting without PII | M2 |
| Enterprise policy | later |
| Safe Browsing / reputation | non-goal v1 (no service); local download heuristics — M5 |
| Mobile platforms | non-goal v1 |
| Locale/l10n UI | M5 (en, ru) |
