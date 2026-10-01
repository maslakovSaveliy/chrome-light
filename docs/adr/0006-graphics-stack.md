# ADR-0006: Graphics — vello + wgpu, own compositor, CPU fallback

**Status:** Accepted
**Date:** 2026-09-07
**Deciders:** project owner

## Context

We need a cross-platform GPU raster (Metal/DX12/Vulkan), a deterministic CPU path for tests and machines without a GPU, and a compositor with async scroll/animation. Reference: Blink (cc/Viz), Gecko (WebRender — Rust), WebKit (port-specific). Rust ecosystem: `wgpu` 30 (the standard), `vello` 0.10 (compute-based 2D), `tiny-skia` (CPU), `webrender` (Firefox, but a heavy dependency on Gecko specifics).

## Decision

- Raster: **vello** on **wgpu** in the GPU process. CPU fallback: `vello_cpu`/`tiny-skia` for the headless testshell, reftests, no-GPU machines, and as the reference path for differential tests.
- Compositor (`cl-compositor`, our own): property trees (transform/clip/effect/scroll), layerization by criteria (transform/opacity animation, scroll containers, will-change), tiles with damage tracking, async scroll on the compositor thread without the renderer main thread.
- Display list — our own format (not vello Scene directly): the renderer writes the display list into shm → the GPU process validates it → builds a vello Scene. This is the trust boundary.
- Fonts/glyph atlas and decoded images — caches in the GPU process, shared between renderers (memory).
- Color: sRGB in v1; wide gamut/HDR — later.

## Options Considered

- **WebRender** — production-grade, but the API is tailored to Gecko, heavy breaking changes; hard to maintain solo.
- **skia-safe** — C++ FFI, huge build; contradicts ADR-0002.
- **CPU only (tiny-skia)** — determinism and simplicity, but not "faster than Chrome" on animations/scrolling.
- **vello + wgpu (chosen).**

## Consequences

- Easier: pure Rust, one GPU abstraction across three OSes, the same Linebender stack as parley.
- Harder: vello is pre-1.0, breaking changes; the compute-shader approach requires recent drivers → fallback is mandatory; the GPU process sandbox is weaker.
- Revisit when: vello fails to deliver a stable 60 fps when scrolling a typical page on M1/Intel iGPU by M4 — evaluate tiled CPU raster + GPU composite (Chromium-style).

## Action Items

1. [ ] testshell PNG via the CPU path (M1) — the basis for reftests.
2. [ ] GPU process + shm display list + validator + fuzz (M1/M2).
3. [ ] Async scroll on the compositor (M4).
