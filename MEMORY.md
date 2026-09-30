# MEMORY.md — chrome-light project memory

Updated at the end of every working session. Short facts, links to documents. Session history older than ~a month moves to `docs/history/`.

## State as of 2026-09-07

**Phase:** M0 Foundation complete: PR #1 merged into `main` (merge c0b1244), tag `m0`. CI green on macOS-14 / windows-2022 / ubuntu-24.04 (check, test, fuzz-short). Idle memory: 4864 KB total (browser 2496 KB, renderer 2368 KB). Release-sandbox gate passed (unsandboxed renderer refuses, exit 78). Next action: M1 plan (`docs/superpowers/plans/<date>-m1-static-pages.md`).

**Product:** **ChromeLight** (ADR-0013, trademark risk accepted by the owner). License **Apache-2.0 OR MIT** (ADR-0014).

**Repository:** GitHub https://github.com/maslakovSaveliy/chrome-light (public), default branch main; PR #1 (M0) merged, branch deleted; tag `m0` = c0b1244. `.planning/HANDOFF.json` — empty GSD checkpoint (in .gitignore).

**Owner's environment:** macOS 26.5.2, Apple Silicon (arm64), 16 GB RAM, 8 cores, Xcode 26.6, **rustup installed** (1.95.0, targets aarch64-apple-darwin/x86_64-pc-windows-msvc/x86_64-unknown-linux-gnu; cargo-deny/nextest/fuzz/insta present). Note: non-interactive shells don't see `~/.cargo/bin` — use `export PATH="$HOME/.cargo/bin:$PATH"`. cmake/ninja present, docker present, gh present.

## Accepted decisions (brief; full — docs/adr/)

| ADR | Decision | Status |
|---|---|---|
| 0001 | Project class: independent engine + full browser, solo + AI agents | Accepted |
| 0002 | Rust stable, MSRV pinned, `unsafe` only in FFI crates | Accepted |
| 0003 | Own: DOM, layout, paint, compositor, IPC, processes, net-service, storage, shell. Reused: html5ever, cssparser+stylo, taffy (flex/grid math), parley/swash, vello/wgpu, rustls/hyper/quinn, url, image, accesskit, rusqlite, V8 | Accepted |
| 0004 | JS VM: V8 via the `v8` crate behind the `JsRuntime` trait; own VM — not before v2 | Accepted (revisit M4) |
| 0005 | Multi-process from the first milestone: browser / renderer-per-site / net / gpu / utility; own typed IPC; sandbox: seatbelt / seccomp+namespaces / AppContainer | Accepted |
| 0006 | Graphics: vello + wgpu, CPU fallback; own compositor with property trees | Accepted |
| 0007 | Sync: (a) one-way import/mirror of the local Chrome profile; (b) own sync server over Chromium `sync.proto`. Google Chrome Sync — non-goal forever | Accepted |
| 0008 | Shell UI: egui on wgpu at the start; later a privileged web UI on our own engine | Accepted (revisit M5) |
| 0009 | All three platforms from day one: CI matrix macOS/Win/Linux, platform code only in cl-platform | Accepted |
| 0010 | Tests: WPT + Test262 from the first milestone, headless testshell, reftests, cargo-fuzz | Accepted |
| 0011 | Extensions: MV3 only, own chrome.* runtime; CRX from the Web Store; late phase | Accepted |
| 0012 | Memory is a first-class requirement; budgets and measurement in CI | Accepted |
| 0013 | Product name ChromeLight; Google trademark risk accepted by the owner; disclaimer "not affiliated with Google" | Accepted |
| 0014 | License of our own code: Apache-2.0 OR MIT; deps checked via cargo-deny | Accepted |

## Open questions for the owner

1. Chrome 153 baseline in `docs/history/bench-2026-09.md` — measure manually on the owner's machine.

## Known debts after M0

Deferred to M1+; do not block the public beta:

1. `BootstrapServer::accept_with_timeout` starts one blocked accept-thread per call; the leak is bounded by one `spawn()` (not by the browser process lifetime) — revisit the handshake in M1.
2. Release sandbox-refusal path takes ~15 s (browser waits for the bootstrap timeout instead of polling `try_wait`); worst case `spawn` = 2×timeout (accept + handshake) — M1 optimization.
3. `browser_side` masks `Violation` if the reject-ack send fails — log-and-ignore.
4. `BrowserEndpoint`/`ChildEndpoint` are structurally identical — generic helper as the API grows.
5. Fuzz job in CI without rust-cache.
6. Test temp-dir boilerplate is duplicated (testshell, chromelight tests) — helper.
7. Browser-side `recv_timeout` — M0-ONLY poll via `park_timeout(1 ms)`; `park_timeout` is not in disallowed-methods; blocking `BrowserEndpoint::recv` remains pub (used by tests). M1: `IpcReceiverSet`/multiplexed wait, type-level ban on unbounded recv on the browser side.
8. `browser.rs`: `renderer.wait()` after `Shutdown` — unbounded wait on the process; M1: `try_wait` poll with a deadline + kill (same class as ADR-0005 §4).
9. CI: fmt/clippy — in the 3-OS matrix; `deny`/`doc`/scripts — ubuntu only (platform-independent).

## Key external facts (snapshot 2026-09-07, details — docs/RESEARCH-2026-09.md)

- Servo 0.5.0 (21 Aug 2026) on crates.io; 0.1.0 LTS since 13 Apr 2026. The embedding API is growing, but compatibility is partial. Verso archived.
- Ladybird: C++→Rust, PRs closed, alpha (Linux/macOS) in 2026, beta 2027.
- Chrome Sync API closed to third-party builds since 15 Mar 2021. Brave `go-sync` — an open-source server speaking `sync.proto`.
- MV2 is dead: disabled in Chrome 138 (Jul 2025), Web Store purged 31 Aug 2026.
- Chrome 153 — 8 Sep 2026; releases every 2 weeks after that.
- Chrome 127+ on Windows: App-Bound Encryption for cookies (planned for passwords) — a third-party process cannot decrypt the Chrome profile without user involvement.
- Crate versions as of 2026-09-07: stylo 0.20, cssparser 0.37, html5ever 0.39, taffy 0.14, parley 0.11, vello 0.10, wgpu 30.0, winit 0.30.13, accesskit 0.25, v8 152.2, hyper 1.11, rustls 0.23.43, quinn 0.11.11, rusqlite 0.40, ipc-channel 0.23, prost 0.14, tokio 1.53.

## Session log

### 2026-09-30 — session 3: docs translated to English
- Owner decision: all docs are English. CLAUDE.md §3.4 language rule updated (AGENTS.md synced); chat with the owner stays Russian.
- Translated (pure language, no content changes, line-aligned diffs): README, MEMORY, docs/*.md, docs/adr/*, docs/history/*, Russian lines in the M0 plan. Repo-wide Cyrillic grep is empty.
- Source inconsistencies noticed and left as is (not translation issues): README "Status" still says pre-M0 and Quick start uses `-p chrome-light` (package is `chromelight`); frozen background tab budget ≤ 15 MB in ARCHITECTURE §9 vs ≤ 25 MB in ADR-0012 / PLAN M4.
- Not run: cargo build/test/clippy (docs-only change).
- Next action: M1 plan (`docs/superpowers/plans/<date>-m1-static-pages.md`).

### 2026-09-07 — session 2: M0 execution (subagent-driven)
- Executed Tasks 1–11 from `docs/superpowers/plans/2026-09-07-m0-foundation.md`: T1 workspace/lints/deny (1 fix), T2 cl-platform (no fixes), T3–T5 cl-ipc codec/validate/bootstrap/handshake (no fixes), T6–T7 cl-process sandbox/spawn (no fixes), T8 browser/child ping-pong + tracing (1 fix: `--browser-fail-after-handshake`), T9 testshell PNG (no fixes), T10 bench script (no fixes; Chrome baseline pending owner action), T11 CI check-agents + Gates (3 fixes: rustfmt nightly options noted, cargo-fuzz flags fixed, Exit codes verified).
- CI result: check ✅, test (macOS-14/windows-2022/ubuntu-24.04) ✅, fuzz-short ✅ on all three OSes.
- Measurements: idle RSS 4864 KB (browser 2496, renderer 2368). Release sandbox gate PASS (unsandboxed exit 78). Fuzz run 60 s: 75.2M runs, 0 crashes.
- Task 12: updated PLAN.md exit-criteria (5/6 ticked, Chrome baseline remains owner-action), MEMORY.md state/debts/log, ARCHITECTURE.md crate status.
- Next action: final branch review → merge into main → tag `m0` → M1 plan (`docs/superpowers/plans/<date>-m1-static-pages.md`).
- Final branch review (sonnet; opus returned 429): 0 Critical, 2 Important fixed in the fix wave (bounded recv, 3-OS lint), minor debts collected into the list above.

### 2026-09-07 — session 1: research + documentation foundation
- Read the `browser-engine-research` skill (6 references, snapshot as of 2026-09-05).
- Web verification: Servo, Ladybird, CEF/cef-rs, Chrome Sync, MV2/MV3, Chromium release cadence, Rust stack, App-Bound Encryption, Rust GUI, wry.
- The owner chose: own engine from scratch in Rust; solo + AI agents; all three platforms at once; sync = local Chrome profile + own server.
- Created: CLAUDE.md, AGENTS.md, MEMORY.md, README.md, docs/ (ARCHITECTURE, DEVELOPMENT, CODING_STANDARDS, SECURITY, TESTING, RESEARCH-2026-09, FEATURE_MATRIX, SPEC_REGISTRY, DEPENDENCIES, GLOSSARY), docs/adr/0001–0014, toolchain configs.
- Owner: license — as permissive as possible (→ Apache-2.0 OR MIT), name — ChromeLight, `git init` — yes, rustup installed.
- Created: `docs/PLAN.md` (M0–M6 program, exit criteria, rituals, risks), `docs/superpowers/plans/2026-09-07-m0-foundation.md` (12 tasks with code and tests), LICENSE-APACHE/MIT; ADR-0013/0014 → Accepted; git repository initialized, first commit — all documentation.
- **Next action:** execute M0 per the plan (subagent-driven or inline). For the owner: create a GitHub remote for the CI matrix; manually measure Chrome 153 for `docs/history/bench-2026-09.md` (Task 10).
