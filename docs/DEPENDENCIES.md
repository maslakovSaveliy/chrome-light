# Зависимости

Каждая новая зависимость — строка здесь + `cargo deny check`. Версии — в `[workspace.dependencies]`; здесь — намерение и обоснование. Лицензии перепроверять при добавлении (`cargo deny list`).

Allowed licenses (`deny.toml`): MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-2-Clause, BSD-3-Clause, ISC, Zlib, MPL-2.0 (file-level copyleft, совместимо с нашим Apache/MIT при неизменённых файлах; изменения stylo/cssparser — только через upstream или отдельный форк с публикацией), Unicode-3.0, CC0-1.0. Запрещены: GPL/LGPL/AGPL (кроме явного ADR), SSPL, BUSL.

## Движок

| Crate | Роль | Почему этот | Альтернативы | Риск |
|---|---|---|---|---|
| `html5ever`, `markup5ever` | HTML tokenizer/tree builder | spec-conformant, Servo, WPT-проверен, `Atom` | свой парсер | средний — API меняется |
| `cssparser` | CSS tokenizer | требуется stylo | — | низкий |
| `stylo_traits`, `stylo_dom`, `stylo_atoms`, `stylo_static_prefs` | часть stylo | та же версия | — | низкий |
| `selectors` | CSS селекторы | требуется stylo | — | низкий |
| `stylo` (+`stylo_atoms`, `stylo_dom`, `selectors`, `servo_arc`) | CSS cascade/computed style | Firefox-grade, параллельный, огромное покрытие | свой cascade (годы) | высокий — unsafe в cl-style, Python 3 на сборке (ADR-0015) |
| `taffy` | flex/grid/block math | CSS-корректный, используется Blitz/Bevy | свой | низкий |
| `parley`, `swash`, `fontdb`, `skrifa` | text layout, shaping, fonts | Linebender-стек, чистый Rust | harfbuzz-rs (C), cosmic-text | средний — pre-1.0 |
| `fontique` | font discovery | Linebender-стек | system fontconfig | низкий |
| `tendril` | веб-строки (AtomicRefCell-обёртка) | Servo | — | низкий |
| `vello`, `wgpu`, `peniko`, `kurbo` | 2D GPU raster, GPU abstraction | чистый Rust, Metal/DX12/Vulkan | skia-safe (C++), tiny-skia only | средний — wgpu breaking каждый релиз |
| `tiny-skia` | CPU raster fallback, reftests | детерминизм | vello_cpu | низкий |
| `winit` | окна, input | стандарт | tao | низкий |
| `accesskit` (+platform adapters) | accessibility | единственный кросс-платформенный | — | средний |
| `v8` | JS/Wasm VM | Chrome-семантика, stable, версия = Chrome | `mozjs`, `boa`, свой | высокий — C++, большой бинарник, prebuilt download; см. ADR-0004 |
| `image`, `zune-jpeg`, `png`, `image-webp`, `ravif`/`dav1d` (AVIF — later) | декодеры | чистый Rust где возможно | libjpeg-turbo | средний — производительность vs C |
| `resvg`/`usvg` | SVG | зрелые, чистый Rust | свой SVG на cl-paint | низкий (позже интеграция с DOM SVG потребует своего) |
| `url`, `idna` | WHATWG URL | стандарт де-факто | — | низкий |
| `encoding_rs` | WHATWG Encoding | Firefox | — | низкий |
| `data-url`, `mime`, `percent-encoding` | утилиты | — | — | низкий |

## Сеть

| Crate | Роль | Почему | Риск |
|---|---|---|---|
| `hyper` 1.x, `http`, `http-body` | HTTP/1.1, /2 | зрелый, low-level | низкий |
| `rustls`, `rustls-platform-verifier`, `webpki-roots` (fallback) | TLS | чистый Rust, системные root stores | низкий |
| `quinn`, `h3`, `h3-quinn` | QUIC/HTTP/3 | единственный зрелый Rust QUIC | средний — `h3` 0.0.x |
| `hickory-resolver` | DNS | async, DoH/DoT | низкий |
| `tokio` | async runtime | стандарт | низкий |
| `tokio-tungstenite` или свой на hyper upgrade | WebSocket | — | низкий |
| `brotli`, `flate2`, `zstd` | content-encoding | — | низкий |
| `cookie` (парсинг) — вероятно свой | cookies | RFC 6265bis нюансы | — |

## Платформа / процессы / IPC

| Crate | Роль | Риск |
|---|---|---|
| `ipc-channel` | IPC транспорт | средний — Servo-специфика; возможно свой на `interprocess` |
| `postcard` + `serde` | сериализация сообщений | низкий |
| `shared_memory` / свой через `memmap2` | shm | средний |
| `seccompiler` / `libseccomp` (Linux) | seccomp-bpf | средний |
| `nix`, `libc`, `windows-sys`, `objc2`/`objc2-foundation` | OS API | низкий (unsafe изолирован) |
| `security-framework` (macOS Keychain), `windows` DPAPI, `secret-service` (Linux) | keystore | низкий |
| `minidumper`, `crash-handler`, `minidump-writer` | crash dumps | средний |
| `sysinfo` | memory pressure metrics | низкий |

## Хранилище / продукт

| Crate | Роль | Риск |
|---|---|---|
| `rusqlite` (bundled) | SQLite | низкий |
| `egui`, `eframe`/`egui-wgpu`, `egui-winit` | shell UI (M5) | средний — миграция позже |
| `prost`, `prost-build` | sync.proto | низкий |
| `axum`, `tower` | sync-server | низкий |
| `ed25519-dalek`, `sha2`, `aes-gcm`, `argon2`, `hkdf` | подписи, шифрование профиля/sync | низкий (RustCrypto, аудировано) |
| `zip`/свой CRX3 reader, `x509-parser` | CRX3 | низкий |
| `tracing`, `tracing-subscriber`, `tracing-chrome` | observability | низкий |
| `thiserror`, `anyhow` | ошибки | низкий |
| `clap` | CLI флаги | низкий |
| `insta`, `proptest`, `arbitrary`, `libfuzzer-sys`, `criterion` | тесты | низкий |

## Bundled assets

| Ресурс | Лицензия | Примечание |
|---|---|---|
| `Ahem.ttf` | CC0-1.0 | WPT fonts/Ahem.ttf |
| `NotoSans-Regular.ttf` | OFL-1.1 | unmodified (Reserved Font Name clause не затрагивает) |

## Запрещено

`openssl`, `native-tls`, `reqwest` (в движке), `curl`, `gtk`/`webkit2gtk`, `cef`, любые crate-ы с сетевыми `build.rs` кроме `v8` (checksum-verified prebuilt).
