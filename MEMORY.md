# MEMORY.md — память проекта chrome-light

Обновляется в конце каждой рабочей сессии. Короткие факты, ссылки на документы. История сессий старше ~месяца переезжает в `docs/history/`.

## Состояние на 2026-09-16

**Фаза:** M1a Static Pipeline — **все 23 задачи плана сделаны** на ветке `m1a-static-pipeline` (PR #2, draft, https://github.com/maslakovSaveliy/chrome-light/pull/2). План: `docs/superpowers/plans/2026-09-07-m1a-static-pipeline.md`. Журнал исполнения (SDD ledger, только на машине владельца, в `.gitignore`): `.superpowers/sdd/2026-09-07-m1a-static-pipeline/progress.md` — главный источник состояния. Следующее действие: **ждёт финального ревью ветки и решения владельца о мерже PR #2 и теге `m1a`** (см. Task 23 report) → план M1b.

**M1a, задачи:** 1–23 — все сделаны; 1–15 отревьюированы в предыдущих сессиях; 16 (`cl-layout`, ранее запаркована в `exclude`) доделана и включена обратно в workspace; 17–23 (block/inline layout, fonts-adapter, paint, gfx, testshell, reftests, close-out) исполнены subagent-driven в сессии 2026-09-16, ревью ещё не проводилось — часть финального ревью ветки.

**В workspace:** `cl-platform`, `cl-ipc`, `cl-process`, `cl-testshell`, `cl-net`, `cl-dom`, `cl-html`, `cl-style`, `cl-fonts`, `cl-layout`, `cl-paint`, `cl-gfx`, `chromelight`. Fuzz-таргеты (5): `ipc_decode` (M0), `url_parse`, `html_parse`, `css_stylesheet`, `display_list_validate` (M1a).

**Конформанс (гейты — измерения, не цели):** WPT `urltestdata.json` 828/893 (92.7%), гейт 0.92 = потолок crate `url` 2.5.8 (Servo фиксирует те же провалы). html5lib tree-construction 1307/1313 (99.5%), 0 паник, гейт 0.90; 6 провалов — пробелы html5ever 0.39 (см. `docs/SPEC_REGISTRY.md`). Reftests: **21/21 PASS** (`cargo run -p cl-testshell -- reftest`, `crates/testshell/tests/reftests.rs`; 20 из брифа плюс `margin-collapse-siblings-whitespace`, регрессия на баг ниже). Golden dumps — 5 стадий (dom/style/box-tree/fragments/display-list). Смоук: 1 МБ страница 1.51 с (release) / ~4.7 с (debug), peak RSS ~330 МиБ — **над** критерием «< 300 МБ» (`docs/history/perf-m1a-2026-09.md`).

**Продукт:** **ChromeLight** (ADR-0013, риск торговой марки принят владельцем). Лицензия **Apache-2.0 OR MIT** (ADR-0014).

**Репозиторий:** GitHub https://github.com/maslakovSaveliy/chrome-light (public), default branch main; PR #1 (M0) смержен, ветка удалена; тег `m0` = c0b1244. `.planning/HANDOFF.json` — пустой чекпоинт GSD (в .gitignore).

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

1. Chrome 153 baseline в `docs/history/bench-2026-09.md` — вручную измерить на машине владельца.

## Известные долги после M0

Деferred до M1+; не блокируют публичную бету:

1. `BootstrapServer::accept_with_timeout` заводит один blocked accept-thread на вызов; утечка ограничена одним `spawn()` (не сроком жизни browser process) — пересмотреть handshake в M1.
2. Release sandbox-refusal path занимает ~15 с (browser ждёт bootstrap timeout вместо poll `try_wait`); worst case `spawn` = 2×timeout (accept + handshake) — оптимизация M1.
3. `browser_side` маскирует `Violation`, если send reject-ack провалится — log-and-ignore.
4. `BrowserEndpoint`/`ChildEndpoint` структурно идентичны — generic helper при росте API.
5. Fuzz job в CI без rust-cache.
6. Test temp-dir boilerplate дублируется (testshell, chromelight tests) — helper.
7. Browser-side `recv_timeout` — M0-ONLY poll через `park_timeout(1 ms)`; `park_timeout` не в disallowed-methods; блокирующий `BrowserEndpoint::recv` остаётся pub (используется тестами). M1: `IpcReceiverSet`/мультиплексированное ожидание, запрет unbounded recv на стороне browser на уровне типов.
8. `browser.rs`: `renderer.wait()` после `Shutdown` — unbounded ожидание процесса; M1: `try_wait` poll с дедлайном + kill (тот же класс, что ADR-0005 §4).
9. CI: fmt/clippy — в 3-OS матрице; `deny`/`doc`/скрипты — только ubuntu (платформонезависимы).

## Долги M1a (открытые на 2026-09-16, после Task 23)

1. **Квадратичный парсинг/стилизация на глубокой вложенности** — потенциальный DoS-вектор на враждебной странице с искусственно большой глубиной дерева; не пофикшено в M1a, приоритет для M2 перед тем, как открывать произвольный веб.
2. **Peak RSS ~330 МиБ на 1 МБ smoke-странице** — над критерием плана «< 300 МБ» (`docs/history/perf-m1a-2026-09.md`); оптимизация памяти `cl-style`/`cl-layout` не входила в мандат M1a.
3. **Тесты в release-профиле ни разу не прогонялись** — пересборка stylo с LTO (`lto = "thin"`, `codegen-units = 1`) слишком долгая на машине владельца; всё проверено в debug. `ElementDataWrapper::borrow_mut` в release не проверяет алиасинг — debug-сборка и `cargo careful` несут реальную нагрузку (ADR-0015, дополнение 2026-09-09).
4. **Нет синтеза bold/italic** — только реальные начертания шрифта (Noto Regular); `font-weight`/`font-style` на несуществующем начертании не имеют видимого эффекта.
5. **Фон/рамки инлайновых элементов не рисуются** — `<span style="background/border">` красится только по своему тексту (Task 18 решение), сама рамка/фон — никогда; нужны inline-фрагменты со своей box-моделью (позже M).
6. **Свой `line-height`/`vertical-align` для вложенных инлайнов не реализован** — высота строки = `line-height` контейнера, а не максимум по вложенным инлайнам.
7. **`line-height: normal` = 1.2 × font-size** — приближение (`normal_line_height`), не honoring font-specific metrics.
8. **`FontNotBundled` не протестирован** — путь ошибки в `cl-fonts` для нераспознанного семейства не покрыт тестом.
9. ~~Self-collapsing пустые блоки не реализованы~~ — **исправлено в Task 23** (CSS 2.1 §9.2.2.1, не §8.3.1 — обнаружено при написании reftest-пар: пробельный текстовый узел между двумя block-level соседями (обычная разметка с переносами строк) порождал реальный (нулевой высоты) anonymous block, ломавший margin-коллапс на два хопа и суммировавший margin-ы вместо max). Контроллер квалифицировал это как Critical (каждая настоящая страница имеет переносы строк между `<div>`) и потребовал фикс до ревью, а не документирование как M1a-débit. Фикс: `crates/layout/src/box_tree.rs`'s `wrap_inline_runs` больше не порождает бокс для прогона, целиком состоящего из схлопывающегося-под-`white-space:normal` текста. Тесты: `box_tree_should_not_generate_boxes_for_collapsible_whitespace_between_blocks`, `box_tree_should_keep_whitespace_under_white_space_pre`, `whitespace_between_siblings_should_not_defeat_margin_collapsing`, реф-пара `margin-collapse-siblings-whitespace`. 14 golden-снапшотов (`box_tree_goldens`/`fragment_tree_goldens`/`display_list_goldens`) переприняты — каждое изменение проверено вручную и объясняется этим же правилом (structural: `AnonymousBlock{InlineText}` пропадает; geometric: margin-коллапс через `<html>`/`<body>`/`<p>` цепочки теперь корректно берёт max, а не сумму — см. Task 23 fix report).
10. **17+ мелких находок ревью (deferred)** — живут в SDD ledger как строки `minor (deferred)`; триаж — на финальном ревью ветки, не в этом списке поимённо.

## Открытия M1a, которые не надо переоткрывать

1. **stylo, хранилище.** `Box<[UnsafeCell<Option<ElementData>>]>` из ADR-0015 невозможен: `ElementDataMut`/`ElementDataRef` создаются только через `ElementDataWrapper`. В продакшн-коде `cl-style` ноль `unsafe`-блоков; `allow(unsafe_code)` только в `stylo_dom.rs` (реализация `unsafe fn` трейта).
2. **stylo, потоки.** `!Sync` ничего не гарантирует: `SendNode`/`SendElement` — безусловный `unsafe impl Send`. Проверка потока-владельца — жёсткий `assert_eq!` в девяти точках входа. Traversal только последовательный (`traverse_dom(.., None)`).
3. **stylo, ширина хэндла.** `TElement` обязан быть ровно в одно слово (`sharing/mod.rs`: `FakeCandidate { _element: usize }` + assert размеров). Поэтому `NodeHandle` — односложная ссылка в per-pass `NodeArena`; `slot_count == doc.len()` проверяется в `NodeArena::new`. `resolve()` обязан использовать `engine.lock.clone()` — чужой guard роняет `Locked::read_with`.
4. **Ловушка границ (layout).** stylo вычисляет `border-*-width` в `3px` (`medium`) даже при `border-style: none`. Адаптер layout обязан обнулять ширину при `none`/`hidden`.
5. **Шрифты.** `fontique`/`parley` с фичей `system` линкуют fontconfig/DirectWrite/CoreText — закреплены `default-features = false, features = ["std"]`; движок рендерит только встроенными шрифтами (Ahem + Noto Sans).
6. **`file:`-URL с удалённым хостом** отклоняется на всех ОС (на Windows это UNC-путь = SMB-запрос). `localhost` и пустой хост разрешены.
7. **html5lib-tests:** в `master` больше нет `tree-construction/`; корпус закреплён на `f994590f528ac8b6073665791ddb1ed85c66dfb2`.
8. **`tendril`** не прямая зависимость — брать через `markup5ever::tendril`.
9. **Сборка stylo на CI** ~40–70 с холодной; локальные 18 мин были артефактом параллельных агентов (`docs/history/build-times.md`).
10. **Платформенные cfg:** `tools/check-platform-cfg.sh` запрещает `#[cfg(target_os|windows|unix)]` вне `crates/{platform,process,gfx}` — тесты писать платформонезависимо (`std::env::temp_dir()`).
11. **Процесс:** нижний порог модели исполнителя — sonnet (haiku в M0 стоил лишних раундов ревью). `git add` только явных путей, никогда `-A`; гейт цепочкой через `&&` перед коммитом.

## Ключевые внешние факты (снимок 2026-09-07, детали — docs/RESEARCH-2026-09.md)

- Servo 0.5.0 (21 авг 2026) на crates.io; 0.1.0 LTS с 13 апр 2026. Embedding API растёт, но совместимость частичная. Verso заархивирован.
- Ladybird: C++→Rust, PR закрыты, alpha (Linux/macOS) в 2026, beta 2027.
- Chrome Sync API закрыт для сторонних сборок с 15 марта 2021. Brave `go-sync` — открытый сервер по `sync.proto`.
- MV2 мёртв: отключён в Chrome 138 (июль 2025), Web Store очищен 31 авг 2026.
- Chrome 153 — 8 сент 2026; далее релизы каждые 2 недели.
- Chrome 127+ на Windows: App-Bound Encryption для cookies (планы на пароли) — сторонний процесс не расшифрует профиль Chrome без участия пользователя.
- Версии crate-ов на 2026-09-07: stylo 0.20, cssparser 0.37, html5ever 0.39, taffy 0.14, parley 0.11, vello 0.10, wgpu 30.0, winit 0.30.13, accesskit 0.25, v8 152.2, hyper 1.11, rustls 0.23.43, quinn 0.11.11, rusqlite 0.40, ipc-channel 0.23, prost 0.14, tokio 1.53.

## Журнал сессий

### 2026-09-16 — сессия 6: Tasks 16–23 (subagent-driven), M1a закрыт
- T16 `cl-layout` box tree доделан и возвращён в workspace (был запаркован в `exclude`): anonymous-block wrapping (CSS 2.1 §9.2.1.1), display:none пропуск, `<br>` → line-break, block-in-inline блокификация.
- T17 `Au` геометрия + `style_adapt.rs` (границы обнуляются для `none`/`hidden` независимо от stylo-ширины — "border-width/border-style trap").
- T18 inline formatting context (parley): line breaking, `text-align`, `line-height`, whitespace normal/pre; решение — фон/рамки/vertical-align вложенных инлайнов не реализуются в M1a, `<span>` красит только текст.
- T19 `cl-paint` display list: paint order CSS2 Appendix E, canvas-фон пропагация `<html>`/`<body>` → canvas (§14.2).
- T20 display-list validator + `arbitrary`-фаззинг (`display_list_validate`).
- T21 `cl-gfx` CPU raster (tiny-skia + swash), детерминизм (без хинтинга, без системных шрифтов).
- T22 `cl-testshell` реальный однопроцессный pipeline (render/dump/compare), determinism+smoke тесты; культинг офскрин-айтемов в paint (иначе 1 МБ страница проваливала validate).
- T23 (эта сессия): reftest-раннер (`crates/testshell/src/reftest.rs`) + 20 reftest-пар из брифа; `<hr>` получил UA-рамку (HTML §15.3.11, `solid` вместо `inset`); CI: явный `reftest`-шаг в `test`-матрице + upload artifacts on failure; закрыты все Step 2 доки. Найден реальный движковый баг (пробел между block-соседями ломает margin-коллапс, CSS 2.1 §9.2.2.1) — контроллер квалифицировал его Critical и потребовал фикс до ревью вместо документирования как долг; фикс в `crates/layout/src/box_tree.rs` (см. долг №9 выше для деталей), 21-я reftest-пара `margin-collapse-siblings-whitespace` как регрессия, 14 golden-снапшотов переприняты и вручную проверены. Финал: **21/21 reftest, все гейты зелёные**. Полная верификационная цепочка + fuzz — см. `.superpowers/sdd/2026-09-07-m1a-static-pipeline/task-23-report.md`.
- Итог: **все 23 задачи M1a сделаны**; ветка ждёт финального ревью и решения владельца о мерже/теге `m1a`.

### 2026-09-16 — сессия 5: онбординг нового controller-а, MEMORY.md
- MEMORY.md приведён к состоянию M1a по SDD ledger (был на уровне M0). Очередь: ревью долгов (Task 14, `2d38014`, `cl-net` `550db26`/`d410182`) → Task 16 → 17–23.

### 2026-09-08 … 2026-09-10 — сессии 3–4: исполнение M1a (subagent-driven), задачи 1–15
- T1 ADR-0015 + scope-скрипты + python3 в CI; T2 stylo build spike (компилируется на 3 ОС); T3–T4 `cl-dom` arena + html5lib-сериализатор; T5–T6 `cl-net` `Url` + file loader + WPT url harness (гейт снижен до 0.92 — потолок crate `url`, `Ruling` в ledger); T7 encoding sniffer (1 fix: unmatched quote в `<meta content>`); T8 `TreeSink` над ареной (2 fix-раунда, тесты); T9 html5lib harness 1307/1313; T10 `html_parse` fuzz; T11 stylo DOM traits — **гейт пройден**, ноль `unsafe`, ADR-0015 дополнен дважды; T12 UA stylesheet + сбор `<style>`/`<link>`; T13 `resolve()` через последовательный traversal, хэндл переделан в односложную ссылку (третье дополнение ADR-0015), computed-style goldens; T14 `css_stylesheet` fuzz (ревью в очереди); T15 `cl-fonts` (Ahem + Noto, без системных бэкендов; ревью подтвердило sha256 и лицензии).
- Инциденты: `git add -A` контроллера утащил недописанный `cl-fonts` в чужой коммит (исправлено, правило записано); Windows CI: `to_file_path` для `file://host/...` — движок исправлен (`550db26`), не тест.
- T16 прерван владельцем посреди правки; частичная работа закоммичена (`fc4ae0d`), crate исключён из workspace.


### 2026-09-07 — сессия 2: исполнение M0 (subagent-driven)
- Исполнены Tasks 1–11 из `docs/superpowers/plans/2026-09-07-m0-foundation.md`: T1 workspace/lints/deny (1 fix), T2 cl-platform (no fixes), T3–T5 cl-ipc codec/validate/bootstrap/handshake (no fixes), T6–T7 cl-process sandbox/spawn (no fixes), T8 browser/child ping-pong + tracing (1 fix: `--browser-fail-after-handshake`), T9 testshell PNG (no fixes), T10 bench script (no fixes; Chrome baseline pending owner action), T11 CI check-agents + Gates (3 fixes: rustfmt nightly options noted, cargo-fuzz flags fixed, Exit codes verified).
- CI результат: check ✅, test (macOS-14/windows-2022/ubuntu-24.04) ✅, fuzz-short ✅ на всех трёх ОС.
- Измерения: idle RSS 4864 КБ (browser 2496, renderer 2368). Release sandbox gate PASS (unsandboxed exit 78). Fuzz run 60 s: 75.2M runs, 0 crashes.
- Task 12: обновлены PLAN.md exit-criteria (5/6 тикнуты, Chrome baseline остаётся owner-action), MEMORY.md state/debts/log, ARCHITECTURE.md crate status.
- Следующее действие: финальное ревью ветки → merge в main → тег `m0` → план M1 (`docs/superpowers/plans/<date>-m1-static-pages.md`).
- Финальное ревью ветки (sonnet; opus дал 429): 0 Critical, 2 Important исправлены в фикс-волне (bounded recv, 3-OS lint), минорные долги сведены в список выше.

### 2026-09-07 — сессия 1: исследование + фундамент документации
- Прочитан skill `browser-engine-research` (6 референсов, срез 2026-09-05).
- Веб-проверка: Servo, Ladybird, CEF/cef-rs, Chrome Sync, MV2/MV3, Chromium release cadence, Rust-стек, App-Bound Encryption, Rust GUI, wry.
- Владелец выбрал: свой движок с нуля на Rust; соло + AI-агенты; все три платформы сразу; sync = локальный профиль Chrome + свой сервер.
- Созданы: CLAUDE.md, AGENTS.md, MEMORY.md, README.md, docs/ (ARCHITECTURE, DEVELOPMENT, CODING_STANDARDS, SECURITY, TESTING, RESEARCH-2026-09, FEATURE_MATRIX, SPEC_REGISTRY, DEPENDENCIES, GLOSSARY), docs/adr/0001–0014, конфиги тулчейна.
- Владелец: лицензия — максимально свободная (→ Apache-2.0 OR MIT), имя — ChromeLight, `git init` — да, rustup установлен.
- Создано: `docs/PLAN.md` (программа M0–M6, exit-критерии, ритуалы, риски), `docs/superpowers/plans/2026-09-07-m0-foundation.md` (12 задач с кодом и тестами), LICENSE-APACHE/MIT; ADR-0013/0014 → Accepted; git-репозиторий инициализирован, первый коммит — вся документация.
- **Следующее действие:** исполнить M0 по плану (subagent-driven или inline). Владельцу: создать GitHub-ремоут для CI-матрицы; вручную измерить Chrome 153 для `docs/history/bench-2026-09.md` (Task 10).
