# Dependencies

Every new dependency — a row here + `cargo deny check`. Versions — in `[workspace.dependencies]`; here — intent and rationale. Re-check licenses when adding (`cargo deny list`).

Allowed licenses (`deny.toml`): MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, MPL-2.0 (file-level copyleft, compatible with our Apache/MIT as long as files are unmodified; changes to stylo/cssparser — only via upstream or a separate published fork), Unicode-3.0, CC0-1.0. Forbidden: GPL/LGPL/AGPL (except via an explicit ADR), SSPL, BUSL.

## Engine

`parley` and `fontique` are pulled in as `default-features = false, features = ["std"]`: this turns off `system` (fontconfig/DirectWrite/CoreText) — the engine renders only its own bundled fonts (reftest determinism, ADR-0009) — and `complex-scripts` (dictionary line breaking for CJK/Thai/Khmer; a Task 18 decision, to be revisited together with support for those scripts).

| Crate | Role | Why this one | Alternatives | Risk |
|---|---|---|---|---|
| `html5ever`, `markup5ever` | HTML tokenizer/tree builder | spec-conformant, Servo, WPT-verified, `Atom` | own parser | medium — API changes |
| `cssparser` | CSS tokenizer | required by stylo | — | low |
| `stylo_traits`, `stylo_dom`, `stylo_atoms`, `stylo_static_prefs` | part of stylo | the same version | — | low |
| `selectors` | CSS selectors | required by stylo | — | low |
| `stylo` (+`stylo_atoms`, `stylo_dom`, `selectors`, `servo_arc`) | CSS cascade/computed style | Firefox-grade, parallel, huge coverage | own cascade (years) | high — Python 3 at build time, API without stability guarantees (pre-1.0, ADR-0015). On `unsafe`: there is **none** in `cl-style` — the ADR-0015 amendments removed the original assumption, and `tools/check-unsafe-scope.sh` keeps `#![forbid(unsafe_code)]` in every M1a crate |
| `euclid` 0.22 | typed 2D geometry (`Size2D`, `Scale`) for `style::device::Device::new` (viewport/device-pixel-ratio) | the version `stylo` 0.20 itself uses internally (`style_traits`/`app_units`); not re-exported from `style::`, so it is a direct dependency of `cl-style` (found in the task-2 build spike) | — | low |
| `taffy` | flex/grid/block math | CSS-correct, used by Blitz/Bevy | own | low |
| `parley` 0.11 (Apache-2.0 OR MIT) | shaping + line breaking + `text-align` inside `cl-layout` (Task 18: inline formatting context); pulls in `harfrust` (shaper), `skrifa`, `parley_data`, `icu_segmenter` | Linebender stack, pure Rust, the same `fontique` as `cl-fonts` | harfbuzz-rs (C), cosmic-text | medium — pre-1.0 |
| `swash` 0.2 (Apache-2.0 OR MIT) | rasterising glyphs into 8-bit alpha masks inside `cl-gfx` (Task 21: CPU backend) — `ScaleContext`/`Render::new(&[Source::Outline])`/`Format::Alpha`, hinting off for determinism; pulls in `zeno` (scan conversion) and `yazi` (`woff2` table decompression) | Linebender stack, pure Rust, the same `skrifa` as `parley`; `tiny-skia` cannot rasterise glyphs itself | harfbuzz-rs + FreeType (C), `vello_cpu` | medium — pre-1.0 |
| `fontique` | font discovery | Linebender stack | system fontconfig | low |
| `skrifa` (two in `Cargo.lock`: 0.46.2 as a direct dev-dependency of `cl-fonts`, 0.44.0 transitively via `parley` 0.11.1 and `swash` 0.2.10) | font-table parsing (glyph metrics/outlines) | found by Task 23 (`cargo tree -d`): `parley`/`swash` have not moved to 0.46 yet | update `parley`/`swash` once they release on `skrifa` 0.46 | low — the two versions' types never meet in our code (`cl-fonts` and `parley`/`swash` never pass `skrifa::Tag`/`FontRef` values to each other), but it bloats the binary; track on `parley`/`swash` updates |
| `tendril` | web strings (rope/small-string for text nodes) | Servo | — | low — **not a direct dependency**: it comes through `markup5ever` and is used as `markup5ever::tendril` (markup5ever 0.39 has tendril 0.5; a separate 0.4 pin would give two incompatible `StrTendril`s) |
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
| `ipc-channel` | IPC transport | **high** — Servo-specific; possibly own on `interprocess`. **Vendored and patched** in `third_party/ipc-channel` (0.23.1 + one change, `[patch.crates-io]`; `third_party/ipc-channel/CHROMELIGHT-PATCH.md`): on Windows the receiver duplicated every received shm/channel handle from itself again and leaked the sender's copy — one handle per shm region (found by the M1b leak test on windows-2022, 137 → 637 handles over 500 frames); to be reported upstream and the copy dropped once a release has the fix. **Minimum 0.23.1** (2026-11-03): 0.23.0 is affected by GHSA-fj85-w6rj-rxrc (high, macOS: a sender could make the receiver read and deallocate arbitrary memory). The receive path is still not robust against a hostile peer in 0.23.1 — servo/ipc-channel#457 (Linux: a short datagram underflows `set_len`, `pop().unwrap()`, peer-sized `reserve_exact`), Windows `expect`/`unwrap`/peer-sized `reserve`, and attached channel ends dropped behind a `debug_assert!` — see ADR-0016 Context fact 3 and `docs/SECURITY.md` "Known gaps"; must be closed before Gate S0 (M2; owner decision 2026-11-03). The advisory was a repository advisory only (not in RustSec, not in the global GitHub advisory database), so neither `cargo deny` nor Dependabot flags it: on every bump read https://github.com/servo/ipc-channel/security/advisories and the open issues. Deserialising an `IpcSender`/`IpcReceiver` with a bad index still panics (`src/ipc.rs` FIXME) — channel ends travel only browser → child (ADR-0016) |
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
| `insta`, `proptest`, `arbitrary`, `libfuzzer-sys`, `criterion` | tests | low — `arbitrary` (MIT OR Apache-2.0) is optional, behind the `arbitrary` feature in `cl-fonts`/`cl-layout`/`cl-paint`: it provides `derive(Arbitrary)` for the types the `display_list_validate`/`display_list_rasterize` fuzzers generate structurally, plus `FontKey::from_raw` (doc-hidden, tests/fuzzing only). Off in the normal engine build |
| `serde_json` | parsing WPT/html5lib JSON/test fixtures in the conformance harnesses (dev-dependency) | low |

## Bundled assets

| Asset | License | Note |
|---|---|---|
| `Ahem.ttf` | CC0-1.0 | WPT fonts/Ahem.ttf |
| `NotoSans-Regular.ttf` | OFL-1.1 | unmodified (the Reserved Font Name clause does not apply) |

## Forbidden

`openssl`, `native-tls`, `reqwest` (in the engine), `curl`, `gtk`/`webkit2gtk`, `cef`, any crates with networking `build.rs` except `v8` (checksum-verified prebuilt).
