# ADR-0012: Память — требование первого класса

**Status:** Accepted
**Date:** 2026-09-07
**Deciders:** владелец проекта

## Context

«В разы легче Chrome» — основной продуктовый дифференциатор. Rust сам по себе память не экономит; экономит архитектура. Chrome 140+ (2026): пустой ~300–400 МБ, 10 вкладок ~1.4 ГБ, тяжёлые web-apps 0.5–1.5 ГБ/вкладка, Memory Saver — «до 80%» на discarded tab.

## Decision

Бюджеты v1 (гипотезы; подтвердить измерениями к M2 и скорректировать ADR-ом):

| Метрика | Цель |
|---|---|
| Пустой браузер + 1 пустая вкладка (сумма процессов, RSS) | ≤ 120 МБ |
| Активная типичная страница (renderer) | ≤ 70 МБ |
| 10 активных вкладок (сумма) | ≤ 600 МБ |
| Frozen фоновая вкладка | ≤ 25 МБ |
| Hibernated вкладка (состояние на диске) | ≤ 5 МБ RAM |
| Cold start → первый кадр | ≤ 400 мс |

Архитектурные тактики (обязательные, не «оптимизация потом»):

1. **Три уровня фона:** freeze (JS/timers стоп, V8 heap compaction) → discard (renderer убит, tab в UI) → **hibernate** (сериализация DOM+scroll+form state на диск, восстановление без сети где возможно). Политика в browser process по давлению памяти ОС и ML-free эвристике (время с последнего фокуса, pinned, audio).
2. **V8:** один isolate на renderer, lazy context, startup snapshot, `--optimize-for-size`, `--jitless` во фоне; heap limits per site.
3. **Процессы:** renderer per site с reuse-политикой при давлении (не per tab); один network, один GPU на профиль; utility-процессы short-lived.
4. **Кэши в GPU process:** glyph atlas, decoded images, шарятся между renderer-ами; лимиты и LRU.
5. **Арены и индексы** вместо `Rc`-графов (CODING_STANDARDS §1); `Box<str>`/атомы для строк DOM.
6. **Нет фоновых сервисов** без пользы: без preloading по умолчанию, без телеметрии в v1.

Измерение: `tools/bench/mem.sh` на фиксированном corpus в CI; регрессия >5% блокирует merge; ежемесячное сравнение с актуальным Chrome на той же машине — `docs/history/bench-YYYY-MM.md`.

## Consequences

- Легче: бюджет — критерий приёмки каждой фичи; дифференциатор измерим.
- Труднее: некоторые Chrome-фичи (агрессивный prefetch, process-per-frame) намеренно не копируем; hibernate — сложная фича.
- Пересмотреть когда: измерения M2 покажут, что V8 idle isolate + wgpu device уже съедают >80 МБ — бюджеты пересчитать честно, не «подгонять».

## Action Items

1. [ ] `tools/bench` corpus + RSS/PSS сбор per process на 3 ОС (M1).
2. [ ] Baseline Chrome 153 на машине владельца (M1) — `docs/history/bench-2026-10.md`.
3. [ ] Freeze/discard (M4), hibernate (M5).
