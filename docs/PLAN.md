# План: программа milestone-ов ChromeLight

Версия: 2026-09-07. Это **программа** (что, в каком порядке, критерии выхода). Детальные исполняемые планы — по одному на milestone или под-milestone в `docs/superpowers/plans/YYYY-MM-DD-<name>.md` (формат: задачи → тесты → коммиты). Первый: `2026-09-07-m0-foundation.md`.

Оценки — **порядок величины в «агент-неделях»** (одна неделя работы владельца с AI-агентами), не обещания. Каждый milestone закрывается только при выполнении **всех** exit-критериев на **трёх ОС**; исключения фиксируются в MEMORY.md.

## 0. Принципы последовательности

1. Безопасность и инфраструктура раньше фич: процессы/IPC/sandbox/тесты — M0–M2, до открытого веба.
2. Каждый milestone заканчивается работающим бинарником, который можно запустить и измерить.
3. Correctness → measurement → optimization. Reference-path (CPU raster, single-thread layout) сохраняется для differential-тестов.
4. Сначала вертикальный срез (одна страница end-to-end), потом ширина (feature matrix).
5. Chrome-интероп (импорт, sync, расширения, DevTools) — после стабильного ядра, но API-границы под них (StorageKey, capability handles, isolated worlds) закладываются раньше.

## 1. Карта milestone-ов

```
M0 Foundation ─► M1 Static pages ─► M2 Secure + JS ─► M3 Interactive platform
                                                            │
                                    M4 Isolation + Storage ◄┘
                                            │
                                    M5 Browser product ─► M6 Chrome interop ─► Alpha
```

| M | Название | Суть | Порядок |
|---|---|---|---|
| M0 | Foundation | workspace, CI на 3 ОС, cl-platform, cl-ipc, cl-process (spawn+handshake, sandbox type-state), testshell PNG, bench-скрипт | 2–3 нед |
| M1 | Static pages | URL/encoding, html5ever→cl-dom, stylo→cl-style, block/inline layout, text, paint→display list, CPU raster в GPU-process, egui shell с одной вкладкой, `file://`+localhost, WPT adapter, sandbox macOS/Linux | 8–12 нед |
| M2 | Secure + JS | network process (rustls/hyper h1/h2, Fetch, cache, cookies), sandbox Windows, V8 через cl-js, Web IDL codegen, event loop, базовый DOM API, `fetch`/XHR, site-per-process assignment, crash reporting, Gate S0 → первый открытый HTTPS-сайт | 10–14 нед |
| M3 | Interactive platform | events/forms/focus/IME/selection, flex/grid/tables, incremental invalidation, dedicated workers, Canvas 2D, SVG, images в utility process, a11y tree, WebSocket | 10–14 нед |
| M4 | Isolation + storage | OOPIF, compositor async scroll/animations, freeze/discard, localStorage/IndexedDB/Cache Storage/OPFS/quota, partitioning, HTTP/3, shared workers, memory budgets подтверждены | 8–12 нед |
| M5 | Browser product | tabs/omnibox/history/bookmarks/downloads/permissions/cert UI, профили, private mode, session restore, пароли/autofill, hibernate, updater подписанный, l10n en/ru, service workers, media playback базово | 10–14 нед |
| M6 | Chrome interop | импорт/зеркало профиля Chrome, sync client + server, MV3 runtime + Web Store install, CDP-подмножество + DevTools frontend, WebDriver BiDi; Gate S1 → **Alpha** | 10–14 нед |

Сумма — порядок **60–85 агент-недель** до альфы. Это оптимистичный порядок; long tail совместимости живёт после альфы бессрочно.

## 2. Milestone-ы подробно

### M0 — Foundation

**Цель:** скелет, на который встают все остальные crate-ы; multiprocess с первого коммита; CI на трёх ОС.

Scope: root workspace + lints + `deny.toml`; `cl-platform` (ProcessType, Clock, paths); `cl-ipc` (сообщения, postcard-кодек с лимитами, `Validate`, bootstrap через ipc-channel, handshake, fuzz `ipc_decode`); `cl-process` (spawn same-binary child, timeout, `Sandbox<Unapplied→Applied>`, `NotImplementedPolicy`/`DevNoSandbox`); `apps/chromelight` (`--type`, browser↔renderer ping/pong, tracing, `--trace-out`); `cl-testshell` (`render` → белый PNG, `compare`); `tools/bench/mem.sh`; CI workflow; проверочные скрипты (AGENTS.md sync, platform cfg).

Exit-критерии:
- [x] `cargo build/test/clippy -D warnings/fmt` зелёные на macOS, Windows, Linux в CI; `deny`/`doc`/скрипты — на Linux (платформонезависимы).
- [x] `chromelight --exit-after-handshake --no-sandbox` завершает handshake browser↔renderer на 3 ОС (integration test).
- [x] Release-бинарник отказывается спавнить renderer без sandbox (`NotImplementedPolicy`) — тест.
- [x] `cargo fuzz run ipc_decode` работает 60 с без падений.
- [x] `cl-testshell render` пишет PNG 800×600; `compare` возвращает 0/1.
- [x] `tools/bench/mem.sh` пишет JSON с RSS по процессам; в `docs/history/bench-2026-09.md` — benchmark script готов.
- [ ] Baseline Chrome 153 на машине владельца (ручное измерение) — владелец.

Детальный план: `docs/superpowers/plans/2026-09-07-m0-foundation.md`.

### M1 — Static pages

**Цель:** `chromelight file:///page.html` рендерит статичную страницу с CSS через настоящий pipeline в трёх процессах; reftests и WPT-подмножество живые.

Scope по crate-ам:
- `cl-net` (минимум): `file://` и `http://localhost` (hyper client без TLS) — только для тестов; URL через `url`; encoding sniffing (`encoding_rs`).
- `cl-html`: html5ever `TreeSink` → `cl-dom` arena.
- `cl-dom`: `NodeId` arena, Document/Element/Text/Comment, атрибуты (атомы), tree traversal, `querySelector` без JS (для тестов).
- `cl-style`: stylo `TElement/TNode` impl, stylesheet loading (`<style>`, `<link>` через cl-net), cascade, computed style; `@media` базово.
- `cl-layout`: box tree, block formatting context, inline formatting (parley), replaced elements (`<img>` через utility decode), positioned (`relative/absolute`), floats базово, overflow/scroll containers статично, `Au` единицы, fragment tree.
- `cl-paint`: display list (background, border, text runs, images, clips, transforms 2D), hit-test структура.
- `cl-compositor` (минимум): один layer, tiles не нужны; `cl-gfx`: CPU raster (tiny-skia) в GPU process; vello/wgpu путь за флагом.
- `cl-shell-ui`: egui окно, адресная строка, одна вкладка, отображение кадра из GPU process (shm → texture).
- `cl-process`: sandbox **macOS seatbelt** и **Linux namespaces+seccomp** для renderer; Windows — заглушка с явным отказом.
- `cl-testshell`: настоящий рендер в PNG; reftest runner; WPT product adapter (`tools/wpt/`), директории `url`, `encoding`, `html/syntax`, `dom/nodes` (без скриптов — только парсер-тесты через testshell dump), `css/CSS2` reftests подмножество.
- Fuzz: `html_tokenizer`, `css_stylesheet`, `url_parse`, `display_list_validate`.

Exit-критерии:
- [ ] 20 reftests (`tests/ref/`) зелёные на 3 ОС с bundled fonts.
- [ ] WPT: `url` ≥ 95%, `encoding` ≥ 90%, `html/syntax/parsing` ≥ 90% (tree dump), `css/CSS2/normal-flow` ≥ 60% — числа фиксируются в дашборде, expected-fail с bug ID.
- [ ] Sandbox renderer применяется на macOS и Linux; `tests/security/sandbox_fs.rs` (renderer не может открыть `/etc/passwd`) зелёный.
- [ ] Бюджет: пустой браузер + пустая вкладка ≤ 120 МБ RSS суммарно (измерение, при провале — пересмотр ADR-0012 честными цифрами).
- [ ] Chrome baseline на corpus записан (`docs/history/bench-2026-10.md`).

### M2 — Secure + JS

**Цель:** первый **произвольный HTTPS-сайт** открывается за sandbox на всех трёх ОС; JS работает.

Scope: `cl-net` полный network process (rustls + platform verifier, hyper h1/h2, Fetch state machine с redirects/CORS/credentials, HTTP cache RFC 9111 память+диск, cookie store RFC 6265bis, mixed content, HSTS preload); `cl-js` (V8 isolate/context, JsRuntime trait, event loop tasks/microtasks/timers); `cl-bindings` (Web IDL parser → codegen; Node/Element/Document/Event/Window/console/setTimeout/fetch/XHR минимум); `cl-webapi::fetch`; DOM mutation из JS → инвалидация style/layout (полная перерисовка допустима, `M2-ONLY`); `cl-browser::site` (SiteInstance, site-per-process assignment); Windows sandbox; crash dumps (minidumper); Test262 qualification set; WPT `fetch`, `xhr`, `dom/events`, `html/webappapis`.

Exit-критерии:
- [ ] Gate S0 выполнен на 3 ОС; `--no-sandbox` не существует в release.
- [ ] Открываются и рендерятся без JS-ошибок первого уровня: example.com, wikipedia.org (статья), news.ycombinator.com, mdn (документная страница), github.com README-страница — curated corpus v0 (5 сайтов) с чек-листом.
- [ ] WPT: `fetch/api` ≥ 60%, `xhr` ≥ 60%, `dom/events` ≥ 70%, `html/webappapis/scripting` ≥ 60%.
- [ ] Test262 qualification set 100% (V8) — harness работает.
- [ ] Renderer crash → вкладка перезапускается, browser process жив (integration test с `--crash-renderer-after=…`).
- [ ] Память: активная страница corpus v0 ≤ 70 МБ renderer (или пересмотр ADR-0012 с цифрами).
- [ ] V8 обновлён до текущей Chrome-версии по `tools/v8-bump.sh` хотя бы один раз.

### M3 — Interactive platform

**Цель:** сайты, с которыми можно взаимодействовать: формы, клики, скролл, прокрутка, flex/grid-layout современных страниц.

Scope: UI Events/Pointer Events, focus/tab order, forms (input/textarea/select/checkbox/radio, submit, validation), IME/composition через winit, selection/clipboard (copy с gesture), scrolling (main-thread), flex/grid через taffy + baseline/intrinsic sizing, tables, `position: sticky/fixed`, incremental style/layout/paint invalidation (удаление всех `M2-ONLY`), dedicated workers, Canvas 2D (tiny-skia/vello), inline SVG (resvg-подход + DOM), images AVIF/WebP в utility process, MutationObserver, Shadow DOM + custom elements, WebSocket, `cl-a11y` accesskit tree, WPT `css-flexbox/css-grid/css-position/css-text`, `pointerevents`, `uievents`, `html/semantics/forms`, `workers`, `custom-elements`, `shadow-dom`.

Exit-критерии:
- [ ] Corpus v1 (15 сайтов, включая login-формы и SPA на React/Vue): сценарии «открыть, кликнуть, заполнить, отправить, проскроллить» проходят по чек-листу.
- [ ] WPT: `css-flexbox` ≥ 70%, `css-grid` ≥ 50%, `html/semantics/forms` ≥ 50%, `workers` ≥ 60%, `shadow-dom` ≥ 70%.
- [ ] Инкрементальный relayout: 1000 DOM-мутаций на 10k-node странице ≤ 16 мс/кадр в среднем на M1 (bench).
- [ ] Screen reader (VoiceOver) читает заголовки/ссылки/формы тестовой страницы.

### M4 — Isolation + storage

**Цель:** безопасность и память на уровне архитектурных обещаний: OOPIF, async compositor, freeze/discard, полное хранилище.

Scope: OOPIF (RemoteFrame в renderer, surface embedding в GPU process, input routing), COOP/COEP/CORP, ORB, `cl-compositor` property trees/tiles/async scroll/transform-opacity animations, vello/wgpu основной путь + CPU fallback, freeze/discard policy, `cl-storage` (localStorage sync-API через local cache + async commit, IndexedDB на SQLite, Cache Storage, OPFS, quota/eviction, private mode), storage partitioning, shared workers, HTTP/3 (quinn/h3), WPT `storage`, `IndexedDB`, `webstorage`, `html/browsers/origin`, `cross-origin-*`, `css-transforms`, `css-animations` подмножество.

Exit-критерии:
- [ ] Cross-site iframe живёт в отдельном renderer; `tests/security/site_isolation.rs` — renderer A не получает ответы site B (ORB) и не может обратиться к его storage.
- [ ] Скролл длинной страницы 60 fps при занятом main thread (bench с busy-loop JS).
- [ ] 10 вкладок corpus v1 ≤ 600 МБ суммарно; фоновая frozen ≤ 25 МБ (ADR-0012).
- [ ] WPT `IndexedDB` ≥ 70%, `webstorage` ≥ 90%, `storage` ≥ 70%.
- [ ] Revisit ADR-0004 (доля V8 в памяти) и ADR-0006 (vello fps) — записаны решения.

### M5 — Browser product

**Цель:** ежедневно пригодный браузер для владельца (dogfooding) с подписанными обновлениями.

Scope: `cl-browser` полный (tabs, NavigationController, history, bookmarks, downloads с quarantine, permissions state machine + prompts, cert interstitials, session restore, профили, private mode, memory pressure policy + hibernate), пароли/autofill (шифрование platform keystore), `cl-shell-ui` полный egui (tab strip, omnibox с origin display, settings, downloads, history/bookmarks manager), l10n en/ru, service workers (`cl-webapi::sw` + Cache Storage), `<video>/<audio>` базово (utility decode, MSE later), updater (ed25519 + OS signing, staged rollout, rollback), installers (dmg notarized, msi/msix, AppImage/deb), `tools/rename-checklist.md`, revisit ADR-0008 (privileged web UI).

Exit-критерии:
- [ ] Владелец использует ChromeLight как основной браузер 5 дней подряд; журнал проблем ≤ 20 блокеров.
- [ ] Updater drill: версия N → N+1 → rollback на 3 ОС пройден.
- [ ] Corpus v2 (30 сайтов, включая YouTube-страница без DRM, Gmail-логин до 2FA, Google Docs просмотр) — чек-лист.
- [ ] Hibernated вкладка ≤ 5 МБ RAM и восстанавливается офлайн.
- [ ] WPT `service-workers` ≥ 50%.

### M6 — Chrome interop → Alpha

**Цель:** пользователь Chrome переезжает без потерь; расширения; DevTools; sync.

Scope: `cl-chrome-import` (парсеры профиля, расшифровка на macOS/Linux/Windows-v10, зеркало через watcher), `cl-sync` + `cl-sync-server` (pinned sync.proto, E2E-шифрование, типы bookmarks/history/passwords/preferences/tabs/extensions), `cl-extensions` (MV3 manifest/permissions, CRX3, Web Store install, background SW, content scripts isolated world, `chrome.runtime/storage/tabs/scripting/declarativeNetRequest/action/contextMenus/webNavigation/cookies/alarms/notifications`), `cl-devtools` (CDP: Runtime/Debugger через V8 Inspector, DOM, CSS, Network, Page, Log, Target; Chrome DevTools frontend), WebDriver BiDi endpoint, Gate S1 (внешний security review, IPC fuzz ≥ 30 дней, disclosure policy).

Exit-критерии:
- [ ] Импорт профиля Chrome владельца: закладки/история/пароли (macOS)/настройки/список расширений — проверено вручную; фикстурные тесты на 3 ОС.
- [ ] Sync между двумя устройствами владельца через self-hosted сервер: закладки и вкладки сходятся ≤ 30 с.
- [ ] Расширения-корпус: uBlock Origin Lite, Bitwarden, Dark Reader, Vimium, React DevTools — работают по чек-листу.
- [ ] Chrome DevTools frontend подключается: Elements, Console, Sources (breakpoints), Network.
- [ ] Gate S1 закрыт → **Alpha** для технических пользователей (macOS/Linux/Windows), disclaimer «not affiliated with Google».

## 3. Постоянные ритуалы (с M2)

| Ритуал | Частота | Что |
|---|---|---|
| V8 bump | ≤ 7 дней после релиза Chrome (каждые 2 недели) | `tools/v8-bump.sh`, Test262 qualification, bench |
| Upstream bump (stylo, wgpu, vello, parley, html5ever…) | ежемесячно | отдельный PR, changelog-ссылки, WPT-диф |
| Bench vs Chrome | ежемесячно | `docs/history/bench-YYYY-MM.md` |
| WPT full run | nightly | дашборд, регрессии в issues |
| Fuzz long | nightly | 30 мин все targets |
| ADR review | при закрытии milestone | пересмотр помеченных «revisit» |
| MEMORY.md | каждая сессия | журнал, следующий шаг |

## 4. Риски программы и стоп-условия

| Риск | Сигнал | Действие |
|---|---|---|
| stylo-интеграция с arena-DOM не идёт | M1 spike > 2 недели без cascade | fallback: собственный selector matching + упрощённый cascade для M1, stylo в M3 |
| V8 не укладывается в память | M2: idle renderer > 80 МБ | `--jitless`/snapshot/lazy; ADR-0004 revisit раньше |
| Windows sandbox | M2 > 3 недель | Windows переводится в nightly-матрицу до M3, release для Windows блокируется |
| vello на слабых GPU | M4 fps < 30 | CPU tiles + GPU composite |
| Соло-скорость ниже оценок | любой milestone > 2× оценки | сократить feature matrix milestone-а, не качество гейтов; записать в MEMORY.md |
| ADR-0001 review через 12 мес | WPT фокус < 50% или corpus < 30% | рассмотреть Servo как fallback-движок для несовместимых вкладок |

## 5. Как рождается детальный план milestone-а

1. Прочитать этот документ, ARCHITECTURE, ADR субсистем.
2. Разбить milestone на под-планы по crate-ам (каждый под-план — рабочее, тестируемое ПО).
3. Написать по формату `superpowers:writing-plans`: файлы, интерфейсы, TDD-шаги с кодом, коммиты.
4. Исполнять через `superpowers:subagent-driven-development` (свежий агент на задачу + ревью) или `executing-plans`.
5. По завершении — обновить FEATURE_MATRIX, SPEC_REGISTRY, MEMORY.md; тег `mN`.
