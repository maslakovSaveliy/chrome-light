# Architecture Decision Records

Format — `template.md`. Once an ADR is Accepted it is not edited in substance; a change = a new ADR with `Supersedes`.

| # | Title | Status |
|---|---|---|
| [0001](0001-project-class.md) | Project class: independent engine + full browser | Accepted |
| [0002](0002-language-and-unsafe-policy.md) | Rust stable, edition 2024, `unsafe` policy | Accepted |
| [0003](0003-own-vs-reused-components.md) | What we write ourselves, what we take from the ecosystem | Accepted |
| [0004](0004-js-vm-v8.md) | JS/Wasm VM: V8 behind the `JsRuntime` trait | Accepted (revisit M4) |
| [0005](0005-process-model-ipc-sandbox.md) | Multi-process, typed IPC, sandbox from the first milestone | Accepted |
| [0006](0006-graphics-stack.md) | Graphics: vello + wgpu, own compositor | Accepted |
| [0007](0007-chrome-interop-and-sync.md) | Chrome interop: profile import, own sync over `sync.proto` | Accepted |
| [0008](0008-shell-ui.md) | Shell UI: egui now, privileged web UI later | Accepted (revisit M5) |
| [0009](0009-cross-platform-from-day-one.md) | Three platforms from day one | Accepted |
| [0010](0010-testing-and-conformance.md) | WPT/Test262/fuzz from the first milestone | Accepted |
| [0011](0011-extensions-mv3.md) | Extensions: MV3 only, own runtime | Accepted |
| [0012](0012-memory-budget.md) | Memory as a first-class requirement | Accepted |
| [0013](0013-naming-and-trademark.md) | ChromeLight name and trademark (risk accepted) | Accepted |
| [0014](0014-licensing.md) | Apache-2.0 OR MIT license | Accepted |
