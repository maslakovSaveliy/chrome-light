# Coding standards (Rust)

Baseline — Apollo Rust Best Practices (skill `rust-best-practices`) + browser specifics. The rules below are mandatory; exceptions — via `#[expect(lint, reason = "…")]`, not `#[allow]`.

## 1. Ownership and types

- Function parameters: `&str`, `&[T]`, `&Path`, `impl AsRef<…>`; `String`/`Vec<T>`/`PathBuf` — only when transferring ownership.
- `Copy` types ≤ 24 bytes — by value. Anything larger — by reference.
- `Cow<'_, str>` where ownership depends on the input (URL normalization, entity decoding).
- `.clone()` in the hot path (parser loop, style resolution, layout, paint) — only with a `// clone: <why>` comment.
- No `Rc<RefCell<…>>` in DOM/layout: DOM is an arena (`NodeId` indices into `Vec`/slab), layout tree is an immutable fragment tree per pass. References between trees are indices, not pointers.
- Newtypes for all IDs and units: `NodeId`, `SiteId`, `Px(f32)`, `Au(i32)` (app units for layout), `OriginKey`. A bare `u32`/`f32` in public API — reject.
- `Send`/`Sync`: everything that crosses threads is explicitly `Send`. DOM objects are `!Send` (renderer main thread); their indices are `Send`.

## 2. Errors

- Library crates: `thiserror`, a hierarchy per crate (`cl_net::Error`, `cl_dom::Error`), with `#[source]`.
- Binaries (`apps/`, `cl-sync-server`, `tools/`): `anyhow` is allowed.
- **Untrusted input never panics.** `unwrap`/`expect`/`indexing[]` on data from the network/page/IPC/profile file is a security-class bug. Use `.get()`, `checked_*`, `try_from`.
- `unwrap()`/`expect()` are allowed only in `#[cfg(test)]`, `build.rs`, and for invariants proven by the line above, with `expect("invariant: …")`.
- Propagate `Result` with `?`; don't `match` just to repackage.
- Panic policy: `panic = "abort"` in release for child processes (crash → clean dump, no unwinding through FFI).

## 3. Clippy and lints

Workspace `Cargo.toml`:

```toml
[workspace.lints.rust]
unsafe_code = "deny"            # forbid cannot be overridden; every non-FFI crate adds #![forbid(unsafe_code)] in lib.rs
missing_docs = "warn"
unused_must_use = "deny"
rust_2018_idioms = "warn"

[workspace.lints.clippy]
all = "warn"
pedantic = "warn"
perf = "deny"
unwrap_used = "deny"
expect_used = "warn"
indexing_slicing = "warn"       # deny in cl-net, cl-html, cl-ipc, cl-bindings
panic = "deny"
todo = "deny"
dbg_macro = "deny"
print_stdout = "deny"           # logging only via tracing
large_enum_variant = "warn"
redundant_clone = "warn"
needless_collect = "warn"
module_name_repetitions = "allow"
must_use_candidate = "allow"
```

`clippy.toml`: `too-many-arguments-threshold = 8`, `type-complexity-threshold = 300`, `cognitive-complexity-threshold = 30`, `disallowed-methods` for `std::process::exit` outside `apps/`, `std::env::var` outside `cl-platform`.

## 4. `unsafe`

Allowed **only** in: `cl-platform`, `cl-process`, `cl-gfx` (backend modules), `cl-js` (V8 FFI), `cl-bindings/runtime`. There: `#![deny(unsafe_code)]` at crate level + `#[allow(unsafe_code)]` on the specific module; `#![deny(unsafe_op_in_unsafe_fn)]`; `#![deny(clippy::undocumented_unsafe_blocks)]`.

Every block:

```rust
// SAFETY: `ptr` is obtained from a `v8::Local` in the current HandleScope and lives until the end of the scope;
// we do not keep it any longer (see lifetime 's).
unsafe { ... }
```

All `unsafe` changes — review checklist in the PR template; `cargo miri` for pure unsafe modules without FFI; `cargo +nightly careful` in nightly CI.

## 5. Performance

- Profile before optimizing: `samply`/Instruments on macOS, `perf` on Linux, ETW on Windows. Numbers go in the commit.
- Iterators instead of index loops; no intermediate `collect()`.
- Allocations in the hot path — `SmallVec`, arenas (`bumpalo`) for per-pass data (layout pass, paint pass), `Box<str>` instead of `String` for immutable strings in the DOM.
- `Box` large enum variants (`large_enum_variant`).
- DOM strings: atoms (`string_cache`/`markup5ever` `Atom`) for tags/attributes; text nodes — `Tendril`/`Box<str>`.
- Before M3, do not optimize anything a profile has not pointed to. Keep the reference path for differential tests.

## 6. Generics and dispatch

- Static dispatch in the engine. `dyn Trait` — only at boundaries: `JsRuntime`, `GfxBackend`, `StorageBackend`, `SandboxPolicy`, `PlatformFs`.
- Don't box inside a crate "for convenience"; box at the API boundary.
- Type-state for protocols: `IpcConnection<Handshaking>` → `IpcConnection<Ready>`; `Fetch<Pending>` → `Fetch<Redirected>` → `Fetch<Done>`; `Sandbox<Unapplied>` → `Sandbox<Applied>` (renderer main does not start without `Applied`).

## 7. Documentation and comments

- `///` on every pub item: what, invariants, panic contract ("never panics on any input"), link to the spec: `/// Implements <https://html.spec.whatwg.org/#tokenization> §13.2.5.1`.
- `//` — only *why*: workaround, security rationale, spec deviation with a link to WPT/issue.
- `// TODO(#123): …` — only with an issue. `todo!()` is forbidden (clippy deny).
- `// SPEC-DEVIATION(<spec>#<anchor>): <reason>; tracked in SPEC_REGISTRY` — a mandatory marker for any deviation.
- `// M1-ONLY:` — code that is allowed only until the specified milestone; grep gate in CI when the milestone closes.

## 8. Tests

- Names: `tokenizer_should_emit_eof_when_input_empty`. One assertion per test where possible.
- Golden/snapshot — `insta` (`cargo insta review`): parse trees, computed style, fragment trees, display lists.
- Property tests — `proptest` for URL, cookies, cache keys, IPC (de)serialization.
- Fuzz — `cargo-fuzz` targets in `tools/fuzz/`, `arbitrary` for structured inputs.
- No network tests without a local server (`tools/testserver`).
- A flake is a bug. A test with `sleep` — reject; use the deterministic clock from `cl-platform::Clock`.

## 9. Style

- `rustfmt.toml`: `edition = "2024"`, `max_width = 100`, `imports_granularity = "Crate"`, `group_imports = "StdExternalCrate"`.
- Modules: one concept — one file; we don't use `mod.rs` (`foo.rs` + `foo/`).
- A crate's public API — in `lib.rs` via `pub use`, internals `pub(crate)`.
- Feature flags — only for optional Web APIs (`webapi/canvas`, `webapi/workers`), not for platforms.

## 10. Dependencies

- Adding one — `cargo deny check` + a line in `docs/DEPENDENCIES.md` (crate, version, license, why, alternatives).
- Prefer crates with: >1 maintainer, a release within the last year, no `unsafe` or audited (`cargo vet`/RustSec).
- Versions pinned in `[workspace.dependencies]`; `Cargo.lock` is committed; updates — a separate PR with changelog links.
- Forbidden: `openssl` (rustls), `native-tls`, `reqwest` in the engine (own Fetch), any crates with networked build scripts except `v8` (prebuilt download with checksum).
