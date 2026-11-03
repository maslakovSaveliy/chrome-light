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
- Commit only on a fully green gate (§5, chained with `&&`). A red gate is reported with its output, never committed around — not even under machine load or a nudge to "finish". Stage explicit paths (`git add <paths>`), never `git add -A`.
- Tests come from the contract, not from the implementation: a new test must fail on the old behaviour (show the RED run), expected values come from the spec or the task's acceptance cases, and the integration under test is never mocked. Changing an assertion, a skip, an expectations file or a time/size bound needs a written reason in the task log.
- Any test that asserts on wall-clock time is listed by exact name in `.config/nextest.toml` (`threads-required = "num-cpus"`). The owner's Mac often runs other heavy workloads: report `uptime` load next to every timing, and never conclude from one loaded run.
- Language: code, comments, commit messages, ADRs, and all docs (`README.md`, `docs/**/*.md`, `MEMORY.md`, `tools/**/README.md`) — English. Chat with the owner — Russian, caveman-terse unless asked otherwise.

### 3.5 Working with agents

Distilled from the owner's research note "Как организовать разработку с AI-агентами" (2026-10-26) and from what went wrong in M1a. Aim: a verifiable engineering loop, not a virtual company of agents.

- **One owner per change; implementation is not delegated.** The main agent writes the code of its task itself, end to end (explore → short plan → implement → tests → gate). Subagents are used only where they pay off: a fresh read-only review, independent research or fact-checking, read-only exploration of a large area, the final whole-branch review. No implementer subagents, no mandatory planner → coder → tester chain. Why: in M1a, per-task implementer subagents stalled, lost notifications, hit rate limits, fought over the cargo lock and needed the context re-explained in every brief, while fresh reviewers found the real Critical bugs.
- **Pick the mode before starting:**
  - *fast* — docs, typos, local change with no contract change: change + targeted check; no subagent review.
  - *normal* — feature or bugfix: explore → short plan → one coherent slice → tests → one fresh reviewer.
  - *risky* — IPC/sandbox/process lifecycle, parsers of untrusted input, `unsafe`, CI/release, new dependencies, anything security- or data-relevant: state the threat and the rollback first, add negative/hostile tests, independent review, the owner accepts the risk.
- **Every non-fast task starts from a brief:** goal; current behaviour; expected behaviour; non-goals; invariants and risks; acceptance cases (main path, boundary/negative, what must not break); what is allowed vs what needs the owner (dependencies, pushes, merges, network, deletions).
- **Review** runs in a fresh context, read-only, from the brief plus the diff (not from the author's summary alone). Each finding: severity, file:line, failure scenario, evidence, what was and was not checked. Zero findings is a valid answer; style and speculative refactors do not block. The main agent verifies each finding before fixing it (reviewers can be wrong), fixes it itself, and sends only the fix diff to a scoped re-review. At most 5 fix rounds per task, then the main agent decides and records a `Ruling:` (what — why — cost if wrong) in the task log.
- **One writer per checkout.** Only the main agent edits, builds and commits. A reviewer running while the main agent builds does not run cargo. Parallel writers only in separate worktrees on disjoint areas with one integrator, and only when the owner asks for it. A worktree is not a sandbox.
- **Rules that matter are enforced by checks.** `tools/check-*.sh`, `cargo deny`, clippy lints, CI and `.config/nextest.toml` are the real rules; prose here is advisory. If agents keep breaking a prose rule, turn it into a check.
- **Context and handoff.** One chat per coherent task. A handoff carries: goal and acceptance; decisions and why; branch, commit, dirty files; what is done and what is not; checks actually run with results; known failures; the next concrete step. Do not reset a working context on a schedule; after repeated failed attempts, write the facts down and start clean.
- **Stalled agents.** An agent that "waits on background work" with no cargo/test process alive has lost its notification: check `ps`, then resume it with an explicit instruction.
- **Evidence over reports.** Judge work by commands run and their output, not by a confident summary, a benchmark score or a commit count.
- **The owner decides:** what to build and what not to; risky architecture; merges to `main`, tags, releases, pushes to shared branches; new dependencies. An agent's approval never replaces the owner's on security, data or release.
- **Do not:** write giant prompts, run autonomous loops without a budget and a stop condition, or build our own agent harness instead of the product.

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

Verified against the repository on 2026-10-28. Non-interactive shells: `export PATH="$HOME/.cargo/bin:$PATH"` first.

```bash
# The gate — run before every commit, exactly as chained:
cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features --locked -- -D warnings && cargo nextest run --workspace --locked && cargo deny check && bash tools/check-unsafe-scope.sh && bash tools/check-stylo-scope.sh && bash tools/check-platform-cfg.sh && bash scripts/check-agents-md.sh && cargo run -p cl-testshell -- reftest

cargo nextest run -p <crate> --locked                               # focused tests while iterating
cargo run -p cl-testshell -- render <file|file-url> --png out.png   # deterministic single-process render
cargo run -p cl-testshell -- dump <file> --stage dom|style|box-tree|fragments|display-list
cargo run -p cl-testshell -- reftest [--filter <name>]              # 22 pairs in crates/testshell/tests/ref/
cargo run -p chromelight -- --no-sandbox --exit-after-handshake      # debug only: browser + renderer handshake
cargo +nightly fuzz run --fuzz-dir tools/fuzz --target aarch64-apple-darwin <target> -- -max_total_time=60
bash tools/bench/mem.sh                                             # per-process RSS (macOS/Linux)
bash scripts/sync-agents-md.sh                                      # after editing CLAUDE.md
```

Not yet built: the WPT runner and the Test262 runner (M2). Setup, toolchain gotchas: `docs/DEVELOPMENT.md`.

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
