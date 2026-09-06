# Исследование: состояние браузерных движков и Rust-экосистемы

**Срез: 2026-09-07.** Базовые референсы — skill `browser-engine-research` (срез 2026-09-05); volatile-факты перепроверены в сети 2026-09-07. Факты старше 3 месяцев перепроверять перед использованием.

## 1. Вывод для проекта

«Полный аналог Chrome, на Rust, в разы легче» — три цели, которые нельзя достичь одновременно в v1. Владелец выбрал класс **независимый движок с нуля** (соло + AI-агенты), поэтому:

- совместимость — асимптотическая цель, измеряемая WPT/Test262/corpus, а не обещание паритета;
- «легче» — измеримый бюджет (ADR-0012), достигаемый архитектурой (freeze/hibernate, один isolate, отсутствие лишних процессов), а не «Rust сам по себе»;
- «всё как в Chrome» — реализуется через Chrome-интероп (импорт профиля, MV3, CDP), а не через Chromium-код.

Реалистичный масштаб по референсу: узкий secure engine для контролируемого контента — 8–20 инженеров × 2–4 года; движок с полезной долей открытого веба — десятки инженеров, 5+ лет. Соло + агенты меняет производительность, но не объём спецификаций. Отсюда жёсткие milestone-гейты и feature matrix.

## 2. Движки (сентябрь 2026)

| Проект | Статус | Значение для нас |
|---|---|---|
| **Servo** | 0.1.0 LTS на crates.io (2026-04-13), 0.5.0 (2026-08-21). Embedding API: proxies, root certs, cookies, local/sessionStorage, dialogs, console, DevTools. Multithreaded canvas (+55% fps), Linux aarch64, Android 10+. Донаты ~7.8k USD/мес. Multiprocess есть, sandbox незрелый. | Не embed-им. Источник архитектурных решений и crate-ов (stylo, servo_arc, ipc-channel, webrender-идеи). Verso (браузер на Servo) заархивирован — не поспевал за API. |
| **Ladybird** | C++→Rust: LibJS frontend (фев 2026), HTML parser (май), style+layout (июль). PR закрыты (июнь 2026). Alpha Linux/macOS — 2026, beta — 2027. Swift-направление удалено. | Не embeddable. Reference для «vertical stack с нуля», подтверждение что AI-assisted порт C++→Rust реален. |
| **Chromium/Blink/V8** | Chrome 153 — 2026-09-08; далее **релиз каждые 2 недели**. MV2 отключён с Chrome 138 (июль 2025), Web Store очищен 2026-08-31. Rust в Chromium production с M119 (PNG/JSON/fonts парсеры). | Compat-target и источник V8. 2-недельный ритм = мы обновляем V8 crate так же часто. |
| **CEF** | crate `cef` 151.8.1 (2026-09-03); bitbucket-релизы отстают от crate. | Отвергнут (ADR-0001): не «легче», не «свой». |
| **Blitz (DioxusLabs)** | `blitz-dom` 0.2.4: stylo + taffy + parley + vello, experimental. | Доказательство сборки движка из crate-ов; заимствуем структуру интеграции stylo↔taffy (`stylo_taffy`). |
| **wry/Tauri** | системные WebView: WebView2 / WKWebView / WebKitGTK. | Отвергнут: три разных движка, нет паритета. |

## 3. Chrome-интероп: факты

- **Chrome Sync через Google-аккаунт** закрыт для сторонних сборок с 2021-03-15 (аудит Google). Обходы (флаги/патчи) — нарушение ToS. **Non-goal навсегда.**
- **Протокол** `components/sync/protocol/sync.proto` открыт; Brave `go-sync` — open-source сервер, «поддерживает любой Chromium-браузер». Vivaldi — свой закрытый. Есть `chromium-sync-server` (Python, experimental). → ADR-0007: свой сервер на Rust по этому протоколу.
- **Локальный профиль Chrome:** `Bookmarks` (JSON), `History`/`Login Data`/`Web Data`/`Cookies` (SQLite, lock при запущенном Chrome — копировать), `Preferences` (JSON), `Extensions/<id>/<ver>/manifest.json`. Пароли/cookies шифруются: macOS — Keychain «Chrome Safe Storage» (AES-128-CBC, PBKDF2), Linux — libsecret/kwallet/«peanuts», Windows — DPAPI (`v10`) и **App-Bound Encryption `v20`** с Chrome 127 (июль 2024) для cookies, план расширения на пароли/платежи. ABE привязан к SYSTEM-сервису Chrome → сторонний процесс легально не расшифрует. Импорт на Windows — только через пользовательский экспорт (CSV паролей) либо ограниченный набор (закладки, история, настройки).
- **MV3**: declarative permissions, background service worker, content scripts в isolated world, `declarativeNetRequest`, запрет remote code. CRX3 формат, Web Store update URL публичен (Chromium использует его же).
- **CDP** — tip-of-tree нестабилен; **WebDriver BiDi** — стандарт. Делаем CDP-подмножество для DevTools frontend + BiDi для automation.

## 4. Память Chrome (открытые источники, 2026)

- Memory Saver с Chrome 140 (сент 2025): ML-предсказание возврата к вкладке, три режима; «до 80% меньше» на discarded tab.
- 10 активных вкладок в Chrome 140+ — ~1.4 ГБ (1.8 ГБ в Chrome 135). Тяжёлые web-apps (Figma/Notion/Slack) — 0.5–1.5 ГБ на вкладку.
- Energy Saver замораживает JS, но не освобождает память; Memory Saver discard-ит renderer.

→ Наши цели в ADR-0012 ставятся относительно этих чисел и подтверждаются `tools/bench` ежемесячно на той же машине.

## 5. Rust-стек: версии на 2026-09-07

| Crate | Версия | Роль | Лицензия (проверить в DEPENDENCIES) |
|---|---|---|---|
| stylo | 0.20.0 | CSS cascade/computed style (Firefox/Servo) | MPL-2.0 |
| cssparser | 0.37.0 | CSS tokenizer | MPL-2.0 |
| html5ever | 0.39.0 | HTML tokenizer/tree builder | MIT/Apache |
| taffy | 0.14.0 | flex/grid/block layout math | MIT |
| parley | 0.11.1 | text layout, line breaking, bidi | MIT/Apache |
| swash | 0.2.10 | shaping, glyph rasterization | MIT/Apache |
| fontdb | 0.24.0 | font discovery | MIT |
| vello | 0.10.0 | GPU 2D renderer | MIT/Apache |
| wgpu | 30.0.1 | GPU abstraction (Metal/DX12/Vulkan) | MIT/Apache |
| winit | 0.30.13 | windows/input | Apache-2.0 |
| accesskit | 0.25.0 | accessibility tree → AX/UIA/AT-SPI | MIT/Apache |
| v8 (rusty_v8) | 152.2.0 | V8 bindings; версии = Chrome | MIT |
| deno_core | 0.411.0 | reference для интеграции V8 (не используем напрямую) | MIT |
| mozjs | 0.26.0 | SpiderMonkey (альтернатива, не выбрана) | MPL-2.0 |
| hyper | 1.11.1 | HTTP/1.1, HTTP/2 | MIT |
| rustls | 0.23.43 | TLS 1.2/1.3 | MIT/Apache/ISC |
| quinn / h3 | 0.11.11 / 0.0.8 | QUIC / HTTP/3 | MIT/Apache; h3 pre-1.0 — риск |
| url | 2.5.8 | WHATWG URL | MIT/Apache |
| image | 0.25.10 | декодеры (в utility process) | MIT/Apache |
| rusqlite | 0.40.2 | SQLite | MIT |
| ipc-channel | 0.23.0 | IPC транспорт (Servo) | MIT/Apache |
| prost | 0.14.4 | protobuf для sync.proto | Apache-2.0 |
| tokio | 1.53.1 | async runtime (network process, browser process) | MIT |
| tracing | 0.1.44 | observability | MIT |
| insta / proptest / arbitrary | 1.48 / 1.11 / 1.4.2 | тесты | MIT/Apache |
| tiny-skia | 0.12.0 | CPU raster fallback | BSD-3 |

Хост: rustc 1.95.0 (2026-04-14). Edition 2024.

## 6. Rust GUI для shell

egui — быстрее всего до окна, immediate mode, wgpu backend; iced — Elm-стиль; Slint — DSL + коммерческая лицензия; Xilem — не production. Для соло-разработчика с wgpu-стеком выбран egui (ADR-0008) с планом миграции на privileged web UI на своём движке.

## 7. Источники

- Servo: https://servo.org/blog/ ; https://servo.org/about/ ; https://github.com/servo/servo/wiki/Roadmap ; https://byteiota.com/servo-0-1-0-ships-on-crates-io-embeddable-rust-browser/ ; https://www.phoronix.com/news/Servo-January-2026 ; https://www.osnews.com/story/140462/verso-a-browser-using-servo/
- Ladybird: https://ladybird.org/ ; https://linuxiac.com/ladybird-browser-closes-public-pull-requests-ahead-of-first-alpha/ ; https://alternativeto.net/news/2026/2/ladybird-web-browser-begins-rust-adoption-starting-with-javascript-engine-with-ai-help
- Chrome Sync: https://www.xda-developers.com/google-cracks-down-third-party-chromium-browser-chrome-sync/ ; https://groups.google.com/a/chromium.org/g/chromium-packagers/c/SG6jnsP4pWM ; https://github.com/brave/go-sync ; https://github.com/jackyzy823/chromium-sync-server
- MV2/MV3: https://www.ghacks.net/2026/09/01/manifest-v2-is-dead-as-chrome-web-store-permanently-purges-legacy-extensions/ ; https://developer.chrome.com/docs/extensions/develop/migrate/what-is-mv3
- Release cadence: https://developer.chrome.com/blog/chrome-two-week-release ; https://chromereleases.googleblog.com/2026/09/
- App-Bound Encryption: https://thehackernews.com/2024/08/google-chrome-adds-app-bound-encryption.html ; https://blog.elcomsoft.com/2026/01/browser-forensics-in-2026-app-bound-encryption-and-live-triage/
- Memory: https://whysogeek.com/chrome-memory-saver-energy-saver-performance-2026/ ; https://www.superchargebrowser.com/library/chrome-native-memory-saver-review/
- CEF: https://lib.rs/crates/cef ; https://github.com/chromiumembedded/cef
- Chromium Rust: https://chromium.googlesource.com/chromium/src/+/refs/heads/main/docs/rust.md ; https://www.chromium.org/Home/chromium-security/memory-safety/
- Blitz: https://github.com/DioxusLabs/blitz
- rusty_v8: https://deno.com/blog/rusty-v8-stabilized ; https://github.com/denoland/rusty_v8
- wry: https://docs.rs/wry/latest/wry/
- Rust GUI: https://wrenlearnsrust.com/posts/2026-03-11-rust-gui-landscape-2026.html ; https://blog.logrocket.com/state-rust-gui-libraries/
- Supporters of Chromium-Based Browsers: https://blog.chromium.org/2025/01/announcing-supporters-of-chromium-based.html
- Версии crate-ов: crates.io API, 2026-09-07.
- Нормативные спецификации и архитектурные референсы: см. `docs/SPEC_REGISTRY.md` и skill `browser-engine-research/references/*` (33 + 45 + 23 + 37 источников).
