# ChromeLight

Независимый веб-браузер и браузерный движок на Rust для macOS, Windows и Linux. Цель — поведение уровня Chrome при кратно меньшем потреблении памяти, с безопасной многопроцессной архитектурой с первого дня.

> ChromeLight is an independent project and is not affiliated with, endorsed by, or sponsored by Google LLC. "Chrome" is a trademark of Google LLC. См. ADR-0013.

## Статус

**Пре-альфа, стадия проектирования.** Кода движка нет. Заложены правила, архитектура, threat model, ADR, план тестирования и план milestone-ов (`docs/PLAN.md`). Исполнение начинается с M0 (`docs/superpowers/plans/2026-09-07-m0-foundation.md`).

Честная оценка масштаба (из `docs/RESEARCH-2026-09.md`): независимый движок с полезной долей открытого веба — многолетняя программа. Проект ведётся одним разработчиком с AI-агентами, поэтому scope на каждом milestone жёстко ограничен, а совместимость измеряется, а не обещается.

## Что строим

| Слой | Решение |
|---|---|
| Движок (DOM, layout, paint, compositor, net, IPC, процессы) | своё, Rust |
| CSS | `stylo` (движок Firefox/Servo) + `cssparser` |
| HTML-парсер | `html5ever` → собственный DOM |
| Текст | `parley` + `swash` |
| Графика | `vello` + `wgpu`, CPU fallback |
| JS/Wasm | V8 через crate `v8`, за трейтом `JsRuntime` |
| Сеть | `rustls`, `hyper` (h1/h2), `quinn`/`h3`, свой HTTP-кэш и cookie store |
| Хранилище | SQLite (`rusqlite`) |
| Доступность | `accesskit` |
| Chrome-интероп | импорт локального профиля Chrome; свой sync-сервер по протоколу Chromium `sync.proto`; MV3-расширения; CDP-совместимый DevTools |

Что **не** делаем: Google Chrome Sync (API закрыт Google с 2021), MV2, мобильные платформы (v1), DRM, WebRTC (v1), собственный JIT (v1).

## Документация

| Файл | Что внутри |
|---|---|
| [CLAUDE.md](CLAUDE.md) / [AGENTS.md](AGENTS.md) | Правила для разработчика и AI-агентов. Читать первым. |
| [MEMORY.md](MEMORY.md) | Память проекта: состояние, решения, открытые вопросы, журнал сессий. |
| [docs/PLAN.md](docs/PLAN.md) | Программа milestone-ов M0–M6 с exit-критериями и гейтами. |
| [docs/superpowers/plans/](docs/superpowers/plans/) | Детальные исполняемые планы по milestone-ам (задачи, тесты, коммиты). |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Процессная модель, pipeline URL→пиксели, карта crate-ов, бюджеты памяти. |
| [docs/adr/](docs/adr/README.md) | Architecture Decision Records 0001–0014. |
| [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) | Окружение, сборка, кросс-платформа, команды. |
| [docs/CODING_STANDARDS.md](docs/CODING_STANDARDS.md) | Правила Rust: ownership, ошибки, `unsafe`, clippy, документация. |
| [docs/SECURITY.md](docs/SECURITY.md) | Threat model, границы доверия, матрица защит, политика раскрытия. |
| [docs/TESTING.md](docs/TESTING.md) | Пирамида тестов: unit, golden, reftest, WPT, Test262, fuzz, perf. |
| [docs/FEATURE_MATRIX.md](docs/FEATURE_MATRIX.md) | Что поддерживаем, в какой фазе, что non-goal. |
| [docs/SPEC_REGISTRY.md](docs/SPEC_REGISTRY.md) | Реестр спецификаций: раздел → crate → тесты → статус. |
| [docs/DEPENDENCIES.md](docs/DEPENDENCIES.md) | Зависимости, лицензии, зачем. |
| [docs/RESEARCH-2026-09.md](docs/RESEARCH-2026-09.md) | Снимок исследования рынка/технологий на 2026-09-07 с источниками. |
| [docs/GLOSSARY.md](docs/GLOSSARY.md) | Термины. |

## Быстрый старт (после появления кода)

```bash
cargo build --workspace
cargo test --workspace
cargo run -p chrome-light
```

Требования и установка тулчейна — `docs/DEVELOPMENT.md`.

## Лицензия

Apache-2.0 OR MIT на выбор получателя (`LICENSE-APACHE`, `LICENSE-MIT`, ADR-0014). Зависимости проверяются `cargo deny`.
