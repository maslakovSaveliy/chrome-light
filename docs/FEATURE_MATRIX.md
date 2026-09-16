# Feature Matrix

Статусы: **planned(Mx)** — в milestone x; **later** — после v1; **non-goal** — не делаем, причина указана. Milestones ориентировочно: M0 фундамент/тулинг, M1 статичные страницы, M2 sandbox + сеть + JS, M3 интерактивная платформа, M4 site isolation + storage, M5 продукт, M6 интероп (импорт/sync/расширения/DevTools). Детальные exit-критерии — в плане (следующий документ).

## Движок

| Область | Фича | Статус |
|---|---|---|
| URL/encoding | WHATWG URL, Encoding (UTF-8, legacy sniffing) | M1a: done (URL 828/893 WPT = 92.7%, потолок `url`-крейта; encoding — BOM/transport/`<meta>` prescan, дефолт UTF-8) / M1 |
| HTML | tokenizer/tree builder (html5ever), parser-script reentrancy, `document.write` | M1a: done (tree construction 1307/1313 html5lib-tests = 99.5%) / M3 (reentrancy, `document.write`) |
| DOM | Node tree, events (capture/bubble), MutationObserver, Range/Selection, Shadow DOM, custom elements | M1a: done (статичное Node tree, без событий) / M1 / M3 |
| CSS | stylo: cascade, layers, custom properties, media/container queries, nesting | M1a: partial (cascade работает, `cl-layout` читает только M1a-подмножество из 22 longhand-ов) / M1 |
| Layout | block, inline, positioned, floats | M1a: partial (block + inline + `position:relative` реализованы и покрыты 20/20 reftest-парами; floats, absolute/fixed — нет) / M1 |
| Layout | flex, grid (taffy math), tables | M3 |
| Layout | multicol, fragmentation, writing modes vertical | later |
| Text | shaping, bidi, line breaking, font fallback, variable fonts, emoji | M1a: partial (Ahem/Noto Sans через parley/swash, только LTR, без per-inline line-height/vertical-align) / M1 базово / M3 полно |
| Paint/compositor | display lists, stacking, clips, transforms, opacity, filters; async scroll; transform/opacity animations на compositor | M1a: partial (display list + сплошные рамки + фон + текст + `overflow:hidden` клип, CPU raster) / M1 / M4 |
| GPU | vello+wgpu; CPU fallback | M1a: done (CPU fallback — tiny-skia + swash, детерминированный) / M1 (vello+wgpu) |
| Test infra | `cl-testshell`: render/dump/compare/reftest CLI, 20 reftest-пар (`crates/testshell/tests/ref`), golden dumps 5 стадий, 5 fuzz-таргетов | M1a: done |
| Images | PNG, JPEG, GIF, WebP, AVIF (utility process) | M1 / M3 (AVIF) |
| SVG | inline SVG рендер (usvg/resvg-подход) | M3 |
| JS | V8, ES2025, modules, Wasm | M2 |
| Event loop | tasks/microtasks, timers, rAF, throttling | M2 |
| Fetch API, XHR | CORS, credentials, streams | M2 |
| Forms | inputs, submit, validation, IME | M3 |
| Storage | cookies, localStorage, sessionStorage | M2/M4 |
| Storage | IndexedDB, Cache Storage, OPFS, quota | M4 |
| Workers | dedicated, shared | M3 / M4 |
| Service workers | | M5 |
| Canvas 2D | | M3 |
| WebGL | | later |
| WebGPU | | later |
| Web Audio, `<audio>/<video>` (MSE) | | M5 / later |
| WebRTC | | non-goal v1 (объём + privacy surface) |
| EME/DRM | | non-goal v1 (лицензирование CDM) |
| WebXR | | non-goal |
| Accessibility | accesskit tree, focus/keyboard | M1 базово / M3 |
| Printing/PDF | | later |
| HTTP | 1.1, 2 | M2 |
| HTTP | 3/QUIC | M4 |
| WebSocket, WebTransport | | M3 / later |
| bfcache | | later |
| Site isolation | site-per-process | M2 (assignment) / M4 (OOPIF) |

## Продукт

| Фича | Статус |
|---|---|
| Окна, вкладки, omnibox, back/forward, history | M5 |
| Профили, private mode, session restore | M5 |
| Закладки, загрузки, permissions UI, cert UI | M5 |
| Пароли, autofill | M5 |
| Импорт профиля Chrome (закладки, история, пароли, настройки, список расширений) | M6 |
| Зеркало профиля Chrome (периодический one-way diff) | M6 |
| Sync-клиент по Chromium `sync.proto` + self-hosted сервер | M6 |
| Google Chrome Sync | **non-goal** (API закрыт Google, 2021) |
| Расширения MV3, установка из Chrome Web Store | M6 |
| Расширения MV2 | **non-goal** (Chrome убрал) |
| DevTools: CDP-подмножество + Chrome DevTools frontend | M6 |
| WebDriver BiDi | M6 |
| Автообновления подписанные, staged rollout | M5 (блокер беты) |
| Crash reporting без PII | M2 |
| Enterprise policy | later |
| Safe Browsing / reputation | non-goal v1 (нет сервиса); локальные эвристики загрузок — M5 |
| Мобильные платформы | non-goal v1 |
| Locale/l10n UI | M5 (en, ru) |
