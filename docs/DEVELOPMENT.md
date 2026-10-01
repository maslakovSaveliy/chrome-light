# Development

## 1. Host requirements

| Component | Version | Note |
|---|---|---|
| Rust | per `rust-toolchain.toml` (stable 1.95) | **via rustup**, not Homebrew: cross targets and `rustfmt`/`clippy` components of the same version are required |
| cmake, ninja | any recent | building the `v8` crate (prebuilt binaries are downloaded by default; from source — hours), we don't use `mozjs` |
| Python 3 | ≥3.11 | WPT runner, Test262 harness, codegen scripts |
| macOS | Xcode 26 + CLT | seatbelt profiles, code signing |
| Windows | VS 2026 Build Tools (MSVC), Windows 11 SDK | `x86_64-pc-windows-msvc` |
| Linux | clang, pkg-config, libxkbcommon, wayland/x11 dev, libssl not needed (rustls) | Ubuntu 24.04 as CI baseline |
| Disk | ~20 GB for target/, +5 GB WPT checkout | |

Owner's host (2026-09-07): macOS 26.5.2 arm64, 16 GB, rustup 1.95 with three targets, cargo-deny/nextest/fuzz/insta, cmake/ninja installed.

### Setup on macOS

```bash
brew uninstall rust            # otherwise conflicts with rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup component add rustfmt clippy rust-src
cargo install cargo-deny cargo-nextest cargo-fuzz cargo-insta
brew install cmake ninja python@3.12
```

Non-interactive shells (agents, scripts) don't see `~/.cargo/bin`: `export PATH="$HOME/.cargo/bin:$PATH"` at the start of every command, or `source ~/.cargo/env`.

## 2. Repository structure

See `docs/ARCHITECTURE.md` §8. Workspace root: `Cargo.toml` with `[workspace.dependencies]` (unified versions) and `[workspace.lints]`.

## 3. Commands

```bash
cargo build --workspace
cargo nextest run --workspace            # faster than cargo test; fallback: cargo test --workspace
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
cargo deny check
cargo doc --workspace --no-deps
cargo run -p chrome-light -- [url]
cargo run -p cl-testshell -- --headless tests/ref/basic.html --png /tmp/out.png
./tools/wpt/run.sh url                   # WPT directory
./tools/test262/run.sh
cargo +nightly fuzz run html_tokenizer   # fuzzing — the only place where nightly is needed
./tools/bench/mem.sh                     # memory budgets over the corpus
```

Child processes are launched as the same binary: `chrome-light --type=renderer|network|gpu|utility --ipc-handle=…`. To debug a single process: `--single-process` (dev only, blocked in release builds).

## 4. Cross-platform

- All three platforms are mandatory CI targets (ADR-0009): `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu`. Additionally a nightly job: `x86_64-apple-darwin`, `aarch64-unknown-linux-gnu`.
- Locally from macOS: Linux — via Docker (`tools/ci/linux.Dockerfile`), Windows — via GitHub Actions/a local VM; MSVC cross-compilation from macOS is not supported — don't waste time on it.
- Platform-specific code — only `cl-platform`, `cl-process`, `cl-gfx` backends. `#[cfg(target_os = …)]` in other crates is grounds for reject in review.
- Paths, file encodings, line endings: `PathBuf`/`OsStr`, never `String` for paths; `.gitattributes` pins LF for text fixtures.

## 5. Change workflow

1. Read the subsystem's ADR. If the needed one does not exist — ADR first (`docs/adr/template.md`).
2. Test first: unit/golden/reftest or WPT expectation (`tools/wpt/expectations/`).
3. Implementation. For a parser/decoder — fuzz target in the same change.
4. `cargo fmt`, `clippy -D warnings`, `nextest`, affected WPT directories.
5. Update `docs/SPEC_REGISTRY.md` (feature row), `docs/FEATURE_MATRIX.md` if the status changed, `MEMORY.md` (session log).
6. Commit: `scope: imperative summary (spec §ref)`; body — what and why, numbers for perf/memory.

## 6. CI (GitHub Actions, `.github/workflows/`)

| Job | Trigger | What |
|---|---|---|
| `check` | PR | fmt, clippy, deny, doc |
| `test-{macos,windows,linux}` | PR | build + nextest |
| `wpt-smoke` | PR | affected directories + smoke shard, Linux |
| `wpt-full` | nightly | full sharded run, 3 OSes; pass/fail/crash/timeout dashboard |
| `fuzz-short` | PR | 60 s on changed targets |
| `fuzz-long` | nightly | 30 min on all targets, corpus in artifacts |
| `bench-mem` | PR with `perf` label + nightly | corpus, RSS per process, −5% gate |
| `release` | tag | signed builds, SBOM (`cargo cyclonedx`), macOS notarization |

## 7. Debugging

- `RUST_LOG=cl_net=debug,cl_ipc=trace` — `tracing-subscriber` env filter.
- `--trace-out=trace.json` — Perfetto-compatible trace of all processes.
- `--single-process --no-sandbox` — only for the debugger; in release these flags are physically absent (cfg).
- Renderer crash → `~/Library/Application Support/chrome-light/crashes/*.dmp` (macOS); similarly per OS. Dumps do not contain page content.
- DevTools: `--remote-debugging-port=9222` → CDP; connect with the Chrome DevTools frontend.

## 8. Secrets and privacy in dev

- Test profiles — synthetic only. Never commit a real profile (Chrome's or ours).
- Chrome import is tested on fixtures in `tests/fixtures/chrome-profile/` (generated by a script), not on the owner's profile.
- Sync server locally: `cargo run -p cl-sync-server -- --dev` (SQLite, self-signed TLS).
