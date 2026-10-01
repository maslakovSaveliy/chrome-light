# Testing

Pass rate is not the only KPI. Unsupported, timeout, crash and a wrong baseline are different things; the dashboard shows them separately.

## 1. Pyramid

| Level | Tool | What it covers | Gate |
|---|---|---|---|
| Unit / property | `cargo nextest`, `proptest` | URL, tokenizer, selectors, cascade helpers, length math, cookie parsing, cache keys, IPC (de)serialization | PR |
| Golden / snapshot | `insta` | parse trees, computed style, fragment trees, display lists | PR |
| Reftest / pixel | `cl-testshell --png` + comparison against reference HTML; perceptual tolerance only for raster differences | layout/paint | PR (affected), nightly (all) |
| WPT | `wptrunner` + product adapter `tools/wpt/product/chrome_light.py` | observable web platform behavior | PR: affected directories + smoke shard; nightly: full run on 3 OSes |
| Test262 | `tools/test262/` harness | ECMAScript (V8) — qualification set on every V8 update or bindings change | on V8 update |
| Integration | `tests/integration/` with `tools/testserver` | redirects, navigation races, history, process crash/restart, storage eviction, permissions | PR |
| Security | `tests/security/` | origin/CORS/CSP matrices, malicious IPC corpus, sandbox policy assertions (open(2) attempt from renderer → EPERM), cert edge cases | PR |
| Fuzz | `cargo-fuzz` (`tools/fuzz/`) | HTML/CSS/URL/cookie/image/font/IPC decoders, display list validator, DOM mutation sequences | PR 60 s; nightly 30 min |
| Differential | `tools/diff/` | one input → our testshell vs headless Chrome (CDP screenshot/DOM dump) | nightly, triage signal, not proof |
| Performance / memory | `tools/bench/` | cold start, first frame, RSS/PSS per process on the corpus, scroll fps, input latency | PR with `perf` label, nightly |
| UI E2E | WebDriver BiDi (own endpoint in `cl-devtools`) + OS-level (AX API) | omnibox, dialogs, updater, a11y | nightly |
| Real-site corpus | `tools/chrome-corpus/sites.toml` | product acceptance: list of sites × scenarios (load, login form, scroll, video) | milestone gate |

## 2. Determinism

- Fixed fonts in testshell (bundled test fonts, no system ones), DPR=1, viewport 800×600, animations disabled, fixed `Clock`, fixed RNG seed, `Math.random` deterministic in test mode.
- `cl-testshell` — single process? **No:** multiprocess by default even in tests, to catch IPC bugs; `--single-process` only for debugging.

## 3. WPT integration

1. Product adapter: launching the binary, profile in a temp directory, WPT ports/certificates, timeouts, cleanup, crash detection by exit code and dump.
2. Directory enablement order: `url`, `encoding`, `dom`, `html/syntax`, `css/css-cascade`, `css/CSS2`, `fetch`, `xhr`, `html/webappapis`, then `css/css-flexbox`, `css/css-grid`, `css/css-text`, `workers`, `IndexedDB`, `service-workers`…
3. Expected failures — versioned metadata `tools/wpt/expectations/**/*.ini` with bug ID and review date. Rewriting an expectation to hide a regression is forbidden.
4. Every compat fix → a minimal regression test; if it is spec behavior — upstream it to WPT.
5. Dashboard: pass / expected-fail / crash / timeout per directory and commit; new regressions separately.

## 4. Test262

V8 already passes Test262; we run a **qualification set** (~500 tests, coverage: modules, realms, Intl, host hooks, Atomics) on every V8 bump and on changes to `cl-bindings`/event loop, because our host hooks and job queue are our own code.

## 5. Fuzzing

- Targets appear in the same PR as the parser.
- Structured inputs via `arbitrary`; corpora in `tools/fuzz/corpus/` (git LFS later).
- Sanitizers: for pure Rust — miri on unsafe modules; for FFI (V8, wgpu) — ASan build in the nightly Linux job.
- Found crash → minimization → regression test → fix → close with a link.

## 6. Performance and memory

- Corpus: 20 pages (static copies, `tools/bench/corpus/`, license checked) + 5 synthetic stress tests (10k DOM nodes, deep nesting, huge table, flex reflow, 1000 images).
- Metrics: cold start → first frame; navigation → LCP analog; RSS/PSS of each process at 1/5/10 tabs; after freeze; after hibernate; scroll fps on a long page; input latency.
- Gate: a memory regression >5% or time regression >10% blocks merge unless explained in the commit.
- Monthly — comparison against current Chrome on the same corpus, same machine; numbers — in `docs/history/bench-YYYY-MM.md`.

## 7. Definition of "done" for a feature

- a row in `SPEC_REGISTRY.md` with a link to the spec and tests;
- WPT directory enabled, expectations recorded;
- fuzz target (if parser/decoder);
- reftest (if layout/paint);
- no `M*-ONLY` markers older than the current milestone;
- memory budgets not violated.
