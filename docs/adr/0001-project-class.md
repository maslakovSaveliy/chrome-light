# ADR-0001: Класс проекта — независимый движок и полный браузер

**Status:** Accepted
**Date:** 2026-09-07
**Deciders:** владелец проекта

## Context

Запрос: браузер-аналог Chrome, кратно легче по памяти, быстрее, на Rust, три десктопные ОС, полный функционал, интероп с Chrome. Исследование (`docs/RESEARCH-2026-09.md`, skill `browser-engine-research`) показывает, что эти цели конфликтуют: 100% функционала Chrome сегодня даёт только Chromium-код (CEF/fork), который не «в разы легче»; Rust-движки (Servo) легче, но не совместимы на 100%; свой движок — многолетняя программа. Команда — один разработчик + AI-агенты.

Владелец, зная оценки масштаба, выбрал независимый движок.

## Decision

Строим **независимый open-Web движок на Rust + полный браузерный продукт**. Не форк Chromium, не shell над CEF/WebView/Servo. Экосистемные crate-ы используем как компоненты (ADR-0003), но архитектура, DOM, layout, paint, compositor, процессная модель, IPC, сеть, storage, продукт — свои. Chrome-паритет — направление, измеряемое WPT/Test262/corpus. Функционал «как у Chrome» для пользователя даётся через интероп (ADR-0007, 0011), а не через Chromium-код.

## Options Considered

### Option A: Shell на CEF (Rust crate `cef` 151.x)
| Dimension | Assessment |
|---|---|
| Complexity | Low–Med |
| Cost | кварталы до MVP |
| Memory/perf | = Chrome минус UI; выигрыш 20–40% через policy |
| Security | Chromium-grade, патчи через CEF с лагом |
| Compat | 100%, MV3, DevTools |
**Pros:** быстро, совместимо. **Cons:** не «свой», не «в разы легче», зависимость от CEF-релизов, C++ FFI.

### Option B: Embedding Servo
| Dimension | Assessment |
|---|---|
| Complexity | Med |
| Cost | кварталы до демо, годы до daily-driver |
| Memory/perf | легче Chrome |
| Security | multiprocess есть, sandbox незрел |
| Compat | частичная; нет расширений, DevTools-паритета |
**Pros:** Rust, реальный движок. **Cons:** мы не контролируем roadmap; Verso умер именно от гонки за API; паритет с Chrome недостижим.

### Option C: Форк Chromium
Отвергнут: 100+ ГБ сборка, C++, релиз каждые 2 недели = вечная гонка патчей, не Rust, не легче.

### Option D: Свой движок (выбран)
| Dimension | Assessment |
|---|---|
| Complexity | Very High |
| Cost | годы; порядок — сотни engineer-years для широкой совместимости |
| Memory/perf | под нашим контролем — единственный путь к «в разы легче» |
| Security | наша ответственность целиком; Rust снижает класс memory-багов |
| Compat | растёт асимптотически |
**Pros:** полный контроль, память/безопасность как архитектурные цели, Rust. **Cons:** масштаб; риск никогда не достичь широкой совместимости; соло.

## Trade-off Analysis

Единственный вариант, удовлетворяющий «Rust + свой + легче». Плата — время и неполная совместимость на годы. Смягчение: жёсткий feature matrix, milestone-гейты, переиспользование аудированных crate-ов (stylo, V8, html5ever), Chrome-интероп для пользовательской ценности до полной совместимости.

## Consequences

- Легче: любое архитектурное решение по памяти/безопасности — наше.
- Труднее: каждая Web API — наша реализация + bindings + тесты; long tail совместимости.
- Пересмотреть когда: через 12 месяцев если WPT-покрытие фокус-областей < 50% или curated corpus < 30% — вернуться к Option B как fallback-движку для несовместимых вкладок.

## Action Items

1. [x] Зафиксировать non-goals в CLAUDE.md и FEATURE_MATRIX.
2. [ ] План milestones с exit-критериями (следующий документ).
3. [ ] Через 12 месяцев — review этого ADR по метрикам.
