# ADR-0014: Лицензия собственного кода

**Status:** Accepted (владелец 2026-09-07: «бесплатно, опенсорс, пусть делают что хотят»)
**Date:** 2026-09-07
**Deciders:** владелец проекта

## Context

Зависимости: stylo/cssparser/mozjs — MPL-2.0 (файловый copyleft); V8 — BSD-3; vello/wgpu/parley/html5ever/taffy — MIT/Apache; tiny-skia — BSD-3; Chromium `sync.proto`, CRX3 — BSD-3 (Chromium). Rust-экосистема ожидает `MIT OR Apache-2.0`. Собственный код должен быть совместим и оставлять свободу выбора модели (open source / open core / проприетарный shell).

## Decision

- Собственный код: **Apache-2.0 OR MIT** (dual) — максимально permissive, стандарт Rust-экосистемы; получатель выбирает любую. Патентная защита Apache + простота MIT.
- MPL-2.0 зависимости используем без модификации файлов; наши правки — через upstream PR или отдельный fork-репозиторий с публикацией изменённых файлов (требование MPL).
- `deny.toml` allowlist: MIT, Apache-2.0 (+LLVM-exception), BSD-2/3, ISC, Zlib, MPL-2.0, Unicode-3.0, CC0-1.0, OpenSSL (не используем), BSL/SSPL/GPL/LGPL/AGPL — запрещены.
- SBOM (`cargo cyclonedx`) в release-артефактах; `THIRD_PARTY_NOTICES` генерируется `cargo about`.

## Options Considered

| | Apache-2.0 OR MIT | MPL-2.0 | GPL-3.0 | Проприетарная |
|---|---|---|---|---|
| Совместимость с deps | полная | полная | полная (но односторонняя) | полная при соблюдении notices |
| Вклад сообщества | максимальный | хороший | ограничен | нет |
| Защита от «взяли и закрыли» | нет | файловая | сильная | — |
| Коммерческие опции позже | да | да | сложно | да |

## Consequences

- Легче: contributors, переиспользование в других Rust-проектах.
- Труднее: конкуренты могут взять код; для соло-проекта это не главный риск.

## Action Items

1. [x] Владелец подтвердил.
2. [x] `LICENSE-APACHE`, `LICENSE-MIT`, `deny.toml`; заголовки в файлах не требуются (SPDX в Cargo.toml `license = "Apache-2.0 OR MIT"`).
