# Разработка

## 1. Требования к хосту

| Компонент | Версия | Примечание |
|---|---|---|
| Rust | по `rust-toolchain.toml` (stable 1.95) | **через rustup**, не Homebrew: нужны cross-target и `rustfmt`/`clippy` компоненты одной версии |
| cmake, ninja | любые свежие | сборка `v8` crate (prebuilt binaries скачиваются по умолчанию; из исходников — часы), `mozjs` не используем |
| Python 3 | ≥3.10 | **обязателен для сборки** (stylo codegen), WPT/Test262 tooling |
| macOS | Xcode 26 + CLT | seatbelt profiles, code signing |
| Windows | VS 2026 Build Tools (MSVC), Windows 11 SDK | `x86_64-pc-windows-msvc` |
| Linux | clang, pkg-config, libxkbcommon, wayland/x11 dev, libssl не нужен (rustls) | Ubuntu 24.04 как CI baseline |
| Диск | ~20 ГБ для target/, +5 ГБ WPT checkout | |

Хост владельца (2026-09-07): macOS 26.5.2 arm64, 16 ГБ, rustup 1.95 с тремя target-ами, cargo-deny/nextest/fuzz/insta, cmake/ninja установлены.

### Установка на macOS

```bash
brew uninstall rust            # иначе конфликт с rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup component add rustfmt clippy rust-src
cargo install cargo-deny cargo-nextest cargo-fuzz cargo-insta
brew install cmake ninja python@3.12
```

Неинтерактивные shell-ы (агенты, скрипты) не видят `~/.cargo/bin`: `export PATH="$HOME/.cargo/bin:$PATH"` в начале каждой команды, либо `source ~/.cargo/env`.

## 2. Структура репозитория

См. `docs/ARCHITECTURE.md` §8. Workspace-корень: `Cargo.toml` с `[workspace.dependencies]` (единые версии) и `[workspace.lints]`.

## 3. Команды

```bash
cargo build --workspace
cargo nextest run --workspace            # быстрее cargo test; fallback: cargo test --workspace
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo fmt --all -- --check
cargo deny check
cargo doc --workspace --no-deps
cargo run -p chrome-light -- [url]
cargo run -p cl-testshell -- --headless tests/ref/basic.html --png /tmp/out.png
./tools/wpt/run.sh url                   # директория WPT
./tools/test262/run.sh
cargo +nightly fuzz run html_tokenizer   # fuzzing — единственное место, где нужен nightly
./tools/bench/mem.sh                     # бюджеты памяти по corpus
```

Child-процессы запускаются как тот же бинарник: `chrome-light --type=renderer|network|gpu|utility --ipc-handle=…`. Для отладки одного процесса: `--single-process` (только dev, блокируется в release-сборке).

## 4. Кросс-платформа

- Все три платформы — обязательные CI-таргеты (ADR-0009): `aarch64-apple-darwin`, `x86_64-pc-windows-msvc`, `x86_64-unknown-linux-gnu`. Дополнительно nightly-job: `x86_64-apple-darwin`, `aarch64-unknown-linux-gnu`.
- Локально с macOS: Linux — через Docker (`tools/ci/linux.Dockerfile`), Windows — через GitHub Actions/локальную VM; cross-компиляция MSVC с macOS не поддерживается — не тратить время.
- Platform-specific код — только `cl-platform`, `cl-process`, backends `cl-gfx`. `#[cfg(target_os = …)]` в других crate-ах — причина для reject на review.
- Пути, кодировки файлов, line endings: `PathBuf`/`OsStr`, никогда `String` для путей; `.gitattributes` фиксирует LF для текстовых фикстур.

## 5. Рабочий цикл изменения

1. Прочитать ADR субсистемы. Если нужного нет — сначала ADR (`docs/adr/template.md`).
2. Тест первым: unit/golden/reftest или WPT-expectation (`tools/wpt/expectations/`).
3. Реализация. Для парсера/декодера — fuzz target в том же изменении.
4. `cargo fmt`, `clippy -D warnings`, `nextest`, затронутые WPT-директории.
5. Обновить `docs/SPEC_REGISTRY.md` (строка фичи), `docs/FEATURE_MATRIX.md` если статус изменился, `MEMORY.md` (журнал сессии).
6. Коммит: `scope: imperative summary (spec §ref)`; тело — что и почему, цифры для perf/memory.

## 6. CI (GitHub Actions, `.github/workflows/`)

| Job | Триггер | Что |
|---|---|---|
| `check` | PR | fmt, clippy, deny, doc |
| `test-{macos,windows,linux}` | PR | build + nextest |
| `wpt-smoke` | PR | затронутые директории + smoke shard, Linux |
| `wpt-full` | nightly | полный sharded прогон, 3 ОС; дашборд pass/fail/crash/timeout |
| `fuzz-short` | PR | 60 с на изменённые targets |
| `fuzz-long` | nightly | 30 мин на все targets, corpus в артефактах |
| `bench-mem` | PR с меткой `perf` + nightly | corpus, RSS по процессам, гейт −5% |
| `release` | tag | подписанные сборки, SBOM (`cargo cyclonedx`), нотаризация macOS |

## 7. Отладка

- `RUST_LOG=cl_net=debug,cl_ipc=trace` — `tracing-subscriber` env filter.
- `--trace-out=trace.json` — Perfetto-совместимый trace всех процессов.
- `--single-process --no-sandbox` — только для отладчика; в release эти флаги отсутствуют физически (cfg).
- Renderer crash → `~/Library/Application Support/chrome-light/crashes/*.dmp` (macOS); аналогично per-OS. Дампы не содержат содержимого страниц.
- DevTools: `--remote-debugging-port=9222` → CDP; подключаться Chrome DevTools frontend.

## 8. Secrets и приватность в dev

- Тестовые профили — только синтетические. Никогда не коммитить реальный профиль Chrome/наш.
- Импорт из Chrome тестируется на фикстурах `tests/fixtures/chrome-profile/` (сгенерированных скриптом), не на профиле владельца.
- Sync-сервер локально: `cargo run -p cl-sync-server -- --dev` (SQLite, самоподписанный TLS).
