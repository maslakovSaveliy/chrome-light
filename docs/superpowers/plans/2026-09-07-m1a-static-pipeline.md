# M1a Static Pipeline Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> Approved by the owner 2026-09-07; executed from this file.

## Context

M0 (tag `m0`) delivered the multi-process skeleton, IPC, sandbox type-state and a testshell that writes a blank PNG. M1 ("Static pages", `docs/PLAN.md` §2) is 8–12 agent-weeks and is split into five sub-plans that each ship working software:

| Sub-plan | Delivers | Plan file |
|---|---|---|
| **M1a (this)** | `cl-testshell render page.html --png out.png` through a real single-process pipeline: bytes → encoding sniff → html5ever → arena DOM → **stylo** cascade → own block/inline layout with parley text → own display list → tiny-skia CPU raster. JS-free conformance harnesses (html5lib-tests, WPT `urltestdata.json`), 20 reftests, 4 fuzz targets, golden dumps. | `2026-09-07-m1a-static-pipeline.md` |
| M1b | Renderer + GPU processes: display list over shared memory, `chromelight` renders a file in three processes | later |
| M1c | Sandbox macOS (seatbelt) + Linux (Landlock + seccomp + no_new_privs), `--probe` tests | later |
| M1d | egui shell, `file://` + `http://localhost` via `cl-net`, one tab | later |
| M1e | Bench corpus, Chrome baseline, M1 exit gates | later |

Owner decisions (2026-09-07): stylo from day one (ADR-0003), which makes `cl-style` the fifth `unsafe`-permitted crate and Python 3 a build dependency (new **ADR-0015**); JS-free conformance in M1, wptrunner product adapter moves to M2 (updates `docs/PLAN.md`, `docs/TESTING.md`, ADR-0010 action items); only M1a is detailed now.

Research snapshot 2026-09-07 (verified on crates.io/docs.rs/GitHub; re-verify signatures marked *verify* at implementation): stylo 0.20 (`stylo`, `stylo_traits`, `stylo_dom`, `stylo_atoms`, `stylo_static_prefs`, `selectors` 0.40, `cssparser` 0.37; default feature `servo`, never `gecko`; `style/build.rs` runs `python3 properties/build.py`); html5ever/markup5ever 0.39 (`TreeSink` takes `&self`); parley 0.11.1 / fontique 0.11.1 / swash 0.2.10 / skrifa 0.46; tiny-skia 0.12 (no text API); encoding_rs 0.8.40; url 2.5.8; insta 1.48; Blitz `main` (`packages/blitz-dom/src/stylo.rs`, `node/stylo_data.rs`, `stylo_device.rs`, `packages/stylo_taffy/src/convert.rs`) is the reference stylo embedding.

**Goal:** A deterministic, single-process static renderer with real HTML/CSS/text, conformance harnesses, reftests and fuzzing — the correctness baseline every later milestone diffs against.

**Architecture:** Arena DOM (`Vec<Node>` + `NodeId`), stylo driven through `Copy` handles (`&Document`, `&StyleStore`, `NodeId`) with a per-pass side table of `ElementData` (the only `unsafe`), layout in integer app units (`Au = 1/60 px`) producing an immutable fragment tree, display list validated before raster, CPU raster via tiny-skia + swash alpha bitmaps. `servo_arc`/`style::` types never leave `cl-style` except through `cl_layout::style_adapt`.

**Tech Stack:** Rust 1.95 stable (edition 2024), stylo 0.20, html5ever 0.39, parley 0.11, fontique 0.11, swash 0.2, tiny-skia 0.12, encoding_rs 0.8, url 2.5, insta 1.48, proptest 1, cargo-fuzz, Python 3.12 (build-time only).

**Spec:** `docs/PLAN.md` §2 M1 (as amended by Task 1), `docs/ARCHITECTURE.md` §4 & §8, `docs/SECURITY.md` §3, `docs/CODING_STANDARDS.md`, ADR-0002, ADR-0003, ADR-0015 (Task 1).

## Global Constraints

Everything from the M0 plan still holds (toolchain pin, clippy `-D warnings`, no `unwrap`/`expect` outside tests, no panic on untrusted input, platform cfg only in platform/process/gfx, commit format with `Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>`, `export PATH="$HOME/.cargo/bin:$PATH"` in non-interactive shells, paste only real terminal output into reports). Delta for M1a:

- `unsafe` is allowed in exactly four crates: `cl-platform`, `cl-process`, `cl-gfx`, **`cl-style`**. In `cl-style` it is module-scoped: crate root has `#![deny(unsafe_code)]`, `#![deny(unsafe_op_in_unsafe_fn)]`, `#![deny(clippy::undocumented_unsafe_blocks)]`; only `store.rs`, `handle.rs`, `stylo_dom.rs` carry `#![allow(unsafe_code)]`. Every block has a `// SAFETY:` comment naming the aliasing argument and the single-thread invariant. Enforced by `tools/check-unsafe-scope.sh` (Task 1).
- `servo_arc::*` and `style::*` paths may appear only in `crates/style/**` and `crates/layout/src/style_adapt.rs`. Enforced by `tools/check-stylo-scope.sh` (Task 1).
- Python 3 (≥3.10) is a **build** dependency (stylo codegen). CI installs it with `actions/setup-python@v5` on all three runners.
- Determinism: viewport 800×600, DPR 1, bundled fonts only (Ahem CC0 + Noto Sans OFL-1.1), font hinting off, no system font enumeration, no golden PNGs in git — pixels are compared only render-vs-render on one machine; text goldens (`insta`) for DOM / computed style / fragment tree / display list.
- Layout arithmetic is integer `Au`; `f32` is allowed only inside `cl_layout::text` (parley/shaping) and rounded to `Au` at the run boundary.
- CSS scope of M1a layout: `display: block|inline|none`; `width/height/min-*/max-*` (px, %, auto); `margin` (incl. `auto`), `padding`, `border-*-width/-style: solid|none/-color`; `box-sizing`; `position: static|relative`; `color`, `background-color`; `font-family/size/weight/style`, `line-height`, `text-align: left|right|center`, `white-space: normal|pre`; `overflow: visible|hidden` (clip); `<br>`. Everything else stylo parses is ignored by layout and listed `partial` in `docs/SPEC_REGISTRY.md`. Floats, abs/fixed positioning, `<img>`, lists markers, tables, flex, grid: **not in M1a**.
- Every parser/decoder/validator introduced ships its fuzz target in the same task: `html_parse`, `css_stylesheet`, `url_parse`, `display_list_validate`.
- Crate package names `cl-<name>`, lib names `cl_<name>`, `license = "Apache-2.0 OR MIT"`, `[lints] workspace = true`, `version.workspace = true`, listed in root `[workspace.dependencies]` with `version = "0.0.1"`.
- Task tiers: **cheap** (haiku) when the brief carries complete code; **mid** (sonnet) when the implementer must adapt to a live crate API (stylo, html5ever assoc types, parley, fontique, swash) — those briefs say "verify on docs.rs / registry source" explicitly.
- The stylo gate (Task 11): after 3 implementer attempts or 5 working days without a passing `p_with_color_rule_should_resolve_to_red`, execute the **fallback** documented in ADR-0015 Option B (own minimal cascade behind the identical `cl_style` API) and continue; stylo re-enters in M2.

---

## File Structure

```
Cargo.toml                          + members cl-net, cl-dom, cl-html, cl-style, cl-fonts, cl-layout, cl-paint, cl-gfx; pinned deps
crates/net/                         cl-net (M1a subset: Url newtype + file loader; grows in M1d/M2)
  src/lib.rs  src/url.rs  src/file.rs  src/error.rs  tests/urltestdata.rs
crates/dom/                         cl-dom (#![forbid(unsafe_code)])
  src/lib.rs  src/node.rs  src/element.rs  src/document.rs  src/traverse.rs  src/serialize.rs  src/error.rs
crates/html/                        cl-html
  src/lib.rs  src/encoding.rs  src/prescan.rs  src/sink.rs  src/error.rs  tests/tree_construction.rs
crates/style/                       cl-style (unsafe: store.rs, handle.rs, stylo_dom.rs)
  src/lib.rs  src/engine.rs  src/sheets.rs  src/store.rs  src/handle.rs  src/stylo_dom.rs  src/stylo_selectors.rs
  src/traversal.rs  src/dump.rs  src/error.rs  assets/ua.css
crates/fonts/                       cl-fonts (leaf: bundled fonts + fontique collection)
  src/lib.rs  src/bundled.rs  src/db.rs  assets/Ahem.ttf  assets/LICENSE-Ahem  assets/NotoSans-Regular.ttf  assets/LICENSE-OFL
crates/layout/                      cl-layout
  src/lib.rs  src/au.rs  src/geom.rs  src/style_adapt.rs  src/box_tree.rs  src/block.rs  src/inline.rs  src/text.rs
  src/fragment.rs  src/dump.rs
crates/paint/                       cl-paint
  src/lib.rs  src/list.rs  src/build.rs  src/validate.rs  src/dump.rs  src/error.rs
crates/gfx/                         cl-gfx (CPU backend only in M1a)
  src/lib.rs  src/cpu/mod.rs  src/cpu/shapes.rs  src/cpu/text.rs  src/cpu/glyph_cache.rs  src/error.rs
crates/testshell/                   cl-testshell
  src/lib.rs (keeps compare_png/parse_viewport; render_blank removed)  src/pipeline.rs  src/dump.rs  src/reftest.rs  src/main.rs
  tests/cli.rs  tests/determinism.rs  tests/smoke.rs
tests/ref/                          20 reftest pairs <name>.html / <name>-ref.html + fonts.css
tools/conformance/html5lib/         fetch.sh, PINNED_COMMIT, LICENSE, tree-construction/*.dat (vendored subset), expectations.txt
tools/conformance/url/              fetch.sh, PINNED_COMMIT, urltestdata.json, expectations.txt
tools/fuzz/fuzz_targets/            html_parse.rs  css_stylesheet.rs  url_parse.rs  display_list_validate.rs
tools/check-unsafe-scope.sh  tools/check-stylo-scope.sh
docs/adr/0015-stylo-integration.md  docs/PLAN.md  docs/TESTING.md  docs/DEPENDENCIES.md  docs/DEVELOPMENT.md  docs/CODING_STANDARDS.md
.github/workflows/ci.yml
```

Dependency DAG (must stay acyclic; `tools/check-deps.sh` is M1b): `cl-fonts` and `cl-net` are leaves; `cl-dom` depends on markup5ever only; `cl-html` → dom, net, encoding_rs, html5ever; `cl-style` → dom, net, stylo family; `cl-layout` → style, dom, fonts, parley; `cl-paint` → layout; `cl-gfx` → paint, fonts, tiny-skia, swash; `cl-testshell` → all.

---

### Task 1: ADR-0015, plan/docs amendments, CI python, scope-check scripts

**Files:**
- Create: `docs/adr/0015-stylo-integration.md`, `tools/check-unsafe-scope.sh`, `tools/check-stylo-scope.sh`
- Modify: `docs/adr/0002-language-and-unsafe-policy.md` (status line), `docs/adr/README.md`, `docs/PLAN.md` (§2 M1), `docs/TESTING.md` (§2 exception, §3 WPT → M2), `docs/DEPENDENCIES.md` (stylo family, parley family, fonts assets section), `docs/DEVELOPMENT.md` (§1 Python 3 required to build), `docs/CODING_STANDARDS.md` (§4 crate list), `.github/workflows/ci.yml`, `Cargo.toml` (workspace deps only; no members yet)

**Interfaces:**
- Produces: the binding rules every later task cites; pinned versions in `[workspace.dependencies]`.

- [ ] **Step 1: Write ADR-0015** (`docs/adr/0015-stylo-integration.md`, Russian like the others):

```markdown
# ADR-0015: Интеграция stylo — unsafe в cl-style, Python 3 на сборке, pinned версия

**Status:** Accepted
**Date:** 2026-09-07
**Deciders:** владелец проекта
**Amends:** ADR-0002 (список unsafe-crates), ADR-0010 (action item 1 — WPT adapter → M2)

## Context
ADR-0003 выбрал stylo как CSS-движок. Разведка (2026-09-07): stylo 0.20 требует `python3` в `build.rs` (Mako-кодогенерация свойств); embedder реализует `style::dom::{TNode,TElement,...}` на `Copy`-хэндлах, а `TElement::{ensure_data, clear_data, set_dirty_descendants, ...}` — `unsafe fn`, хранилище `ElementData` требует interior mutability; Blitz (референс) держит сырой `*mut` на арену (открытая проблема DioxusLabs/blitz#151). Владелец подтвердил stylo с первого дня.

## Decision
1. `cl-style` — пятый crate с разрешённым `unsafe`, только в модулях `store.rs`, `handle.rs`, `stylo_dom.rs`; хэндл — `Copy`-тройка `(&Document, &StyleStore, NodeId)`, не сырой указатель; `ElementData` — side table на один проход стиля (`Box<[UnsafeCell<Option<ElementData>>]>`), пересоздаётся каждый `resolve()`; traversal только последовательный (`traverse_dom(.., None)`); `debug_assert` проверки потока в каждом `unsafe` входе.
2. `servo_arc`/`style::*` не покидают `crates/style/**` и `crates/layout/src/style_adapt.rs` (скрипт `tools/check-stylo-scope.sh`).
3. Python 3 ≥ 3.10 — build-зависимость; CI ставит `actions/setup-python@v5` во всех job-ах.
4. Версии закреплены в `[workspace.dependencies]`: stylo 0.20.0, stylo_traits 0.20.0, stylo_dom 0.20.0, stylo_atoms 0.20.0, selectors 0.40, cssparser 0.37, html5ever/markup5ever 0.39 (одна версия markup5ever в дереве — проверка `cargo tree -d`). Обновление — только целиком, отдельным PR.
5. `cargo miri` для cl-style невозможен (build.rs/кодоген); вместо него nightly `cargo careful test -p cl-style` + `careful` в CI nightly.
6. **Option B (fallback):** если задача интеграции трейтов не проходит тест `p_with_color_rule_should_resolve_to_red` после 3 попыток / 5 рабочих дней — минимальный собственный cascade за тем же API `cl_style::{StyleEngine, StyledDocument, computed()}` (cssparser + selectors, ~20 longhand-свойств, без @media/custom properties); stylo возвращается в M2. Решение фиксируется здесь как «Option B taken» с датой.

## Consequences
- Легче: Firefox-grade cascade, selectors, @media, custom properties бесплатно.
- Труднее: время сборки, Python в CI, unsafe-review для трёх модулей, Windows-сборка stylo.
- Пересмотреть: при смене мажорной версии stylo или если Blitz#151 даст safe-паттерн — перейти на него.
```

- [ ] **Step 2: Amend ADR-0002 and README**: in `0002-language-and-unsafe-policy.md` change `**Status:** Accepted` to `**Status:** Accepted (amended by ADR-0015: cl-style added to the unsafe list)` and in the Decision bullet list add `cl-style` (modules `store.rs`, `handle.rs`, `stylo_dom.rs`). Add row `| [0015](0015-stylo-integration.md) | Интеграция stylo: unsafe в cl-style, Python 3, pinned версия, fallback Option B | Accepted |` to `docs/adr/README.md`.

- [ ] **Step 3: Rewrite `docs/PLAN.md` §2 M1**: keep the goal line; replace the "Scope по crate-ам" list with a sub-plan table (M1a–M1e as in Context above, one line each with what it delivers); replace the exit criteria block with:

```markdown
Exit-критерии M1 (объединение M1a–M1e):
- [ ] M1a: single-process pipeline; html5lib tree-construction ≥ 90% (0 panics), WPT `urltestdata.json` ≥ 95%, 20 reftests, 4 fuzz-targets, golden dumps 4 стадий — детальные критерии в `docs/superpowers/plans/2026-09-07-m1a-static-pipeline.md`.
- [ ] M1b: `chromelight file:///page.html` рендерит через renderer + GPU процессы (display list через shm).
- [ ] M1c: sandbox renderer применяется на macOS и Linux; `--probe` тесты (open /etc/passwd, socket) → EPERM.
- [ ] M1d: egui shell с одной вкладкой, `file://` и `http://localhost`.
- [ ] M1e: бюджет ≤ 120 МБ RSS (пустой браузер + пустая вкладка), Chrome baseline на corpus записан.
- wptrunner product adapter перенесён в M2 (testharness.js требует JS; текущий wptrunner ожидает WebDriver classic от продукта).
```

Also in §0 add: "Python 3 — build-зависимость с M1a (stylo)."; in §4 risks table update the stylo row: "fallback = ADR-0015 Option B".

- [ ] **Step 4: `docs/TESTING.md`**: §2 add "Исключение M1a: `cl-testshell` рендерит в одном процессе (`// M1a-ONLY`), multiprocess testshell — M1b." §3 heading becomes "WPT интеграция (M2+)" with one sentence: "В M1 — JS-free корпуса: html5lib-tests tree-construction (`tools/conformance/html5lib/`), WPT `urltestdata.json` (`tools/conformance/url/`), запускаются как `cargo test`." `docs/DEVELOPMENT.md` §1: Python row → "≥3.10 | **обязателен для сборки** (stylo codegen), WPT/Test262 tooling". `docs/CODING_STANDARDS.md` §4 first line: add `cl-style` (модули `store.rs`, `handle.rs`, `stylo_dom.rs`). `docs/DEPENDENCIES.md`: in "Движок" table replace the stylo row's risk cell with "высокий — unsafe в cl-style, Python 3 на сборке (ADR-0015)"; add rows `stylo_traits`, `stylo_dom`, `stylo_atoms`, `stylo_static_prefs` ("часть stylo, та же версия"), `selectors 0.40`, `encoding_rs 0.8.40`, `fontique 0.11`, `skrifa 0.46`, `tendril`; add section "## Bundled assets" with `Ahem.ttf — CC0-1.0 (WPT fonts/Ahem.ttf)` and `NotoSans-Regular.ttf — OFL-1.1, unmodified (Reserved Font Name clause не затрагивает)`.

- [ ] **Step 5: Scope-check scripts** (both `chmod +x`):

`tools/check-unsafe-scope.sh`:
```bash
#!/usr/bin/env bash
# CI gate: `unsafe` only in cl-platform, cl-process, cl-gfx, cl-style (ADR-0002 + ADR-0015).
set -euo pipefail
cd "$(dirname "$0")/.."
ALLOWED='^(crates/platform/|crates/process/|crates/gfx/|crates/style/src/(store|handle|stylo_dom)\.rs)'
HITS=$(grep -rnE '\bunsafe\b' --include='*.rs' crates apps 2>/dev/null \
  | grep -vE '^[^:]+:[0-9]+:\s*//' \
  | grep -vE 'forbid\(unsafe_code\)|deny\(unsafe_code\)|allow\(unsafe_code\)|unsafe_op_in_unsafe_fn|undocumented_unsafe_blocks' \
  | grep -vE "$ALLOWED" || true)
if [ -n "$HITS" ]; then echo "unsafe outside allowed scope:" >&2; echo "$HITS" >&2; exit 1; fi
echo "unsafe scope check ok"
```

`tools/check-stylo-scope.sh`:
```bash
#!/usr/bin/env bash
# CI gate: stylo types stay inside cl-style and cl-layout's adapter (ADR-0015 §2).
set -euo pipefail
cd "$(dirname "$0")/.."
ALLOWED='^(crates/style/|crates/layout/src/style_adapt\.rs)'
HITS=$(grep -rnE '\b(servo_arc|style::|stylo|selectors::|cssparser)\b' --include='*.rs' crates apps 2>/dev/null \
  | grep -vE '^[^:]+:[0-9]+:\s*//' | grep -vE "$ALLOWED" || true)
if [ -n "$HITS" ]; then echo "stylo types outside cl-style/style_adapt:" >&2; echo "$HITS" >&2; exit 1; fi
echo "stylo scope check ok"
```

- [ ] **Step 6: CI**: in `.github/workflows/ci.yml` add to `check`, `test` and `fuzz-short` jobs, right after `actions/checkout@v4`:
```yaml
      - uses: actions/setup-python@v5
        with:
          python-version: "3.12"
      - run: python3 --version
```
Add to `check` job after the two existing script steps: `- run: bash tools/check-unsafe-scope.sh` and `- run: bash tools/check-stylo-scope.sh`. In `fuzz-short`, extend the run step to loop: `for t in ipc_decode html_parse css_stylesheet url_parse display_list_validate; do cargo +nightly fuzz run --fuzz-dir tools/fuzz --target x86_64-unknown-linux-gnu "$t" -- -max_total_time=45; done` (targets appear in later tasks; until then keep only `ipc_decode` — **this task keeps the loop with `ipc_decode` only and a comment listing the future names**). Note in the commit message that `python3` is on PATH on windows-2022 via setup-python.

- [ ] **Step 7: Pin versions** in root `Cargo.toml` `[workspace.dependencies]` (add, do not create crates yet):
```toml
cl-net = { path = "crates/net", version = "0.0.1" }
cl-dom = { path = "crates/dom", version = "0.0.1" }
cl-html = { path = "crates/html", version = "0.0.1" }
cl-style = { path = "crates/style", version = "0.0.1" }
cl-fonts = { path = "crates/fonts", version = "0.0.1" }
cl-layout = { path = "crates/layout", version = "0.0.1" }
cl-paint = { path = "crates/paint", version = "0.0.1" }
cl-gfx = { path = "crates/gfx", version = "0.0.1" }

cssparser = "0.37"
encoding_rs = "0.8"
fontique = "0.11"
html5ever = "0.39"
insta = { version = "1.48", features = ["yaml"] }
markup5ever = "0.39"
parley = "0.11"
selectors = "0.40"
skrifa = "0.46"
smallvec = "1"
stylo = { version = "0.20", default-features = false, features = ["servo"] }
stylo_atoms = "0.20"
stylo_dom = "0.20"
stylo_traits = "0.20"
swash = "0.2"
tendril = "0.4"
url = "2.5"
```
(`tiny-skia`, `serde`, `thiserror`, `tracing`, `proptest` already present.) Run `cargo metadata --format-version 1 >/dev/null` — unused workspace deps are fine.

- [ ] **Step 8: Verify and commit**: `bash scripts/check-agents-md.sh && bash tools/check-unsafe-scope.sh && bash tools/check-stylo-scope.sh && cargo deny check && cargo test --workspace --locked` (all green — nothing compiled yet beyond M0). YAML validate. Commit `docs: ADR-0015 stylo integration, M1 split into sub-plans, python3 + scope gates in CI`.

---

### Task 2: stylo build spike (no DOM yet)

**Files:** Create `crates/style/Cargo.toml`, `crates/style/src/lib.rs`, `crates/style/src/error.rs`, `crates/style/src/engine.rs`; modify root `Cargo.toml` members (glob already covers `crates/*`).

**Interfaces:**
- Produces: `cl_style::StyleEngine::new(viewport: (f32, f32), dpr: f32) -> Result<StyleEngine, StyleError>`, `StyleEngine::add_author_sheet(&mut self, css: &str, base: &str) -> Result<(), StyleError>`, `StyleEngine::author_rule_count(&self) -> usize`.
- Exit criterion: the crate compiles on macOS, Windows, Linux CI with Python present, and a stylesheet with one rule is accepted.

- [ ] **Step 1: `Cargo.toml`**
```toml
[package]
name = "cl-style"
description = "ChromeLight style system: stylo cascade over the arena DOM"
version.workspace = true
edition.workspace = true
license.workspace = true
rust-version.workspace = true

[lib]
name = "cl_style"

[dependencies]
cl-dom.workspace = true        # added in Task 11; until then comment this line out
cl-net.workspace = true        # idem
cssparser.workspace = true
markup5ever.workspace = true
selectors.workspace = true
stylo.workspace = true
stylo_atoms.workspace = true
stylo_dom.workspace = true
stylo_traits.workspace = true
thiserror.workspace = true
tracing.workspace = true

[dev-dependencies]
insta.workspace = true

[lints]
workspace = true
```
In this task leave the two `cl-*` lines commented out.

- [ ] **Step 2: Failing test in `engine.rs`**
```rust
//! Owns the stylo `Stylist`, `Device` and the shared lock. Sheet bookkeeping is manual
//! (`stylist.append_stylesheet`) like Blitz, not `DocumentStylesheetSet`.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::expect_used)]
    fn stylist_should_accept_one_author_rule() {
        let mut engine = StyleEngine::new((800.0, 600.0), 1.0).expect("engine");
        engine.add_author_sheet("p { color: red }", "file:///test.html").expect("sheet");
        assert_eq!(engine.author_rule_count(), 1);
    }
}
```

- [ ] **Step 3: Implement** (mid tier — verify each constructor on docs.rs `stylo` 0.20: `style::stylist::Stylist::new`, `style::media_queries::{Device, MediaType, MediaList}`, `style::shared_lock::SharedRwLock`, `style::stylesheets::{Stylesheet, Origin, DocumentStyleSheet, AllowImportRules, UrlExtraData}`, `style::context::QuirksMode`, `style::font_metrics::FontMetricsProvider` (implement a `DefaultFontMetricsProvider` returning zeros/ex=0.5em), `style::properties::ComputedValues::initial_values_with_font_override`, `style::color::AbsoluteColor`, `style::values::specified::color::ColorScheme`). Reference: Blitz `stylo_device.rs::make_device` and `document.rs::make_stylesheet`. Skeleton:
```rust
pub struct StyleEngine {
    lock: SharedRwLock,
    stylist: Stylist,
    author: Vec<DocumentStyleSheet>,
}
impl StyleEngine {
    pub fn new(viewport: (f32, f32), dpr: f32) -> Result<Self, StyleError> { /* Device::new(MediaType::screen(), QuirksMode::NoQuirks, size, size, Scale::new(dpr), Box::new(DefaultFontMetricsProvider), initial_values, ColorScheme::Light) ; Stylist::new(device, QuirksMode::NoQuirks) */ }
    pub fn add_author_sheet(&mut self, css: &str, base: &str) -> Result<(), StyleError> { /* UrlExtraData from url; Stylesheet::from_str(css, url_data, Origin::Author, Arc::new(self.lock.wrap(MediaList::empty())), self.lock.clone(), None, None, QuirksMode::NoQuirks, AllowImportRules::No); wrap DocumentStyleSheet(Arc::new(sheet)); self.stylist.append_stylesheet(sheet.clone(), &self.lock.read()); push */ }
    pub fn author_rule_count(&self) -> usize { /* sum of sheet.contents.rules.read_with(&guard).0.len() */ }
}
```
`StyleError` (thiserror): `Device`, `Sheet(String)`, `Url(String)`. `lib.rs`: `#![deny(unsafe_code)] #![deny(unsafe_op_in_unsafe_fn)] #![deny(clippy::undocumented_unsafe_blocks)] #![deny(missing_docs)]` + `pub mod engine; pub mod error; pub use engine::StyleEngine; pub use error::StyleError;`.

- [ ] **Step 4: Measure and record**: `time cargo build -p cl-style` cold and warm; paste both into the report and into `docs/history/build-times.md` (new file: date, machine, cold/warm seconds). If cold > 10 min, note it as a risk for the CI cache.

- [ ] **Step 5: Gate + commit** `style: stylo build spike — StyleEngine with one author sheet`. Push a temporary branch? No — the controller pushes the feature branch after this task specifically to see the 3-OS build result before continuing to Task 11.

---

### Task 3: `cl-dom` arena core

**Files:** Create `crates/dom/Cargo.toml`, `src/lib.rs`, `src/error.rs`, `src/node.rs`, `src/element.rs`, `src/document.rs`.

**Interfaces (produced; used by every later crate):**
```rust
pub struct NodeId(u32);                       // Copy, Eq, Hash, Ord, Debug; NodeId::index(self) -> usize
pub enum QuirksMode { NoQuirks, LimitedQuirks, Quirks }
pub struct Attr { pub name: QualName, pub value: StrTendril }
pub struct Element { pub name: QualName, pub attrs: Vec<Attr>, pub template_contents: Option<NodeId> }
pub struct Doctype { pub name: StrTendril, pub public_id: StrTendril, pub system_id: StrTendril }
pub enum NodeKind { Document, Doctype(Doctype), Element(Element), Text(StrTendril), Comment(StrTendril),
                    ProcessingInstruction { target: StrTendril, data: StrTendril }, DocumentFragment }
pub struct Node { pub kind: NodeKind, parent: Option<NodeId>, first_child: Option<NodeId>, last_child: Option<NodeId>,
                  prev_sibling: Option<NodeId>, next_sibling: Option<NodeId> }  // link getters are pub fns
pub struct Document { nodes: Vec<Node>, quirks: QuirksMode, base_url: String }
impl Document {
    pub fn new(base_url: &str) -> Self;                       // node 0 = Document
    pub fn root(&self) -> NodeId;
    pub fn get(&self, id: NodeId) -> Option<&Node>;
    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut Node>;
    pub fn len(&self) -> usize; pub fn is_empty(&self) -> bool;
    pub fn create(&mut self, kind: NodeKind) -> NodeId;        // detached
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), DomError>;
    pub fn insert_before(&mut self, sibling: NodeId, new: NodeId) -> Result<(), DomError>;
    pub fn detach(&mut self, id: NodeId) -> Result<(), DomError>;
    pub fn reparent_children(&mut self, from: NodeId, to: NodeId) -> Result<(), DomError>;
    pub fn quirks_mode(&self) -> QuirksMode; pub fn set_quirks_mode(&mut self, q: QuirksMode);
    pub fn base_url(&self) -> &str;
    pub fn element(&self, id: NodeId) -> Option<&Element>;    // convenience
    pub fn attr(&self, id: NodeId, local: &LocalName) -> Option<&str>;
}
pub enum DomError { NoSuchNode(NodeId), Cycle { ancestor: NodeId, descendant: NodeId }, NotDetached(NodeId), IsDocument }
```
`QualName`, `LocalName`, `Namespace`, `StrTendril` are re-exported from `markup5ever`/`tendril` (`pub use markup5ever::{QualName, LocalName, Namespace, ns, local_name, namespace_url}; pub use tendril::StrTendril;`).

- [ ] **Step 1: Cargo.toml** (deps: `markup5ever`, `tendril`, `thiserror`; dev: `proptest`; `[lints] workspace = true`; lib `cl_dom`). `lib.rs`: `#![forbid(unsafe_code)] #![deny(missing_docs)]`, modules `document, element, error, node, traverse, serialize` (last two are Task 4 — create as doc-only stubs with `//! Filled in Task 4.` **and do not reference them from lib.rs until Task 4**).

- [ ] **Step 2: Failing tests** in `document.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{NodeKind, local_name, ns, QualName};

    fn el(doc: &mut Document, name: &str) -> NodeId {
        let qn = QualName::new(None, ns!(html), LocalName::from(name));
        doc.create(NodeKind::Element(Element { name: qn, attrs: Vec::new(), template_contents: None }))
    }

    #[test]
    fn new_document_should_have_only_root() {
        let doc = Document::new("about:blank");
        assert_eq!(doc.len(), 1);
        assert!(matches!(doc.get(doc.root()).map(|n| &n.kind), Some(NodeKind::Document)));
    }

    #[test]
    fn append_child_should_link_parent_and_siblings() {
        let mut doc = Document::new("about:blank");
        let root = doc.root();
        let a = el(&mut doc, "a");
        let b = el(&mut doc, "b");
        doc.append_child(root, a).expect("append a");
        doc.append_child(root, b).expect("append b");
        let r = doc.get(root).expect("root");
        assert_eq!((r.first_child(), r.last_child()), (Some(a), Some(b)));
        assert_eq!(doc.get(a).expect("a").next_sibling(), Some(b));
        assert_eq!(doc.get(b).expect("b").prev_sibling(), Some(a));
        assert_eq!(doc.get(b).expect("b").parent(), Some(root));
    }

    #[test]
    fn insert_before_should_place_node_between_siblings() {
        let mut doc = Document::new("about:blank");
        let root = doc.root();
        let a = el(&mut doc, "a"); let c = el(&mut doc, "c"); let b = el(&mut doc, "b");
        doc.append_child(root, a).expect("a"); doc.append_child(root, c).expect("c");
        doc.insert_before(c, b).expect("insert");
        let order: Vec<NodeId> = std::iter::successors(doc.get(root).and_then(|r| r.first_child()), |&id| doc.get(id).and_then(|n| n.next_sibling())).collect();
        assert_eq!(order, vec![a, b, c]);
    }

    #[test]
    fn detach_should_unlink_and_keep_node_alive() {
        let mut doc = Document::new("about:blank");
        let root = doc.root();
        let a = el(&mut doc, "a"); let b = el(&mut doc, "b");
        doc.append_child(root, a).expect("a"); doc.append_child(root, b).expect("b");
        doc.detach(a).expect("detach");
        assert_eq!(doc.get(root).expect("root").first_child(), Some(b));
        assert_eq!(doc.get(b).expect("b").prev_sibling(), None);
        assert_eq!(doc.get(a).expect("a").parent(), None);
        assert_eq!(doc.len(), 3);
    }

    #[test]
    fn append_child_should_reject_cycle() {
        let mut doc = Document::new("about:blank");
        let root = doc.root();
        let a = el(&mut doc, "a"); let b = el(&mut doc, "b");
        doc.append_child(root, a).expect("a"); doc.append_child(a, b).expect("b");
        assert!(matches!(doc.append_child(b, a), Err(DomError::Cycle { .. })));
    }

    #[test]
    fn append_child_should_reject_attached_node() {
        let mut doc = Document::new("about:blank");
        let root = doc.root();
        let a = el(&mut doc, "a"); let b = el(&mut doc, "b");
        doc.append_child(root, a).expect("a"); doc.append_child(root, b).expect("b");
        assert!(matches!(doc.append_child(a, b), Err(DomError::NotDetached(_))));
    }

    #[test]
    fn reparent_children_should_move_all_children_in_order() {
        let mut doc = Document::new("about:blank");
        let root = doc.root();
        let from = el(&mut doc, "from"); let to = el(&mut doc, "to");
        let x = el(&mut doc, "x"); let y = el(&mut doc, "y");
        doc.append_child(root, from).expect("from"); doc.append_child(root, to).expect("to");
        doc.append_child(from, x).expect("x"); doc.append_child(from, y).expect("y");
        doc.reparent_children(from, to).expect("reparent");
        assert_eq!(doc.get(from).expect("from").first_child(), None);
        assert_eq!(doc.get(to).expect("to").first_child(), Some(x));
        assert_eq!(doc.get(y).expect("y").parent(), Some(to));
    }

    #[test]
    fn get_should_return_none_for_unknown_id() {
        let doc = Document::new("about:blank");
        assert!(doc.get(NodeId::from_index(99)).is_none());
    }
}
```
(`#[allow(clippy::expect_used)]` on the module.) Plus a proptest in `tests/` (Task 4 adds traversal; here add `proptest!` that random sequences of `append_child`/`detach` on ≤ 50 nodes keep the invariant: for every node, `parent.first_child … next_sibling` chain visits it exactly once and `prev_sibling` mirrors `next_sibling`). Write the invariant checker `fn check_invariants(doc: &Document) -> Result<(), String>` as `pub(crate)` in `document.rs` under `#[cfg(test)]`.

- [ ] **Step 3: Implement** with no recursion (iterative ancestor walk for `Cycle`), all lookups via `get`/`get_mut` returning `Option` (no indexing), `detach` fixing four links, `create` pushing to `nodes` and returning `NodeId(u32::try_from(len)...)` — return `DomError`? Use `NodeId::from_index(usize) -> NodeId` with `u32::try_from(...).unwrap_or(u32::MAX)` documented as "arena capped at 2^32-1 nodes". Node link getters: `parent()`, `first_child()`, `last_child()`, `prev_sibling()`, `next_sibling()`.

- [ ] **Step 4: Gate + commit** `dom: arena Document with NodeId links, append/insert/detach/reparent`.

---

### Task 4: `cl-dom` traversal and html5lib serializer

**Files:** `crates/dom/src/traverse.rs`, `crates/dom/src/serialize.rs`; modify `lib.rs`.

**Interfaces:**
```rust
impl Document {
    pub fn children(&self, id: NodeId) -> Children<'_>;          // Iterator<Item = NodeId>
    pub fn descendants(&self, id: NodeId) -> Descendants<'_>;    // pre-order, excluding `id`, iterative (explicit stack)
    pub fn ancestors(&self, id: NodeId) -> Ancestors<'_>;
}
pub mod serialize {
    pub fn html5lib_tree(doc: &Document) -> String;   // exact html5lib tree-construction "#document" format
    pub fn dom_dump(doc: &Document) -> String;        // same, used for insta goldens
}
```

- [ ] **Step 1: Failing tests** in `serialize.rs` (the html5lib format: root line `#document`, children indented by `| ` + 2 spaces per depth; elements `<tag>` (with `svg `/`math ` prefix for those namespaces), attrs sorted by name on their own lines `  name="value"`, text `"text"`, comments `<!-- text -->`, doctype `<!DOCTYPE name "public" "system">` (`<!DOCTYPE name>` when both ids empty), template contents under a `content` pseudo-line). Test: build `<!DOCTYPE html><html><head></head><body><p class="x" id="y">hi<!--c--></p></body></html>` by hand with `Document` API and assert exact string:
```
#document
| <!DOCTYPE html>
| <html>
|   <head>
|   <body>
|     <p>
|       class="x"
|       id="y"
|       "hi"
|       <!-- c -->
```
Also test `descendants` order on that tree and `ancestors(p) == [body, html, root]`.

- [ ] **Step 2: Implement** iteratively; `html5lib_tree` uses an explicit `(NodeId, depth)` stack; attributes sorted by `(ns, local)`; namespace prefixes: `ns!(svg)` → `svg `, `ns!(mathml)` → `math `.

- [ ] **Step 3: Gate + commit** `dom: iterative traversal and html5lib tree serializer`.

---

### Task 5: `cl-net` URL newtype and file loader

**Files:** `crates/net/Cargo.toml`, `src/lib.rs`, `src/error.rs`, `src/url.rs`, `src/file.rs`.

**Interfaces:**
```rust
pub struct Url(url::Url);   // Clone, Debug, Eq, Display
impl Url {
    pub fn parse(input: &str) -> Result<Url, NetError>;
    pub fn parse_with_base(input: &str, base: &Url) -> Result<Url, NetError>;   // WHATWG "URL parser with base"
    pub fn from_file_path(p: &Path) -> Result<Url, NetError>;
    pub fn scheme(&self) -> &str; pub fn as_str(&self) -> &str; pub fn to_file_path(&self) -> Option<PathBuf>;
    pub fn join(&self, relative: &str) -> Result<Url, NetError>;
}
pub const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
pub fn load_file(url: &Url) -> Result<Vec<u8>, NetError>;   // only scheme "file"; size-capped; no symlink following outside? (M1a: plain read)
pub enum NetError { Url(String), UnsupportedScheme(String), Io(std::io::Error), TooLarge { size: u64, max: u64 } }
```

- [ ] **Step 1: Failing tests**: `parse_with_base_should_resolve_relative_path` (`../a.css` against `file:///x/y/z.html` → `file:///x/a.css`), `join_should_keep_query_and_fragment_semantics` (`?q` and `#f` per WHATWG), `load_file_should_reject_http_scheme` (`UnsupportedScheme`), `load_file_should_reject_files_over_cap` (write a temp file, call with a test-only `load_file_with_cap(url, 16)` → `TooLarge`), `load_file_should_read_bytes` (temp file round trip). Implement with `url` crate; `load_file_with_cap` is `pub` but doc-marked test-support.

- [ ] **Step 2: Gate + commit** `net: Url newtype over the url crate and capped file loader`.

---

### Task 6: URL conformance harness + `url_parse` fuzz target

**Files:** `tools/conformance/url/{fetch.sh,PINNED_COMMIT,urltestdata.json,expectations.txt,README.md}`, `crates/net/tests/urltestdata.rs`, `tools/fuzz/fuzz_targets/url_parse.rs`, `tools/fuzz/Cargo.toml` (+bin, +dep `cl-net`).

- [ ] **Step 1: `fetch.sh`** downloads `https://raw.githubusercontent.com/web-platform-tests/wpt/<PINNED_COMMIT>/url/resources/urltestdata.json` into the directory (PINNED_COMMIT = the current wpt master sha at execution time, recorded in the file). Vendor the JSON (≈300 KB, BSD-3 WPT license → `LICENSE` file). `README.md`: what it is, how to refresh.

- [ ] **Step 2: Harness** `crates/net/tests/urltestdata.rs`: parse the JSON with `serde_json` (dev-dep); entries are strings (comments) or objects `{input, base: Option<String>, href?, failure?: bool, ...}`; for each object run `Url::parse_with_base(input, base)` (or `Url::parse` when base null); pass = (`failure` ⇒ `Err`) or (`href` == `result.as_str()`); load `expectations.txt` (lines `input\tbase` of known failures with `# reason` comments); assert `pass_rate >= 0.95` and assert every failing case is listed in expectations (and every listed expectation actually fails — no stale entries). Print `url conformance: {pass}/{total} ({pct:.1}%)`.

- [ ] **Step 3: Fuzz target** `url_parse.rs`: `fuzz_target!(|data: &[u8]| { if let Ok(s) = std::str::from_utf8(data) { let _ = cl_net::Url::parse(s); if let Ok(base) = cl_net::Url::parse("http://example.com/a/b") { let _ = cl_net::Url::parse_with_base(s, &base); } } });` Run 60 s locally.

- [ ] **Step 4: Gate + commit** `net: WPT urltestdata conformance harness and url_parse fuzz target`.

---

### Task 7: Encoding sniffer (`cl-html::encoding`, `prescan`)

**Files:** `crates/html/Cargo.toml`, `src/lib.rs`, `src/error.rs`, `src/encoding.rs`, `src/prescan.rs` (sink/lib parse fns are Task 8 — stub `sink.rs` doc-only, not referenced).

**Interfaces:**
```rust
pub enum EncodingSource { Bom, MetaPrescan, TransportLabel, Default }
pub fn sniff_encoding(bytes: &[u8], transport_label: Option<&str>) -> (&'static encoding_rs::Encoding, EncodingSource);
pub(crate) fn prescan_meta_charset(bytes: &[u8]) -> Option<&'static Encoding>;  // HTML §13.2.3.2 over min(len, 1024) bytes
pub fn decode(bytes: &[u8], transport_label: Option<&str>) -> (Cow<'_, str>, &'static Encoding, EncodingSource);
```
Order per spec: BOM → transport label (`Content-Type charset`) → prescan → default `windows-1252`? **Decision:** default **UTF-8** (documented `SPEC-DEVIATION(html#determining-the-character-encoding)`: spec default is locale-dependent, Chrome uses windows-1252 for unlabeled; we choose UTF-8 in M1a; row in SPEC_REGISTRY as `deviation`). UTF-16 labels from `<meta>` map to UTF-8 per spec; `x-user-defined` → windows-1252.

- [ ] **Step 1: Failing tests** (`encoding.rs`): `bom_should_win_over_meta` (`\xEF\xBB\xBF<meta charset=windows-1251>` → UTF_8, Bom), `utf16le_bom_should_be_detected`, `transport_label_should_beat_prescan`, `meta_charset_should_be_found_in_first_1024_bytes`, `meta_charset_beyond_1024_bytes_should_be_ignored` (pad with 1100 spaces), `meta_http_equiv_content_type_should_be_parsed` (`<meta http-equiv="Content-Type" content="text/html; charset=koi8-r">`), `meta_inside_comment_should_be_ignored` (`<!-- <meta charset=big5> -->`), `unknown_label_should_fall_back_to_default`, `utf16_meta_label_should_become_utf8`, `decode_should_replace_invalid_sequences_not_fail`. `prescan.rs`: state machine per spec algorithm (comment skip, `<meta` attribute loop with "get an attribute" sub-algorithm, `charset` attr, `content` attr with "extract a character encoding from a meta element"). Use `encoding_rs::Encoding::for_bom`, `for_label`.

- [ ] **Step 2: Implement + gate + commit** `html: encoding sniffing (BOM, transport label, meta prescan) with UTF-8 default`.

---

### Task 8: `cl-html` `TreeSink` → arena DOM (**mid**)

**Files:** `crates/html/src/sink.rs`, `src/lib.rs`; deps `cl-dom`, `cl-net`, `html5ever`, `markup5ever`, `tendril`, `encoding_rs`.

**Interfaces:**
```rust
pub struct ParseOutput { pub document: Document, pub encoding: &'static Encoding, pub encoding_source: EncodingSource, pub parse_errors: Vec<String> }
pub fn parse_document(bytes: &[u8], base_url: &Url, transport_label: Option<&str>) -> Result<ParseOutput, HtmlError>;
pub fn parse_document_str(html: &str, base_url: &Url) -> Result<ParseOutput, HtmlError>;
```
`DomSink { doc: RefCell<Document>, errors: RefCell<Vec<Cow<'static,str>>> }` implementing `html5ever::interface::TreeSink` with `Handle = NodeId`, `Output = ParseOutputInner`, `ElemName<'a>` = a small struct holding `&'a QualName` implementing `ElemName` (**verify** the exact associated-type/trait shape on docs.rs html5ever 0.39 `interface::TreeSink`/`ElemName`). Rules: `append` merges `AppendText` into a trailing `Text` sibling; `create_element` for `<template>` creates a `DocumentFragment` node and stores it in `Element::template_contents`; `get_template_contents` returns it; `append_based_on_parent_node` = if `element` has a parent → `append_before_sibling`, else `append(prev_element, child)`; `same_node` = id equality; `remove_from_parent` = `detach`; `reparent_children` = `Document::reparent_children`; `add_attrs_if_missing` only adds names not present; `set_quirks_mode` maps html5ever `QuirksMode` → `cl_dom::QuirksMode`; `parse_error` pushes; `finish` returns the `Document`. Driver: `html5ever::driver::parse_document(sink, ParseOpts { tree_builder: TreeBuilderOpts { drop_doctype: false, scripting_enabled: false, ..Default::default() }, ..Default::default() }).one(StrTendril::from(&*text))` (**verify** `TendrilSink::one` on 0.39).

- [ ] **Step 1: Failing tests** (`sink.rs` tests module): `parse_should_build_html_head_body_when_missing` (input `<p>hi` → html5lib tree with implied html/head/body), `text_nodes_should_merge_adjacent_appends` (`a<!--x-->b` vs `ab` — the second has one text node), `template_contents_should_be_stored_in_fragment` (`<template><b>x</b></template>` → `content` line in the dump), `attributes_should_keep_first_occurrence` (`<p a=1 a=2>` → one attr `a="1"`), `quirks_mode_should_be_set_without_doctype`, `parse_errors_should_be_collected_not_fatal` (`</p>` stray → ≥1 error, still `Ok`), `parse_document_should_decode_windows_1251_via_meta` (bytes with `<meta charset=windows-1251>` + Cyrillic → text node contains `Привет`), `malformed_bytes_should_never_panic` (proptest on random bytes ≤ 4 KiB: `parse_document` returns `Ok` or `Err`, never panics).

- [ ] **Step 2: Implement + gate + commit** `html: html5ever TreeSink over the arena DOM, parse_document API`.

---

### Task 9: html5lib tree-construction harness (**mid**)

**Files:** `tools/conformance/html5lib/{fetch.sh,PINNED_COMMIT,LICENSE,README.md,tree-construction/*.dat,expectations.txt}`, `crates/html/tests/tree_construction.rs`.

- [ ] **Step 1: Vendor** via `fetch.sh` from `https://github.com/html5lib/html5lib-tests/tree/<PINNED_COMMIT>/tree-construction/` the files: `tests1.dat … tests26.dat`, `doctype01.dat`, `entities01.dat`, `entities02.dat`, `comments01.dat`, `adoption01.dat`, `adoption02.dat`, `tables01.dat`, `template.dat`, `tricky01.dat`, `webkit01.dat`, `webkit02.dat` (skip `scripted/` — needs JS; skip `*.dat` that require `#script-on` sections: honour `#script-off` only). MIT license file.

- [ ] **Step 2: Harness**: parse `.dat` format (`#data` … `#errors` … optional `#new-errors`, optional `#document-fragment <context>` (skip fragment cases in M1a — count as skipped, not failed), optional `#script-on`/`#script-off` (run only `#script-off` or unspecified), `#document` block until blank line). For each case: `parse_document_str(data, base)` → `html5lib_tree` → compare to expected `#document` block exactly (trim trailing whitespace per line). Expectations file: lines `<file>:<case-index>` with `# reason`. Assert pass rate ≥ 0.90 over non-skipped cases, zero panics (wrap each case in `std::panic::catch_unwind` and count panics separately — a panic fails the whole test regardless of rate), no stale expectations. Print per-file summary.

- [ ] **Step 3: Triage** the failures into `expectations.txt` with honest reasons (e.g. "foreign content namespace prefix in serializer", "adoption agency edge case in html5ever"). Commit `html: html5lib-tests tree-construction harness with expectations`.

---

### Task 10: `html_parse` fuzz target

**Files:** `tools/fuzz/fuzz_targets/html_parse.rs`, `tools/fuzz/Cargo.toml`.
- [ ] `fuzz_target!(|data: &[u8]| { let base = cl_net::Url::parse("file:///fuzz.html").expect("static url"); let _ = cl_html::parse_document(data, &base, None).map(|o| cl_dom::serialize::html5lib_tree(&o.document)); });` (expect on a constant is acceptable in the fuzz harness — comment why). Run 60 s. Commit `fuzz: html_parse target (sniff + parse + serialize)`.

---

### Task 11: stylo DOM traits over the arena (**mid→high; the gate**)

**Files:** `crates/style/src/store.rs`, `src/handle.rs`, `src/stylo_dom.rs`, `src/stylo_selectors.rs`; modify `Cargo.toml` (enable `cl-dom`, `cl-net`), `lib.rs`.

**Interfaces (crate-private except where noted):**
```rust
// store.rs  #![allow(unsafe_code)]
pub(crate) struct StyleStore { slots: Box<[UnsafeCell<Option<ElementData>>]>, owner: std::thread::ThreadId }
impl StyleStore {
    pub(crate) fn new(len: usize) -> Self;
    /// SAFETY contract: caller is the single style-traversal thread (checked by debug_assert on `owner`)
    /// and no other reference to slot `id` is alive.
    pub(crate) unsafe fn slot(&self, id: NodeId) -> &mut Option<ElementData>;
    pub(crate) fn get(&self, id: NodeId) -> Option<&ElementData>;   // safe: only after traversal finished (store is &self, traversal done)
}
// handle.rs
#[derive(Clone, Copy)] pub(crate) struct NodeHandle<'a> { pub doc: &'a Document, pub store: &'a StyleStore, pub id: NodeId }
#[derive(Clone, Copy)] pub(crate) struct ElementHandle<'a>(pub NodeHandle<'a>);
#[derive(Clone, Copy)] pub(crate) struct DocumentHandle<'a>(pub NodeHandle<'a>);
// stylo_dom.rs: impl TDocument for DocumentHandle, NodeInfo + TNode for NodeHandle, TShadowRoot for a never-constructed `NoShadowRoot<'a>` (all methods unreachable-by-construction returning None/empty — NO todo!()), TElement for ElementHandle
// stylo_selectors.rs: impl selectors::Element for ElementHandle (Impl = style::selector_parser::SelectorImpl)
```
Attribute matching uses `cl_dom::Attr` values; classes split on ASCII whitespace; `id` attr; pseudo-classes supported in M1a: `:root`, `:first-child`, `:last-child`, `:only-child`, `:empty`, `:link`/`:any-link` (any `<a href>`), `:not()`/`:is()`/`:where()` via selectors; everything else returns `false`. `is_html_element_in_html_document` true for `ns!(html)`. `LayoutIterator` over children. Reference: Blitz `stylo.rs` (adapt method-by-method; do not copy raw pointers).

- [ ] **Step 1: Failing test** in `stylo_dom.rs` tests (needs Task 13's `resolve`; so this task's test drives the pieces directly): `element_handle_should_expose_tag_id_and_classes` (`<p id=a class="x y">` → `local_name() == p`, `has_id(a)`, `has_class(x)`, `!has_class(z)`), `node_handle_should_walk_tree_links` (parent/first/last/prev/next agree with `Document`), `store_slot_should_be_none_before_ensure_data` — and the **gate test**, placed in Task 13 but written now as `#[ignore]` to be un-ignored there: `p_with_color_rule_should_resolve_to_red`.

- [ ] **Step 2: Implement** all trait methods; each `unsafe` block documented; `debug_assert_eq!(std::thread::current().id(), self.store.owner)` in `ensure_data`/`clear_data`/`mutate_data`; if stylo requires `Send + Sync` on the handle types, add `unsafe impl Send for NodeHandle<'_> {}` etc. with `// SAFETY: traversal is sequential (traverse_dom(.., None)); the store's owner thread check enforces it at runtime in debug builds`.

- [ ] **Step 3: Gate + commit** `style: stylo DOM/selector traits over arena handles with per-pass ElementData store`. If the implementer reports BLOCKED after adapting to the real trait surface, the controller re-dispatches with a stronger model (round 2), then a fresh implementer (round 3); after that → **ADR-0015 Option B** (new tasks 11′/13′: `cl_style::fallback` with `cssparser` + `selectors` matching + cascade for the M1a property list, same public API, `dump` identical shape).

---

### Task 12: UA stylesheet and sheet collection

**Files:** `crates/style/assets/ua.css`, `src/sheets.rs`.

- [ ] **Step 1: Author `ua.css`** from HTML §15 Rendering (own text, not copied): `html,body{display:block}`, `head,script,style,title,meta,link,template,[hidden]{display:none}`, block elements list (`address,article,aside,blockquote,div,dl,dd,dt,fieldset,figure,figcaption,footer,form,h1-h6,header,hr,main,nav,ol,p,pre,section,ul,li{display:block}`), margins for `body{margin:8px}`, `p,blockquote,ul,ol,dl,pre,h1-h6{margin-block:1em}` (h1 2em/0.67em etc.), `pre{white-space:pre;font-family:monospace}`, `b,strong{font-weight:bolder}`, `i,em{font-style:italic}`, `a:link{color:#0000ee}`, `html{font-family:sans-serif;font-size:16px;color:#000;background:transparent}`. Keep ≤ 120 lines.

- [ ] **Step 2: `sheets.rs`**: `StyleEngine::add_ua_sheet(&mut self)` (Origin::UserAgent, `include_str!("../assets/ua.css")`), `StyleEngine::collect_document_sheets(&mut self, doc: &Document, load: &dyn Fn(&Url) -> Result<Vec<u8>, NetError>) -> Result<Vec<SheetWarning>, StyleError>`: walks `<style>` (text children) and `<link rel~=stylesheet href>` (resolve against `doc.base_url()`, load via callback — testshell passes `cl_net::load_file`; failures become warnings, not errors), in document order. Tests: `div_should_be_display_block_without_author_css` (needs resolve — mark `#[ignore]` until Task 13), `collect_should_parse_style_elements_in_order` (author sheet count = 2 for two `<style>`), `collect_should_warn_not_fail_on_missing_link`.

- [ ] **Step 3: Gate + commit** `style: UA stylesheet and document sheet collection`.

---

### Task 13: `StyleEngine::resolve`, traversal, computed-style dump (**mid**)

**Files:** `crates/style/src/traversal.rs`, `src/engine.rs` (extend), `src/dump.rs`, `src/lib.rs`.

**Interfaces (public, consumed by cl-layout):**
```rust
pub struct StyledDocument { doc: Document, store: StyleStore }
impl StyledDocument {
    pub fn document(&self) -> &Document;
    pub fn computed(&self, id: NodeId) -> Option<&ComputedValues>;   // primary style; None for non-elements
    pub fn into_document(self) -> Document;
}
impl StyleEngine { pub fn resolve(&mut self, doc: Document) -> Result<StyledDocument, StyleError>; }
pub mod dump { pub fn computed_style_dump(styled: &StyledDocument) -> String; }   // per element: `<tag>` + selected longhands (display, color, background-color, font-size, font-weight, margin-*, padding-*, border-*-width, width, height, position, white-space, text-align, line-height) as `name: value` lines, indented like html5lib_tree
```
Traversal: `struct RecalcStyle<'a> { context: SharedStyleContext<'a> }` implementing `style::traversal::DomTraversal<ElementHandle>`; `process_preorder` → `recalc_style_at`; `needs_postorder_traversal() == false`; drive with `style::driver::traverse_dom(&traversal, token, None)` where `token = RecalcStyle::pre_traverse(root_element, &context)` (**verify** names on docs.rs 0.20; Blitz `stylo.rs:61-190`). `SharedStyleContext` needs `stylist`, `options: StyleSystemOptions::default()`, `guards`, `visited_styles_enabled: false`, `animations: Default`, `current_time_for_animations`, `snapshot_map: &SnapshotMap`, `traversal_flags: TraversalFlags::empty()`, `registered_speculative_painters`. `ComputedValues` exposure: `computed()` returns `&ComputedValues` by dereferencing `ElementData::styles.primary()` (`servo_arc::Arc<ComputedValues>`).

- [ ] **Step 1: Un-ignore and complete the gate test** `p_with_color_rule_should_resolve_to_red`: parse `<p>hi</p>` (Task 8), engine with UA sheet + author `p { color: red }`, `resolve`, find the `<p>` NodeId, `computed(p).get_inherited_text().color` (**verify** accessor) == rgb(255,0,0). Plus `div_should_be_display_block_without_author_css` (from Task 12) and `span_should_inherit_color_from_parent`, `display_none_should_still_have_computed_values`. Golden: `insta::assert_snapshot!(computed_style_dump(&styled))` for 5 documents in `crates/style/tests/golden/*.html` (minimal doc, nested divs with margins, inline spans, `<pre>`, `<link>`-less doc with two `<style>` sheets and cascade order/specificity conflict).

- [ ] **Step 2: Implement + gate + commit** `style: resolve() via sequential stylo traversal; computed-style dump goldens`.

---

### Task 14: `css_stylesheet` fuzz target

- [ ] `tools/fuzz/fuzz_targets/css_stylesheet.rs`: `fuzz_target!(|data: &[u8]| { if let Ok(css) = std::str::from_utf8(data) { let Ok(mut e) = cl_style::StyleEngine::new((800.0, 600.0), 1.0) else { return }; let _ = e.add_author_sheet(css, "file:///fuzz.css"); } });`. Run 60 s. Commit `fuzz: css_stylesheet target`.

---

### Task 15: `cl-fonts` bundled font database (**mid**)

**Files:** `crates/fonts/Cargo.toml`, `src/lib.rs`, `src/bundled.rs`, `src/db.rs`, `assets/*`.

**Interfaces:**
```rust
pub struct FontKey(u32);                                        // Copy, Eq, Hash; index into FontDb
pub struct FontDb { collection: fontique::Collection, source_cache: fontique::SourceCache, faces: Vec<FontFace> }
pub struct FontFace { pub key: FontKey, pub family: &'static str, pub data: &'static [u8], pub index: u32 }
impl FontDb {
    pub fn bundled() -> Result<FontDb, FontError>;                          // Ahem + Noto Sans only, NO system fonts
    pub fn families(&self) -> impl Iterator<Item = &'static str>;
    pub fn face(&self, key: FontKey) -> Option<&FontFace>;
    pub fn key_for(&self, family: &str) -> Option<FontKey>;               // exact family name; "sans-serif" → Noto Sans, "monospace" → Noto Sans (M1a), "Ahem" → Ahem
    pub fn fontique(&mut self) -> (&mut fontique::Collection, &mut fontique::SourceCache);   // for parley FontContext
}
```
Fetch assets in a `fetch.sh` (Ahem from `https://raw.githubusercontent.com/web-platform-tests/wpt/master/fonts/Ahem.ttf`, Noto Sans Regular from Google Fonts GitHub `notofonts/notofonts.github.io` … pin the URL/sha256 in `assets/SHA256SUMS`), commit the binaries (~600 KB total) — acceptable, no LFS.

- [ ] **Step 1: Failing tests**: `bundled_db_should_contain_exactly_two_families`, `bundled_db_should_not_read_system_fonts` (assert `families().count() == 2` and no family named `Helvetica`/`Arial`/`DejaVu`), `ahem_should_map_from_generic_alias` (`key_for("Ahem")` some), `sans_serif_should_map_to_noto`, `ahem_glyph_should_be_square_em` (using `skrifa` metrics: advance of `x` == units_per_em).

- [ ] **Step 2: Implement** with `fontique::Collection::new(CollectionOptions { system_fonts: false, ..Default::default() })` (**verify** option name; fallback: build via `Collection::new` then `register_fonts(Blob)`); register `include_bytes!` blobs. Commit `fonts: bundled Ahem + Noto Sans font database without system fonts`.

---

### Task 16: `cl-layout` foundations — `Au`, geometry, style adapter, box tree

**Files:** `crates/layout/Cargo.toml`, `src/lib.rs`, `src/au.rs`, `src/geom.rs`, `src/style_adapt.rs`, `src/box_tree.rs`, `src/error.rs`.

**Interfaces:**
```rust
pub struct Au(pub i32);            // 1/60 css px; Copy/Ord; from_px(f32)->Au (round half away from zero), to_px(self)->f32, saturating add/sub/mul_by_f32, MAX/ZERO
pub struct Point { pub x: Au, pub y: Au } pub struct Size { pub w: Au, pub h: Au } pub struct Rect { pub origin: Point, pub size: Size }
pub struct Sides<T> { pub top: T, pub right: T, pub bottom: T, pub left: T }
pub enum Length { Auto, Px(Au), Percent(f32) }
pub enum Display { Block, Inline, None }
pub enum Position { Static, Relative }
pub enum WhiteSpace { Normal, Pre }  pub enum TextAlign { Left, Right, Center }  pub enum BoxSizing { ContentBox, BorderBox }  pub enum Overflow { Visible, Hidden }
pub struct Rgba8 { pub r: u8, pub g: u8, pub b: u8, pub a: u8 }
pub struct LayoutStyle { pub display: Display, pub position: Position, pub width: Length, pub height: Length, pub min_width: Length, pub max_width: Option<Length>, pub min_height: Length, pub max_height: Option<Length>,
    pub margin: Sides<Length>, pub padding: Sides<Length>, pub border_width: Sides<Au>, pub border_color: Sides<Rgba8>, pub border_solid: Sides<bool>, pub box_sizing: BoxSizing, pub overflow: Overflow,
    pub offset: Sides<Length> /* relative */, pub color: Rgba8, pub background: Rgba8, pub font_family: Vec<String>, pub font_size: Au, pub font_weight: u16, pub font_italic: bool, pub line_height: Au, pub text_align: TextAlign, pub white_space: WhiteSpace }
pub fn style_adapt::adapt(cv: &ComputedValues) -> LayoutStyle;   // the ONLY stylo-typed function outside cl-style
pub struct BoxId(u32);
pub enum BoxKind { Block, Inline, InlineText(NodeId), AnonymousBlock, AnonymousInline, LineBreak }
pub struct LayoutBox { pub node: Option<NodeId>, pub kind: BoxKind, pub style: LayoutStyle, children: Vec<BoxId> }
pub struct BoxTree { boxes: Vec<LayoutBox>, pub root: BoxId }
pub fn box_tree::build(doc: &StyledDocument) -> BoxTree;   // display:none subtree skipped; block container with mixed block/inline children wraps inline runs in AnonymousBlock; text nodes → InlineText; <br> → LineBreak
pub mod dump { pub fn box_tree_dump(t: &BoxTree) -> String; }
```

- [ ] **Step 1: Failing tests**: `au_from_px_should_round_half_away_from_zero` (0.5px → 30 Au; -0.5 → -30), `au_should_saturate_on_overflow` (proptest: `a + b` never panics), `adapt_should_map_display_and_lengths` (needs a styled doc: `<div style="width:50%;margin:0 auto;border:2px solid #f00">` → `width == Percent(50)`, `margin.left == Auto`, `border_width.left == Au(120)`, `border_color.left == red`), `box_tree_should_wrap_inline_runs_in_anonymous_blocks` (`<div>a<p>b</p>c</div>` → Block(div){ AnonymousBlock{InlineText}, Block(p){InlineText}, AnonymousBlock{InlineText} }), `box_tree_should_skip_display_none`, `box_tree_should_emit_line_break_for_br`; insta golden of `box_tree_dump` for the 5 golden documents.

- [ ] **Step 2: Implement** (`style_adapt.rs` is mid-tier: use `ComputedValues::get_box()/get_margin()/get_padding()/get_border()/get_position()/get_inherited_text()/get_font()/get_background()/get_text()` — **verify** accessor names and `LengthPercentageOrAuto`/`Size`/`Color` types on docs.rs; font-size and line-height resolve to `Au` here; percentages stay `Percent`).

- [ ] **Step 3: Gate + commit** `layout: Au geometry, ComputedValues adapter, box tree with anonymous blocks`.

---

### Task 17: Block formatting context and fragment tree

**Files:** `crates/layout/src/block.rs`, `src/fragment.rs`, `src/lib.rs`, `src/dump.rs`.

**Interfaces:**
```rust
pub struct FragmentId(u32);
pub enum FragmentKind { Block, AnonymousBlock, Line, Text { runs: Vec<GlyphRun> } }
pub struct Fragment { pub node: Option<NodeId>, pub kind: FragmentKind, pub border_box: Rect, pub padding_box: Rect, pub content_box: Rect, pub style: StyleId /* index into FragmentTree.styles */, children: Vec<FragmentId> }
pub struct FragmentTree { fragments: Vec<Fragment>, styles: Vec<LayoutStyle>, pub root: FragmentId, pub viewport: Size }
pub struct Viewport { pub size: Size }
pub fn layout(doc: &StyledDocument, viewport: Viewport, fonts: &mut FontDb) -> Result<FragmentTree, LayoutError>;
pub fn dump::fragment_tree_dump(t: &FragmentTree) -> String;   // `Block <div> border=(x,y,w,h) content=(...)` lines, Au printed as px with 2 decimals
```
Block layout (CSS2 §10): containing block width from parent content box; `width:auto` fills; `margin:auto` centres when width is definite; percentages resolve against containing block width (height % against definite height, else auto); min/max clamp after resolution; `box-sizing`; vertical margin collapsing between adjacent sibling blocks and parent/first-child (M1a: sibling + parent-first/last collapsing only, no negative margins special cases beyond max/min rule); `position: relative` offsets applied to the fragment after layout (does not affect siblings); `overflow: hidden` recorded in style for paint. Inline content in this task: a placeholder that lays out an `AnonymousBlock` with height = `line_height` per `InlineText` group (replaced by Task 18).

- [ ] **Step 1: Failing tests** (12): `auto_width_should_fill_containing_block`, `fixed_width_with_auto_margins_should_center`, `percent_width_should_resolve_against_parent_content_box`, `border_box_sizing_should_include_padding_and_border`, `min_width_should_clamp_percent_result`, `max_height_should_clamp_auto_height`, `sibling_vertical_margins_should_collapse_to_max`, `parent_and_first_child_top_margins_should_collapse`, `padding_should_prevent_margin_collapsing`, `relative_position_should_offset_fragment_only`, `display_none_child_should_take_no_space`, `nested_blocks_should_stack_vertically`; golden `fragment_tree_dump` for the 5 documents (line layout placeholder).

- [ ] **Step 2: Implement + gate + commit** `layout: block formatting context, margin collapsing, fragment tree`.

---

### Task 18: Inline formatting context with parley (**mid**)

**Files:** `crates/layout/src/inline.rs`, `src/text.rs`; modify `block.rs` to call inline layout for `AnonymousBlock`/inline children.

**Interfaces:**
```rust
pub struct Glyph { pub id: u16, pub x: Au, pub y: Au, pub advance: Au }
pub struct GlyphRun { pub font: FontKey, pub size: Au, pub origin: Point, pub glyphs: Vec<Glyph>, pub color: Rgba8 }
pub(crate) struct TextShaper { font_cx: parley::FontContext, layout_cx: parley::LayoutContext<Rgba8> }
impl TextShaper {
    pub(crate) fn new(fonts: &mut FontDb) -> Self;                        // parley FontContext from FontDb::fontique()
    pub(crate) fn layout_inline(&mut self, items: &[InlineItem], style: &LayoutStyle, available_width: Au) -> Vec<LineBox>;
}
pub(crate) enum InlineItem<'a> { Text { text: &'a str, style: &'a LayoutStyle, node: NodeId }, Break }
pub(crate) struct LineBox { pub height: Au, pub baseline: Au, pub runs: Vec<GlyphRun>, pub width: Au }
```
`white-space: normal` collapses whitespace runs to one space and trims line edges (HTML whitespace processing per CSS Text §4.1.1, implemented before shaping); `pre` preserves and only breaks on `\n`/`<br>`; `text-align` shifts run origins; `line-height` sets line box height; baseline from font ascent (parley `Line::metrics()`). `f32` → `Au` rounding at the run boundary. Fragment output: `Line` fragments with `Text` children.

- [ ] **Step 1: Failing tests**: `short_text_should_produce_one_line`, `long_text_should_wrap_at_available_width` (Ahem 16px in 100px container: `"xxxxx xxxxx xxxxx"` → 3 lines, each ≤ 100px, first line 5 glyphs), `br_should_force_line_break`, `white_space_pre_should_preserve_spaces_and_newlines`, `white_space_normal_should_collapse_runs_of_spaces`, `text_align_center_should_offset_runs`, `line_height_should_set_line_box_height` (line-height 40px → each Line fragment height 2400 Au), `ahem_glyph_advance_should_equal_font_size` (16px Ahem → advance Au(960)); update the 5 goldens.

- [ ] **Step 2: Implement** (verify parley 0.11.1 `StyleProperty::{FontSize, FontStack/FontFamily, FontWeight, LineHeight}`, `break_all_lines`, `align`, `Line::items`, `GlyphRun::glyphs`/`run().font()`/`font_size()` on docs.rs). Commit `layout: inline formatting context with parley shaping and line breaking`.

---

### Task 19: `cl-paint` display list builder and dump

**Files:** `crates/paint/Cargo.toml`, `src/lib.rs`, `src/error.rs`, `src/list.rs`, `src/build.rs`, `src/dump.rs`.

**Interfaces:**
```rust
pub enum DisplayItem { Rect { rect: Rect, color: Rgba8 }, Border { rect: Rect, widths: Sides<Au>, colors: Sides<Rgba8> }, Text { run: GlyphRun }, PushClip { rect: Rect }, PopClip }
pub struct DisplayList { pub items: Vec<DisplayItem>, pub bounds: Rect }
pub fn build(tree: &FragmentTree) -> DisplayList;      // per fragment: background Rect (if alpha>0) → Border (solid sides with width>0) → PushClip if overflow:hidden → children → PopClip; Text items for Text fragments; root canvas background = body/html background propagation per CSS2 §14.2 (html background else body background painted over whole viewport)
pub mod dump { pub fn display_list_dump(dl: &DisplayList) -> String; }
```

- [ ] **Step 1: Failing tests**: `build_should_emit_background_before_border_before_children`, `build_should_skip_transparent_backgrounds`, `overflow_hidden_should_wrap_children_in_clip`, `body_background_should_propagate_to_canvas`, `text_fragment_should_emit_text_item_with_color`; goldens of `display_list_dump` for the 5 docs.
- [ ] **Step 2: Implement + gate + commit** `paint: display list builder with CSS2 paint order and canvas background propagation`.

---

### Task 20: `cl-paint::validate` + `display_list_validate` fuzz target

**Files:** `crates/paint/src/validate.rs`, `tools/fuzz/fuzz_targets/display_list_validate.rs`; `DisplayItem`/`DisplayList` get `serde` + `arbitrary` derives (feature `arbitrary` on cl-paint, cl-layout geometry types).

**Interfaces:** `pub fn validate(dl: &DisplayList, bounds: Rect) -> Result<(), DisplayListError>`; `DisplayListError::{UnbalancedClip { depth_at_end: i32 }, PopWithoutPush { index: usize }, OutOfBounds { index: usize }, TooManyItems { count: usize, max: usize }, InvalidRect { index: usize }, TooManyGlyphs { index: usize } }`; limits: `MAX_ITEMS = 1_000_000`, `MAX_GLYPHS_PER_RUN = 65_536`, rects must have non-negative size and lie within `bounds` inflated by 4096 px (offscreen allowed), no `i32` overflow when computing `origin + size` (use checked ops).

- [ ] **Step 1: Tests**: `valid_list_should_pass`, `pop_without_push_should_fail`, `unbalanced_push_should_fail`, `negative_size_should_fail`, `overflowing_rect_should_fail_not_panic` (origin `Au(i32::MAX)`), `too_many_items_should_fail`.
- [ ] **Step 2: Fuzz target**: `fuzz_target!(|dl: cl_paint::DisplayList| { let _ = cl_paint::validate(&dl, cl_layout::Rect::from_px(0.0, 0.0, 800.0, 600.0)); });` (structured via `arbitrary`). Run 60 s. Commit `paint: display list validator with limits and fuzz target`.

---

### Task 21: `cl-gfx` CPU raster backend (**mid**)

**Files:** `crates/gfx/Cargo.toml`, `src/lib.rs`, `src/error.rs`, `src/cpu/mod.rs`, `src/cpu/shapes.rs`, `src/cpu/text.rs`, `src/cpu/glyph_cache.rs`.

**Interfaces:**
```rust
pub fn cpu::rasterize(dl: &DisplayList, width: u32, height: u32, fonts: &FontDb) -> Result<tiny_skia::Pixmap, GfxError>;   // calls cl_paint::validate first; white canvas; clip stack via tiny_skia::Mask (intersect rects); Rect → fill_rect; Border → four fill_rects (M1a: no mitred corners); Text → glyph alpha bitmaps composited with color
pub(crate) struct GlyphCache { map: HashMap<(FontKey, u16, u32 /*size Au*/), Arc<GlyphBitmap>> }  // swash Render, Source::Outline, Format::Alpha, hinting off
```
Determinism: same inputs → same bytes (no time, no threads, no system state).

- [ ] **Step 1: Tests**: `rect_should_fill_exact_pixels` (10×10 red at (5,5) → pixel (5,5) red, (4,4) white, (14,14) red, (15,15) white), `clip_should_restrict_children` (PushClip 0,0,10,10 then Rect 0,0,20,20 → pixel (15,15) white), `border_should_paint_four_sides`, `text_should_paint_ahem_square` (Ahem 16px `x` at origin → 16×16 black square, exact), `glyph_cache_should_hit_on_second_use`, `rasterize_should_reject_invalid_list` (PopClip alone → `GfxError::Invalid`).
- [ ] **Step 2: Implement** (verify swash `ScaleContext`, `Render::new(&[Source::Outline])`, `.format(Format::Alpha)`, `.render(&mut scaler, glyph_id)` → `Image { placement, data }` on docs.rs 0.2.10). Commit `gfx: CPU raster backend (tiny-skia + swash glyphs) with clip stack and glyph cache`.

---

### Task 22: Testshell pipeline, dump command, determinism and smoke tests

**Files:** `crates/testshell/src/pipeline.rs`, `src/dump.rs`, `src/lib.rs` (remove `render_blank`), `src/main.rs`, `tests/cli.rs` (update), `tests/determinism.rs`, `tests/smoke.rs`, `Cargo.toml` (deps on all engine crates).

**Interfaces:**
```rust
pub struct RenderOptions { pub viewport: (u32, u32) }   // default 800×600
pub struct RenderOutput { pub pixmap: Pixmap, pub stages: Stages }
pub struct Stages { pub dom: String, pub style: String, pub box_tree: String, pub fragments: String, pub display_list: String }   // dumps
pub fn render_file(path: &Path, opts: &RenderOptions) -> Result<RenderOutput, ShellError>;   // M1a-ONLY: single process
pub fn render_bytes(bytes: &[u8], base: &Url, opts: &RenderOptions) -> Result<RenderOutput, ShellError>;
```
CLI: `render <input> --png <out> [--viewport WxH]` (real render), `dump <input> --stage dom|style|box-tree|fragments|display-list`, `compare` unchanged.

- [ ] **Step 1: Tests**: `tests/determinism.rs`: render `tests/ref/text-basic.html` twice → identical `pixmap.data()`; render in a second `std::thread` → identical; `tests/smoke.rs`: generate 1 MB HTML (`<div><p>lorem…</p></div>` × N) into a temp file, render must finish in < 5 s (measure with `Instant`; assert; on CI allow 15 s via env `CL_SMOKE_SLOW=1`) and not panic; truncated file (first 100 bytes of a valid doc) → `Ok` (parser recovers); non-UTF-8 garbage 64 KiB → `Ok` or `Err`, no panic; `tests/cli.rs`: update `render_then_compare_should_round_trip_through_cli` to use a real page and assert `compare` = 0; add `dump_should_print_stage`.
- [ ] **Step 2: Implement + gate + commit** `testshell: real single-process pipeline, dump stages, determinism and smoke tests`.

---

### Task 23: Reftests, close-out, tag `m1a`

**Files:** `tests/ref/*.html` (20 pairs) + `tests/ref/fonts.css` (`@font-face{font-family:Ahem;src:local(Ahem)}` — bundled fonts are resolved by family name; `src: local()` is parsed by stylo and ignored by cl-fonts; document this), `crates/testshell/src/reftest.rs` + CLI `reftest [--dir tests/ref] [--filter name]`, `tests/reftests.rs` (runs the suite as a cargo test), `.github/workflows/ci.yml` (reftest step in `test` matrix), docs.

Reftest pairs (each `<name>.html` and `<name>-ref.html`, all with `font-family: Ahem`, Ahem-only so pixels are geometric): `text-basic`, `block-width-auto`, `block-margin-auto-center`, `block-percent-width`, `box-sizing-border-box`, `margin-collapse-siblings`, `margin-collapse-parent-child`, `padding-blocks-collapse`, `border-solid-four-sides`, `background-body-propagation`, `display-none`, `display-inline-run`, `anonymous-block-wrapping`, `line-wrap-ahem`, `br-line-break`, `white-space-pre`, `text-align-center`, `line-height`, `overflow-hidden-clip`, `position-relative-offset`. Each ref uses only `div` + explicit px sizes/positions (no feature under test).

- [ ] **Step 1: `reftest.rs`**: discover pairs, render both at 800×600, `compare_png`-style pixel diff in memory, report `PASS/FAIL name (n differing)`; on FAIL write both PNGs and a diff PNG to `target/reftest-failures/<name>/` for humans. `tests/reftests.rs` asserts all 20 pass.
- [ ] **Step 2: Docs**: `docs/SPEC_REGISTRY.md` rows → `partial` with test links (url, encoding, tokenizer/tree construction, cascade, CSS2 block/inline, paint order); `docs/FEATURE_MATRIX.md` (M1a items done); `docs/DEPENDENCIES.md` (any adaptation); `docs/PLAN.md` M1a checkbox; `MEMORY.md` state + session log + debts; `docs/history/build-times.md`.
- [ ] **Step 3: Full verification**: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets --all-features --locked -- -D warnings && cargo nextest run --workspace --locked && cargo deny check && bash scripts/check-agents-md.sh && bash tools/check-platform-cfg.sh && bash tools/check-unsafe-scope.sh && bash tools/check-stylo-scope.sh && cargo tree -d -e normal | grep -c markup5ever` (must print 0 duplicates); CI green on 3 OS; fuzz targets 60 s each.
- [ ] **Step 4: Commit** `docs: close M1a static pipeline` and tag `m1a` after merge to main.

---

## Verification (end-to-end)

1. `cargo run -p cl-testshell -- render tests/ref/text-basic.html --png /tmp/a.png` twice → `cmp /tmp/a.png /tmp/b.png` silent.
2. `cargo run -p cl-testshell -- dump tests/ref/text-basic.html --stage fragments` shows Line/Text fragments with Ahem glyph advances = font size.
3. `cargo run -p cl-testshell -- reftest` → 20/20 PASS on macOS; CI shows the same on Windows and Linux.
4. `cargo test -p cl-html --test tree_construction` prints ≥ 90% and `cargo test -p cl-net --test urltestdata` prints ≥ 95%.
5. `cd tools/fuzz && for t in html_parse css_stylesheet url_parse display_list_validate; do cargo +nightly fuzz run --fuzz-dir . $t -- -max_total_time=60; done` → no crashes.
6. Smoke: 1 MB document < 5 s, peak RSS (via `/usr/bin/time -l`) < 300 MB, numbers recorded in `docs/history/`.
7. Gates: `tools/check-unsafe-scope.sh`, `tools/check-stylo-scope.sh` green; `cargo tree -d` shows one `markup5ever`.

## Risks and stop conditions (from the design review)

| Risk | Signal | Action |
|---|---|---|
| stylo trait integration (Task 11/13) | 3 attempts or 5 days without the gate test green | ADR-0015 Option B fallback (own minimal cascade behind the same API); stylo → M2 |
| stylo build time | cold build > 10 min / CI job > 45 min | `Swatinem/rust-cache` keyed on Cargo.lock; `check` job switches to `cargo check` |
| `servo_arc`/`style::` leakage | scope script fails | keep `style_adapt.rs` the only adapter |
| stylo `Send + Sync` bounds on handles | trait bound errors | `unsafe impl Send/Sync` + thread `debug_assert`, sequential driver |
| parley / fontique / swash API churn | compile errors vs the brief | implementer runs `cargo doc -p <crate>` first and adapts; design unchanged |
| Cross-OS glyph differences | reftest pair diverges on one OS | same code + same fonts ⇒ real bug; goldens are text, PNGs never committed |
| Windows CI python3 | stylo build fails on windows-2022 | setup-python shim; if unfixable, Windows → nightly matrix for M1a (MEMORY.md) |

## Self-review

- **Spec coverage:** PLAN.md M1a scope → Tasks 3–22; conformance → 6, 9; reftests/fuzz/goldens → 6, 10, 14, 20, 23 and per-crate goldens; docs/ADR/CI → 1, 23; stylo gate + fallback → 11/13 + ADR-0015 §6.
- **Placeholders:** mid-tier tasks carry exact interfaces, real tests and "verify on docs.rs" pointers instead of invented stylo/parley call code; no TBD/TODO.
- **Type consistency:** `NodeId`, `Document`, `StyledDocument::computed`, `LayoutStyle`, `FragmentTree`, `GlyphRun`, `DisplayList`, `FontDb/FontKey`, `Url` names match across Tasks 3–22; `Rgba8`, `Rect`, `Sides`, `Au` defined once in `cl-layout` and re-used by `cl-paint`/`cl-gfx`.
- **Ordering:** 1 → 2 (build spike, pushed for 3-OS check) → 3,4 → 5,6 → 7,8,9,10 → 11 (gate) → 12,13,14 → 15 → 16,17,18 → 19,20 → 21 → 22 → 23. Tasks 3–10 may run before Task 2's CI result returns, but never two implementers at once.
