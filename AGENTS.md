# CLAUDE.md — chrome-light

> AGENTS.md is a verbatim copy of this file. Edit CLAUDE.md, then run `scripts/sync-agents-md.sh` (or copy manually). Never edit AGENTS.md directly.

## 0. Read order (every session)

1. This file.
2. `MEMORY.md` — project memory: current state, open decisions, last session log.
3. `docs/PLAN.md` — milestone program; the active executable plan is under `docs/superpowers/plans/`.
4. `docs/ARCHITECTURE.md` — crate map, process model, pipeline.
5. The ADR(s) under `docs/adr/` relevant to the subsystem you touch.
6. `docs/CODING_STANDARDS.md` before writing Rust. `docs/SECURITY.md` before touching IPC, sandbox, net, bindings, parsers.

Shell note: non-interactive shells on the owner's Mac lack `~/.cargo/bin` in PATH. Prefix cargo commands with `export PATH="$HOME/.cargo/bin:$PATH"` or run `source ~/.cargo/env`.

If a task contradicts an Accepted ADR: stop, say so, propose a superseding ADR. Do not silently deviate.

## 1. What this project is

**ChromeLight** (repo `chrome-light`, binary `chromelight`, crates `cl-*`; trademark risk accepted by owner, ADR-0013 — never imply affiliation with Google) — an independent web browser **and** browser engine written in Rust, targeting macOS, Windows, Linux simultaneously. License: Apache-2.0 OR MIT (ADR-0014).

Project class (ADR-0001): **independent open-Web engine + full browser product**, built by one developer plus AI agents. Not a Chromium fork, not a CEF/WebView shell, not a Servo embedding. Servo/Ladybird/Blitz are references and crate sources, not the engine.

Product goals, in priority order:

1. **Security boundaries first.** Multi-process, sandboxed renderers, validated IPC from the first milestone that loads untrusted content. No arbitrary Web before the sandbox gate (ADR-0005).
2. **Memory as a first-class requirement** (ADR-0012). Every subsystem has a budget. Regressions block merges.
3. **Chrome-level behaviour as the compatibility target**, reached asymptotically: WPT + Test262 + curated site corpus. Parity is a direction, not a milestone promise.
4. **Chrome interop for users**: one-way import/mirror of the local Chrome profile (ADR-0007), MV3-only extensions (ADR-0011), CDP-compatible DevTools protocol.
5. **Own sync** via a self-hosted server speaking the Chromium `sync.proto` protocol (ADR-0007). Google-account Chrome Sync is a permanent non-goal (Google closed the API in March 2021).

Explicit non-goals: Google Chrome Sync, MV2 extensions, mobile platforms (v1), DRM/EME (v1), WebRTC (v1), our own JS VM with an optimizing JIT (v1 uses V8 behind a trait, ADR-0004).

## 2. Layer vocabulary — never collapse these

| Term | Meaning here |
|---|---|
| **browser / product** | `cl-browser` + `cl-shell-ui` + profile, sync, import, updater |
| **engine** | everything from URL to pixels: `cl-net`, `cl-html`, `cl-dom`, `cl-style`, `cl-layout`, `cl-paint`, `cl-compositor`, `cl-gfx`, `cl-bindings`, `cl-webapi` |
| **JS/Wasm VM** | V8 via the `v8` crate, wrapped by `cl-js` behind `JsRuntime` trait. DOM is not part of the VM. |
| **platform services** | `cl-platform`, `cl-process`, `cl-ipc`, `cl-storage`, `cl-a11y` |

"Chromium", "Blink", "V8" are three different things. Say which one you mean.

## 3. Hard rules

### 3.1 Security (non-negotiable)

- The renderer is hostile. Every message crossing renderer → browser/net/gpu is validated at the receiver against a schema; origin/site checks happen in the browser process, never trusted from the renderer.
- No renderer ever holds: raw sockets, user files, keychain, update mechanism, clipboard write without gesture. Access is brokered via capability handles in `cl-ipc`.
- No synchronous IPC from browser → renderer. Renderer → browser sync calls need an ADR.
- Parsers (HTML, CSS, image, font, IPC decoders) get a fuzz target in the same PR that introduces them.
- No `unsafe` outside the FFI crates listed in `docs/CODING_STANDARDS.md` §4. Every `unsafe` block has a `// SAFETY:` comment. `#![forbid(unsafe_code)]` everywhere else.
- Never reduce a security check to make a site work. File a compat issue instead.
- Secrets never in logs, traces, crash dumps, test fixtures, or git.

### 3.2 Rust

- Toolchain pinned in `rust-toolchain.toml`. Stable only. No nightly features.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` must pass. Lints configured in the workspace `Cargo.toml` `[workspace.lints]` and `clippy.toml`.
- No `unwrap()`/`expect()` outside tests and `build.rs`. Libraries use `thiserror`; binaries may use `anyhow`.
- No `panic!` on untrusted input. A malformed byte is a `Result::Err`, never a crash.
- Pass `&str`/`&[T]`, not `String`/`Vec<T>`, unless ownership is transferred. No `.clone()` in hot loops without a comment.
- Public API of every crate documented; `#![deny(missing_docs)]` on library crates.
- New dependency = `cargo deny check` passes + one line in `docs/DEPENDENCIES.md` with license and why.
- Platform-specific code lives only in `cl-platform` (and `cl-process`, `cl-gfx` backends). Elsewhere `#[cfg(target_os)]` is a review flag.

### 3.3 Standards

- Acceptance criteria come from the normative spec (WHATWG/W3C/TC39/IETF) and its WPT/Test262 tests, not from MDN or from "what Chrome does". If Chrome deviates from spec and sites depend on it, document the quirk in `docs/SPEC_REGISTRY.md` with a WPT reference.
- Every implemented feature is a row in `docs/SPEC_REGISTRY.md`: spec section → crate/module → tests → status → deviations.
- Web IDL is the source of truth for DOM API surfaces. Hand-written bindings are forbidden; use the `cl-bindings` generator.

### 3.4 Process

- Work in small, reviewable changes. One subsystem per PR/commit unless a cross-cutting refactor is announced in `MEMORY.md`.
- TDD for parsers, layout algorithms, IPC schemas, URL/cookie/cache logic: write the failing test (or WPT expectation) first.
- Before claiming "done": build on the host, run the affected crate tests, run clippy, update `MEMORY.md` session log and any touched ADR/registry row. Say what was **not** run.
- CI matrix is macOS-arm64, Windows-x64, Linux-x64 from day one (ADR-0009). Code that only builds on the host is not done.
- Memory/perf-sensitive changes include numbers from `tools/bench` (before/after) in the commit message.
- Commit messages: English, imperative, scoped: `layout: implement inline-block baseline alignment (css-inline-3 §…)`.
- Language: code, comments, commit messages, ADR titles — English. User-facing docs (`README.md`, `docs/*.md`, `MEMORY.md`) — Russian with English technical terms. Chat with the owner — Russian, caveman-terse unless asked otherwise.

## 4. Architecture in one screen

```
 browser process (privileged, Rust, no untrusted parsing)
 ├── cl-browser: tabs, navigation policy, profiles, permissions, downloads, session
 ├── cl-shell-ui: chrome UI (egui/wgpu now; privileged web UI later, ADR-0008)
 ├── cl-storage: SQLite (history, bookmarks, cookies, prefs), IndexedDB/localStorage backends
 ├── cl-chrome-import, cl-sync, cl-extensions(host side), cl-devtools(server), updater
 └── cl-process + cl-ipc: spawn/sandbox children, capability broker, schema-validated messages
        │ IPC (typed, versioned, validated) + shared memory for frames
        ├── renderer process per site (sandboxed)
        │     cl-renderer = cl-html → cl-dom ↔ cl-js(V8) via cl-bindings
        │                   → cl-style(stylo) → cl-layout → cl-paint → commit to compositor
        ├── network process: cl-net (rustls, hyper h1/h2, quinn/h3, cache RFC 9111, cookies, Fetch)
        ├── gpu process: cl-gfx (vello + wgpu, CPU fallback) + cl-compositor (property trees, tiles, async scroll)
        └── utility processes: image/font/media decoders (sandboxed, restartable)
```

Details, budgets and rationale: `docs/ARCHITECTURE.md`. Own-vs-reused component decisions: ADR-0003.

## 5. Commands

```bash
cargo build --workspace                      # host build
cargo test --workspace                       # unit + integration
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
cargo deny check                             # licenses/advisories/bans
cargo run -p chromelight                     # launch browser shell
cargo run -p cl-testshell -- --headless <url|file> --png out.png   # deterministic render
./tools/wpt/run.sh <dir>                     # WPT subset via wptrunner product adapter
./tools/test262/run.sh                       # Test262 qualification set
cargo fuzz run <target>                      # needs cargo-fuzz (nightly toolchain only for fuzzing)
```

Setup, cross-compilation, toolchain gotchas: `docs/DEVELOPMENT.md`.

## 6. Memory protocol for agents

- `MEMORY.md` at repo root is the **project** memory (state, decisions, session log). Update it at the end of every working session: what changed, what's blocked, next action. Keep it under ~300 lines; move history to `docs/history/`.
- ADRs are immutable once Accepted. To change a decision, add a new ADR that supersedes the old one and update the old one's status.
- `docs/RESEARCH-2026-09.md` is a dated snapshot. When you rely on a volatile fact (crate version, Chrome release, Servo status), check the date and re-verify if older than ~3 months.

## 7. When in doubt

- Security vs feature → security.
- Spec vs Chrome behaviour → spec, log the quirk.
- Own code vs audited crate for a commodity parser → audited crate (ADR-0003), but wrap it behind our type and fuzz it.
- Performance vs correctness before M3 → correctness; keep a reference path.
- Unsure whether the owner wants X → ask in one sentence; do the parts that don't depend on the answer.
