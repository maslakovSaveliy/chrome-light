# ADR-0001: Project class — independent engine and full browser

**Status:** Accepted
**Date:** 2026-09-07
**Deciders:** project owner

## Context

Request: a Chrome-like browser, many times lighter on memory, faster, in Rust, three desktop OSes, full functionality, interop with Chrome. Research (`docs/RESEARCH-2026-09.md`, skill `browser-engine-research`) shows these goals conflict: 100% of Chrome's functionality today comes only from Chromium code (CEF/fork), which is not "many times lighter"; Rust engines (Servo) are lighter, but not 100% compatible; our own engine is a multi-year program. The team is one developer + AI agents.

The owner, knowing the scale estimates, chose an independent engine.

## Decision

We build an **independent open-Web engine in Rust + a full browser product**. Not a Chromium fork, not a shell over CEF/WebView/Servo. We use ecosystem crates as components (ADR-0003), but the architecture, DOM, layout, paint, compositor, process model, IPC, network, storage, product are our own. Chrome parity is a direction, measured by WPT/Test262/corpus. "Chrome-like" functionality for the user is delivered via interop (ADR-0007, 0011), not via Chromium code.

## Options Considered

### Option A: Shell on CEF (Rust crate `cef` 151.x)
| Dimension | Assessment |
|---|---|
| Complexity | Low–Med |
| Cost | quarters to MVP |
| Memory/perf | = Chrome minus UI; 20–40% gain via policy |
| Security | Chromium-grade, patches via CEF with a lag |
| Compat | 100%, MV3, DevTools |
**Pros:** fast, compatible. **Cons:** not "our own", not "many times lighter", dependency on CEF releases, C++ FFI.

### Option B: Embedding Servo
| Dimension | Assessment |
|---|---|
| Complexity | Med |
| Cost | quarters to a demo, years to a daily driver |
| Memory/perf | lighter than Chrome |
| Security | multiprocess exists, sandbox immature |
| Compat | partial; no extensions, no DevTools parity |
**Pros:** Rust, a real engine. **Cons:** we don't control the roadmap; Verso died precisely from the race to keep up with the API; parity with Chrome is unreachable.

### Option C: Chromium fork
Rejected: 100+ GB build, C++, a release every 2 weeks = an eternal patch race, not Rust, not lighter.

### Option D: Our own engine (chosen)
| Dimension | Assessment |
|---|---|
| Complexity | Very High |
| Cost | years; order of magnitude — hundreds of engineer-years for broad compatibility |
| Memory/perf | under our control — the only path to "many times lighter" |
| Security | entirely our responsibility; Rust reduces the class of memory bugs |
| Compat | grows asymptotically |
**Pros:** full control, memory/security as architectural goals, Rust. **Cons:** scale; risk of never reaching broad compatibility; solo.

## Trade-off Analysis

The only option that satisfies "Rust + our own + lighter". The price is time and incomplete compatibility for years. Mitigation: strict feature matrix, milestone gates, reuse of audited crates (stylo, V8, html5ever), Chrome interop for user value before full compatibility.

## Consequences

- Easier: every architectural decision on memory/security is ours.
- Harder: every Web API is our implementation + bindings + tests; the long tail of compatibility.
- Revisit when: after 12 months, if WPT coverage of focus areas < 50% or curated corpus < 30% — return to Option B as a fallback engine for incompatible tabs.

## Action Items

1. [x] Record non-goals in CLAUDE.md and FEATURE_MATRIX.
2. [ ] Milestone plan with exit criteria (next document).
3. [ ] After 12 months — review this ADR against the metrics.
