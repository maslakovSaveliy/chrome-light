# ADR-0009: Three platforms from day one

**Status:** Accepted
**Date:** 2026-09-07
**Deciders:** project owner

## Context

The owner chose all three OSes at once (macOS, Windows, Linux). The reference recommends ≤2 OSes for an MVP; this choice raises the up-front cost 2–3×. Mitigation — isolating platform code and a CI matrix, so that "at once" means "compiles and is tested", not "polished" on all three simultaneously.

## Decision

- PR CI matrix: `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu` — build + tests required for merge from M0.
- Platform code — only in `cl-platform` (OS API), `cl-process` (sandbox), `cl-gfx` backends. `#[cfg(target_os)]` in other crates — rejected at review; `tools/check-platform-cfg.sh` in CI.
- Polish priority: macOS (the owner's host) → Linux (CI/sandbox simpler) → Windows (sandbox harder). Functional milestone gates count as passed when the feature works on **all three**; sandbox gate S0 on Windows may lag by one milestone, but without it the Windows build does not open untrusted URLs.
- Cross-compilation from macOS: Linux via Docker; Windows — CI/VM only.

## Consequences

- Easier: no "porting" as a separate phase; platform abstractions are honest from day one.
- Harder: every milestone costs more; the Windows sandbox is a separate area of expertise.
- Revisit when: Windows work blocks >30% of milestone time → temporarily move Windows to the nightly matrix with an explicit note in MEMORY.md.

## Action Items

1. [ ] `.github/workflows/ci.yml` with the matrix (M0).
2. [ ] `cl-platform` API: fs, shm, clock, keystore, quarantine, fonts (M0/M1).
