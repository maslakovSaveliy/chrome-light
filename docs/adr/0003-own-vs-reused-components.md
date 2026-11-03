# ADR-0003: What we write ourselves, what we take from the ecosystem

**Status:** Accepted
**Date:** 2026-09-07
**Deciders:** project owner

## Context

"Our own engine" does not mean "every parser from scratch". Reference roadmap: our own CSS cascade, text shaping, codecs, JS JIT — each one on its own is a multi-year program. The 2026 Rust ecosystem provides Firefox-grade CSS (stylo), spec-conformant HTML (html5ever), GPU 2D (vello), text (parley/swash). Blitz proved that they assemble into an engine.

## Decision

**Our own (defines the architecture and is the core competency):** DOM (arena, events, shadow DOM), layout (box/fragment tree, block/inline/positioned/tables, text integration), paint/display list, compositor (property trees, tiles, async scroll), GPU-process glue, Web IDL codegen and bindings runtime, event loop, Web API (Fetch, Storage, Workers, Canvas…), network process (Fetch algorithm, cache, cookies, CORS/CSP/ORB), IPC schema/validation/broker, process model and sandbox policies, storage backends, browser product, chrome-import, sync client/server, extensions runtime, DevTools server, testshell.

**Reused (behind our types, with a fuzz wrapper):** `html5ever` (tokenizer/tree builder → our DOM sink), `cssparser` + `stylo` (cascade/computed style via our `TElement/TNode` impl), `taffy` (only flex/grid/block sizing math — the box tree is ours), `parley`/`swash`/`fontdb` (text), `vello`/`wgpu`/`tiny-skia` (raster), `winit`, `accesskit`, `v8` (ADR-0004), `hyper`/`rustls`/`quinn`/`h3`/`hickory` (transport — not policy), `url`/`encoding_rs`, the `image` family and `resvg` (decoders — in a utility process), `rusqlite`, `ipc-channel` (transport), `prost`.

**Rule:** commodity parser/decoder/transport — we take it; everything that defines web platform behavior, memory, or security boundaries — our own.

## Options Considered

- **Everything from scratch** — a solo developer can't sustain it; years to the first page.
- **Maximum reuse (Blitz-like glue)** — fast to a demo, but Blitz's DOM/layout are experimental and tailored for GUI, not for the hostile Web and Site Isolation.
- **The chosen hybrid.**

## Consequences

- Easier: CSS/text/HTML correct from day one; focus on layout, security, memory.
- Harder: stylo/wgpu break the API on every release → a dedicated monthly "upstream bump" ritual; stylo's MPL-2.0 license is file-level copyleft (ADR-0014).
- Revisit when: taffy doesn't cover CSS edge cases (baseline, intrinsic sizing with inline) — replace it with our own flex/grid implementation; V8 — per ADR-0004.

## Action Items

1. [ ] Prototype of stylo integration with the arena DOM (spike, M1).
2. [ ] Fuzz wrappers for html5ever, cssparser, url, image in M1.
