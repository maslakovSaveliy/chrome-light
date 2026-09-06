# ADR-0010: WPT, Test262, fuzz и reftests с первого milestone

**Status:** Accepted
**Date:** 2026-09-07
**Deciders:** владелец проекта

## Context

Тесты собственных happy-path не дают совместимости; pass rate не измеряет безопасность. Референс: WPT product adapter, expected failures как versioned metadata, fuzz + sanitizers, differential как triage-сигнал.

## Decision

- `cl-testshell` — headless детерминированный shell (bundled fonts, фиксированный clock/RNG, DPR 1) — существует с M1; он же WPT product и reftest runner.
- WPT product adapter с M1; директории включаются по порядку из TESTING.md §3; expectations в `tools/wpt/expectations/` с bug ID и датой пересмотра.
- Test262 — qualification set при каждом V8 bump / изменении bindings.
- Fuzz target — в том же PR, что парсер/декодер/IPC-сообщение. Nightly fuzz 30 мин.
- Reftests для layout/paint; golden (`insta`) для деревьев.
- Differential против headless Chrome — nightly, только сигнал.
- Bench/memory CI-гейт (ADR-0012).
- Дашборд WPT: pass/expected-fail/crash/timeout раздельно; регрессии отдельно.

## Consequences

- Легче: измеримый прогресс совместимости; регрессии видны по коммитам.
- Труднее: инфраструктура в M0/M1 до «первой красивой страницы».
- Пересмотреть когда: время полного WPT-прогона > 2 ч на 3 ОС → шардинг/выбор поддиректорий.

## Action Items

1. [ ] `tools/wpt/product/chrome_light.py` + `run.sh` (M1).
2. [ ] `tools/test262/` harness (M2).
3. [ ] `tools/fuzz/` с первыми targets: url, html_tokenizer, css, ipc (M1).
