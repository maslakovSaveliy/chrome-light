# MEMORY.md — память проекта chrome-light

Обновляется в конце каждой рабочей сессии. Короткие факты, ссылки на документы. История сессий старше ~месяца переезжает в `docs/history/`.

## Состояние на 2026-09-07

**Фаза:** M0 Foundation реализована на ветке `m0-foundation`, PR #1 (draft). CI зелёный на macOS-14 / windows-2022 / ubuntu-24.04 (check, test, fuzz-short). Память idle: 4864 КБ суммарно (browser 2496 КБ, renderer 2368 КБ). Release-sandbox gate прошла (unsandboxed renderer отказывает, exit 78). Следующее действие: финальное ревью ветки → merge в main → тег m0 → план M1.

**Продукт:** **ChromeLight** (ADR-0013, риск торговой марки принят владельцем). Лицензия **Apache-2.0 OR MIT** (ADR-0014).

**Репозиторий:** `git init -b main` 2026-09-07, ремоута нет. `.planning/HANDOFF.json` — пустой чекпоинт GSD (в .gitignore).

**Окружение владельца:** macOS 26.5.2, Apple Silicon (arm64), 16 ГБ RAM, 8 ядер, Xcode 26.6, **rustup установлен** (1.95.0, targets aarch64-apple-darwin/x86_64-pc-windows-msvc/x86_64-unknown-linux-gnu; cargo-deny/nextest/fuzz/insta есть). Внимание: неинтерактивные shell-ы не видят `~/.cargo/bin` — использовать `export PATH="$HOME/.cargo/bin:$PATH"`. cmake/ninja есть, docker есть, gh есть.

## Принятые решения (кратко; полностью — docs/adr/)

| ADR | Решение | Статус |
|---|---|---|
| 0001 | Класс проекта: независимый движок + полный браузер, соло + AI-агенты | Accepted |
| 0002 | Rust stable, MSRV pinned, `unsafe` только в FFI-crates | Accepted |
| 0003 | Своё: DOM, layout, paint, compositor, IPC, процессы, net-service, storage, shell. Переиспользуем: html5ever, cssparser+stylo, taffy (flex/grid math), parley/swash, vello/wgpu, rustls/hyper/quinn, url, image, accesskit, rusqlite, V8 | Accepted |
| 0004 | JS VM: V8 через crate `v8` за трейтом `JsRuntime`; свой VM — не раньше v2 | Accepted (revisit M4) |
| 0005 | Multi-process с первого milestone: browser / renderer-per-site / net / gpu / utility; собственный typed IPC; sandbox: seatbelt / seccomp+namespaces / AppContainer | Accepted |
| 0006 | Графика: vello + wgpu, CPU fallback; свой compositor с property trees | Accepted |
| 0007 | Sync: (a) односторонний импорт/зеркало локального профиля Chrome; (b) свой sync-сервер по Chromium `sync.proto`. Google Chrome Sync — non-goal навсегда | Accepted |
| 0008 | Shell UI: egui на wgpu на старте; позже privileged web UI на своём движке | Accepted (revisit M5) |
| 0009 | Все три платформы с первого дня: CI-матрица macOS/Win/Linux, platform-код только в cl-platform | Accepted |
| 0010 | Тесты: WPT + Test262 с первого milestone, headless testshell, reftests, cargo-fuzz | Accepted |
| 0011 | Расширения: только MV3, свой runtime chrome.*; CRX из Web Store; фаза поздняя | Accepted |
| 0012 | Память — требование первого класса; бюджеты и измерение в CI | Accepted |
| 0013 | Имя продукта ChromeLight; риск торговой марки Google принят владельцем; disclaimer «not affiliated with Google» | Accepted |
| 0014 | Лицензия своего кода: Apache-2.0 OR MIT; проверка deps через cargo-deny | Accepted |

## Открытые вопросы владельцу

1. GitHub-ремоут: создать? (нужен для CI-матрицы Windows/Linux.)
2. Chrome 153 baseline в `docs/history/bench-2026-09.md` — вручную измерить на машине владельца.

## Известные долги после M0

Деferred до M1+; не блокируют публичную бету:

1. `BootstrapServer::accept_with_timeout` leaks один заблокированный accept-thread на каждый timeout — пересмотреть архитектуру handshake.
2. Release sandbox-refusal path занимает ~15 с (browser ждёт bootstrap timeout вместо poll на `try_wait` exit child) — оптимизация M1.
3. `rustfmt.toml` использует nightly-only опции (`imports_granularity`, `group_imports`) — stable rustfmt warned, но CI пасс; переход на stable-compatible опции в M1.
4. `cl-testshell` имеет неиспользуемую зависимость `thiserror` — cleanup.
5. `browser_side` (IPC validate) маскирует `Violation` если send reject-ack провалится — улучшить error reporting.
6. `BrowserEndpoint` и `ChildEndpoint` структурно идентичны — рассмотреть generic helper, но сейчас явность предпочтительна.
7. Fuzz job в CI не имеет cache — добавить в M1.
8. Малый дублированный cleanup в `spawn` и в test temp-dir boilerplate — minor refactor M1.

## Ключевые внешние факты (снимок 2026-09-07, детали — docs/RESEARCH-2026-09.md)

- Servo 0.5.0 (21 авг 2026) на crates.io; 0.1.0 LTS с 13 апр 2026. Embedding API растёт, но совместимость частичная. Verso заархивирован.
- Ladybird: C++→Rust, PR закрыты, alpha (Linux/macOS) в 2026, beta 2027.
- Chrome Sync API закрыт для сторонних сборок с 15 марта 2021. Brave `go-sync` — открытый сервер по `sync.proto`.
- MV2 мёртв: отключён в Chrome 138 (июль 2025), Web Store очищен 31 авг 2026.
- Chrome 153 — 8 сент 2026; далее релизы каждые 2 недели.
- Chrome 127+ на Windows: App-Bound Encryption для cookies (планы на пароли) — сторонний процесс не расшифрует профиль Chrome без участия пользователя.
- Версии crate-ов на 2026-09-07: stylo 0.20, cssparser 0.37, html5ever 0.39, taffy 0.14, parley 0.11, vello 0.10, wgpu 30.0, winit 0.30.13, accesskit 0.25, v8 152.2, hyper 1.11, rustls 0.23.43, quinn 0.11.11, rusqlite 0.40, ipc-channel 0.23, prost 0.14, tokio 1.53.

## Журнал сессий

### 2026-09-07 — сессия 2: исполнение M0 (subagent-driven)
- Исполнены Tasks 1–11 из `docs/superpowers/plans/2026-09-07-m0-foundation.md`: T1 workspace/lints/deny (1 fix), T2 cl-platform (no fixes), T3–T5 cl-ipc codec/validate/bootstrap/handshake (no fixes), T6–T7 cl-process sandbox/spawn (no fixes), T8 browser/child ping-pong + tracing (1 fix: `--browser-fail-after-handshake`), T9 testshell PNG (no fixes), T10 bench script (no fixes; Chrome baseline pending owner action), T11 CI check-agents + Gates (3 fixes: rustfmt nightly options noted, cargo-fuzz flags fixed, Exit codes verified).
- CI результат: check ✅, test (macOS-14/windows-2022/ubuntu-24.04) ✅, fuzz-short ✅ на всех трёх ОС.
- Измерения: idle RSS 4864 КБ (browser 2496, renderer 2368). Release sandbox gate PASS (unsandboxed exit 78). Fuzz run 60 s: 75.2M runs, 0 crashes.
- Task 12: обновлены PLAN.md exit-criteria (5/6 тикнуты, Chrome baseline остаётся owner-action), MEMORY.md state/debts/log, ARCHITECTURE.md crate status.
- Следующее действие: финальное ревью ветки → merge в main → тег `m0` → план M1 (`docs/superpowers/plans/<date>-m1-static-pages.md`).

### 2026-09-07 — сессия 1: исследование + фундамент документации
- Прочитан skill `browser-engine-research` (6 референсов, срез 2026-09-05).
- Веб-проверка: Servo, Ladybird, CEF/cef-rs, Chrome Sync, MV2/MV3, Chromium release cadence, Rust-стек, App-Bound Encryption, Rust GUI, wry.
- Владелец выбрал: свой движок с нуля на Rust; соло + AI-агенты; все три платформы сразу; sync = локальный профиль Chrome + свой сервер.
- Созданы: CLAUDE.md, AGENTS.md, MEMORY.md, README.md, docs/ (ARCHITECTURE, DEVELOPMENT, CODING_STANDARDS, SECURITY, TESTING, RESEARCH-2026-09, FEATURE_MATRIX, SPEC_REGISTRY, DEPENDENCIES, GLOSSARY), docs/adr/0001–0014, конфиги тулчейна.
- Владелец: лицензия — максимально свободная (→ Apache-2.0 OR MIT), имя — ChromeLight, `git init` — да, rustup установлен.
- Создано: `docs/PLAN.md` (программа M0–M6, exit-критерии, ритуалы, риски), `docs/superpowers/plans/2026-09-07-m0-foundation.md` (12 задач с кодом и тестами), LICENSE-APACHE/MIT; ADR-0013/0014 → Accepted; git-репозиторий инициализирован, первый коммит — вся документация.
- **Следующее действие:** исполнить M0 по плану (subagent-driven или inline). Владельцу: создать GitHub-ремоут для CI-матрицы; вручную измерить Chrome 153 для `docs/history/bench-2026-09.md` (Task 10).
