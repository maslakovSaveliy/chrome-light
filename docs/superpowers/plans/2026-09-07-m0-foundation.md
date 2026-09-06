# M0 Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A Cargo workspace where the `chromelight` binary spawns a sandboxed-by-type renderer child of itself, completes a validated IPC handshake, and a headless testshell writes/compares PNGs — all green in CI on macOS, Windows, Linux.

**Architecture:** Multi-process from the first commit (ADR-0005): one binary, `--type=` selects browser/renderer/network/gpu/utility. Bootstrap uses `ipc-channel` one-shot server named in argv; all payloads go over raw bytes channels encoded with our `postcard` codec so the decoder we fuzz is the real wire. Sandbox is a type-state (`Sandbox<Unapplied> → Sandbox<Applied>`): renderer main cannot start without it; in M0 the only release policy is `NotImplementedPolicy` (always fails), dev builds accept `--no-sandbox`.

**Tech Stack:** Rust 1.95 stable, edition 2024, `ipc-channel` 0.23, `postcard` 1, `serde` 1, `thiserror` 2, `clap` 4, `tracing` 0.1 + `tracing-subscriber` 0.3 + `tracing-chrome` 0.7, `tiny-skia` 0.12, `parking_lot` 0.12, `directories` 6, `proptest` 1, `cargo-fuzz`, `cargo-nextest`, `cargo-deny`.

**Spec:** `docs/PLAN.md` §2 M0, `docs/ARCHITECTURE.md` §2–3, §8, ADR-0002, ADR-0005, ADR-0009, ADR-0010.

## Global Constraints

- Rust toolchain pinned by `rust-toolchain.toml`: `channel = "1.95.0"`, targets `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu`. Edition 2024.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` must pass. No `unwrap()`/`expect()` outside `#[cfg(test)]`. No `panic!` on untrusted input.
- `unsafe_code = "deny"` at workspace; every non-FFI crate has `#![forbid(unsafe_code)]`. M0 writes zero `unsafe`.
- Platform-specific code (`#[cfg(target_os)]`, `cfg(windows)`, `cfg(unix)`) only in `crates/platform`, `crates/process`, `crates/gfx`. Enforced by `tools/check-platform-cfg.sh`.
- Non-interactive shells on the owner's Mac: prefix commands with `export PATH="$HOME/.cargo/bin:$PATH"`.
- Commit messages: English, imperative, `scope: summary`. End with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`.
- Crate naming: package `cl-<name>`, lib target `cl_<name>`. Binary package `chromelight`.
- License field on every crate: `license = "Apache-2.0 OR MIT"`.
- Logging only via `tracing`; `println!` only in `apps/` and `crates/testshell` binaries with `#![expect(clippy::print_stdout, reason = "...")]`.
- Every IPC message type implements `cl_ipc::Validate`; every decoder gets a fuzz target in the same task.

---

## File Structure

```
Cargo.toml                       workspace: members, lints, shared deps, profiles
crates/platform/                 cl-platform: ProcessType, Clock (+FakeClock), paths
  Cargo.toml
  src/lib.rs
  src/process_type.rs
  src/clock.rs
  src/paths.rs
crates/ipc/                      cl-ipc: message enums, codec (postcard + size limit), Validate, bootstrap, handshake
  Cargo.toml
  src/lib.rs
  src/error.rs
  src/message.rs
  src/codec.rs
  src/validate.rs
  src/bootstrap.rs
  src/handshake.rs
  tests/bootstrap_roundtrip.rs
crates/process/                  cl-process: spawn child of same binary, sandbox type-state
  Cargo.toml
  src/lib.rs
  src/error.rs
  src/sandbox.rs
  src/spawn.rs
crates/testshell/                cl-testshell: lib (render_blank, compare_png, parse_viewport) + bin
  Cargo.toml
  src/lib.rs
  src/main.rs
  tests/cli.rs
apps/chromelight/                main binary
  Cargo.toml
  src/main.rs                    arg parsing, tracing init, dispatch by --type
  src/args.rs
  src/browser.rs                 browser process main
  src/child.rs                   renderer/network/gpu/utility main (M0: all identical)
  src/telemetry.rs               tracing subscriber + --trace-out
  tests/spawn.rs                 integration: full spawn + handshake
tools/fuzz/                      separate cargo-fuzz workspace (excluded from root)
  Cargo.toml
  fuzz_targets/ipc_decode.rs
tools/bench/mem.sh               RSS per process → JSON
tools/bench/README.md
tools/check-platform-cfg.sh
scripts/check-agents-md.sh
.github/workflows/ci.yml
docs/history/bench-2026-09.md    manual Chrome baseline (owner)
```

---

### Task 1: Workspace skeleton and `cl-platform::ProcessType`

**Files:**
- Create: `Cargo.toml`
- Create: `crates/platform/Cargo.toml`
- Create: `crates/platform/src/lib.rs`
- Create: `crates/platform/src/process_type.rs`
- Test: inline `#[cfg(test)]` in `process_type.rs`

**Interfaces:**
- Produces: `cl_platform::ProcessType { Browser, Renderer, Network, Gpu, Utility }` with `as_str(self) -> &'static str`, `is_sandboxed(self) -> bool`, `FromStr` (error `UnknownProcessType`), `Display`, `Serialize/Deserialize`.

- [ ] **Step 1: Create the workspace `Cargo.toml`**

```toml
[workspace]
resolver = "3"
members = ["crates/*", "apps/*"]
exclude = ["tools/fuzz"]

[workspace.package]
version = "0.0.1"
edition = "2024"
license = "Apache-2.0 OR MIT"
rust-version = "1.95"
repository = "https://github.com/chromelight/chrome-light"

[workspace.dependencies]
cl-platform = { path = "crates/platform" }
cl-ipc = { path = "crates/ipc" }
cl-process = { path = "crates/process" }
cl-testshell = { path = "crates/testshell" }

anyhow = "1"
clap = { version = "4", features = ["derive"] }
directories = "6"
ipc-channel = "0.23"
parking_lot = "0.12"
postcard = { version = "1", features = ["use-std"] }
proptest = "1"
serde = { version = "1", features = ["derive"] }
thiserror = "2"
tiny-skia = "0.12"
tracing = "0.1"
tracing-chrome = "0.7"
tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }

[workspace.lints.rust]
unsafe_code = "deny"
missing_docs = "warn"
unused_must_use = "deny"
rust_2018_idioms = { level = "warn", priority = -1 }

[workspace.lints.clippy]
all = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }
perf = { level = "deny", priority = -1 }
unwrap_used = "deny"
expect_used = "warn"
indexing_slicing = "warn"
panic = "deny"
todo = "deny"
dbg_macro = "deny"
print_stdout = "deny"
large_enum_variant = "warn"
redundant_clone = "warn"
needless_collect = "warn"
module_name_repetitions = "allow"
must_use_candidate = "allow"
missing_errors_doc = "allow"
missing_panics_doc = "allow"

[profile.release]
panic = "abort"
codegen-units = 1
lto = "thin"

# Release optimisations but debug assertions on, so DevNoSandbox exists. Used by tools/bench until real sandboxes land (M1/M2).
[profile.release-dev]
inherits = "release"
debug-assertions = true
```

- [ ] **Step 2: Create `crates/platform/Cargo.toml`**

```toml
[package]
name = "cl-platform"
description = "ChromeLight OS abstraction layer: process types, clocks, paths"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[lib]
name = "cl_platform"

[dependencies]
directories.workspace = true
parking_lot.workspace = true
serde.workspace = true
thiserror.workspace = true

[lints]
workspace = true
```

- [ ] **Step 3: Write the failing test in `crates/platform/src/process_type.rs`**

```rust
//! Process types of the multi-process architecture (ADR-0005).

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

/// Which role a process plays. One binary, selected by `--type=`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[non_exhaustive]
pub enum ProcessType {
    /// The single privileged process: UI, navigation, broker.
    Browser,
    /// Sandboxed web-content process, one per site.
    Renderer,
    /// Network service: TLS, HTTP, cache, cookies.
    Network,
    /// Compositor and raster.
    Gpu,
    /// Short-lived sandboxed helper (decoders).
    Utility,
}

/// Error for an unrecognised `--type=` value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown process type {0:?} (expected browser|renderer|network|gpu|utility)")]
pub struct UnknownProcessType(pub String);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_str_should_parse_all_known_names() {
        assert_eq!("browser".parse::<ProcessType>(), Ok(ProcessType::Browser));
        assert_eq!("renderer".parse::<ProcessType>(), Ok(ProcessType::Renderer));
        assert_eq!("network".parse::<ProcessType>(), Ok(ProcessType::Network));
        assert_eq!("gpu".parse::<ProcessType>(), Ok(ProcessType::Gpu));
        assert_eq!("utility".parse::<ProcessType>(), Ok(ProcessType::Utility));
    }

    #[test]
    fn from_str_should_reject_unknown_name() {
        assert_eq!(
            "kernel".parse::<ProcessType>(),
            Err(UnknownProcessType("kernel".to_owned()))
        );
    }

    #[test]
    fn as_str_should_round_trip_through_from_str() {
        for pt in [
            ProcessType::Browser,
            ProcessType::Renderer,
            ProcessType::Network,
            ProcessType::Gpu,
            ProcessType::Utility,
        ] {
            assert_eq!(pt.as_str().parse::<ProcessType>(), Ok(pt));
        }
    }

    #[test]
    fn only_browser_should_be_unsandboxed() {
        assert!(!ProcessType::Browser.is_sandboxed());
        assert!(ProcessType::Renderer.is_sandboxed());
        assert!(ProcessType::Network.is_sandboxed());
        assert!(ProcessType::Gpu.is_sandboxed());
        assert!(ProcessType::Utility.is_sandboxed());
    }
}
```

- [ ] **Step 4: Create `crates/platform/src/lib.rs` and run the test to see it fail**

```rust
//! ChromeLight OS abstraction layer. The only crate (besides `cl-process` and
//! `cl-gfx` backends) allowed to contain platform-specific code. M0 contains no `unsafe`.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod process_type;

pub use process_type::{ProcessType, UnknownProcessType};
```

Run: `export PATH="$HOME/.cargo/bin:$PATH" && cargo test -p cl-platform`
Expected: compile error — `as_str`, `is_sandboxed`, `FromStr` not implemented.

- [ ] **Step 5: Implement `ProcessType` methods**

Append to `crates/platform/src/process_type.rs` above the `tests` module:

```rust
impl ProcessType {
    /// Stable command-line name used in `--type=<name>`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Browser => "browser",
            Self::Renderer => "renderer",
            Self::Network => "network",
            Self::Gpu => "gpu",
            Self::Utility => "utility",
        }
    }

    /// Every process except the browser runs under a sandbox policy (ADR-0005).
    #[must_use]
    pub const fn is_sandboxed(self) -> bool {
        !matches!(self, Self::Browser)
    }
}

impl FromStr for ProcessType {
    type Err = UnknownProcessType;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "browser" => Ok(Self::Browser),
            "renderer" => Ok(Self::Renderer),
            "network" => Ok(Self::Network),
            "gpu" => Ok(Self::Gpu),
            "utility" => Ok(Self::Utility),
            other => Err(UnknownProcessType(other.to_owned())),
        }
    }
}

impl fmt::Display for ProcessType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
```

- [ ] **Step 6: Run tests, clippy, fmt**

Run: `cargo test -p cl-platform && cargo clippy -p cl-platform --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: 4 tests pass, no warnings.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock crates/platform
git commit -m "platform: add workspace skeleton and ProcessType

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 2: `cl-platform::Clock` and `paths`

**Files:**
- Create: `crates/platform/src/clock.rs`
- Create: `crates/platform/src/paths.rs`
- Modify: `crates/platform/src/lib.rs`

**Interfaces:**
- Produces: `trait Clock: Send + Sync { fn now(&self) -> Instant; }`, `SystemClock`, `FakeClock::new() / advance(Duration)`, `paths::default_profile_dir() -> Result<PathBuf, PathsError>`.

- [ ] **Step 1: Write failing tests in `crates/platform/src/clock.rs`**

```rust
//! Injectable clock. Engine code never calls `Instant::now()` directly, so tests are deterministic
//! (see docs/CODING_STANDARDS.md §8 — no `sleep` in tests).

use std::time::{Duration, Instant};

use parking_lot::Mutex;

/// Source of monotonic time.
pub trait Clock: Send + Sync {
    /// Current monotonic instant.
    fn now(&self) -> Instant;
}

/// Real monotonic clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

/// Manually advanced clock for tests.
#[derive(Debug)]
pub struct FakeClock {
    now: Mutex<Instant>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_clock_should_not_move_without_advance() {
        let clock = FakeClock::new();
        let a = clock.now();
        let b = clock.now();
        assert_eq!(a, b);
    }

    #[test]
    fn fake_clock_should_move_by_exactly_advanced_duration() {
        let clock = FakeClock::new();
        let a = clock.now();
        clock.advance(Duration::from_millis(250));
        assert_eq!(clock.now() - a, Duration::from_millis(250));
    }

    #[test]
    fn system_clock_should_be_monotonic() {
        let clock = SystemClock;
        let a = clock.now();
        let b = clock.now();
        assert!(b >= a);
    }
}
```

- [ ] **Step 2: Write failing test in `crates/platform/src/paths.rs`**

```rust
//! Well-known directories. Profile data lives under the OS-appropriate local data dir.

use std::path::PathBuf;

/// Error locating a platform directory.
#[derive(Debug, thiserror::Error)]
pub enum PathsError {
    /// The OS did not report a home/data directory (e.g. broken `$HOME`).
    #[error("no local data directory available on this system")]
    NoDataDir,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_profile_dir_should_end_with_product_and_profile_segments() {
        let dir = default_profile_dir().expect("data dir available in test environment");
        let mut comps = dir.components().rev();
        assert_eq!(comps.next().map(|c| c.as_os_str().to_string_lossy().into_owned()), Some("Default".to_owned()));
        assert_eq!(comps.next().map(|c| c.as_os_str().to_string_lossy().into_owned()), Some("ChromeLight".to_owned()));
    }
}
```

- [ ] **Step 3: Register modules in `lib.rs` and run to see failures**

Replace the body of `crates/platform/src/lib.rs` after the attributes with:

```rust
pub mod clock;
pub mod paths;
pub mod process_type;

pub use clock::{Clock, FakeClock, SystemClock};
pub use paths::{PathsError, default_profile_dir};
pub use process_type::{ProcessType, UnknownProcessType};
```

Run: `cargo test -p cl-platform`
Expected: compile errors — missing `impl Clock`, `FakeClock::new`, `default_profile_dir`.

- [ ] **Step 4: Implement**

In `clock.rs`, above `tests`:

```rust
impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

impl FakeClock {
    /// A fake clock starting at an arbitrary fixed instant.
    #[must_use]
    pub fn new() -> Self {
        Self { now: Mutex::new(Instant::now()) }
    }

    /// Move the clock forward.
    pub fn advance(&self, by: Duration) {
        let mut now = self.now.lock();
        *now += by;
    }
}

impl Default for FakeClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Instant {
        *self.now.lock()
    }
}
```

In `paths.rs`, above `tests`:

```rust
/// `<os local data dir>/ChromeLight/Default`, e.g.
/// `~/Library/Application Support/ChromeLight/Default` on macOS,
/// `%LOCALAPPDATA%\ChromeLight\Default` on Windows, `~/.local/share/ChromeLight/Default` on Linux.
pub fn default_profile_dir() -> Result<PathBuf, PathsError> {
    let base = directories::BaseDirs::new().ok_or(PathsError::NoDataDir)?;
    Ok(base.data_local_dir().join("ChromeLight").join("Default"))
}
```

- [ ] **Step 5: Run tests, clippy, fmt**

Run: `cargo test -p cl-platform && cargo clippy -p cl-platform --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: 8 tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/platform
git commit -m "platform: add Clock/FakeClock and default_profile_dir

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 3: `cl-ipc` messages, codec and validation

**Files:**
- Create: `crates/ipc/Cargo.toml`
- Create: `crates/ipc/src/lib.rs`
- Create: `crates/ipc/src/error.rs`
- Create: `crates/ipc/src/message.rs`
- Create: `crates/ipc/src/codec.rs`
- Create: `crates/ipc/src/validate.rs`

**Interfaces:**
- Produces: `cl_ipc::PROTOCOL_VERSION: u32`, `message::{Hello, ToBrowser, ToChild}`, `codec::{encode, decode, MAX_MESSAGE_BYTES}`, `validate::{Validate, ReceiverCtx, IpcViolation}`, `error::IpcError`.
- Consumes: `cl_platform::ProcessType`.

- [ ] **Step 1: Create `crates/ipc/Cargo.toml`**

```toml
[package]
name = "cl-ipc"
description = "ChromeLight typed, validated IPC: messages, codec, bootstrap, handshake"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[lib]
name = "cl_ipc"

[dependencies]
cl-platform.workspace = true
ipc-channel.workspace = true
postcard.workspace = true
serde.workspace = true
thiserror.workspace = true
tracing.workspace = true

[dev-dependencies]
proptest.workspace = true

[lints]
workspace = true
```

- [ ] **Step 2: Write `error.rs`**

```rust
//! IPC error types.

use crate::validate::IpcViolation;

/// Any failure on an IPC endpoint.
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    /// OS-level transport failure (pipe closed, mach port dead, ...).
    #[error("ipc transport: {0}")]
    Transport(#[from] std::io::Error),
    /// `ipc-channel` reported an error (peer disconnected, deserialization of bootstrap).
    #[error("ipc channel: {0}")]
    Channel(String),
    /// Our own wire codec failed.
    #[error("ipc codec: {0}")]
    Codec(#[from] crate::codec::CodecError),
    /// Message failed validation at the receiver — treated as hostile.
    #[error("ipc violation: {0}")]
    Violation(#[from] IpcViolation),
    /// Peer sent a message not legal in the current handshake state.
    #[error("unexpected message during handshake: {0}")]
    UnexpectedMessage(&'static str),
    /// The peer rejected our hello.
    #[error("handshake rejected by peer")]
    Rejected,
}

impl From<ipc_channel::Error> for IpcError {
    fn from(e: ipc_channel::Error) -> Self {
        Self::Channel(e.to_string())
    }
}
```

- [ ] **Step 3: Write `message.rs` with tests**

```rust
//! Wire messages for M0. Every enum is `#[non_exhaustive]`-free on purpose: the codec must
//! reject unknown variants, so adding a variant is a protocol version bump.

use cl_platform::ProcessType;
use serde::{Deserialize, Serialize};

/// Bump on any wire-incompatible change. Checked in `Hello`.
pub const PROTOCOL_VERSION: u32 = 1;

/// First message from a child after bootstrap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    /// Must equal [`PROTOCOL_VERSION`].
    pub protocol_version: u32,
    /// What the child claims to be. Cross-checked against what the browser spawned.
    pub process_type: ProcessType,
    /// Child's own pid. Cross-checked against the spawned pid.
    pub pid: u32,
}

/// Child → browser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToBrowser {
    /// Handshake opener.
    Hello(Hello),
    /// Reply to [`ToChild::Ping`] carrying the same nonce.
    Pong(u64),
}

/// Browser → child.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToChild {
    /// Handshake answer.
    HelloAck {
        /// `false` means the child must exit immediately.
        accepted: bool,
    },
    /// Liveness probe with a nonce.
    Ping(u64),
    /// Orderly exit request.
    Shutdown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_version_should_be_one_in_m0() {
        assert_eq!(PROTOCOL_VERSION, 1);
    }

    #[test]
    fn hello_should_be_clone_and_eq() {
        let h = Hello { protocol_version: 1, process_type: ProcessType::Renderer, pid: 42 };
        assert_eq!(h.clone(), h);
    }
}
```

- [ ] **Step 4: Write `codec.rs` with failing tests**

```rust
//! Wire codec: `postcard` with a hard size limit. This is the exact decoder that runs on
//! hostile bytes in every process, so it is fuzzed (`tools/fuzz/fuzz_targets/ipc_decode.rs`).

use serde::{Serialize, de::DeserializeOwned};

/// Largest message we will encode or decode. Bulk data (frames, resources) goes through shared
/// memory, never through messages.
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

/// Codec failure. Never panics; hostile input yields `Err`.
#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    /// Encoded size exceeds [`MAX_MESSAGE_BYTES`].
    #[error("message too large: {size} bytes > {max}")]
    TooLarge {
        /// Offending size.
        size: usize,
        /// The limit.
        max: usize,
    },
    /// `postcard` could not (de)serialize.
    #[error("postcard: {0}")]
    Postcard(#[from] postcard::Error),
    /// Bytes remained after a complete value — a framing violation.
    #[error("trailing bytes after message: {0}")]
    Trailing(usize),
}

#[cfg(test)]
mod tests {
    use cl_platform::ProcessType;
    use proptest::prelude::*;

    use super::*;
    use crate::message::{Hello, ToBrowser};

    #[test]
    fn encode_then_decode_should_round_trip_hello() {
        let msg = ToBrowser::Hello(Hello { protocol_version: 1, process_type: ProcessType::Gpu, pid: 7 });
        let bytes = encode(&msg).expect("encode");
        let back: ToBrowser = decode(&bytes).expect("decode");
        assert_eq!(back, msg);
    }

    #[test]
    fn decode_should_reject_oversized_input_before_parsing() {
        let big = vec![0u8; MAX_MESSAGE_BYTES + 1];
        let err = decode::<ToBrowser>(&big).expect_err("must reject");
        assert!(matches!(err, CodecError::TooLarge { .. }));
    }

    #[test]
    fn decode_should_reject_trailing_bytes() {
        let msg = ToBrowser::Pong(9);
        let mut bytes = encode(&msg).expect("encode");
        bytes.push(0xFF);
        let err = decode::<ToBrowser>(&bytes).expect_err("must reject");
        assert!(matches!(err, CodecError::Trailing(1)));
    }

    #[test]
    fn decode_should_reject_garbage_without_panicking() {
        for bytes in [&[][..], &[0xFF][..], &[0x05, 0x00, 0x00][..]] {
            let _ = decode::<ToBrowser>(bytes);
        }
    }

    proptest! {
        #[test]
        fn pong_round_trips_for_any_nonce(n in any::<u64>()) {
            let bytes = encode(&ToBrowser::Pong(n)).expect("encode");
            prop_assert_eq!(decode::<ToBrowser>(&bytes).expect("decode"), ToBrowser::Pong(n));
        }

        #[test]
        fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..256)) {
            let _ = decode::<ToBrowser>(&bytes);
        }
    }
}
```

- [ ] **Step 5: Write `validate.rs` with failing tests**

```rust
//! Receiver-side validation. The receiver decides what it expects; the payload never gets to
//! assert its own identity (docs/SECURITY.md §3).

use cl_platform::ProcessType;

use crate::message::{Hello, PROTOCOL_VERSION, ToBrowser, ToChild};

/// What the receiving side knows independently of the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReceiverCtx {
    /// The process type the browser actually spawned.
    pub expected_process_type: ProcessType,
    /// The pid the browser actually spawned (None when unknown, e.g. in-process tests).
    pub expected_pid: Option<u32>,
}

/// A message that fails validation. Treated as hostile: log and drop the connection.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum IpcViolation {
    /// Wire protocol mismatch.
    #[error("protocol version {got} != expected {expected}")]
    VersionMismatch {
        /// Version claimed by peer.
        got: u32,
        /// Our version.
        expected: u32,
    },
    /// Peer claims to be a different process type than we spawned.
    #[error("process type mismatch: claimed {claimed}, expected {expected}")]
    ProcessTypeMismatch {
        /// Claimed.
        claimed: ProcessType,
        /// Spawned.
        expected: ProcessType,
    },
    /// Peer claims a pid we did not spawn.
    #[error("pid mismatch: claimed {claimed}, expected {expected}")]
    PidMismatch {
        /// Claimed.
        claimed: u32,
        /// Spawned.
        expected: u32,
    },
}

/// Implemented by every message type crossing a process boundary.
pub trait Validate {
    /// Check the message against what the receiver independently knows.
    fn validate(&self, ctx: &ReceiverCtx) -> Result<(), IpcViolation>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ReceiverCtx {
        ReceiverCtx { expected_process_type: ProcessType::Renderer, expected_pid: Some(100) }
    }

    fn hello() -> Hello {
        Hello { protocol_version: PROTOCOL_VERSION, process_type: ProcessType::Renderer, pid: 100 }
    }

    #[test]
    fn valid_hello_should_pass() {
        assert_eq!(hello().validate(&ctx()), Ok(()));
    }

    #[test]
    fn hello_with_wrong_version_should_fail() {
        let h = Hello { protocol_version: PROTOCOL_VERSION + 1, ..hello() };
        assert_eq!(
            h.validate(&ctx()),
            Err(IpcViolation::VersionMismatch { got: PROTOCOL_VERSION + 1, expected: PROTOCOL_VERSION })
        );
    }

    #[test]
    fn hello_claiming_browser_type_should_fail() {
        let h = Hello { process_type: ProcessType::Browser, ..hello() };
        assert_eq!(
            h.validate(&ctx()),
            Err(IpcViolation::ProcessTypeMismatch { claimed: ProcessType::Browser, expected: ProcessType::Renderer })
        );
    }

    #[test]
    fn hello_with_foreign_pid_should_fail() {
        let h = Hello { pid: 999, ..hello() };
        assert_eq!(h.validate(&ctx()), Err(IpcViolation::PidMismatch { claimed: 999, expected: 100 }));
    }

    #[test]
    fn hello_pid_should_be_skipped_when_receiver_does_not_know_pid() {
        let c = ReceiverCtx { expected_pid: None, ..ctx() };
        let h = Hello { pid: 999, ..hello() };
        assert_eq!(h.validate(&c), Ok(()));
    }

    #[test]
    fn pong_and_to_child_messages_should_always_pass() {
        assert_eq!(ToBrowser::Pong(1).validate(&ctx()), Ok(()));
        assert_eq!(ToChild::Ping(1).validate(&ctx()), Ok(()));
        assert_eq!(ToChild::Shutdown.validate(&ctx()), Ok(()));
        assert_eq!(ToChild::HelloAck { accepted: true }.validate(&ctx()), Ok(()));
    }
}
```

- [ ] **Step 6: Create `lib.rs` and run tests to see failures**

```rust
//! ChromeLight IPC: typed messages, size-limited codec, receiver-side validation, bootstrap and
//! handshake between the browser process and its children (ADR-0005, docs/SECURITY.md §3).
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod bootstrap;
pub mod codec;
pub mod error;
pub mod handshake;
pub mod message;
pub mod validate;

pub use error::IpcError;
pub use message::PROTOCOL_VERSION;
pub use validate::{IpcViolation, ReceiverCtx, Validate};
```

Temporarily create empty `bootstrap.rs` and `handshake.rs` containing only `//! Filled in Task 4.` so the crate compiles.

Run: `cargo test -p cl-ipc`
Expected: compile errors — `encode`/`decode` and `Validate` impls missing.

- [ ] **Step 7: Implement codec**

In `codec.rs`, above `tests`:

```rust
/// Serialize a message. Fails if the result would exceed [`MAX_MESSAGE_BYTES`].
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, CodecError> {
    let bytes = postcard::to_allocvec(value)?;
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(CodecError::TooLarge { size: bytes.len(), max: MAX_MESSAGE_BYTES });
    }
    Ok(bytes)
}

/// Deserialize exactly one message from `bytes`. Rejects oversized input before touching it and
/// rejects trailing bytes. Never panics on any input.
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, CodecError> {
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(CodecError::TooLarge { size: bytes.len(), max: MAX_MESSAGE_BYTES });
    }
    let (value, rest) = postcard::take_from_bytes::<T>(bytes)?;
    if !rest.is_empty() {
        return Err(CodecError::Trailing(rest.len()));
    }
    Ok(value)
}
```

- [ ] **Step 8: Implement `Validate`**

In `validate.rs`, above `tests`:

```rust
impl Validate for Hello {
    fn validate(&self, ctx: &ReceiverCtx) -> Result<(), IpcViolation> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(IpcViolation::VersionMismatch { got: self.protocol_version, expected: PROTOCOL_VERSION });
        }
        if self.process_type != ctx.expected_process_type {
            return Err(IpcViolation::ProcessTypeMismatch {
                claimed: self.process_type,
                expected: ctx.expected_process_type,
            });
        }
        if let Some(expected) = ctx.expected_pid
            && self.pid != expected
        {
            return Err(IpcViolation::PidMismatch { claimed: self.pid, expected });
        }
        Ok(())
    }
}

impl Validate for ToBrowser {
    fn validate(&self, ctx: &ReceiverCtx) -> Result<(), IpcViolation> {
        match self {
            Self::Hello(h) => h.validate(ctx),
            Self::Pong(_) => Ok(()),
        }
    }
}

impl Validate for ToChild {
    fn validate(&self, _ctx: &ReceiverCtx) -> Result<(), IpcViolation> {
        match self {
            Self::HelloAck { .. } | Self::Ping(_) | Self::Shutdown => Ok(()),
        }
    }
}
```

- [ ] **Step 9: Run tests, clippy, fmt**

Run: `cargo test -p cl-ipc && cargo clippy -p cl-ipc --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: all codec/validate/message tests pass (12 + 2 proptest).

- [ ] **Step 10: Commit**

```bash
git add crates/ipc Cargo.lock
git commit -m "ipc: add messages, size-limited postcard codec and receiver-side validation

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 4: `cl-ipc` bootstrap transport and handshake

**Files:**
- Modify: `crates/ipc/src/bootstrap.rs`
- Modify: `crates/ipc/src/handshake.rs`
- Test: `crates/ipc/tests/bootstrap_roundtrip.rs`

**Interfaces:**
- Produces:
  - `bootstrap::BootstrapServer::new() -> Result<Self, IpcError>`, `.name() -> &str`, `.accept_with_timeout(Duration) -> Result<BrowserEndpoint, IpcError>`
  - `bootstrap::connect_child(server_name: &str) -> Result<ChildEndpoint, IpcError>`
  - `BrowserEndpoint::send(&self, &ToChild)`, `::recv(&self) -> Result<ToBrowser, IpcError>`
  - `ChildEndpoint::send(&self, &ToBrowser)`, `::recv(&self) -> Result<ToChild, IpcError>`
  - `handshake::browser_side(&BrowserEndpoint, &ReceiverCtx) -> Result<Hello, IpcError>`
  - `handshake::child_side(&ChildEndpoint, ProcessType, pid: u32) -> Result<(), IpcError>`

- [ ] **Step 1: Write the failing integration test `crates/ipc/tests/bootstrap_roundtrip.rs`**

```rust
//! Two threads in one process play browser and child over real ipc-channel transport.

use std::{thread, time::Duration};

use cl_ipc::{
    ReceiverCtx,
    bootstrap::{BootstrapServer, connect_child},
    handshake,
    message::{ToBrowser, ToChild},
};
use cl_platform::ProcessType;

#[test]
fn child_should_connect_handshake_and_answer_ping() {
    let server = BootstrapServer::new().expect("server");
    let name = server.name().to_owned();

    let child = thread::spawn(move || {
        let ep = connect_child(&name).expect("connect");
        handshake::child_side(&ep, ProcessType::Renderer, 4242).expect("child handshake");
        loop {
            match ep.recv().expect("recv") {
                ToChild::Ping(n) => ep.send(&ToBrowser::Pong(n)).expect("send pong"),
                ToChild::Shutdown => break,
                ToChild::HelloAck { .. } => panic!("duplicate ack"),
            }
        }
    });

    let ep = server.accept_with_timeout(Duration::from_secs(10)).expect("accept");
    let ctx = ReceiverCtx { expected_process_type: ProcessType::Renderer, expected_pid: Some(4242) };
    let hello = handshake::browser_side(&ep, &ctx).expect("browser handshake");
    assert_eq!(hello.pid, 4242);

    ep.send(&ToChild::Ping(77)).expect("send ping");
    assert_eq!(ep.recv().expect("recv"), ToBrowser::Pong(77));
    ep.send(&ToChild::Shutdown).expect("send shutdown");
    child.join().expect("child thread");
}

#[test]
fn browser_should_reject_child_claiming_wrong_process_type() {
    let server = BootstrapServer::new().expect("server");
    let name = server.name().to_owned();

    let child = thread::spawn(move || {
        let ep = connect_child(&name).expect("connect");
        // Claims Gpu although the browser expects Renderer.
        let result = handshake::child_side(&ep, ProcessType::Gpu, 1);
        assert!(result.is_err(), "child must see rejection");
    });

    let ep = server.accept_with_timeout(Duration::from_secs(10)).expect("accept");
    let ctx = ReceiverCtx { expected_process_type: ProcessType::Renderer, expected_pid: Some(1) };
    let err = handshake::browser_side(&ep, &ctx).expect_err("must reject");
    assert!(matches!(err, cl_ipc::IpcError::Violation(_)), "got {err:?}");
    child.join().expect("child thread");
}

#[test]
fn accept_should_time_out_when_no_child_connects() {
    let server = BootstrapServer::new().expect("server");
    let err = server.accept_with_timeout(Duration::from_millis(200)).expect_err("must time out");
    assert!(matches!(err, cl_ipc::IpcError::Transport(_)), "got {err:?}");
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test -p cl-ipc --test bootstrap_roundtrip`
Expected: compile error — `bootstrap`/`handshake` items missing.

- [ ] **Step 3: Implement `bootstrap.rs`**

```rust
//! Process bootstrap. The browser creates a one-shot named server; the child receives the name
//! in argv, connects, and hands over two raw byte channels. All later traffic uses those channels
//! with our [`crate::codec`], so `ipc-channel`'s own serializer only ever sees the bootstrap
//! message.

use std::{sync::mpsc, thread, time::Duration};

use ipc_channel::ipc::{IpcBytesReceiver, IpcBytesSender, IpcOneShotServer, IpcSender, bytes_channel};
use serde::{Deserialize, Serialize};

use crate::{
    codec,
    error::IpcError,
    message::{ToBrowser, ToChild},
};

/// The single message that crosses the one-shot server: the child's ends of two byte channels.
#[derive(Serialize, Deserialize)]
struct BootstrapMsg {
    to_browser: IpcBytesReceiver,
    to_child: IpcBytesSender,
}

/// Browser-side endpoint talking to one child.
#[derive(Debug)]
pub struct BrowserEndpoint {
    tx: IpcBytesSender,
    rx: IpcBytesReceiver,
}

/// Child-side endpoint talking to the browser.
#[derive(Debug)]
pub struct ChildEndpoint {
    tx: IpcBytesSender,
    rx: IpcBytesReceiver,
}

impl BrowserEndpoint {
    /// Send one message to the child.
    pub fn send(&self, msg: &ToChild) -> Result<(), IpcError> {
        let bytes = codec::encode(msg)?;
        self.tx.send(&bytes)?;
        Ok(())
    }

    /// Block for the next message from the child. Decoding failure is an error, never a panic.
    pub fn recv(&self) -> Result<ToBrowser, IpcError> {
        let bytes = self.rx.recv()?;
        Ok(codec::decode(&bytes)?)
    }
}

impl ChildEndpoint {
    /// Send one message to the browser.
    pub fn send(&self, msg: &ToBrowser) -> Result<(), IpcError> {
        let bytes = codec::encode(msg)?;
        self.tx.send(&bytes)?;
        Ok(())
    }

    /// Block for the next message from the browser.
    pub fn recv(&self) -> Result<ToChild, IpcError> {
        let bytes = self.rx.recv()?;
        Ok(codec::decode(&bytes)?)
    }
}

/// One-shot named server created by the browser before spawning a child.
#[derive(Debug)]
pub struct BootstrapServer {
    server: IpcOneShotServer<BootstrapMsg>,
    name: String,
}

impl BootstrapServer {
    /// Create a server with a fresh OS-level name to pass to the child as `--ipc-bootstrap=`.
    pub fn new() -> Result<Self, IpcError> {
        let (server, name) = IpcOneShotServer::new()?;
        Ok(Self { server, name })
    }

    /// The name the child must connect to.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Wait for the child to connect. `ipc-channel`'s accept has no timeout, so it runs on a
    /// helper thread; a hung or crashed child yields `IpcError::Transport(TimedOut)` and the
    /// caller kills it.
    pub fn accept_with_timeout(self, timeout: Duration) -> Result<BrowserEndpoint, IpcError> {
        let (tx, rx) = mpsc::channel();
        let server = self.server;
        thread::Builder::new()
            .name("ipc-bootstrap-accept".into())
            .spawn(move || {
                let result = server.accept().map(|(_, msg)| msg);
                // Receiver may have timed out and gone away; nothing to do then.
                let _ = tx.send(result);
            })?;
        match rx.recv_timeout(timeout) {
            Ok(Ok(msg)) => Ok(BrowserEndpoint { tx: msg.to_child, rx: msg.to_browser }),
            Ok(Err(e)) => Err(IpcError::Channel(e.to_string())),
            Err(mpsc::RecvTimeoutError::Timeout) => Err(IpcError::Transport(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "child did not connect to bootstrap server in time",
            ))),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(IpcError::Channel("bootstrap accept thread died".into()))
            }
        }
    }
}

/// Called by a child with the name from `--ipc-bootstrap=`.
pub fn connect_child(server_name: &str) -> Result<ChildEndpoint, IpcError> {
    let (to_browser_tx, to_browser_rx) = bytes_channel()?;
    let (to_child_tx, to_child_rx) = bytes_channel()?;
    let boot: IpcSender<BootstrapMsg> = IpcSender::connect(server_name.to_owned())?;
    boot.send(BootstrapMsg { to_browser: to_browser_rx, to_child: to_child_tx })?;
    Ok(ChildEndpoint { tx: to_browser_tx, rx: to_child_rx })
}
```

- [ ] **Step 4: Implement `handshake.rs`**

```rust
//! Hello / HelloAck exchange. The browser validates the child's claims against what it spawned.

use cl_platform::ProcessType;
use tracing::warn;

use crate::{
    bootstrap::{BrowserEndpoint, ChildEndpoint},
    error::IpcError,
    message::{Hello, PROTOCOL_VERSION, ToBrowser, ToChild},
    validate::{ReceiverCtx, Validate},
};

/// Browser side: wait for `Hello`, validate, ack. On violation the child is told `accepted: false`
/// and the error is returned so the caller kills the process.
pub fn browser_side(ep: &BrowserEndpoint, ctx: &ReceiverCtx) -> Result<Hello, IpcError> {
    let msg = ep.recv()?;
    let ToBrowser::Hello(hello) = msg else {
        ep.send(&ToChild::HelloAck { accepted: false })?;
        return Err(IpcError::UnexpectedMessage("expected Hello"));
    };
    if let Err(violation) = hello.validate(ctx) {
        warn!(%violation, "rejecting child during handshake");
        ep.send(&ToChild::HelloAck { accepted: false })?;
        return Err(violation.into());
    }
    ep.send(&ToChild::HelloAck { accepted: true })?;
    Ok(hello)
}

/// Child side: send `Hello`, wait for ack.
pub fn child_side(ep: &ChildEndpoint, process_type: ProcessType, pid: u32) -> Result<(), IpcError> {
    ep.send(&ToBrowser::Hello(Hello { protocol_version: PROTOCOL_VERSION, process_type, pid }))?;
    match ep.recv()? {
        ToChild::HelloAck { accepted: true } => Ok(()),
        ToChild::HelloAck { accepted: false } => Err(IpcError::Rejected),
        ToChild::Ping(_) | ToChild::Shutdown => Err(IpcError::UnexpectedMessage("expected HelloAck")),
    }
}
```

- [ ] **Step 5: Run tests, clippy, fmt**

Run: `cargo test -p cl-ipc && cargo clippy -p cl-ipc --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: 3 integration tests + earlier unit tests pass. If `IpcOneShotServer::new` or `bytes_channel` signatures differ from the 0.23 docs, adjust per `cargo doc -p ipc-channel --open` — the public shape (named one-shot server, `connect(String)`, byte channels) has been stable since 0.16.

- [ ] **Step 6: Commit**

```bash
git add crates/ipc
git commit -m "ipc: add bootstrap over ipc-channel byte channels and validated handshake

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 5: Fuzz target for the IPC decoder

**Files:**
- Create: `tools/fuzz/Cargo.toml`
- Create: `tools/fuzz/fuzz_targets/ipc_decode.rs`
- Create: `tools/fuzz/README.md`

**Interfaces:**
- Consumes: `cl_ipc::codec::decode`, `cl_ipc::message::{ToBrowser, ToChild}`.

- [ ] **Step 1: Create `tools/fuzz/Cargo.toml`** (own workspace; nightly only here)

```toml
[package]
name = "cl-fuzz"
version = "0.0.0"
publish = false
edition = "2024"
license = "Apache-2.0 OR MIT"

[package.metadata]
cargo-fuzz = true

[dependencies]
libfuzzer-sys = "0.4"
cl-ipc = { path = "../../crates/ipc" }

[[bin]]
name = "ipc_decode"
path = "fuzz_targets/ipc_decode.rs"
test = false
doc = false
bench = false

[workspace]
```

- [ ] **Step 2: Write `tools/fuzz/fuzz_targets/ipc_decode.rs`**

```rust
#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Both directions share one decoder; any panic here is a security bug (docs/CODING_STANDARDS.md §2).
    let _ = cl_ipc::codec::decode::<cl_ipc::message::ToBrowser>(data);
    let _ = cl_ipc::codec::decode::<cl_ipc::message::ToChild>(data);
});
```

- [ ] **Step 3: Write `tools/fuzz/README.md`**

```markdown
# Fuzz targets

Nightly-only workspace (excluded from root). Run:

    rustup toolchain install nightly
    cd tools/fuzz && cargo +nightly fuzz run ipc_decode -- -max_total_time=60

Targets: `ipc_decode` (cl-ipc codec, both directions). Every new parser/decoder adds a target in the same PR (CLAUDE.md §3.1).
```

- [ ] **Step 4: Run the fuzzer for 60 seconds**

Run: `cd tools/fuzz && cargo +nightly fuzz run ipc_decode -- -max_total_time=60`
Expected: no crashes, exits 0. (If nightly isn't installed: `rustup toolchain install nightly` first.)

- [ ] **Step 5: Commit**

```bash
git add tools/fuzz
git commit -m "fuzz: add ipc_decode target for the postcard wire codec

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 6: `cl-process` sandbox type-state

**Files:**
- Create: `crates/process/Cargo.toml`
- Create: `crates/process/src/lib.rs`
- Create: `crates/process/src/error.rs`
- Create: `crates/process/src/sandbox.rs`

**Interfaces:**
- Produces: `sandbox::{Sandbox, Unapplied, Applied, SandboxPolicy, NotImplementedPolicy, SandboxError}`; `DevNoSandbox` only under `cfg(debug_assertions)`.
  - `Sandbox::<Unapplied>::new() -> Sandbox<Unapplied>`
  - `Sandbox<Unapplied>::apply(self, &dyn SandboxPolicy) -> Result<Sandbox<Applied>, SandboxError>`
  - `Sandbox<Applied>::policy_name(&self) -> &'static str`

- [ ] **Step 1: Create `crates/process/Cargo.toml`**

```toml
[package]
name = "cl-process"
description = "ChromeLight process spawning, sandbox policies and crash handling"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[lib]
name = "cl_process"

[dependencies]
cl-ipc.workspace = true
cl-platform.workspace = true
thiserror.workspace = true
tracing.workspace = true

[lints]
workspace = true
```

- [ ] **Step 2: Write `error.rs`**

```rust
//! Process-layer errors.

/// Failure to spawn or talk to a child.
#[derive(Debug, thiserror::Error)]
pub enum ProcessError {
    /// OS refused to spawn.
    #[error("spawn: {0}")]
    Spawn(#[source] std::io::Error),
    /// Bootstrap/handshake failed; child has been killed.
    #[error("ipc during bootstrap: {0}")]
    Ipc(#[from] cl_ipc::IpcError),
    /// Waiting on / killing the child failed.
    #[error("child process: {0}")]
    Child(#[source] std::io::Error),
}
```

- [ ] **Step 3: Write failing tests in `sandbox.rs`**

```rust
//! Sandbox type-state (ADR-0005, Gate S0). Renderer/network/gpu/utility mains take a
//! `Sandbox<Applied>` by value, so they cannot be constructed without applying a policy.
//!
//! M0 ships only `NotImplementedPolicy` (always fails) and, in debug builds, `DevNoSandbox`.
//! Real seatbelt/seccomp/AppContainer policies land in M1/M2 as further `SandboxPolicy` impls.

use std::marker::PhantomData;

/// Marker: policy not yet applied.
#[derive(Debug)]
pub struct Unapplied;

/// Marker: policy applied to the current process.
#[derive(Debug)]
pub struct Applied;

/// Why a policy could not be applied. Any error here must abort the child.
#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    /// No real policy exists yet for this platform/process type.
    #[error("sandbox not implemented for this platform yet; refusing to run untrusted content")]
    NotImplemented,
    /// OS call failed.
    #[error("sandbox os error: {0}")]
    Os(#[source] std::io::Error),
}

/// A sandbox policy for the calling process.
pub trait SandboxPolicy {
    /// Human-readable name for logs.
    fn name(&self) -> &'static str;
    /// Apply to the current process. Irreversible.
    fn apply(&self) -> Result<(), SandboxError>;
}

/// Proof-of-application token. `S` is [`Unapplied`] or [`Applied`].
#[derive(Debug)]
pub struct Sandbox<S> {
    policy_name: &'static str,
    _state: PhantomData<S>,
}

/// Release default: always fails. Exists so release binaries cannot accidentally run children
/// without a sandbox (docs/SECURITY.md §5 Gate S0).
#[derive(Debug, Default, Clone, Copy)]
pub struct NotImplementedPolicy;

/// Debug-only escape hatch behind `--no-sandbox`. Does not exist in release builds.
#[cfg(debug_assertions)]
#[derive(Debug, Default, Clone, Copy)]
pub struct DevNoSandbox;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_implemented_policy_should_fail_to_apply() {
        let err = Sandbox::new().apply(&NotImplementedPolicy).expect_err("must fail");
        assert!(matches!(err, SandboxError::NotImplemented));
    }

    #[cfg(debug_assertions)]
    #[test]
    fn dev_no_sandbox_should_apply_and_report_its_name() {
        let sb = Sandbox::new().apply(&DevNoSandbox).expect("dev policy applies");
        assert_eq!(sb.policy_name(), "dev-no-sandbox");
    }

    #[test]
    fn applied_sandbox_should_not_be_constructible_without_apply() {
        // Compile-time property: `Sandbox<Applied>` has no public constructor. This test documents
        // it; the real guard is the private field + PhantomData. If someone adds a constructor,
        // review must reject it.
        fn takes_applied(_: Sandbox<Applied>) {}
        #[cfg(debug_assertions)]
        takes_applied(Sandbox::new().apply(&DevNoSandbox).expect("dev policy applies"));
        #[cfg(not(debug_assertions))]
        let _ = takes_applied;
    }
}
```

- [ ] **Step 4: Create `lib.rs` and see tests fail**

```rust
//! ChromeLight process layer: spawning same-binary children, sandbox type-state, crash handling.
//! Allowed to hold platform-specific code and (later) `unsafe` FFI to OS sandbox APIs; M0 has none.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod error;
pub mod sandbox;
pub mod spawn;

pub use error::ProcessError;
pub use sandbox::{Applied, NotImplementedPolicy, Sandbox, SandboxError, SandboxPolicy, Unapplied};
pub use spawn::{ChildProcess, spawn};
```

Create `spawn.rs` with only `//! Filled in Task 7.` for now.

Run: `cargo test -p cl-process`
Expected: compile errors — `Sandbox::new`, `apply`, `policy_name`, policy impls missing.

- [ ] **Step 5: Implement**

In `sandbox.rs`, above `tests`:

```rust
impl Sandbox<Unapplied> {
    /// Start the type-state. Nothing has happened to the process yet.
    #[must_use]
    pub fn new() -> Self {
        Self { policy_name: "unapplied", _state: PhantomData }
    }

    /// Apply `policy` to the current process and return proof.
    pub fn apply(self, policy: &dyn SandboxPolicy) -> Result<Sandbox<Applied>, SandboxError> {
        policy.apply()?;
        tracing::info!(policy = policy.name(), "sandbox applied");
        Ok(Sandbox { policy_name: policy.name(), _state: PhantomData })
    }
}

impl Default for Sandbox<Unapplied> {
    fn default() -> Self {
        Self::new()
    }
}

impl Sandbox<Applied> {
    /// Name of the policy that was applied.
    #[must_use]
    pub fn policy_name(&self) -> &'static str {
        self.policy_name
    }
}

impl SandboxPolicy for NotImplementedPolicy {
    fn name(&self) -> &'static str {
        "not-implemented"
    }

    fn apply(&self) -> Result<(), SandboxError> {
        Err(SandboxError::NotImplemented)
    }
}

#[cfg(debug_assertions)]
impl SandboxPolicy for DevNoSandbox {
    fn name(&self) -> &'static str {
        "dev-no-sandbox"
    }

    fn apply(&self) -> Result<(), SandboxError> {
        tracing::warn!("running WITHOUT a sandbox (debug build, --no-sandbox). Never load untrusted content.");
        Ok(())
    }
}
```

- [ ] **Step 6: Run tests, clippy, fmt (debug and release)**

Run: `cargo test -p cl-process && cargo test -p cl-process --release && cargo clippy -p cl-process --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: debug: 3 tests pass; release: 2 tests pass (dev-policy test compiled out).

- [ ] **Step 7: Commit**

```bash
git add crates/process Cargo.lock
git commit -m "process: add Sandbox<Unapplied -> Applied> type-state with NotImplemented and dev policies

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 7: `cl-process::spawn` — launch a child of the same binary

**Files:**
- Modify: `crates/process/src/spawn.rs`
- Test: inline unit test for arg construction; process-level test lives in Task 8 (needs the binary).

**Interfaces:**
- Produces: `spawn(exe: &Path, process_type: ProcessType, extra_args: &[String], timeout: Duration) -> Result<ChildProcess, ProcessError>`; `ChildProcess { pub process_type, pub endpoint: BrowserEndpoint, pub hello: Hello }` with `pid()`, `wait(&mut self) -> Result<ExitStatus, ProcessError>`, `kill(&mut self) -> Result<(), ProcessError>`; `pub fn child_args(process_type, bootstrap_name) -> [String; 2]`.

- [ ] **Step 1: Write the failing unit test in `spawn.rs`**

```rust
//! Spawn a child of the same executable with `--type=` and `--ipc-bootstrap=`, then run the
//! validated handshake. Any failure kills the child: a process that did not prove its identity
//! must not exist.

use std::{
    path::Path,
    process::{Child, Command, ExitStatus, Stdio},
    time::Duration,
};

use cl_ipc::{
    ReceiverCtx,
    bootstrap::{BootstrapServer, BrowserEndpoint},
    handshake,
    message::Hello,
};
use cl_platform::ProcessType;

use crate::error::ProcessError;

/// A live, handshaken child.
#[derive(Debug)]
pub struct ChildProcess {
    /// What we spawned it as.
    pub process_type: ProcessType,
    /// Channel to the child.
    pub endpoint: BrowserEndpoint,
    /// The validated hello.
    pub hello: Hello,
    child: Child,
}

/// Command-line arguments a child receives. Public so tests and the binary agree on the format.
#[must_use]
pub fn child_args(process_type: ProcessType, bootstrap_name: &str) -> [String; 2] {
    [format!("--type={process_type}"), format!("--ipc-bootstrap={bootstrap_name}")]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_args_should_encode_type_and_bootstrap_name() {
        assert_eq!(
            child_args(ProcessType::Renderer, "srv-1"),
            ["--type=renderer".to_owned(), "--ipc-bootstrap=srv-1".to_owned()]
        );
    }
}
```

- [ ] **Step 2: Run to see it fail**

Run: `cargo test -p cl-process`
Expected: compile error — `spawn` missing from `lib.rs` re-export.

- [ ] **Step 3: Implement spawn**

Append above `tests`:

```rust
/// Spawn `exe` as `process_type`, wait up to `timeout` for bootstrap, run the handshake.
pub fn spawn(
    exe: &Path,
    process_type: ProcessType,
    extra_args: &[String],
    timeout: Duration,
) -> Result<ChildProcess, ProcessError> {
    let server = BootstrapServer::new()?;
    let args = child_args(process_type, server.name());
    let mut child = Command::new(exe)
        .args(args)
        .args(extra_args)
        .stdin(Stdio::null())
        .spawn()
        .map_err(ProcessError::Spawn)?;
    let pid = child.id();
    tracing::info!(%process_type, pid, "spawned child");

    let endpoint = match server.accept_with_timeout(timeout) {
        Ok(ep) => ep,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e.into());
        }
    };
    let ctx = ReceiverCtx { expected_process_type: process_type, expected_pid: Some(pid) };
    let hello = match handshake::browser_side(&endpoint, &ctx) {
        Ok(h) => h,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e.into());
        }
    };
    Ok(ChildProcess { process_type, endpoint, hello, child })
}

impl ChildProcess {
    /// OS pid.
    #[must_use]
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Wait for exit.
    pub fn wait(&mut self) -> Result<ExitStatus, ProcessError> {
        self.child.wait().map_err(ProcessError::Child)
    }

    /// Kill and reap.
    pub fn kill(&mut self) -> Result<(), ProcessError> {
        self.child.kill().map_err(ProcessError::Child)?;
        self.child.wait().map_err(ProcessError::Child)?;
        Ok(())
    }
}
```

- [ ] **Step 4: Run tests, clippy, fmt**

Run: `cargo test -p cl-process && cargo clippy -p cl-process --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: pass.

- [ ] **Step 5: Commit**

```bash
git add crates/process
git commit -m "process: spawn same-binary child with bootstrap timeout and handshake

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 8: `chromelight` binary — browser ↔ renderer ping/pong

**Files:**
- Create: `apps/chromelight/Cargo.toml`
- Create: `apps/chromelight/src/main.rs`
- Create: `apps/chromelight/src/args.rs`
- Create: `apps/chromelight/src/telemetry.rs`
- Create: `apps/chromelight/src/browser.rs`
- Create: `apps/chromelight/src/child.rs`
- Test: `apps/chromelight/tests/spawn.rs`

**Interfaces:**
- Consumes: `cl_process::{spawn, Sandbox, NotImplementedPolicy, DevNoSandbox}`, `cl_ipc::{bootstrap::connect_child, handshake, message::*}`, `cl_platform::ProcessType`.
- Produces: CLI `chromelight [--type=T] [--ipc-bootstrap=NAME] [--no-sandbox] [--exit-after-handshake] [--idle-seconds=N] [--trace-out=PATH]`. Exit codes: 0 ok, 1 generic error, 78 (`EX_CONFIG`) sandbox unavailable.
- stdout line on success: `handshake ok renderer pid=<pid>`.

- [ ] **Step 1: Write the failing integration test `apps/chromelight/tests/spawn.rs`**

```rust
//! End-to-end: the real binary spawns a renderer child of itself and completes the handshake.

use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_chromelight"))
}

#[test]
fn browser_should_spawn_renderer_and_complete_handshake() {
    let out = bin().args(["--exit-after-handshake", "--no-sandbox"]).output().expect("run binary");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "exit={:?}\nstdout:\n{stdout}\nstderr:\n{stderr}", out.status.code());
    assert!(stdout.contains("handshake ok renderer pid="), "stdout:\n{stdout}\nstderr:\n{stderr}");
}

#[test]
fn renderer_without_sandbox_flag_should_exit_with_config_error() {
    // A child started directly with no bootstrap name and no --no-sandbox must refuse to run.
    let out = bin().args(["--type=renderer", "--ipc-bootstrap=does-not-exist"]).output().expect("run binary");
    assert_eq!(out.status.code(), Some(78), "stderr:\n{}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn trace_out_should_write_a_chrome_trace_file() {
    let dir = std::env::temp_dir().join(format!("cl-trace-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("tmp dir");
    let path = dir.join("trace.json");
    let out = bin()
        .args(["--exit-after-handshake", "--no-sandbox", "--trace-out"])
        .arg(&path)
        .output()
        .expect("run binary");
    assert!(out.status.success(), "stderr:\n{}", String::from_utf8_lossy(&out.stderr));
    let meta = std::fs::metadata(&path).expect("trace file exists");
    assert!(meta.len() > 2, "trace file should not be empty");
    let _ = std::fs::remove_dir_all(&dir);
}
```

- [ ] **Step 2: Create `apps/chromelight/Cargo.toml`**

```toml
[package]
name = "chromelight"
description = "ChromeLight browser — single binary, process role selected by --type"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[[bin]]
name = "chromelight"
path = "src/main.rs"

[dependencies]
anyhow.workspace = true
clap.workspace = true
cl-ipc.workspace = true
cl-platform.workspace = true
cl-process.workspace = true
tracing.workspace = true
tracing-chrome.workspace = true
tracing-subscriber.workspace = true

[lints]
workspace = true
```

- [ ] **Step 3: Write `args.rs`**

```rust
//! Command line. Children get `--type` and `--ipc-bootstrap` from the browser (see
//! `cl_process::spawn::child_args`); everything else is for humans and tests.

use std::path::PathBuf;

use clap::Parser;
use cl_platform::ProcessType;

/// ChromeLight process entry arguments.
#[derive(Debug, Parser)]
#[command(name = "chromelight", version, about)]
pub struct Args {
    /// Process role. Omit for the browser process.
    #[arg(long = "type", value_parser = parse_process_type, default_value = "browser")]
    pub process_type: ProcessType,

    /// Bootstrap server name handed to a child by the browser.
    #[arg(long = "ipc-bootstrap")]
    pub ipc_bootstrap: Option<String>,

    /// Debug builds only: run children without a sandbox. Never loads untrusted content.
    #[arg(long = "no-sandbox")]
    pub no_sandbox: bool,

    /// Browser: spawn one renderer, ping it, shut it down, exit 0. Used by tests and CI.
    #[arg(long = "exit-after-handshake")]
    pub exit_after_handshake: bool,

    /// Browser: stay alive this many seconds after handshake (benchmarks), then shut down.
    #[arg(long = "idle-seconds")]
    pub idle_seconds: Option<u64>,

    /// Write a Chrome-trace-format JSON of all tracing spans to this path.
    #[arg(long = "trace-out")]
    pub trace_out: Option<PathBuf>,
}

fn parse_process_type(s: &str) -> Result<ProcessType, String> {
    s.parse::<ProcessType>().map_err(|e| e.to_string())
}
```

- [ ] **Step 4: Write `telemetry.rs`**

```rust
//! tracing subscriber: human logs to stderr (RUST_LOG filter) plus optional Chrome trace file.

use std::path::Path;

use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

/// Keep alive until process exit so the trace file is flushed.
pub struct TraceGuard {
    _chrome: Option<tracing_chrome::FlushGuard>,
}

/// Install the global subscriber. Call once, first thing in `main`.
pub fn init(trace_out: Option<&Path>) -> anyhow::Result<TraceGuard> {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let fmt_layer = tracing_subscriber::fmt::layer().with_writer(std::io::stderr).with_target(true);

    let (chrome_layer, guard) = match trace_out {
        Some(path) => {
            let (layer, guard) = tracing_chrome::ChromeLayerBuilder::new().file(path).include_args(true).build();
            (Some(layer), Some(guard))
        }
        None => (None, None),
    };

    tracing_subscriber::registry().with(filter).with(fmt_layer).with(chrome_layer).try_init()?;
    Ok(TraceGuard { _chrome: guard })
}
```

- [ ] **Step 5: Write `browser.rs`**

```rust
//! Browser process main (M0: spawn one renderer, ping, optionally idle, shut down).

use std::time::Duration;

use cl_ipc::message::{ToBrowser, ToChild};
use cl_platform::ProcessType;
use cl_process::spawn;
use tracing::info;

use crate::args::Args;

const BOOTSTRAP_TIMEOUT: Duration = Duration::from_secs(15);

pub fn run(args: &Args) -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let mut extra = Vec::new();
    if args.no_sandbox {
        extra.push("--no-sandbox".to_owned());
    }
    if let Some(p) = &args.trace_out {
        // Child traces go next to the browser's trace, suffixed by role.
        let mut child_path = p.clone();
        child_path.set_extension("renderer.json");
        extra.push(format!("--trace-out={}", child_path.display()));
    }

    let mut renderer = spawn(&exe, ProcessType::Renderer, &extra, BOOTSTRAP_TIMEOUT)?;
    info!(pid = renderer.pid(), "renderer handshake complete");

    renderer.endpoint.send(&ToChild::Ping(1))?;
    match renderer.endpoint.recv()? {
        ToBrowser::Pong(1) => {}
        other => anyhow::bail!("expected Pong(1), got {other:?}"),
    }
    println!("handshake ok renderer pid={}", renderer.pid());

    if let Some(secs) = args.idle_seconds {
        info!(secs, "idling for benchmark");
        idle(Duration::from_secs(secs));
    }

    renderer.endpoint.send(&ToChild::Shutdown)?;
    let status = renderer.wait()?;
    anyhow::ensure!(status.success(), "renderer exited with {status}");
    if args.exit_after_handshake || args.idle_seconds.is_some() {
        return Ok(());
    }
    // M0-ONLY: no UI yet; a plain launch behaves like --exit-after-handshake.
    Ok(())
}

#[expect(clippy::disallowed_methods, reason = "benchmark idle in the binary, not engine code")]
fn idle(d: Duration) {
    std::thread::sleep(d);
}
```

- [ ] **Step 6: Write `child.rs`**

```rust
//! Main for every non-browser role. In M0 all roles behave identically: apply sandbox, connect,
//! handshake, answer pings until Shutdown.

use cl_ipc::{
    bootstrap::connect_child,
    handshake,
    message::{ToBrowser, ToChild},
};
use cl_process::{Sandbox, SandboxError, SandboxPolicy};
use tracing::{error, info};

use crate::args::Args;

/// `EX_CONFIG` from sysexits.h: the environment cannot run this role safely.
pub const EXIT_SANDBOX_UNAVAILABLE: i32 = 78;

/// Returns the process exit code.
pub fn run(args: &Args) -> i32 {
    let policy: Box<dyn SandboxPolicy> = choose_policy(args.no_sandbox);
    let sandbox = match Sandbox::new().apply(policy.as_ref()) {
        Ok(s) => s,
        Err(e @ SandboxError::NotImplemented) => {
            error!(%e, "refusing to start {} without sandbox", args.process_type);
            return EXIT_SANDBOX_UNAVAILABLE;
        }
        Err(e) => {
            error!(%e, "sandbox failed");
            return EXIT_SANDBOX_UNAVAILABLE;
        }
    };
    match serve(args, &sandbox) {
        Ok(()) => 0,
        Err(e) => {
            error!(%e, "child exiting with error");
            1
        }
    }
}

fn choose_policy(no_sandbox: bool) -> Box<dyn SandboxPolicy> {
    #[cfg(debug_assertions)]
    if no_sandbox {
        return Box::new(cl_process::sandbox::DevNoSandbox);
    }
    let _ = no_sandbox;
    Box::new(cl_process::NotImplementedPolicy)
}

fn serve(args: &Args, sandbox: &Sandbox<cl_process::Applied>) -> anyhow::Result<()> {
    let name = args.ipc_bootstrap.as_deref().ok_or_else(|| anyhow::anyhow!("--ipc-bootstrap is required"))?;
    let ep = connect_child(name)?;
    handshake::child_side(&ep, args.process_type, std::process::id())?;
    info!(role = %args.process_type, policy = sandbox.policy_name(), "child ready");
    loop {
        match ep.recv()? {
            ToChild::Ping(n) => ep.send(&ToBrowser::Pong(n))?,
            ToChild::Shutdown => return Ok(()),
            ToChild::HelloAck { .. } => anyhow::bail!("duplicate HelloAck"),
        }
    }
}
```

- [ ] **Step 7: Write `main.rs`**

```rust
//! ChromeLight entry point. One binary; `--type` selects the role (ADR-0005).
#![forbid(unsafe_code)]
#![expect(clippy::print_stdout, reason = "CLI success line consumed by tests and tooling")]

mod args;
mod browser;
mod child;
mod telemetry;

use clap::Parser;
use cl_platform::ProcessType;

fn main() {
    let args = args::Args::parse();
    let _guard = match telemetry::init(args.trace_out.as_deref()) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("chromelight: failed to init telemetry: {e}");
            std::process::exit(1);
        }
    };

    let code = match args.process_type {
        ProcessType::Browser => match browser::run(&args) {
            Ok(()) => 0,
            Err(e) => {
                tracing::error!(%e, "browser process failed");
                1
            }
        },
        ProcessType::Renderer | ProcessType::Network | ProcessType::Gpu | ProcessType::Utility => child::run(&args),
    };
    drop(_guard);
    std::process::exit(code);
}
```

Note: `std::process::exit` is on the clippy `disallowed-methods` list for engine crates; `apps/` is exempt per `clippy.toml` intent. If clippy flags it, add `#[expect(clippy::disallowed_methods, reason = "binary entry point")]` on `main`.

- [ ] **Step 8: Run the integration tests**

Run: `cargo test -p chromelight`
Expected: 3 tests pass (`browser_should_spawn…`, `renderer_without_sandbox…` exits 78 because `--no-sandbox` is absent, `trace_out…`). Then: `cargo clippy --workspace --all-targets -- -D warnings && cargo fmt --all -- --check`.

- [ ] **Step 9: Verify release refuses to run without sandbox**

Run: `cargo build --release -p chromelight && ./target/release/chromelight --exit-after-handshake --no-sandbox; echo "exit=$?"`
Expected: browser reports the renderer exited with code 78 → browser exits 1. Log line contains `refusing to start renderer without sandbox`. This is Gate S0 behaving.

- [ ] **Step 10: Commit**

```bash
git add apps/chromelight Cargo.lock
git commit -m "chromelight: single binary with --type dispatch, renderer spawn, ping/pong, trace output

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 9: `cl-testshell` — headless PNG render and compare

**Files:**
- Create: `crates/testshell/Cargo.toml`
- Create: `crates/testshell/src/lib.rs`
- Create: `crates/testshell/src/main.rs`
- Test: inline unit tests + `crates/testshell/tests/cli.rs`

**Interfaces:**
- Produces: `cl_testshell::{parse_viewport(&str) -> Result<(u32,u32), ShellError>, render_blank(u32,u32) -> Result<Pixmap, ShellError>, compare_png(&Path,&Path) -> Result<Diff, ShellError>, Diff { differing_pixels: u64, width: u32, height: u32 }}`.
- CLI: `cl-testshell render <input.html> --png <out> [--viewport WxH]`, `cl-testshell compare <a.png> <b.png> [--max-diff-pixels N]` → exit 0 if `differing_pixels <= N`, else 1.

- [ ] **Step 1: Create `crates/testshell/Cargo.toml`**

```toml
[package]
name = "cl-testshell"
description = "ChromeLight headless deterministic shell: render to PNG, compare PNGs (reftests, WPT product)"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[lib]
name = "cl_testshell"

[[bin]]
name = "cl-testshell"
path = "src/main.rs"

[dependencies]
anyhow.workspace = true
clap.workspace = true
thiserror.workspace = true
tiny-skia.workspace = true

[lints]
workspace = true
```

- [ ] **Step 2: Write failing tests in `lib.rs`**

```rust
//! Headless shell used by reftests and (from M1) the WPT product adapter. Deterministic by
//! construction: fixed viewport, no system fonts, CPU raster.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::path::Path;

use tiny_skia::{Color, Pixmap};

/// Testshell failure.
#[derive(Debug, thiserror::Error)]
pub enum ShellError {
    /// `--viewport` was not `WIDTHxHEIGHT` with both > 0.
    #[error("invalid viewport {0:?}, expected WIDTHxHEIGHT")]
    InvalidViewport(String),
    /// Pixmap allocation failed (zero size or too large).
    #[error("cannot allocate {width}x{height} pixmap")]
    Alloc {
        /// Width.
        width: u32,
        /// Height.
        height: u32,
    },
    /// PNG decode failed.
    #[error("png {path}: {source}")]
    Png {
        /// File.
        path: String,
        /// Cause.
        #[source]
        source: tiny_skia::png::DecodingError,
    },
    /// Two images differ in size.
    #[error("size mismatch: {a_w}x{a_h} vs {b_w}x{b_h}")]
    SizeMismatch {
        /// A width.
        a_w: u32,
        /// A height.
        a_h: u32,
        /// B width.
        b_w: u32,
        /// B height.
        b_h: u32,
    },
    /// I/O.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Result of comparing two PNGs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Diff {
    /// Pixels whose RGBA differs.
    pub differing_pixels: u64,
    /// Common width.
    pub width: u32,
    /// Common height.
    pub height: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_viewport_should_accept_width_x_height() {
        assert_eq!(parse_viewport("800x600").expect("ok"), (800, 600));
    }

    #[test]
    fn parse_viewport_should_reject_zero_and_garbage() {
        assert!(parse_viewport("0x600").is_err());
        assert!(parse_viewport("800").is_err());
        assert!(parse_viewport("axb").is_err());
        assert!(parse_viewport("800x600x1").is_err());
    }

    #[test]
    fn render_blank_should_be_opaque_white_of_requested_size() {
        let pm = render_blank(4, 3).expect("alloc");
        assert_eq!((pm.width(), pm.height()), (4, 3));
        assert!(pm.data().chunks(4).all(|px| px == [255, 255, 255, 255]));
    }

    #[test]
    fn compare_png_should_report_zero_for_identical_files() {
        let dir = std::env::temp_dir().join(format!("cl-ts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let a = dir.join("a.png");
        let b = dir.join("b.png");
        render_blank(8, 8).expect("alloc").save_png(&a).expect("save");
        std::fs::copy(&a, &b).expect("copy");
        let d = compare_png(&a, &b).expect("compare");
        assert_eq!(d, Diff { differing_pixels: 0, width: 8, height: 8 });
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn compare_png_should_count_one_changed_pixel() {
        let dir = std::env::temp_dir().join(format!("cl-ts2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let a = dir.join("a.png");
        let b = dir.join("b.png");
        render_blank(8, 8).expect("alloc").save_png(&a).expect("save");
        let mut pm = render_blank(8, 8).expect("alloc");
        if let Some(px) = pm.pixels_mut().get_mut(10) {
            *px = tiny_skia::PremultipliedColorU8::from_rgba(0, 0, 0, 255).expect("color");
        }
        pm.save_png(&b).expect("save");
        let d = compare_png(&a, &b).expect("compare");
        assert_eq!(d.differing_pixels, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn compare_png_should_fail_on_size_mismatch() {
        let dir = std::env::temp_dir().join(format!("cl-ts3-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let a = dir.join("a.png");
        let b = dir.join("b.png");
        render_blank(8, 8).expect("alloc").save_png(&a).expect("save");
        render_blank(9, 8).expect("alloc").save_png(&b).expect("save");
        assert!(matches!(compare_png(&a, &b), Err(ShellError::SizeMismatch { .. })));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
```

- [ ] **Step 3: Run to see failures**

Run: `cargo test -p cl-testshell`
Expected: compile errors — functions missing.

- [ ] **Step 4: Implement in `lib.rs` above `tests`**

```rust
/// Parse `WIDTHxHEIGHT`.
pub fn parse_viewport(s: &str) -> Result<(u32, u32), ShellError> {
    let bad = || ShellError::InvalidViewport(s.to_owned());
    let (w, h) = s.split_once('x').ok_or_else(bad)?;
    let w: u32 = w.parse().map_err(|_| bad())?;
    let h: u32 = h.parse().map_err(|_| bad())?;
    if w == 0 || h == 0 {
        return Err(bad());
    }
    Ok((w, h))
}

/// An opaque white canvas. M0-ONLY: the real pipeline (cl-html → … → cl-gfx) replaces this in M1;
/// the CLI and reftest harness stay the same.
pub fn render_blank(width: u32, height: u32) -> Result<Pixmap, ShellError> {
    let mut pm = Pixmap::new(width, height).ok_or(ShellError::Alloc { width, height })?;
    pm.fill(Color::WHITE);
    Ok(pm)
}

/// Count differing pixels between two PNGs of equal size.
pub fn compare_png(a: &Path, b: &Path) -> Result<Diff, ShellError> {
    let load = |p: &Path| {
        Pixmap::load_png(p).map_err(|source| ShellError::Png { path: p.display().to_string(), source })
    };
    let pa = load(a)?;
    let pb = load(b)?;
    if (pa.width(), pa.height()) != (pb.width(), pb.height()) {
        return Err(ShellError::SizeMismatch { a_w: pa.width(), a_h: pa.height(), b_w: pb.width(), b_h: pb.height() });
    }
    let differing_pixels = pa.pixels().iter().zip(pb.pixels()).filter(|(x, y)| x != y).count() as u64;
    Ok(Diff { differing_pixels, width: pa.width(), height: pa.height() })
}
```

If clippy warns `cast_possible_truncation` on `as u64`: use `u64::try_from(count).unwrap_or(u64::MAX)`.

- [ ] **Step 5: Write `main.rs`**

```rust
//! `cl-testshell` CLI.
#![forbid(unsafe_code)]
#![expect(clippy::print_stdout, reason = "CLI reports results on stdout")]

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use cl_testshell::{compare_png, parse_viewport, render_blank};

#[derive(Debug, Parser)]
#[command(name = "cl-testshell", version, about)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    /// Render an HTML file to PNG (M0: blank white canvas; input must exist).
    Render {
        input: PathBuf,
        #[arg(long)]
        png: PathBuf,
        #[arg(long, default_value = "800x600")]
        viewport: String,
    },
    /// Compare two PNGs; exit 0 if differing pixels <= max-diff-pixels.
    Compare {
        a: PathBuf,
        b: PathBuf,
        #[arg(long, default_value_t = 0)]
        max_diff_pixels: u64,
    },
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::Render { input, png, viewport } => {
            anyhow::ensure!(input.is_file(), "input {} is not a file", input.display());
            let (w, h) = parse_viewport(&viewport)?;
            render_blank(w, h)?.save_png(&png)?;
            println!("rendered {}x{} -> {}", w, h, png.display());
            Ok(())
        }
        Cmd::Compare { a, b, max_diff_pixels } => {
            let d = compare_png(&a, &b)?;
            println!("differing_pixels={} size={}x{}", d.differing_pixels, d.width, d.height);
            if d.differing_pixels > max_diff_pixels {
                std::process::exit(1);
            }
            Ok(())
        }
    }
}
```

- [ ] **Step 6: Write `crates/testshell/tests/cli.rs`**

```rust
use std::process::Command;

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_cl-testshell"))
}

#[test]
fn render_then_compare_should_round_trip_through_cli() {
    let dir = std::env::temp_dir().join(format!("cl-tscli-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let html = dir.join("page.html");
    std::fs::write(&html, "<!doctype html><p>hi</p>").expect("write");
    let a = dir.join("a.png");
    let b = dir.join("b.png");

    let r = bin().arg("render").arg(&html).arg("--png").arg(&a).arg("--viewport").arg("16x8").status().expect("run");
    assert!(r.success());
    let r = bin().arg("render").arg(&html).arg("--png").arg(&b).arg("--viewport").arg("16x8").status().expect("run");
    assert!(r.success());

    let r = bin().arg("compare").arg(&a).arg(&b).status().expect("run");
    assert_eq!(r.code(), Some(0));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn render_should_fail_when_input_missing() {
    let r = bin().args(["render", "/definitely/missing.html", "--png", "/tmp/x.png"]).status().expect("run");
    assert!(!r.success());
}
```

- [ ] **Step 7: Run tests, clippy, fmt**

Run: `cargo test -p cl-testshell && cargo clippy -p cl-testshell --all-targets -- -D warnings && cargo fmt --all -- --check`
Expected: 6 unit + 2 CLI tests pass.

- [ ] **Step 8: Commit**

```bash
git add crates/testshell Cargo.lock
git commit -m "testshell: headless render-to-PNG and PNG compare CLI for reftests

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 10: Memory bench script and Chrome baseline

**Files:**
- Create: `tools/bench/mem.sh`
- Create: `tools/bench/README.md`
- Create: `docs/history/bench-2026-09.md` (owner fills Chrome numbers)

**Interfaces:**
- Consumes: `chromelight --no-sandbox --idle-seconds N` (release-dev profile).
- Produces: `tools/bench/results/mem-<timestamp>.json` with `{timestamp, total_rss_kb, processes:[{pid, rss_kb, cmd}]}`.

- [ ] **Step 1: Write `tools/bench/mem.sh`**

```bash
#!/usr/bin/env bash
# Sample RSS of every ChromeLight process while the browser idles. macOS/Linux; Windows: M1 (ps1).
set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
cd "$(dirname "$0")/../.."

IDLE="${IDLE_SECONDS:-5}"
cargo build --profile release-dev -p chromelight --quiet
BIN="$(pwd)/target/release-dev/chromelight"
mkdir -p tools/bench/results
OUT="tools/bench/results/mem-$(date +%Y%m%d-%H%M%S).json"

"$BIN" --no-sandbox --idle-seconds "$IDLE" &
BROWSER_PID=$!
sleep 2

TOTAL=0
ROWS=""
for p in $(pgrep -f "$BIN" || true); do
  rss=$(ps -o rss= -p "$p" 2>/dev/null | tr -d ' ' || echo 0)
  cmd=$(ps -o args= -p "$p" 2>/dev/null | sed 's/"/\\"/g' || echo "?")
  [ -z "$rss" ] && continue
  TOTAL=$((TOTAL + rss))
  ROWS="${ROWS}{\"pid\":$p,\"rss_kb\":$rss,\"cmd\":\"$cmd\"},"
done

wait "$BROWSER_PID"
printf '{"timestamp":"%s","total_rss_kb":%d,"processes":[%s]}\n' \
  "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$TOTAL" "${ROWS%,}" > "$OUT"
cat "$OUT"
```

- [ ] **Step 2: Write `tools/bench/README.md`**

```markdown
# Bench

`mem.sh` — idle RSS of all ChromeLight processes (browser + renderer) using the `release-dev` profile
(release optimisations + debug assertions so `--no-sandbox` exists until real sandboxes land).

    ./tools/bench/mem.sh            # IDLE_SECONDS=5 by default
    IDLE_SECONDS=20 ./tools/bench/mem.sh

Results land in `tools/bench/results/` (gitignored). Monthly comparison against Chrome goes to
`docs/history/bench-YYYY-MM.md` (ADR-0012). Windows script and page corpus arrive in M1.
```

- [ ] **Step 3: Write `docs/history/bench-2026-09.md`**

```markdown
# Bench 2026-09 — baseline

Машина: MacBook (Apple Silicon, 16 ГБ), macOS 26.5.2.

## ChromeLight M0 (idle, browser + 1 renderer, no engine yet)

| Метрика | Значение |
|---|---|
| total_rss_kb (`tools/bench/mem.sh`) | заполнить после Task 10 Step 4 |

## Chrome 153 (ручное измерение владельца, Activity Monitor → Memory, сумма процессов Chrome)

| Сценарий | RSS суммарно |
|---|---|
| Пустой Chrome, 1 пустая вкладка (`chrome://blank`), без расширений | заполнить |
| 10 вкладок corpus v0 (example.com, wikipedia статья, HN, MDN страница, GitHub README ×2 повтора) | заполнить |

Метод: свежий профиль (`--user-data-dir=/tmp/chrome-bench`), 60 с ожидания, сумма RSS всех процессов `Google Chrome*`.
```

- [ ] **Step 4: Run it**

Run: `chmod +x tools/bench/mem.sh && ./tools/bench/mem.sh`
Expected: JSON with two processes (browser, renderer) and a total; copy `total_rss_kb` into `docs/history/bench-2026-09.md`.

- [ ] **Step 5: Commit**

```bash
git add tools/bench docs/history/bench-2026-09.md
git commit -m "bench: add idle RSS sampler and 2026-09 baseline sheet

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 11: Repo checks and CI matrix

**Files:**
- Create: `scripts/check-agents-md.sh`
- Create: `tools/check-platform-cfg.sh`
- Create: `.github/workflows/ci.yml`

**Interfaces:**
- CI jobs: `check` (fmt, clippy, deny, docs, scripts) on ubuntu; `test` matrix on macos-14 / windows-2022 / ubuntu-24.04 running `cargo nextest run --workspace` (fallback `cargo test`); `fuzz-short` on ubuntu nightly 60 s.

- [ ] **Step 1: Write `scripts/check-agents-md.sh`**

```bash
#!/usr/bin/env bash
# CI gate: AGENTS.md must be byte-identical to CLAUDE.md (CLAUDE.md §0).
set -euo pipefail
cd "$(dirname "$0")/.."
if ! cmp -s CLAUDE.md AGENTS.md; then
  echo "AGENTS.md differs from CLAUDE.md — run scripts/sync-agents-md.sh" >&2
  exit 1
fi
echo "AGENTS.md in sync"
```

- [ ] **Step 2: Write `tools/check-platform-cfg.sh`**

```bash
#!/usr/bin/env bash
# CI gate: platform cfgs only in cl-platform, cl-process, cl-gfx (ADR-0009).
set -euo pipefail
cd "$(dirname "$0")/.."
ALLOWED='^(crates/platform/|crates/process/|crates/gfx/)'
HITS=$(grep -rnE 'cfg\((target_os|windows|unix)' --include='*.rs' crates apps 2>/dev/null | grep -vE "$ALLOWED" || true)
if [ -n "$HITS" ]; then
  echo "platform-specific cfg outside allowed crates:" >&2
  echo "$HITS" >&2
  exit 1
fi
echo "platform cfg check ok"
```

- [ ] **Step 3: Write `.github/workflows/ci.yml`**

```yaml
name: ci
on:
  push:
    branches: [main]
  pull_request:

env:
  CARGO_TERM_COLOR: always
  RUSTFLAGS: -D warnings

jobs:
  check:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v4
      - run: rustup show   # installs the toolchain from rust-toolchain.toml
      - uses: Swatinem/rust-cache@v2
      - uses: taiki-e/install-action@v2
        with:
          tool: cargo-deny,cargo-nextest
      - run: cargo fmt --all -- --check
      - run: cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
      - run: cargo deny check
      - run: cargo doc --workspace --no-deps
      - run: bash scripts/check-agents-md.sh
      - run: bash tools/check-platform-cfg.sh

  test:
    strategy:
      fail-fast: false
      matrix:
        os: [macos-14, windows-2022, ubuntu-24.04]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - run: rustup show
      - uses: Swatinem/rust-cache@v2
      - uses: taiki-e/install-action@v2
        with:
          tool: cargo-nextest
      - run: cargo build --workspace --locked
      - run: cargo nextest run --workspace --locked
      - run: cargo test --workspace --doc --locked
      - name: release build refuses unsandboxed renderer (Gate S0)
        shell: bash
        run: |
          cargo build --release -p chromelight --locked
          set +e
          ./target/release/chromelight --exit-after-handshake --no-sandbox
          code=$?
          set -e
          test "$code" -ne 0

  fuzz-short:
    runs-on: ubuntu-24.04
    steps:
      - uses: actions/checkout@v4
      - run: rustup toolchain install nightly --profile minimal
      - uses: taiki-e/install-action@v2
        with:
          tool: cargo-fuzz
      - run: cd tools/fuzz && cargo +nightly fuzz build ipc_decode
      - run: cd tools/fuzz && cargo +nightly fuzz run ipc_decode -- -max_total_time=60
```

Windows note: `chromelight --no-sandbox` on Windows uses the same `--exit-after-handshake` path; `ipc-channel` supports Windows named pipes. If the Windows job fails on `pgrep` — that's only in `mem.sh`, which CI does not run.

- [ ] **Step 4: Run checks locally**

Run: `chmod +x scripts/check-agents-md.sh tools/check-platform-cfg.sh && bash scripts/check-agents-md.sh && bash tools/check-platform-cfg.sh && cargo deny check && cargo doc --workspace --no-deps`
Expected: all ok. (`cargo deny check` may flag duplicate crate versions as warnings — acceptable; errors on licenses/bans are not.)

- [ ] **Step 5: Commit and push to trigger CI (needs a GitHub remote — see MEMORY.md open question)**

```bash
git add scripts/check-agents-md.sh tools/check-platform-cfg.sh .github/workflows/ci.yml
git commit -m "ci: add check/test matrix (macOS, Windows, Linux), Gate S0 release check, short fuzz

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
```

---

### Task 12: Close M0 — docs, memory, tag

**Files:**
- Modify: `MEMORY.md` (state, session log, next action = M1 plan)
- Modify: `docs/ARCHITECTURE.md` §8 (mark existing crates)
- Modify: `docs/PLAN.md` M0 exit criteria checkboxes

- [ ] **Step 1: Tick M0 exit criteria in `docs/PLAN.md`** that CI proved; leave unticked anything not green on all three OSes and write why in MEMORY.md.

- [ ] **Step 2: Update `MEMORY.md`** — "Состояние" → M0 done/partial; session log entry with commit hashes; open questions; next: `docs/superpowers/plans/<date>-m1-static-pages.md`.

- [ ] **Step 3: Full verification**

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features --locked -- -D warnings && cargo nextest run --workspace && cargo deny check && bash scripts/check-agents-md.sh && bash tools/check-platform-cfg.sh`
Expected: all green locally; CI green on 3 OS (or documented exceptions).

- [ ] **Step 4: Commit and tag**

```bash
git add MEMORY.md docs/PLAN.md docs/ARCHITECTURE.md
git commit -m "docs: close M0 foundation milestone

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>"
git tag -a m0 -m "M0 foundation: workspace, IPC, process spawn, sandbox type-state, testshell, CI"
```

---

## Self-review

- **Spec coverage (PLAN.md M0):** workspace+lints+deny → T1; cl-platform → T1–T2; cl-ipc codec/validate/bootstrap/handshake/fuzz → T3–T5; cl-process sandbox/spawn → T6–T7; binary with `--type`, ping/pong, tracing, `--trace-out` → T8; testshell render/compare → T9; bench script + Chrome baseline → T10; CI + check scripts → T11; close-out → T12. Exit criterion "release refuses unsandboxed renderer" → T8 Step 9 + CI step in T11.
- **Placeholders:** none; `bench-2026-09.md` intentionally has owner-filled cells (manual measurement), not code placeholders.
- **Type consistency:** `BrowserEndpoint::{send(&ToChild), recv()->ToBrowser}` and `ChildEndpoint::{send(&ToBrowser), recv()->ToChild}` used identically in T4 tests, T7 spawn, T8 browser/child. `Sandbox::new().apply(&dyn SandboxPolicy) -> Sandbox<Applied>` used in T6 tests and T8 child. `child_args` format matches `Args` field names (`--type`, `--ipc-bootstrap`). Exit code 78 defined in T8 `child.rs` and asserted in T8 test and T11 CI.
- **Known risk:** exact `ipc-channel` 0.23 signatures (`IpcOneShotServer::new`, `bytes_channel`, `IpcBytesReceiver: Serialize`) — verified shape from docs, adjust locally with `cargo doc -p ipc-channel` if a name differs; the design does not change.
