# ADR-0002: Rust stable, edition 2024, политика `unsafe`

**Status:** Accepted (amended by ADR-0015: cl-style added to the unsafe list)
**Date:** 2026-09-07
**Deciders:** владелец проекта

## Context

~70% серьёзных security-багов Chromium — memory safety. Rust убирает этот класс в safe-коде, но браузер неизбежно содержит FFI (V8, GPU, ОС). Нужна политика, где `unsafe` живёт и как проверяется.

## Decision

- Rust **stable**, версия закреплена в `rust-toolchain.toml` (1.95 на старте), edition 2024. Nightly — только для `cargo fuzz`/`miri`/`careful` в CI, никогда для сборки продукта.
- `#![forbid(unsafe_code)]` во всех crate-ах, кроме: `cl-platform`, `cl-process`, `cl-gfx` (backend модули), `cl-js` (V8 FFI), `cl-bindings/runtime`, `cl-style` (модули `store.rs`, `handle.rs`, `stylo_dom.rs`). Там — `deny` на crate + `allow` на модуль, `unsafe_op_in_unsafe_fn = deny`, `undocumented_unsafe_blocks = deny`.
- Внешние crate-ы с `unsafe` допустимы при: активном мейнтейнере, `cargo vet`/audit или широком использовании (wgpu, v8, rusqlite).
- `panic = "abort"` в release для child-процессов. Никакой паники на недоверенном входе (см. CODING_STANDARDS §2).

## Options Considered

- **Rust везде без исключений** — невозможно: V8, Metal/DX12, seatbelt — FFI.
- **Смешанный C++/Rust core (как Chromium/Gecko/Ladybird)** — отвергнут: соло-команда не потянет два языка и два тулчейна; теряется главный аргумент Rust.
- **Rust stable + изолированный unsafe (выбран).**

## Consequences

- Легче: review сосредоточен на 5 crate-ах; safe-код не требует memory-safety review.
- Труднее: некоторые оптимизации (custom allocators, SIMD) требуют unsafe → только в разрешённых crate-ах через безопасные обёртки.
- Пересмотреть когда: появится необходимость unsafe в новом crate — новый ADR, не `allow`.

## Action Items

1. [ ] `[workspace.lints]` в корневом Cargo.toml (см. CODING_STANDARDS §3).
2. [ ] CI job: `cargo +nightly miri test -p cl-ipc -p cl-platform` (не-FFI модули), `cargo +nightly careful test`.
