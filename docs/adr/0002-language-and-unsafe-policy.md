# ADR-0002: Rust stable, edition 2024, `unsafe` policy

**Status:** Accepted
**Date:** 2026-09-07
**Deciders:** project owner

## Context

~70% of Chromium's serious security bugs are memory safety. Rust removes this class in safe code, but a browser inevitably contains FFI (V8, GPU, OS). We need a policy for where `unsafe` lives and how it is checked.

## Decision

- Rust **stable**, version pinned in `rust-toolchain.toml` (1.95 at start), edition 2024. Nightly — only for `cargo fuzz`/`miri`/`careful` in CI, never for building the product.
- `#![forbid(unsafe_code)]` in all crates except: `cl-platform`, `cl-process`, `cl-gfx` (backend modules), `cl-js` (V8 FFI), `cl-bindings/runtime`. There — `deny` on the crate + `allow` on the module, `unsafe_op_in_unsafe_fn = deny`, `undocumented_unsafe_blocks = deny`.
- External crates with `unsafe` are acceptable given: an active maintainer, `cargo vet`/audit, or wide usage (wgpu, v8, rusqlite).
- `panic = "abort"` in release for child processes. No panics on untrusted input (see CODING_STANDARDS §2).

## Options Considered

- **Rust everywhere without exceptions** — impossible: V8, Metal/DX12, seatbelt are FFI.
- **Mixed C++/Rust core (like Chromium/Gecko/Ladybird)** — rejected: a solo team can't handle two languages and two toolchains; the main argument for Rust is lost.
- **Rust stable + isolated unsafe (chosen).**

## Consequences

- Easier: review is concentrated on 5 crates; safe code does not require memory-safety review.
- Harder: some optimizations (custom allocators, SIMD) require unsafe → only in allowed crates, via safe wrappers.
- Revisit when: a need for unsafe arises in a new crate — a new ADR, not `allow`.

## Action Items

1. [ ] `[workspace.lints]` in the root Cargo.toml (see CODING_STANDARDS §3).
2. [ ] CI job: `cargo +nightly miri test -p cl-ipc -p cl-platform` (non-FFI modules), `cargo +nightly careful test`.
