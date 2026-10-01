# ChromeLight

An independent web browser and browser engine in Rust for macOS, Windows and Linux. Goal — Chrome-level behavior at several times lower memory consumption, with a secure multi-process architecture from day one.

> ChromeLight is an independent project and is not affiliated with, endorsed by, or sponsored by Google LLC. "Chrome" is a trademark of Google LLC. See ADR-0013.

## Status

**Pre-alpha. M0 Foundation is complete** (tag `m0`): Cargo workspace with `cl-platform`, `cl-ipc`, `cl-process`, `cl-testshell` and the `chromelight` binary; the browser process spawns a renderer and completes a typed, validated IPC handshake; CI is green on macOS, Windows and Linux. There is no rendering engine or UI yet: the renderer does not parse HTML, and `cl-testshell render` produces a blank canvas. Sandbox policies are not implemented yet, so a renderer runs only in debug builds with `--no-sandbox`; release builds refuse to start one. Next: M1 — static pages (`docs/PLAN.md`). Current state, known debts and open questions — `MEMORY.md`.

Honest estimate of scale (from `docs/RESEARCH-2026-09.md`): an independent engine covering a useful share of the open Web is a multi-year program. The project is run by one developer with AI agents, so scope at each milestone is strictly limited, and compatibility is measured, not promised.

## What we are building

| Layer | Decision |
|---|---|
| Engine (DOM, layout, paint, compositor, net, IPC, processes) | own, Rust |
| CSS | `stylo` (the Firefox/Servo engine) + `cssparser` |
| HTML parser | `html5ever` → own DOM |
| Text | `parley` + `swash` |
| Graphics | `vello` + `wgpu`, CPU fallback |
| JS/Wasm | V8 via the `v8` crate, behind the `JsRuntime` trait |
| Network | `rustls`, `hyper` (h1/h2), `quinn`/`h3`, own HTTP cache and cookie store |
| Storage | SQLite (`rusqlite`) |
| Accessibility | `accesskit` |
| Chrome interop | import of the local Chrome profile; own sync server speaking the Chromium `sync.proto` protocol; MV3 extensions; CDP-compatible DevTools |

What we are **not** doing: Google Chrome Sync (API closed by Google since 2021), MV2, mobile platforms (v1), DRM, WebRTC (v1), own JIT (v1).

## Documentation

| File | What's inside |
|---|---|
| [CLAUDE.md](CLAUDE.md) / [AGENTS.md](AGENTS.md) | Rules for the developer and AI agents. Read first. |
| [MEMORY.md](MEMORY.md) | Project memory: state, decisions, open questions, session log. |
| [docs/PLAN.md](docs/PLAN.md) | Milestone program M0–M6 with exit criteria and gates. |
| [docs/superpowers/plans/](docs/superpowers/plans/) | Detailed executable per-milestone plans (tasks, tests, commits). |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Process model, URL→pixels pipeline, crate map, memory budgets. |
| [docs/adr/](docs/adr/README.md) | Architecture Decision Records 0001–0014. |
| [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) | Environment, build, cross-platform, commands. |
| [docs/CODING_STANDARDS.md](docs/CODING_STANDARDS.md) | Rust rules: ownership, errors, `unsafe`, clippy, documentation. |
| [docs/SECURITY.md](docs/SECURITY.md) | Threat model, trust boundaries, defense matrix, disclosure policy. |
| [docs/TESTING.md](docs/TESTING.md) | Test pyramid: unit, golden, reftest, WPT, Test262, fuzz, perf. |
| [docs/FEATURE_MATRIX.md](docs/FEATURE_MATRIX.md) | What we support, in which phase, what is a non-goal. |
| [docs/SPEC_REGISTRY.md](docs/SPEC_REGISTRY.md) | Spec registry: section → crate → tests → status. |
| [docs/DEPENDENCIES.md](docs/DEPENDENCIES.md) | Dependencies, licenses, why. |
| [docs/RESEARCH-2026-09.md](docs/RESEARCH-2026-09.md) | Market/technology research snapshot as of 2026-09-07, with sources. |
| [docs/GLOSSARY.md](docs/GLOSSARY.md) | Terms. |

## Quick start

```bash
cargo build --workspace
cargo test --workspace
# Browser ↔ renderer handshake; debug builds only until the renderer sandbox lands (macOS/Linux M1, Windows M2)
cargo run -p chromelight -- --no-sandbox
# Headless render to PNG (M0: blank 800×600 canvas) and PNG diff (exit 1 on mismatch)
cargo run -p cl-testshell -- render page.html --png out.png
cargo run -p cl-testshell -- compare out.png expected.png
```

Without `--no-sandbox` the renderer exits with code 78 (sandbox unavailable) and the browser fails after the 15 s bootstrap timeout. This is intended: no renderer runs unsandboxed unless explicitly opted out in a debug build.

Requirements and toolchain installation — `docs/DEVELOPMENT.md`.

## License

Apache-2.0 OR MIT at the recipient's option (`LICENSE-APACHE`, `LICENSE-MIT`, ADR-0014). Dependencies are checked with `cargo deny`.
