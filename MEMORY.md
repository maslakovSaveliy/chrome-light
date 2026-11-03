# MEMORY.md — chrome-light project memory

Updated at the end of every working session. Short facts, links to documents. Session history older than ~a month moves to `docs/history/`.

## State as of 2026-11-03

**Phase:** M1b Renderer + GPU processes — started 2026-11-03 on branch `m1b-renderer-gpu` (from `main` `a1c3ea2`). Plan: `docs/superpowers/plans/2026-09-28-m1b-renderer-gpu-processes.md` (14 tasks, order T1 T2 T3 T4 T6 T5 T7 … T14; Task 0 = `ipc-channel` 0.23.1); ADR-0016 (Accepted), spec `docs/superpowers/specs/2026-09-28-m1b-renderer-gpu-processes-design.md`. Owner decisions 2026-11-03: one PR for the whole of M1b, risk accepted once for the plan, the main agent writes all code, fresh opus reviewers per task. Execution log (gitignored): `.superpowers/sdd/2026-11-03-m1b-renderer-gpu/progress.md`.

**M1a: closed.** PR #2 merged into `main` (merge `a1c3ea2`, 2026-11-03), tag `m1a` → `a1c3ea2`, CI on `main` 5/5. Both pre-merge debts closed: **D2** (super-linear style/inline paths; `nextest list` evidence in the M1a ledger) and **D1** (1 MB smoke page: median max RSS 73 424 896 bytes, criterion < 300 MB). Debug smoke-test budget **stays 5 s locally** (owner, 2026-11-03) — 3.3–3.6 s on a quiet machine, 5.3–12 s under external load 5–19; a gate that goes red under load is re-run on a quiet machine, never committed red (CI keeps 15 s via `CL_SMOKE_SLOW=1`). M1a ledger: `.superpowers/sdd/2026-09-07-m1a-static-pipeline/progress.md`.

**M1a, tasks:** 1–23 — all done and reviewed (1–15 in sessions 3–4; 16–23 in session 6, then the final branch review).

**In the workspace:** `cl-platform`, `cl-ipc`, `cl-process`, `cl-testshell`, `cl-net`, `cl-dom`, `cl-html`, `cl-style`, `cl-fonts`, `cl-layout`, `cl-paint`, `cl-gfx`, `chromelight`. Fuzz targets (6): `ipc_decode` (M0), `url_parse`, `html_parse`, `css_stylesheet`, `display_list_validate`, `display_list_rasterize` (M1a; the last one added by the final review — `cl_gfx::cpu::rasterize` behind the validator).

**Conformance (gates are measurements, not targets):** WPT `urltestdata.json` 828/893 (92.7%), gate 0.92 = the ceiling of crate `url` 2.5.8 (Servo records the same failures). html5lib tree-construction 1307/1313 (99.5%), 0 panics, gate 0.90; the 6 failures are html5ever 0.39 gaps (see `docs/SPEC_REGISTRY.md`). **The denominator is the 36 vendored `.dat` files** of the Task 9 list, not the whole upstream corpus: `svg.dat`, `math.dat`, `plain-text-unsafe.dat`, `scriptdata01.dat`, `foreign-fragment.dat` and others are not vendored, so the SVG/MathML branches of tree construction are not covered by conformance at all. Reftests: **22/22 PASS** (`cargo run -p cl-testshell -- reftest`, `crates/testshell/tests/reftests.rs`; the 20 from the brief plus `margin-collapse-siblings-whitespace`, a regression for the bug in debt 9 below, and `max-width-auto-margins`, a regression for B1 of the final review). Golden dumps — 5 stages (dom/style/box-tree/fragments/display-list). Smoke: 1 MB page 1.51 s (release) / ~4.7 s (debug, measured 2026-09-16; 5.9–6.4 s alone on 2026-09-28), peak RSS ~330 MiB before D1 — **above** the "< 300 MB" criterion (`docs/history/perf-m1a-2026-09.md`).

**Product:** **ChromeLight** (ADR-0013, trademark risk accepted by the owner). License **Apache-2.0 OR MIT** (ADR-0014).

**Repository:** GitHub https://github.com/maslakovSaveliy/chrome-light (public), default branch main; PR #1 (M0) merged, branch deleted; tag `m0` = c0b1244; PR #3 (docs in English) merged into `main` (13ddeab). `.planning/HANDOFF.json` — empty GSD checkpoint (in .gitignore).

**Owner's environment:** macOS 26.5.2, Apple Silicon (arm64), 16 GB RAM, 8 cores, Xcode 26.6, **rustup installed** (1.95.0, targets aarch64-apple-darwin/x86_64-pc-windows-msvc/x86_64-unknown-linux-gnu; cargo-deny/nextest/fuzz/insta present). Note: non-interactive shells don't see `~/.cargo/bin` — use `export PATH="$HOME/.cargo/bin:$PATH"`. cmake/ninja present, docker present, gh present. The machine often runs VMs / iOS Simulator / Docker (load average up to hundreds): report `uptime` next to every timing.

## Accepted decisions (brief; full — docs/adr/)

| ADR | Decision | Status |
|---|---|---|
| 0001 | Project class: independent engine + full browser, solo + AI agents | Accepted |
| 0002 | Rust stable, MSRV pinned, `unsafe` only in FFI crates | Accepted |
| 0003 | Own: DOM, layout, paint, compositor, IPC, processes, net-service, storage, shell. Reused: html5ever, cssparser+stylo, taffy (flex/grid math), parley/swash, vello/wgpu, rustls/hyper/quinn, url, image, accesskit, rusqlite, V8 | Accepted |
| 0004 | JS VM: V8 via the `v8` crate behind the `JsRuntime` trait; own VM — not before v2 | Accepted (revisit M4) |
| 0005 | Multi-process from the first milestone: browser / renderer-per-site / net / gpu / utility; own typed IPC; sandbox: seatbelt / seccomp+namespaces / AppContainer | Accepted |
| 0006 | Graphics: vello + wgpu, CPU fallback; own compositor with property trees | Accepted |
| 0007 | Sync: (a) one-way import/mirror of the local Chrome profile; (b) own sync server over Chromium `sync.proto`. Google Chrome Sync — non-goal forever | Accepted |
| 0008 | Shell UI: egui on wgpu at the start; later a privileged web UI on our own engine | Accepted (revisit M5) |
| 0009 | All three platforms from day one: CI matrix macOS/Win/Linux, platform code only in cl-platform | Accepted |
| 0010 | Tests: WPT + Test262 from the first milestone, headless testshell, reftests, cargo-fuzz | Accepted |
| 0011 | Extensions: MV3 only, own chrome.* runtime; CRX from the Web Store; late phase | Accepted |
| 0012 | Memory is a first-class requirement; budgets and measurement in CI | Accepted |
| 0013 | Product name ChromeLight; Google trademark risk accepted by the owner; disclaimer "not affiliated with Google" | Accepted |
| 0014 | License of our own code: Apache-2.0 OR MIT; deps checked via cargo-deny | Accepted |
| 0015 | stylo integration: `cl-style` on the unsafe list (in fact zero `unsafe` blocks), Python 3 at build time, pinned versions, Option B fallback (not activated) | Accepted (3 amendments) |
| 0016 | M1b: the GPU process is a CPU raster service in M1–M3, the browser presents (softbuffer); shm with copy-then-validate; channel ends only browser → child; a bad renderer message kills the renderer | Accepted |

## Open questions for the owner

1. Chrome 153 baseline in `docs/history/bench-2026-09.md` — measure manually on the owner's machine.
2. ADR-0015 carries three in-place amendments, while CLAUDE.md §6 says Accepted ADRs are immutable (final review C) — keep, or supersede with a new ADR.

## Known debts after M0

Deferred to M1+; do not block the public beta:

1. `BootstrapServer::accept_with_timeout` starts one blocked accept-thread per call; the leak is bounded by one `spawn()` (not by the browser process lifetime) — revisit the handshake in M1.
2. ~~Release sandbox-refusal path takes ~15 s~~ — **closed by M1b Task 2**: `spawn` polls the child while waiting for bootstrap and returns `ExitedDuringBootstrap`; the browser exits 78 in < 1 s, and CI Gate S0 asserts `== 78`, the message, ≤ 3 s, and exit 2 for every debug-only flag in release. Still: worst case `spawn` = 2×timeout (accept + handshake) for a child that connects and then stalls.
3. `browser_side` masks `Violation` if the reject-ack send fails — log-and-ignore.
4. ~~`BrowserEndpoint`/`ChildEndpoint` are structurally identical~~ — **closed by M1b Task 3**: both are generic over `ChildProtocol` (typed downward channel, postcard upward).
5. Fuzz job in CI without rust-cache.
6. Test temp-dir boilerplate is duplicated (testshell, chromelight tests) — helper.
7. Browser-side `recv_timeout` — M0-ONLY poll via `park_timeout(1 ms)`; `park_timeout` is not in disallowed-methods; blocking `BrowserEndpoint::recv` remains pub (used by tests). M1: `IpcReceiverSet`/multiplexed wait, type-level ban on unbounded recv on the browser side.
8. ~~`browser.rs`: `renderer.wait()` after `Shutdown` — unbounded wait~~ — **closed by M1b Task 2**: `ChildProcess::wait_timeout` (2 s after `Shutdown`, then kill + reap).
9. CI: fmt/clippy — in the 3-OS matrix; `deny`/`doc`/scripts — ubuntu only (platform-independent).

## M1a debts (open as of 2026-11-03)

1. **Quadratic parsing on deep nesting** — a potential DoS vector on a hostile page with an artificially deep tree; priority for M2 before the arbitrary Web is opened. The DOM half is **closed by the final review**: `Document::check_attach` no longer walks the whole ancestor chain when the inserted node is childless (html5ever's always are) — a 50 000-deep chain builds in 15 ms instead of 8.6 s. **Styling is closed by D2.a (2026-09-28): linear in release** (depth 2 000 → 32 000: 2.0 → 35.1 ms); the debug-profile quadratic is stylo's own debug `StyleBloom::assert_complete` (95% of debug `resolve` samples at depth 16 000), no code changed. **Parsing remains quadratic in release too** (10 ms at depth 2 000 → 2.07 s at 32 000): html5ever's `close_p_element_in_button_scope` walks the whole stack of open elements on every `<div>` (HTML §13.2.6.4.7, "p in button scope"); not our code, the mitigation is a tree-depth cap in `cl-html` (like Blink's 512), needs a separate task with an ADR. Debug/release tables — `docs/history/perf-m1a-2026-09.md`. **Width (O(width²)):** ~~`prev_sibling_element`/`next_sibling_element` scan siblings~~ — **closed by D2.c (2026-09-28)**: links to the nearest elements are computed once per pass in `NodeArena::link`, each step is O(1); `p:nth-child(2n+1)` over 20 000 children in debug 20.1 → 1.9 s (release linear before and after — `NthIndexCache` in `selectors`; the debug quadratic comes from a `debug_assert_eq!` in `selectors` recomputing the index without the cache). **A non-matching `~` stays quadratic in release** (`.nomatch ~ p`, 40 000 siblings: 10.1 → 5.3 s after D2.c) — that is the `selectors` algorithm (as in Gecko/Blink), not our code; needs a style-pass budget or a document size cap. `:nth-*`/`+`/`~` work in M1a (via `selectors`); the earlier "not implemented" note was wrong.
2. ~~**Peak RSS ~330 MiB on the 1 MB smoke page**~~ — **closed by D1 (2026-11-03)**: median max RSS 73 424 896 bytes (release, `cl-testshell render`), criterion < 300 MB. −150 MB from `752e792` (stage dumps opt-in), −95 MB earlier from the final-review fix wave. Table, method and the remaining (untouched) contributors — `docs/history/perf-m1a-2026-09.md`, "Memory after the final review and D1".
3. **Tests in the release profile were never run** — the stylo rebuild with LTO (`lto = "thin"`, `codegen-units = 1`) is too slow on the owner's machine; everything was verified in debug. `ElementDataWrapper::borrow_mut` does not check aliasing in release — the debug build and `cargo careful` carry real weight (ADR-0015, amendment 2026-09-09).
4. **No bold/italic synthesis** — only real font faces (Noto Regular); `font-weight`/`font-style` on a missing face have no visible effect.
5. **Backgrounds/borders of inline elements are not painted** — `<span style="background/border">` paints only its text (Task 18 decision), never the box itself; needs inline fragments with their own box model (later M).
6. **Own `line-height`/`vertical-align` for nested inlines is not implemented** — the line height is the container's `line-height`, not the maximum over nested inlines.
7. **`line-height: normal` = 1.2 × font-size** — an approximation (`normal_line_height`), not honouring font-specific metrics.
8. ~~**`FontNotBundled` untested**~~ — **closed by the final review**. Attribution corrected on the way: `FontNotBundled` is a `cl_gfx::GfxError` variant (the rasteriser got a `FontKey` its `FontDb` lacks; from M1b on such a key arrives over IPC from a renderer with an older font database), not a `cl-fonts` error about an unrecognised family — `cl_fonts::FontDb::key_for` simply returns `None` for an unknown family. Test: `rasterize_should_report_font_not_bundled` (`crates/gfx/tests/cpu.rs`), through a new doc-hidden `FontKey::from_raw` behind the `arbitrary` feature flag.
9. ~~Self-collapsing empty blocks not implemented~~ — **fixed in Task 23** (CSS 2.1 §9.2.2.1, not §8.3.1): a whitespace text node between two block-level siblings (ordinary markup with line breaks) generated a real (zero-height) anonymous block that broke margin collapsing across two hops and summed the margins instead of taking the max. Fix: `crates/layout/src/box_tree.rs`'s `wrap_inline_runs` no longer generates a box for a run made entirely of text that collapses under `white-space:normal`. Tests: `box_tree_should_not_generate_boxes_for_collapsible_whitespace_between_blocks`, `box_tree_should_keep_whitespace_under_white_space_pre`, `whitespace_between_siblings_should_not_defeat_margin_collapsing`, ref pair `margin-collapse-siblings-whitespace`. 14 golden snapshots re-accepted, each change hand-checked (see the Task 23 fix report).
10. **`cl-layout` memory/perf profiling (two known suspects)** — both found by the final review: (a) ~~`span_item` scans spans linearly per cluster~~ — **closed by D2.b (2026-09-28)**: binary search (`partition_point`), a `<p>` of 40 000 `<span>a</span>` — 2 665 → 311 ms (debug), 371 → 27 ms (release), test `many_inline_spans_should_lay_out_in_linear_time`; (b) `LayoutStyle` (with a `Vec<String>` `font_family` inside) is cloned once per fragment, per `Line` and per `Text` — a direct contributor to the peak RSS of debt 2 (D1 suspect 2).
11. **17+ minor review findings (deferred)** — live in the SDD ledger as `minor (deferred)` lines; triaged by the final review, not listed here by name.

## M1a discoveries not to rediscover

1. **stylo, storage.** The `Box<[UnsafeCell<Option<ElementData>>]>` from ADR-0015 is impossible: `ElementDataMut`/`ElementDataRef` are created only through `ElementDataWrapper`. Production code in `cl-style` has zero `unsafe` blocks; `allow(unsafe_code)` only in `stylo_dom.rs` (implementing the trait's `unsafe fn`s).
2. **stylo, threads.** `!Sync` guarantees nothing: `SendNode`/`SendElement` are an unconditional `unsafe impl Send`. The owner-thread check is a hard `assert_eq!` at nine entry points. Traversal is sequential only (`traverse_dom(.., None)`).
3. **stylo, handle width.** `TElement` must be exactly one word (`sharing/mod.rs`: `FakeCandidate { _element: usize }` + a size assert). Hence `NodeHandle` is a one-word reference into a per-pass `NodeArena`; `slot_count == doc.len()` is checked in `NodeArena::new`. `resolve()` must use `engine.lock.clone()` — a foreign guard makes `Locked::read_with` panic.
4. **Border trap (layout).** stylo computes `border-*-width` as `3px` (`medium`) even with `border-style: none`. The layout adapter must zero the width for `none`/`hidden`.
5. **Fonts.** `fontique`/`parley` with the `system` feature link fontconfig/DirectWrite/CoreText — pinned as `default-features = false, features = ["std"]`; the engine renders only with bundled fonts (Ahem + Noto Sans).
6. **`file:` URL with a remote host** is rejected on every OS (on Windows it is a UNC path = an SMB request). `localhost` and the empty host are allowed.
7. **html5lib-tests:** `master` no longer has `tree-construction/`; the corpus is pinned to `f994590f528ac8b6073665791ddb1ed85c66dfb2`.
8. **`tendril`** is not a direct dependency — take it via `markup5ever::tendril`.
9. **stylo build on CI** ~40–70 s cold; the local 18 min were an artifact of parallel agents (`docs/history/build-times.md`).
10. **Platform cfg:** `tools/check-platform-cfg.sh` forbids `#[cfg(target_os|windows|unix)]` outside `crates/{platform,process,gfx}` — write tests platform-independently (`std::env::temp_dir()`).
11. **Process:** `git add` explicit paths only, never `-A`; the gate chained with `&&` before a commit, and never commit on a red gate (a nudge to "finish" is not permission). Wall-clock tests are listed by exact name in `.config/nextest.toml` (`threads-required = "num-cpus"`).
12. **`ipc-channel` (≥ 0.23.1 since 2026-11-03; GHSA-fj85-w6rj-rxrc in 0.23.0, a repository advisory that `cargo deny` and Dependabot do not see):** the receive path is not robust against a hostile peer — Linux short-datagram `set_len` underflow / `pop().unwrap()` / peer-sized `reserve_exact` (servo/ipc-channel#457), Windows `expect`/`unwrap`, attached channel ends dropped behind `debug_assert!` (panic in debug/`release-dev`, leak in release). Known gap for every child → browser channel, latent until children are sandboxed (M1c); must be closed before **Gate S0 (M2)** by a separate task + ADR (owner decision 2026-11-03; ADR-0016 fact 3, SECURITY.md "Known gaps"). M1b keeps ipc-channel. Deserialising an `IpcSender`/`IpcReceiver` with a bad index PANICS (`src/ipc.rs:1025/1054`), and release is `panic = "abort"` → channel ends travel only browser → child (bootstrap excepted). The sender's shm mapping stays writable → always copy-then-validate.
13. **`cl_net::load_file`** does not check for a regular file (a FIFO hangs, `/dev/zero` reads 32 MiB) — the M1b subresource policy closes it.
14. **Upstream debts, no local patches:** html5ever quadratic on deep nesting in release (`close_p_element_in_button_scope`), `selectors` on `.x ~ p`.

## Key external facts (snapshot 2026-09-07, details — docs/RESEARCH-2026-09.md)

- Servo 0.5.0 (21 Aug 2026) on crates.io; 0.1.0 LTS since 13 Apr 2026. The embedding API is growing, but compatibility is partial. Verso archived.
- Ladybird: C++→Rust, PRs closed, alpha (Linux/macOS) in 2026, beta 2027.
- Chrome Sync API closed to third-party builds since 15 Mar 2021. Brave `go-sync` — an open-source server speaking `sync.proto`.
- MV2 is dead: disabled in Chrome 138 (Jul 2025), Web Store purged 31 Aug 2026.
- Chrome 153 — 8 Sep 2026; releases every 2 weeks after that.
- Chrome 127+ on Windows: App-Bound Encryption for cookies (planned for passwords) — a third-party process cannot decrypt the Chrome profile without user involvement.
- Crate versions as of 2026-09-07: stylo 0.20, cssparser 0.37, html5ever 0.39, taffy 0.14, parley 0.11, vello 0.10, wgpu 30.0, winit 0.30.13, accesskit 0.25, v8 152.2, hyper 1.11, rustls 0.23.43, quinn 0.11.11, rusqlite 0.40, ipc-channel 0.23, prost 0.14, tokio 1.53.
- Re-checked 2026-09-28 (M1b fact-check, `.superpowers/m1b-drafts/m1b-facts-2026-09-28.md`): CHANGED — stylo 0.21, cssparser 0.38, html5ever 0.40.1, Servo 0.6.0, Chrome 154 (2026-09-22). A stylo-family bump is a separate whole-family PR (ADR-0015 §4), not part of M1b.

## Session log

### 2026-11-03 — session 9: M1a merged, M1b started
- Owner: merge PR #2 + tag `m1a` (done, CI on `main` 5/5). Branch `m1b-renderer-gpu` from `main`.
- Found `GHSA-fj85-w6rj-rxrc` (ipc-channel 0.23.0, macOS receive path, high) — Task 0 bumps to 0.23.1; RustSec has no entry, so `cargo deny` did not flag it.
- Task 1: ADR-0016 and the M1b spec translated to English and accepted (bad-message policy with an evidence table, channel ownership, subresource policy, OS-layer facts re-checked on 0.23.1); the plan moved into `docs/` with the main-agent execution rules.

### 2026-11-03 — session 8: `main` merged into the M1a branch
- Merged `origin/main` (13ddeab, PR #3 — docs translated to English) into `m1a-static-pipeline`. 13 conflicts, all in docs. CLAUDE.md/AGENTS.md: the branch version (built on top of main's in `c3b1fd2`). Every other conflicted file: main's English text plus the branch's M1a changes, translated (ARCHITECTURE, CODING_STANDARDS, DEPENDENCIES, DEVELOPMENT, FEATURE_MATRIX, PLAN, SECURITY, TESTING, ADR-0002, ADR index, this file). No code changed.
- Still Russian (branch-only files, to translate before the PR #2 merge): listed in the merge commit message — translated in `2bec196`.
- D1 closed: release RSS measured side by side on four commits (each in its own worktree and `target/`): e2c6d58 304 MB → 2efe0e8 209 MB → 4b6a01f 223 MB → HEAD 73 MB (median). Fresh review of `752e792`: Approved, 5 Minor, 4 fixed (stale docs, dead clippy `allow`s, dhat numbers moved to `docs/history/`), 1 left (unreachable). D2 `nextest list` evidence recorded.
- Gate red twice under external load (node/WebKit processes, load 13–22): a timing test 5.3–5.9 s against 5 s; green on a quiet machine (load 2.5–4.2, smoke 3.62 s). Committed only on the green run.

### 2026-10-28 — session 7b: onboarding of a new main agent (study only)
- Re-read CLAUDE.md, the ledger, the D1 brief, the M1b drafts, ADR-0005/0006/0008/0015, SECURITY. Found: the approved plan file `~/.claude/plans/cheerful-dancing-fog.md` is gone (its content is in the ledger, session 2026-09-28); PR #2 was `CONFLICTING`, so `pull_request` CI did not run on `c3b1fd2`/`9f8de32`.
- `c3b1fd2`, `9f8de32`: CLAUDE.md §3.4/§3.5 agent workflow rules (one owner per change, no implementer subagents, fast/normal/risky modes) and the verified gate in §5.

### 2026-09-30 — docs sessions on `main` (PR #3)
- Owner decision: all docs are English; CLAUDE.md §3.4 language rule updated, chat with the owner stays Russian. README, MEMORY, docs/*.md, docs/adr/*, docs/history/* translated (pure language, line-aligned). README "Status"/Quick start refreshed for the post-M0 state (`-p chromelight -- --no-sandbox`, `cl-testshell render/compare`).
- Noticed, left as is: frozen background tab budget ≤ 15 MB in ARCHITECTURE §9 vs ≤ 25 MB in ADR-0012 / PLAN M4.

### 2026-09-28 — session 7: debt D2 (super-linear paths in style/inline layout)
- D2.a: deep nesting measured per stage in debug and release (depths 2 000–32 000). Style resolve is linear in release — the quadratic is debug-only (stylo `StyleBloom::assert_complete`); box tree and layout linear. Parse is quadratic in release too — html5ever `close_p_element_in_button_scope`, out of mandate, left as debt 1. Numbers — `docs/history/perf-m1a-2026-09.md`. No code changed.
- D2.b: `span_item` (`crates/layout/src/text.rs`) — binary search instead of a scan per cluster; layout of a paragraph with N inlines is linear (release 10 000 → 80 000: 6.3 → 58.3 ms, was 28.8 → 1 029 ms). `smoke_time_limit` moved to `crates/layout/tests/common/mod.rs`. Goldens unchanged.
- D2.c: `ElementHandle::{prev,next}_sibling_element`/`first_element_child` — O(1) via links that `NodeArena::link` (the only writer of the slot's `Cell`s) sets once per pass; zero `unsafe`, one-word handle, owner-thread and `slot_count == doc.len()` asserts in place. Test `nth_child_over_twenty_thousand_siblings_should_resolve_under_the_time_bound` (red 13.8 s → green ~2.0 s in debug) + a unit test of equivalence with the old walk. Found: `.nomatch ~ p` is quadratic in release too — the `selectors` algorithm, debt 1. Goldens unchanged.
- D2 fix round (`4b6a01f`): the five wall-clock-bounded tests run alone under nextest (`threads-required = "num-cpus"`); no bound changed. The 1 MB debug smoke test still misses its 5 s bound locally even alone (5.9–6.4 s, also on the unchanged `2efe0e8`); CI unaffected (15 s).
- M1b designed and approved by the owner (ADR-0016 draft, spec, 14-task plan, bad-message policy: a bad renderer message kills the renderer) — drafts in `.superpowers/m1b-drafts/`.
- `752e792`: D1 work saved — testshell stage dumps opt-in.

### 2026-09-16 — session 6: Tasks 16–23 (subagent-driven), M1a closed
- T16 `cl-layout` box tree finished and returned to the workspace (was parked in `exclude`): anonymous-block wrapping (CSS 2.1 §9.2.1.1), display:none skip, `<br>` → line-break, block-in-inline blockification.
- T17 `Au` geometry + `style_adapt.rs` (borders zeroed for `none`/`hidden` regardless of the stylo width — the "border-width/border-style trap").
- T18 inline formatting context (parley): line breaking, `text-align`, `line-height`, whitespace normal/pre; decision — backgrounds/borders/vertical-align of nested inlines are not implemented in M1a, a `<span>` paints only its text.
- T19 `cl-paint` display list: CSS2 Appendix E paint order, canvas background propagation `<html>`/`<body>` → canvas (§14.2).
- T20 display-list validator + `arbitrary` fuzzing (`display_list_validate`).
- T21 `cl-gfx` CPU raster (tiny-skia + swash), determinism (no hinting, no system fonts).
- T22 `cl-testshell` real single-process pipeline (render/dump/compare), determinism + smoke tests; culling of offscreen items in paint.
- T23: reftest runner (`crates/testshell/src/reftest.rs`) + 20 reftest pairs from the brief; `<hr>` got a UA border (HTML §15.3.11, `solid` instead of `inset`); CI: an explicit `reftest` step in the `test` matrix + upload artifacts on failure. Found a real engine bug (whitespace between block siblings breaks margin collapsing, CSS 2.1 §9.2.2.1) — fixed before review (debt 9 above).
- **Final branch review (3 opus reviewers) + fix wave, 4 commits.** Two Critical: (B1) the §10.4 min/max clamp never applied to `width:auto` — `max-width` + `margin:0 auto` neither centred nor limited; (C1) offscreen culling in `cl-paint` and the bounds check in `validate` required full *containment* in the inflated viewport, so a container background taller than `viewport + 4096px` was dropped and the page rendered blank — both became *intersection*. Important: `min-height` did not block bottom-margin collapsing; `box-sizing` not applied to `min-*`/`max-*`; `overflow != visible` now creates a BFC (CSS 2.1 §9.4.1); `ParseOutput::parse_errors` capped at 256 + a suppressed count; `h1`–`h6 { font-weight: bold }` in the UA sheet; `check_attach` no longer quadratic on deep nesting; sixth fuzz target `display_list_rasterize` (774 563 runs in 60 s, 0 crashes). None of the 15 golden snapshots changed. 22 ref pairs (`max-width-auto-margins`). Details: `.superpowers/sdd/2026-09-07-m1a-static-pipeline/final-fix-wave-report.md`. CI on `2efe0e8` 5/5.

### 2026-09-16 — session 5: onboarding of a new controller, MEMORY.md
- MEMORY.md brought to the M1a state from the SDD ledger (was at M0 level). Queue: debt review (Task 14, `2d38014`, `cl-net` `550db26`/`d410182`) → Task 16 → 17–23.

### 2026-09-08 … 2026-09-10 — sessions 3–4: M1a execution (subagent-driven), tasks 1–15
- T1 ADR-0015 + scope scripts + python3 in CI; T2 stylo build spike (compiles on 3 OSes); T3–T4 `cl-dom` arena + html5lib serializer; T5–T6 `cl-net` `Url` + file loader + WPT url harness (gate lowered to 0.92 — the ceiling of crate `url`, `Ruling` in the ledger); T7 encoding sniffer (1 fix: unmatched quote in `<meta content>`); T8 `TreeSink` over the arena (2 fix rounds, tests); T9 html5lib harness 1307/1313; T10 `html_parse` fuzz; T11 stylo DOM traits — **gate passed**, zero `unsafe`, ADR-0015 amended twice; T12 UA stylesheet + `<style>`/`<link>` collection; T13 `resolve()` via sequential traversal, the handle reworked into a one-word reference (third ADR-0015 amendment), computed-style goldens; T14 `css_stylesheet` fuzz; T15 `cl-fonts` (Ahem + Noto, no system backends; review confirmed sha256 and licenses).
- Incidents: the controller's `git add -A` swept the unfinished `cl-fonts` into someone else's commit (fixed, rule recorded); Windows CI: `to_file_path` for `file://host/...` — the engine was fixed (`550db26`), not the test.
- T16 interrupted by the owner mid-edit; partial work committed (`fc4ae0d`), crate excluded from the workspace.

### 2026-09-07 — session 2: M0 execution (subagent-driven)
- Executed Tasks 1–11 from `docs/superpowers/plans/2026-09-07-m0-foundation.md`: T1 workspace/lints/deny (1 fix), T2 cl-platform (no fixes), T3–T5 cl-ipc codec/validate/bootstrap/handshake (no fixes), T6–T7 cl-process sandbox/spawn (no fixes), T8 browser/child ping-pong + tracing (1 fix: `--browser-fail-after-handshake`), T9 testshell PNG (no fixes), T10 bench script (no fixes; Chrome baseline pending owner action), T11 CI check-agents + Gates (3 fixes: rustfmt nightly options noted, cargo-fuzz flags fixed, Exit codes verified).
- CI result: check ✅, test (macOS-14/windows-2022/ubuntu-24.04) ✅, fuzz-short ✅ on all three OSes.
- Measurements: idle RSS 4864 KB (browser 2496, renderer 2368). Release sandbox gate PASS (unsandboxed exit 78). Fuzz run 60 s: 75.2M runs, 0 crashes.
- Task 12: updated PLAN.md exit-criteria (5/6 ticked, Chrome baseline remains owner-action), MEMORY.md state/debts/log, ARCHITECTURE.md crate status.
- Final branch review (sonnet; opus returned 429): 0 Critical, 2 Important fixed in the fix wave (bounded recv, 3-OS lint), minor debts collected into the list above.

### 2026-09-07 — session 1: research + documentation foundation
- Read the `browser-engine-research` skill (6 references, snapshot as of 2026-09-05).
- Web verification: Servo, Ladybird, CEF/cef-rs, Chrome Sync, MV2/MV3, Chromium release cadence, Rust stack, App-Bound Encryption, Rust GUI, wry.
- The owner chose: own engine from scratch in Rust; solo + AI agents; all three platforms at once; sync = local Chrome profile + own server.
- Created: CLAUDE.md, AGENTS.md, MEMORY.md, README.md, docs/ (ARCHITECTURE, DEVELOPMENT, CODING_STANDARDS, SECURITY, TESTING, RESEARCH-2026-09, FEATURE_MATRIX, SPEC_REGISTRY, DEPENDENCIES, GLOSSARY), docs/adr/0001–0014, toolchain configs.
- Owner: license — as permissive as possible (→ Apache-2.0 OR MIT), name — ChromeLight, `git init` — yes, rustup installed.
- Created: `docs/PLAN.md` (M0–M6 program, exit criteria, rituals, risks), `docs/superpowers/plans/2026-09-07-m0-foundation.md` (12 tasks with code and tests), LICENSE-APACHE/MIT; ADR-0013/0014 → Accepted; git repository initialized, first commit — all documentation.
