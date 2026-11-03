# ADR-0012: Memory — a first-class requirement

**Status:** Accepted
**Date:** 2026-09-07
**Deciders:** project owner

## Context

"Several times lighter than Chrome" is the main product differentiator. Rust by itself does not save memory; architecture does. Chrome 140+ (2026): empty ~300–400 MB, 10 tabs ~1.4 GB, heavy web apps 0.5–1.5 GB/tab, Memory Saver — "up to 80%" on a discarded tab.

## Decision

v1 budgets (hypotheses; confirm with measurements by M2 and adjust via an ADR):

| Metric | Target |
|---|---|
| Empty browser + 1 empty tab (sum of processes, RSS) | ≤ 120 MB |
| Active typical page (renderer) | ≤ 70 MB |
| 10 active tabs (sum) | ≤ 600 MB |
| Frozen background tab | ≤ 25 MB |
| Hibernated tab (state on disk) | ≤ 5 MB RAM |
| Cold start → first frame | ≤ 400 ms |

Architectural tactics (mandatory, not "optimize later"):

1. **Three background levels:** freeze (JS/timers stopped, V8 heap compaction) → discard (renderer killed, tab stays in the UI) → **hibernate** (serialize DOM+scroll+form state to disk, restore without network where possible). Policy in the browser process, driven by OS memory pressure and an ML-free heuristic (time since last focus, pinned, audio).
2. **V8:** one isolate per renderer, lazy context, startup snapshot, `--optimize-for-size`, `--jitless` in the background; heap limits per site.
3. **Processes:** renderer per site with a reuse policy under pressure (not per tab); one network, one GPU per profile; utility processes short-lived.
4. **Caches in the GPU process:** glyph atlas, decoded images, shared between renderers; limits and LRU.
5. **Arenas and indices** instead of `Rc` graphs (CODING_STANDARDS §1); `Box<str>`/atoms for DOM strings.
6. **No background services** without a benefit: no preloading by default, no telemetry in v1.

Measurement: `tools/bench/mem.sh` on a fixed corpus in CI; a regression >5% blocks merge; monthly comparison with the current Chrome on the same machine — `docs/history/bench-YYYY-MM.md`.

## Consequences

- Easier: the budget is an acceptance criterion for every feature; the differentiator is measurable.
- Harder: some Chrome features (aggressive prefetch, process-per-frame) are intentionally not copied; hibernate is a complex feature.
- Revisit when: M2 measurements show that a V8 idle isolate + wgpu device already eat >80 MB — recompute the budgets honestly, do not "fudge" them.

## Action Items

1. [ ] `tools/bench` corpus + per-process RSS/PSS collection on 3 OSes (M1).
2. [ ] Chrome 153 baseline on the owner's machine (M1) — `docs/history/bench-2026-10.md`.
3. [ ] Freeze/discard (M4), hibernate (M5).
