# ADR-0010: WPT, Test262, fuzz and reftests from the first milestone

**Status:** Accepted
**Date:** 2026-09-07
**Deciders:** project owner

## Context

Tests of our own happy paths do not deliver compatibility; pass rate does not measure security. Reference: WPT product adapter, expected failures as versioned metadata, fuzz + sanitizers, differential as a triage signal.

## Decision

- `cl-testshell` — a headless deterministic shell (bundled fonts, fixed clock/RNG, DPR 1) — exists from M1; it is also the WPT product and the reftest runner.
- WPT product adapter from M1; directories are enabled in the order given in TESTING.md §3; expectations in `tools/wpt/expectations/` with a bug ID and a revisit date.
- Test262 — qualification set on every V8 bump / bindings change.
- Fuzz target — in the same PR as the parser/decoder/IPC message. Nightly fuzz 30 min.
- Reftests for layout/paint; golden (`insta`) for trees.
- Differential against headless Chrome — nightly, signal only.
- Bench/memory CI gate (ADR-0012).
- WPT dashboard: pass/expected-fail/crash/timeout separately; regressions separately.

## Consequences

- Easier: measurable compatibility progress; regressions visible per commit.
- Harder: infrastructure in M0/M1 before the "first pretty page".
- Revisit when: a full WPT run takes > 2 h across 3 OSes → sharding/choosing subdirectories.

## Action Items

1. [ ] `tools/wpt/product/chrome_light.py` + `run.sh` (M1).
2. [ ] `tools/test262/` harness (M2).
3. [ ] `tools/fuzz/` with the first targets: url, html_tokenizer, css, ipc (M1).
