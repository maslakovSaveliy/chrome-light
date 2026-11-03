# Research: state of browser engines and the Rust ecosystem

**Snapshot: 2026-09-07.** Base references — skill `browser-engine-research` (snapshot 2026-09-05); volatile facts re-verified online 2026-09-07. Re-verify facts older than 3 months before use.

## 1. Conclusion for the project

"A full Chrome equivalent, in Rust, many times lighter" — three goals that cannot be achieved simultaneously in v1. The owner chose the class **independent engine from scratch** (solo + AI agents), therefore:

- compatibility is an asymptotic goal, measured by WPT/Test262/corpus, not a parity promise;
- "lighter" is a measurable budget (ADR-0012), achieved through architecture (freeze/hibernate, one isolate, no extra processes), not "Rust by itself";
- "everything like in Chrome" is delivered via Chrome interop (profile import, MV3, CDP), not via Chromium code.

Realistic scale per the reference: a narrow secure engine for controlled content — 8–20 engineers × 2–4 years; an engine with a useful share of the open Web — dozens of engineers, 5+ years. Solo + agents changes productivity, but not the volume of specs. Hence the hard milestone gates and the feature matrix.

## 2. Engines (September 2026)

| Project | Status | Significance for us |
|---|---|---|
| **Servo** | 0.1.0 LTS on crates.io (2026-04-13), 0.5.0 (2026-08-21). Embedding API: proxies, root certs, cookies, local/sessionStorage, dialogs, console, DevTools. Multithreaded canvas (+55% fps), Linux aarch64, Android 10+. Donations ~7.8k USD/month. Multiprocess exists, sandbox immature. | We don't embed it. Source of architectural decisions and crates (stylo, servo_arc, ipc-channel, webrender ideas). Verso (a Servo-based browser) is archived — it couldn't keep up with the API. |
| **Ladybird** | C++→Rust: LibJS frontend (Feb 2026), HTML parser (May), style+layout (July). PRs closed (June 2026). Alpha Linux/macOS — 2026, beta — 2027. Swift direction dropped. | Not embeddable. Reference for "vertical stack from scratch", confirmation that an AI-assisted C++→Rust port is real. |
| **Chromium/Blink/V8** | Chrome 153 — 2026-09-08; then **a release every 2 weeks**. MV2 disabled since Chrome 138 (July 2025), Web Store purged 2026-08-31. Rust in Chromium production since M119 (PNG/JSON/fonts parsers). | Compat target and source of V8. 2-week cadence = we update the V8 crate just as often. |
| **CEF** | crate `cef` 151.8.1 (2026-09-03); bitbucket releases lag behind the crate. | Rejected (ADR-0001): not "lighter", not "our own". |
| **Blitz (DioxusLabs)** | `blitz-dom` 0.2.4: stylo + taffy + parley + vello, experimental. | Proof of assembling an engine from crates; we borrow the stylo↔taffy integration structure (`stylo_taffy`). |
| **wry/Tauri** | system WebViews: WebView2 / WKWebView / WebKitGTK. | Rejected: three different engines, no parity. |

## 3. Chrome interop: facts

- **Chrome Sync via Google account** closed to third-party builds since 2021-03-15 (Google audit). Workarounds (flags/patches) violate the ToS. **Non-goal forever.**
- **Protocol** `components/sync/protocol/sync.proto` is open; Brave `go-sync` is an open-source server, "supports any Chromium-based browser". Vivaldi — its own closed one. There is `chromium-sync-server` (Python, experimental). → ADR-0007: our own server in Rust on this protocol.
- **Local Chrome profile:** `Bookmarks` (JSON), `History`/`Login Data`/`Web Data`/`Cookies` (SQLite, locked while Chrome is running — copy), `Preferences` (JSON), `Extensions/<id>/<ver>/manifest.json`. Passwords/cookies are encrypted: macOS — Keychain "Chrome Safe Storage" (AES-128-CBC, PBKDF2), Linux — libsecret/kwallet/"peanuts", Windows — DPAPI (`v10`) and **App-Bound Encryption `v20`** since Chrome 127 (July 2024) for cookies, with a plan to extend it to passwords/payments. ABE is bound to Chrome's SYSTEM service → a third-party process cannot legally decrypt. Import on Windows — only via user export (passwords CSV) or a limited set (bookmarks, history, settings).
- **MV3**: declarative permissions, background service worker, content scripts in an isolated world, `declarativeNetRequest`, remote code ban. CRX3 format, the Web Store update URL is public (Chromium uses the same one).
- **CDP** — tip-of-tree is unstable; **WebDriver BiDi** is the standard. We do a CDP subset for the DevTools frontend + BiDi for automation.

## 4. Chrome memory (public sources, 2026)

- Memory Saver since Chrome 140 (Sep 2025): ML prediction of returning to a tab, three modes; "up to 80% less" on a discarded tab.
- 10 active tabs in Chrome 140+ — ~1.4 GB (1.8 GB in Chrome 135). Heavy web apps (Figma/Notion/Slack) — 0.5–1.5 GB per tab.
- Energy Saver freezes JS, but does not free memory; Memory Saver discards the renderer.

→ Our targets in ADR-0012 are set relative to these numbers and confirmed by `tools/bench` monthly on the same machine.

## 5. Rust stack: versions as of 2026-09-07

| Crate | Version | Role | License (check in DEPENDENCIES) |
|---|---|---|---|
| stylo | 0.20.0 | CSS cascade/computed style (Firefox/Servo) | MPL-2.0 |
| cssparser | 0.37.0 | CSS tokenizer | MPL-2.0 |
| html5ever | 0.39.0 | HTML tokenizer/tree builder | MIT/Apache |
| taffy | 0.14.0 | flex/grid/block layout math | MIT |
| parley | 0.11.1 | text layout, line breaking, bidi | MIT/Apache |
| swash | 0.2.10 | shaping, glyph rasterization | MIT/Apache |
| fontdb | 0.24.0 | font discovery | MIT |
| vello | 0.10.0 | GPU 2D renderer | MIT/Apache |
| wgpu | 30.0.1 | GPU abstraction (Metal/DX12/Vulkan) | MIT/Apache |
| winit | 0.30.13 | windows/input | Apache-2.0 |
| accesskit | 0.25.0 | accessibility tree → AX/UIA/AT-SPI | MIT/Apache |
| v8 (rusty_v8) | 152.2.0 | V8 bindings; versions = Chrome | MIT |
| deno_core | 0.411.0 | reference for V8 integration (not used directly) | MIT |
| mozjs | 0.26.0 | SpiderMonkey (alternative, not chosen) | MPL-2.0 |
| hyper | 1.11.1 | HTTP/1.1, HTTP/2 | MIT |
| rustls | 0.23.43 | TLS 1.2/1.3 | MIT/Apache/ISC |
| quinn / h3 | 0.11.11 / 0.0.8 | QUIC / HTTP/3 | MIT/Apache; h3 pre-1.0 — risk |
| url | 2.5.8 | WHATWG URL | MIT/Apache |
| image | 0.25.10 | decoders (in utility process) | MIT/Apache |
| rusqlite | 0.40.2 | SQLite | MIT |
| ipc-channel | 0.23.0 | IPC transport (Servo) | MIT/Apache |
| prost | 0.14.4 | protobuf for sync.proto | Apache-2.0 |
| tokio | 1.53.1 | async runtime (network process, browser process) | MIT |
| tracing | 0.1.44 | observability | MIT |
| insta / proptest / arbitrary | 1.48 / 1.11 / 1.4.2 | tests | MIT/Apache |
| tiny-skia | 0.12.0 | CPU raster fallback | BSD-3 |

Host: rustc 1.95.0 (2026-04-14). Edition 2024.

## 6. Rust GUI for the shell

egui — fastest to a window, immediate mode, wgpu backend; iced — Elm style; Slint — DSL + commercial license; Xilem — not production. For a solo developer with a wgpu stack, egui was chosen (ADR-0008) with a plan to migrate to a privileged web UI on our own engine.

## 7. Sources

- Servo: https://servo.org/blog/ ; https://servo.org/about/ ; https://github.com/servo/servo/wiki/Roadmap ; https://byteiota.com/servo-0-1-0-ships-on-crates-io-embeddable-rust-browser/ ; https://www.phoronix.com/news/Servo-January-2026 ; https://www.osnews.com/story/140462/verso-a-browser-using-servo/
- Ladybird: https://ladybird.org/ ; https://linuxiac.com/ladybird-browser-closes-public-pull-requests-ahead-of-first-alpha/ ; https://alternativeto.net/news/2026/2/ladybird-web-browser-begins-rust-adoption-starting-with-javascript-engine-with-ai-help
- Chrome Sync: https://www.xda-developers.com/google-cracks-down-third-party-chromium-browser-chrome-sync/ ; https://groups.google.com/a/chromium.org/g/chromium-packagers/c/SG6jnsP4pWM ; https://github.com/brave/go-sync ; https://github.com/jackyzy823/chromium-sync-server
- MV2/MV3: https://www.ghacks.net/2026/09/01/manifest-v2-is-dead-as-chrome-web-store-permanently-purges-legacy-extensions/ ; https://developer.chrome.com/docs/extensions/develop/migrate/what-is-mv3
- Release cadence: https://developer.chrome.com/blog/chrome-two-week-release ; https://chromereleases.googleblog.com/2026/09/
- App-Bound Encryption: https://thehackernews.com/2024/08/google-chrome-adds-app-bound-encryption.html ; https://blog.elcomsoft.com/2026/01/browser-forensics-in-2026-app-bound-encryption-and-live-triage/
- Memory: https://whysogeek.com/chrome-memory-saver-energy-saver-performance-2026/ ; https://www.superchargebrowser.com/library/chrome-native-memory-saver-review/
- CEF: https://lib.rs/crates/cef ; https://github.com/chromiumembedded/cef
- Chromium Rust: https://chromium.googlesource.com/chromium/src/+/refs/heads/main/docs/rust.md ; https://www.chromium.org/Home/chromium-security/memory-safety/
- Blitz: https://github.com/DioxusLabs/blitz
- rusty_v8: https://deno.com/blog/rusty-v8-stabilized ; https://github.com/denoland/rusty_v8
- wry: https://docs.rs/wry/latest/wry/
- Rust GUI: https://wrenlearnsrust.com/posts/2026-03-11-rust-gui-landscape-2026.html ; https://blog.logrocket.com/state-rust-gui-libraries/
- Supporters of Chromium-Based Browsers: https://blog.chromium.org/2025/01/announcing-supporters-of-chromium-based.html
- Crate versions: crates.io API, 2026-09-07.
- Normative specs and architectural references: see `docs/SPEC_REGISTRY.md` and skill `browser-engine-research/references/*` (33 + 45 + 23 + 37 sources).
