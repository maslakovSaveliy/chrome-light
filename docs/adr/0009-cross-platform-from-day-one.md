# ADR-0009: Три платформы с первого дня

**Status:** Accepted
**Date:** 2026-09-07
**Deciders:** владелец проекта

## Context

Владелец выбрал все три ОС сразу (macOS, Windows, Linux). Референс рекомендует ≤2 ОС для MVP; выбор увеличивает стоимость на старте в 2–3 раза. Смягчение — изоляция платформенного кода и CI-матрица, чтобы «сразу» означало «компилируется и тестируется», а не «полируется» на всех трёх одновременно.

## Decision

- CI-матрица PR: `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu` — build + tests обязательны для merge с M0.
- Платформенный код — только `cl-platform` (ОС API), `cl-process` (sandbox), backends `cl-gfx`. `#[cfg(target_os)]` в других crate-ах — reject на review; `tools/check-platform-cfg.sh` в CI.
- Приоритет полировки: macOS (хост владельца) → Linux (CI/sandbox проще) → Windows (sandbox сложнее). Функциональные milestone-гейты считаются пройденными, когда фича работает на **всех трёх**; sandbox-гейт S0 на Windows допускает лаг в один milestone, но без него Windows-сборка не открывает untrusted URL.
- Кросс-компиляция с macOS: Linux через Docker; Windows — только CI/VM.

## Consequences

- Легче: нет «портирования» как отдельной фазы; платформенные абстракции честные с первого дня.
- Труднее: каждый milestone дороже; Windows sandbox — отдельная компетенция.
- Пересмотреть когда: Windows-работа блокирует >30% времени milestone → временно перевести Windows в nightly-матрицу с явной пометкой в MEMORY.md.

## Action Items

1. [ ] `.github/workflows/ci.yml` с матрицей (M0).
2. [ ] `cl-platform` API: fs, shm, clock, keystore, quarantine, fonts (M0/M1).
