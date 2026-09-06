# Architecture Decision Records

Формат — `template.md`. ADR после статуса Accepted не редактируется по существу; изменение = новый ADR со `Supersedes`.

| # | Заголовок | Статус |
|---|---|---|
| [0001](0001-project-class.md) | Класс проекта: независимый движок + полный браузер | Accepted |
| [0002](0002-language-and-unsafe-policy.md) | Rust stable, edition 2024, политика `unsafe` | Accepted |
| [0003](0003-own-vs-reused-components.md) | Что пишем сами, что берём из экосистемы | Accepted |
| [0004](0004-js-vm-v8.md) | JS/Wasm VM: V8 за трейтом `JsRuntime` | Accepted (revisit M4) |
| [0005](0005-process-model-ipc-sandbox.md) | Multi-process, typed IPC, sandbox с первого milestone | Accepted |
| [0006](0006-graphics-stack.md) | Графика: vello + wgpu, свой compositor | Accepted |
| [0007](0007-chrome-interop-and-sync.md) | Chrome-интероп: импорт профиля, свой sync по `sync.proto` | Accepted |
| [0008](0008-shell-ui.md) | Shell UI: egui сейчас, privileged web UI позже | Accepted (revisit M5) |
| [0009](0009-cross-platform-from-day-one.md) | Три платформы с первого дня | Accepted |
| [0010](0010-testing-and-conformance.md) | WPT/Test262/fuzz с первого milestone | Accepted |
| [0011](0011-extensions-mv3.md) | Расширения: только MV3, свой runtime | Accepted |
| [0012](0012-memory-budget.md) | Память как требование первого класса | Accepted |
| [0013](0013-naming-and-trademark.md) | Имя ChromeLight и торговая марка (риск принят) | Accepted |
| [0014](0014-licensing.md) | Лицензия Apache-2.0 OR MIT | Accepted |
