# ADR-0003: Что пишем сами, что берём из экосистемы

**Status:** Accepted
**Date:** 2026-09-07
**Deciders:** владелец проекта

## Context

«Свой движок» не означает «каждый парсер с нуля». Референс-roadmap: собственный CSS cascade, шейпинг текста, кодеки, JS JIT — каждый по отдельности многолетняя программа. Rust-экосистема 2026 даёт Firefox-grade CSS (stylo), spec-conformant HTML (html5ever), GPU 2D (vello), текст (parley/swash). Blitz доказал, что они собираются в движок.

## Decision

**Своё (определяет архитектуру и является ядром компетенции):** DOM (arena, events, shadow DOM), layout (box/fragment tree, block/inline/positioned/tables, интеграция text), paint/display list, compositor (property trees, tiles, async scroll), GPU-process glue, Web IDL codegen и bindings runtime, event loop, Web API (Fetch, Storage, Workers, Canvas…), network process (Fetch algorithm, cache, cookies, CORS/CSP/ORB), IPC schema/validation/broker, process model и sandbox-политики, storage backends, browser product, chrome-import, sync client/server, extensions runtime, DevTools server, testshell.

**Переиспользуем (за нашими типами, с fuzz-обёрткой):** `html5ever` (tokenizer/tree builder → наш DOM sink), `cssparser` + `stylo` (cascade/computed style через наши `TElement/TNode` impl), `taffy` (только математика flex/grid/block sizing — box tree наш), `parley`/`swash`/`fontdb` (текст), `vello`/`wgpu`/`tiny-skia` (raster), `winit`, `accesskit`, `v8` (ADR-0004), `hyper`/`rustls`/`quinn`/`h3`/`hickory` (транспорт — не policy), `url`/`encoding_rs`, `image`-семейство и `resvg` (декодеры — в utility process), `rusqlite`, `ipc-channel` (транспорт), `prost`.

**Правило:** commodity-парсер/декодер/транспорт — берём; всё, что определяет поведение web-платформы, память или границы безопасности — своё.

## Options Considered

- **Всё с нуля** — не выдержит соло; годы до первой страницы.
- **Максимальное переиспользование (Blitz-подобный клей)** — быстро до демо, но DOM/layout Blitz экспериментальны и заточены под GUI, не под hostile Web и Site Isolation.
- **Выбранный гибрид.**

## Consequences

- Легче: CSS/текст/HTML корректны с первого дня; фокус на layout, безопасность, память.
- Труднее: stylo/wgpu ломают API на каждом релизе → выделенный «upstream bump» ритуал раз в месяц; лицензия MPL-2.0 stylo — файловый copyleft (ADR-0014).
- Пересмотреть когда: taffy не покроет CSS-краевые случаи (baseline, intrinsic sizing с inline) — заменить своей реализацией flex/grid; V8 — по ADR-0004.

## Action Items

1. [ ] Прототип интеграции stylo с arena-DOM (spike, M1).
2. [ ] Обёртки-fuzz для html5ever, cssparser, url, image в M1.
