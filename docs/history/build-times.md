# Build times

Build time of `cl-style` (stylo 0.20 plus its whole dependency tree: cssparser, selectors, servo_arc,
euclid, string_cache/markup5ever, the icu4x family via `url`/`idna`, ...). Measured in
task-2 (stylo build spike, ADR-0015) as part of risk reduction — if the cold build turned out
too long, it would be a risk for the CI cache (`actions/cache` on `~/.cargo` + `target/`).

Method: `cargo clean` (a full wipe of `target/`, not just `-p cl-style`, so the measurement reflects
a real cold CI runner without a cache) → `time cargo build -p cl-style`. Warm — a repeated
`cargo build -p cl-style` with no source changes right after the cold one.

| Date | Machine | `cargo build -p cl-style` cold | warm (no changes) | warm (edit of an own file) | `target/` after cold |
|---|---|---|---|---|---|
| 2026-09-08 | macOS 26.5.2, Apple Silicon, 8 cores, 16 GB | **18m 37s** (`184.11s user 36.03s system 19% cpu 18:38.90 total`) | 13.16s (`0.23s user 0.40s system` — cargo re-hashes a graph of ~140 crates) | 0.96s (`0.08s user 0.20s system` — only `cl-style` relinks) | 765 MB (debug, only `cl-style`+deps, no tests) |

**Risk confirmed**: the cold build (`cargo clean` → `cargo build -p cl-style`) took **18m 37s**,
above the 10-minute threshold of the task-2 brief. Average CPU load over that time was only 19%
(184 s user+system CPU against 1118 s wall), i.e. most of the time does NOT parallelise across
cores — the bottleneck is probably the single-threaded `rustc` frontend checking the huge
mako-generated `stylo::properties` (thousands of lines from `properties.mako.rs`), not the
Python codegen itself (that finishes in seconds under `cargo build --timings`; not measured
separately in this run). For CI (task 2 of the brief: "controller pushes the feature branch after
this task specifically to see the 3-OS build result") this means: `~/.cargo/registry` and `target/`
must be cached between runs (`actions/cache`), otherwise every job would lose ~19 minutes on a bare
build of `cl-style` alone. Windows/Linux CI runners may be slower/faster — real numbers come
after the first push (see task-2-report.md).

The full `time` output and the full list of compiled crates are in the task-2 report
(`.superpowers/sdd/2026-09-07-m1a-static-pipeline/task-2-report.md`).

## CI (GitHub Actions, run 34242985517, commit cbc0847, 2026-09-08)

The full workspace with stylo, cold cache (`Swatinem/rust-cache` missed — `Cargo.lock` changed), `python3` from `actions/setup-python@v5`:

| Job | clippy (whole workspace) | build (whole workspace) | whole job |
|---|---|---|---|
| ubuntu-24.04 | 36 s | 39 s | 2 m 47 s |
| macos-14 | — | — | 3 m 34 s |
| windows-2022 | 71 s | 71 s | 4 m 56 s |

**Conclusion:** the local 18 m 37 s are a measurement artifact: the measurement ran on a machine where several agents were running their own `cargo` in parallel. The real cold build of stylo is about a minute on a CI runner. The "stylo build time" risk from the M1a plan is closed; no separate caching strategy is needed.
