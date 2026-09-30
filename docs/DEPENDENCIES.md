# Dependencies

Every new dependency — a row here + `cargo deny check`. Versions — in `[workspace.dependencies]`; here — intent and rationale. Re-check licenses when adding (`cargo deny list`).

Allowed licenses (`deny.toml`): MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, MPL-2.0 (file-level copyleft, compatible with our Apache/MIT as long as files are unmodified; changes to stylo/cssparser — only via upstream or a separate published fork), Unicode-3.0, CC0-1.0. Forbidden: GPL/LGPL/AGPL (except via an explicit ADR), SSPL, BUSL.

## Engine

| Crate | Role | Why this one | Alternatives | Risk |
|---|---|---|---|---|
| `html5ever`, `markup5ever` | HTML tokenizer/tree builder | spec-conformant, Servo, WPT-verified, `Atom` | own parser | medium — API changes |
| `cssparser` | CSS tokenizer | required by stylo | — | low |
| `stylo` (+`stylo_atoms`, `stylo_dom`, `selectors`, `servo_arc`) | CSS cascade/computed style | Firefox-grade, parallel, huge coverage | own cascade (years) | medium — heavy integration of the `TElement`/`TNode` traits, breaking releases |
| `taffy` | flex/grid/block math | CSS-correct, used by Blitz/Bevy | own | low |
| `parley`, `swash`, `fontdb`, `skrifa` | text layout, shaping, fonts | Linebender stack, pure Rust | harfbuzz-rs (C), cosmic-text | medium — pre-1.0 |
| `vello`, `wgpu`, `peniko`, `kurbo` | 2D GPU raster, GPU abstraction | pure Rust, Metal/DX12/Vulkan | skia-safe (C++), tiny-skia only | medium — wgpu breaking every release |
| `tiny-skia` | CPU raster fallback, reftests | determinism | vello_cpu | low |
| `winit` | windows, input | standard | tao | low |
| `accesskit` (+platform adapters) | accessibility | the only cross-platform one | — | medium |
| `v8` | JS/Wasm VM | Chrome semantics, stable, version = Chrome | `mozjs`, `boa`, own | high — C++, large binary, prebuilt download; see ADR-0004 |
| `image`, `zune-jpeg`, `png`, `image-webp`, `ravif`/`dav1d` (AVIF — later) | decoders | pure Rust where possible | libjpeg-turbo | medium — performance vs C |
| `resvg`/`usvg` | SVG | mature, pure Rust | own SVG on cl-paint | low (later, integration with DOM SVG will require our own) |
| `url`, `idna` | WHATWG URL | de facto standard | — | low |
| `encoding_rs` | WHATWG Encoding | Firefox | — | low |
| `data-url`, `mime`, `percent-encoding` | utilities | — | — | low |

## Network

| Crate | Role | Why | Risk |
|---|---|---|---|
| `hyper` 1.x, `http`, `http-body` | HTTP/1.1, /2 | mature, low-level | low |
| `rustls`, `rustls-platform-verifier`, `webpki-roots` (fallback) | TLS | pure Rust, system root stores | low |
| `quinn`, `h3`, `h3-quinn` | QUIC/HTTP/3 | the only mature Rust QUIC | medium — `h3` 0.0.x |
| `hickory-resolver` | DNS | async, DoH/DoT | low |
| `tokio` | async runtime | standard | low |
| `tokio-tungstenite` or own on hyper upgrade | WebSocket | — | low |
| `brotli`, `flate2`, `zstd` | content-encoding | — | low |
| `cookie` (parsing) — probably own | cookies | RFC 6265bis nuances | — |

## Platform / processes / IPC

| Crate | Role | Risk |
|---|---|---|
| `ipc-channel` | IPC transport | medium — Servo-specific; possibly own on `interprocess` |
| `postcard` + `serde` | message serialization | low |
| `shared_memory` / own via `memmap2` | shm | medium |
| `seccompiler` / `libseccomp` (Linux) | seccomp-bpf | medium |
| `nix`, `libc`, `windows-sys`, `objc2`/`objc2-foundation` | OS API | low (unsafe isolated) |
| `security-framework` (macOS Keychain), `windows` DPAPI, `secret-service` (Linux) | keystore | low |
| `minidumper`, `crash-handler`, `minidump-writer` | crash dumps | medium |
| `sysinfo` | memory pressure metrics | low |

## Storage / product

| Crate | Role | Risk |
|---|---|---|
| `rusqlite` (bundled) | SQLite | low |
| `egui`, `eframe`/`egui-wgpu`, `egui-winit` | shell UI (M5) | medium — migration later |
| `prost`, `prost-build` | sync.proto | low |
| `axum`, `tower` | sync-server | low |
| `ed25519-dalek`, `sha2`, `aes-gcm`, `argon2`, `hkdf` | signatures, profile/sync encryption | low (RustCrypto, audited) |
| `zip`/own CRX3 reader, `x509-parser` | CRX3 | low |
| `tracing`, `tracing-subscriber`, `tracing-chrome` | observability | low |
| `thiserror`, `anyhow` | errors | low |
| `clap` | CLI flags | low |
| `insta`, `proptest`, `arbitrary`, `libfuzzer-sys`, `criterion` | tests | low |

## Forbidden

`openssl`, `native-tls`, `reqwest` (in the engine), `curl`, `gtk`/`webkit2gtk`, `cef`, any crates with networking `build.rs` except `v8` (checksum-verified prebuilt).
