# ADR-0006: Графика — vello + wgpu, свой compositor, CPU fallback

**Status:** Accepted
**Date:** 2026-09-07
**Deciders:** владелец проекта

## Context

Нужен кросс-платформенный GPU raster (Metal/DX12/Vulkan), детерминированный CPU-путь для тестов и машин без GPU, и compositor с async scroll/animation. Референс: Blink (cc/Viz), Gecko (WebRender — Rust), WebKit (port-specific). Rust-экосистема: `wgpu` 30 (стандарт), `vello` 0.10 (compute-based 2D), `tiny-skia` (CPU), `webrender` (Firefox, но тяжёлая зависимость от Gecko-специфики).

## Decision

- Raster: **vello** на **wgpu** в GPU process. CPU fallback: `vello_cpu`/`tiny-skia` для headless testshell, reftests, отсутствия GPU, и как reference path для differential-тестов.
- Compositor (`cl-compositor`, свой): property trees (transform/clip/effect/scroll), layerization по критериям (transform/opacity animation, scroll containers, will-change), tiles с damage tracking, async scroll на compositor thread без main thread renderer.
- Display list — свой формат (не vello Scene напрямую): renderer пишет display list в shm → GPU process валидирует → строит vello Scene. Это граница доверия.
- Шрифты/glyph atlas и decoded images — кэши в GPU process, шарятся между renderer-ами (память).
- Цвет: sRGB в v1; wide gamut/HDR — later.

## Options Considered

- **WebRender** — production-grade, но API заточен под Gecko, тяжёлые breaking changes; сложно поддерживать соло.
- **skia-safe** — C++ FFI, огромная сборка; противоречит ADR-0002.
- **Только CPU (tiny-skia)** — детерминизм и простота, но не «быстрее Chrome» на анимациях/скролле.
- **vello + wgpu (выбран).**

## Consequences

- Легче: чистый Rust, одна абстракция GPU на три ОС, Linebender-стек единый с parley.
- Труднее: vello pre-1.0, breaking changes; compute-shader подход требует свежих драйверов → fallback обязателен; GPU process sandbox слабее.
- Пересмотреть когда: vello не даст стабильных 60 fps на скролле типичной страницы на M1/Intel iGPU к M4 — оценить tiled CPU raster + GPU composite (Chromium-style).

## Action Items

1. [ ] testshell PNG через CPU-путь (M1) — база для reftests.
2. [ ] GPU process + shm display list + validator + fuzz (M1/M2).
3. [ ] Async scroll на compositor (M4).
