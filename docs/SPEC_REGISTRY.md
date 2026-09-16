# Реестр спецификаций

Одна строка на реализованную/планируемую фичу. Acceptance criteria — нормативный текст + тесты. MDN — пояснение, не источник истины. Статусы: `todo`, `wip`, `done`, `partial`, `deviation`.

Формат: `spec §anchor | feature | crate::module | tests (WPT path / unit) | status | deviations / notes`.

## Нормативные источники

| Орган | Спецификации | URL |
|---|---|---|
| WHATWG | HTML, DOM, Fetch, URL, Encoding, Streams, Storage, Infra, MIME Sniffing, Web IDL, Console, Notifications, XHR, Compat | https://spec.whatwg.org |
| W3C CSSWG | CSS Snapshot 2025 + module drafts | https://www.w3.org/TR/css-2025 ; https://drafts.csswg.org |
| W3C WebAppSec | CSP L3, Mixed Content, Secure Contexts, Referrer Policy, Permissions Policy, Trusted Types, Fetch Metadata, COOP/COEP (в HTML) | https://w3c.github.io/webappsec/ |
| W3C | Permissions, Pointer Events 3, UI Events, Service Workers, IndexedDB 3, Web App Manifest, WebDriver 2, WebDriver BiDi, WAI-ARIA 1.2, HTML-AAM, AccName 1.2 | https://www.w3.org/TR/ |
| TC39 | ECMA-262, ECMA-402, Test262 | https://tc39.es |
| WebAssembly | Core, JS API, Web API | https://webassembly.github.io/spec/ |
| IETF | RFC 9110 (HTTP semantics), 9111 (cache), 9112 (HTTP/1.1), 9113 (HTTP/2), 9114 (HTTP/3), 9000/9001 (QUIC), 9204 (QPACK), 8446 (TLS 1.3), 6455 (WebSocket), 6265bis (cookies), 9297 (WebTransport) | https://www.rfc-editor.org |
| Khronos / GPU for Web | WebGL 2.0, WebGPU, WGSL | https://registry.khronos.org/webgl/ ; https://gpuweb.github.io/gpuweb/ |
| Unicode | UAX #9 (bidi), #14 (line break), #29 (segmentation), UTS #35 (CLDR) | https://unicode.org/reports/ |
| Privacy CG | Storage Partitioning, Storage Access, CHIPS, GPC | https://privacycg.github.io |
| Chromium (compat) | CDP, CRX3, sync.proto, MV3 API surface | https://chromedevtools.github.io/devtools-protocol/ ; https://developer.chrome.com/docs/extensions |

## Реестр

| Spec §anchor | Feature | Crate::module | Tests | Status | Deviations / notes |
|---|---|---|---|---|---|
| url.spec.whatwg.org#url-parsing | URL parser | `cl-net::url` (crate `url` 2.5.8 + idna 1.1) | `crates/net/tests/urltestdata.rs` (WPT `urltestdata.json`, pinned) | deviation | 828/893 (92.7%). 65 known failures в `tools/conformance/url/expectations.txt`, три категории: file-URL slash/drive-letter (47), IDNA/punycode строже WPT (8), percent-encoding в opaque path (10). Это потолок upstream `url`; Servo фиксирует те же провалы. Гейт — 0.92 + точный список ожиданий (0 unexpected, 0 stale). |
| encoding.spec.whatwg.org#decode, html.spec.whatwg.org#determining-the-character-encoding | byte stream decoding, BOM sniffing, `<meta>` prescan | `cl-html::encoding`, `cl-html::prescan` | `crates/html/src/encoding.rs`, `crates/html/src/prescan.rs` (unit + proptest); no wpt/encoding harness wired up yet | deviation | Precedence is spec-conformant (BOM → transport label → `<meta>` prescan, HTML §13.2.3.2), but step 9's final fallback for a wholly unlabelled document is spec-defined as locale-/implementation-dependent (Chrome defaults to windows-1252 for most locales); ChromeLight always defaults to UTF-8 in M1a (no locale plumbing yet, and it matches the overwhelming majority of unlabelled documents served today). Marked in code at `crates/html/src/encoding.rs` (`EncodingSource::Default`). Not a WPT-test failure like the URL row above — no wpt/encoding conformance harness exists yet in M1a — so there is no expectations file/bug ID to link; revisit when M2 wires up wpt/encoding.|
| html.spec.whatwg.org#tokenization | HTML tokenizer | `cl-html` (html5ever) | wpt/html/syntax/parsing | todo | |
| html.spec.whatwg.org#tree-construction | tree builder | `cl-html::sink` | `crates/html/tests/tree_construction.rs` (html5lib-tests tree-construction corpus, pinned, see `tools/conformance/html5lib/`) | deviation | 1307/1313 (99.5%) над non-skipped кейсами (37 skipped: 29 `#document-fragment`, 8 `#script-on`; 0 panics). 6 known failures в `tools/conformance/html5lib/expectations.txt`, две категории, обе апстрим в html5ever 0.39 (не фиксится из `cl-html`/`cl-dom`): annotation-xml integration point (4 кейса, `tests20.dat` indices 54–57) — html5ever не распознаёт `<annotation-xml encoding="text/html"\|"application/xhtml+xml">` как HTML integration point, поэтому вложенный HTML-элемент выносится сиблингом `<math>` вместо вставки внутрь `<annotation-xml>`; customizable-select/`selectedcontent` mirroring (2 кейса, `webkit02.dat` indices 44–45) — html5ever 0.39 предшествует этой фиче HTML Standard (2024/2025) и не зеркалит текст выбранного `<option>` в `<button><selectedcontent>` внутри `<select>`, оставляя `<selectedcontent>` пустым. Гейт — 0.90 + точный список ожиданий (0 unexpected, 0 stale, 0 panics). |
| dom.spec.whatwg.org#nodes | Node, Element, Text, Document | `cl-dom::node` | wpt/dom/nodes | todo | |
| dom.spec.whatwg.org#events | EventTarget, dispatch | `cl-dom::events` | wpt/dom/events | todo | |
| css-cascade-5, css-syntax-3, selectors-4 | cascade, parsing, selectors | `cl-style` (stylo) | `crates/style/tests/computed_style_goldens.rs`, `crates/style/tests/golden/*.html`, gate-скрипты (`tools/check-stylo-scope.sh`); wpt/css/css-cascade, css-syntax, selectors не подключены | partial | stylo — правильный cascade/parsing/selectors движок, но `cl-layout` читает только 22 longhand-а через `style_adapt.rs` (M1a scope); остальные свойства парсятся, но не влияют на layout/paint. Отклонения самого stylo отслеживаются апстрим. |
| css-display-3, CSS2 §9–10 | block/inline formatting | `cl-layout::block`, `::inline`, `::box_tree` | `crates/layout/tests/block.rs`, `crates/layout/tests/inline.rs`, `crates/layout/tests/box_tree.rs`, `crates/layout/tests/box_tree_goldens.rs`, `crates/layout/tests/fragment_tree_goldens.rs`, `crates/testshell/tests/reftests.rs` (22 reftest-пары); wpt/css/CSS2, css-display не подключены | partial | M1a: block-in-inline блокифицируется (инлайновый контейнер становится блочным боксом) вместо сплита по CSS 2.1 §9.2.1.1 (тест `box_tree_should_blockify_inline_with_block_child`). **Исправлено в Task 23** (CSS 2.1 §9.2.2.1: «white space content that would subsequently be collapsed away … does not generate any anonymous inline boxes») — раньше пробельный текстовый узел между двумя block-level соседями (обычная разметка с переносами строк) порождал реальный (нулевой высоты) anonymous block, который ломал коллапс соседних margin-ов на два отдельных хопа и суммировал их вместо max (найдено при написании `crates/testshell/tests/ref/margin-collapse-siblings.html`); теперь `crates/layout/src/box_tree.rs`'s `wrap_inline_runs`/`push_or_drop_run` не порождает бокс вообще для прогона, целиком состоящего из схлопывающегося пробела под `white-space: normal` — тесты `box_tree_should_not_generate_boxes_for_collapsible_whitespace_between_blocks`, `box_tree_should_keep_whitespace_under_white_space_pre` (`crates/layout/tests/box_tree.rs`), `whitespace_between_siblings_should_not_defeat_margin_collapsing` (`crates/layout/tests/block.rs`), реф-пара `margin-collapse-siblings-whitespace`. |
| css-text-3 §4.1.1, §5 (line breaking), CSS2 §9.4.2 (line boxes) | white-space processing, line breaking, `text-align`, строковые боксы | `cl-layout::whitespace`, `::text` (parley), `::inline` | `crates/layout/src/whitespace.rs` (unit), `crates/layout/tests/inline.rs`, `tests/fragment_tree_goldens.rs`; wpt/css/css-text не подключён | partial | Реализовано: `normal` (схлопывание, перенос) и `pre` (сохранение пробелов, перенос только по `\n`/`<br>`, без мягкого переноса — строка шире контейнера переполняет его); строковый бокс шириной с содержащий блок (CSS2 §9.4.2), выравнивание двигает содержимое внутри него; форсированный перенос в конце содержимого не добавляет строкового бокса; схлопывающийся пробел в конце строки «висит» (глифы не выдаются, в ширину не входит). Не реализовано: `pre-wrap`/`pre-line`/`break-spaces` (складываются в `normal`/`pre` в `style_adapt`), `text-transform`, `letter-spacing`/`word-spacing`, `text-indent`, bidi (только LTR), вертикальные writing modes, перенос по словарю для CJK/тайского (`parley/complex-scripts` выключен), собственные `line-height` вложенных инлайнов и `vertical-align` (высота строки = `line-height` контейнера), рамки/фон/padding инлайновых боксов. |
| css-flexbox-1 | flex | `cl-layout::flex` (taffy) | wpt/css/css-flexbox | todo | |
| css-grid-2 | grid | `cl-layout::grid` (taffy) | wpt/css/css-grid | todo | |
| css-position-3 | positioned layout | `cl-layout::positioned` | wpt/css/css-position | todo | |
| css-transforms-2, css-color-4, filter-effects-1 | paint | `cl-paint` | reftests, wpt/css/css-transforms | todo | |
| CSS2 Appendix E (порядок отрисовки), css-backgrounds-3 §2.11.2 (фон канвы) | построение display list | `cl-paint::build` | `crates/paint/tests/build.rs`, `display_list_goldens.rs`, `crates/testshell/tests/reftests.rs` (`border-solid-four-sides`, `background-body-propagation`, `display-inline-run`, `overflow-hidden-clip` и др.) | partial | M1a: только фон + сплошные рамки + текст + клип `overflow:hidden`; фон/рамки инлайновых элементов не рисуются; нет border-radius/изображений/теней. |
| html.spec.whatwg.org/multipage/rendering.html | user-agent stylesheet | `cl-style` (`crates/style/assets/ua.css`) | `crates/testshell/tests/reftests.rs` (весь набор рендерится через UA-лист); нет отдельного unit-теста на сам `ua.css` | deviation | `<hr>` (HTML §15.3.11): рендерится как `color:gray; border-style:solid; border-width:1px; margin:0.5em auto; height:0`. Спека предлагает `border-style:inset` — M1a paint понимает только `solid`/`none` (`crates/paint/src/build.rs`), поэтому `solid` — осознанная замена `inset` (выглядит площе/плоше, но геометрия та же). Ни один golden/reftest не использует `<hr>`. |
| html.spec.whatwg.org#event-loops | event loop | `cl-js::event_loop` | wpt/html/webappapis/scripting | todo | |
| webidl.spec.whatwg.org | bindings semantics | `cl-bindings` | wpt/WebIDL, wpt/dom/idlharness | todo | |
| fetch.spec.whatwg.org#fetching | fetch algorithm, CORS | `cl-net::fetch` | wpt/fetch, wpt/cors | todo | |
| RFC 9111 | HTTP cache | `cl-net::cache` | unit + wpt/fetch/http-cache | todo | |
| RFC 6265bis | cookies | `cl-net::cookies` | wpt/cookies | todo | |
| storage.spec.whatwg.org | storage keys, quota | `cl-storage` | wpt/storage | todo | |
| html #webstorage | localStorage/sessionStorage | `cl-webapi::storage` | wpt/webstorage | todo | |
| w3c IndexedDB 3 | IndexedDB | `cl-storage::idb`, `cl-webapi::idb` | wpt/IndexedDB | todo | |
| webappsec-csp | CSP L3 | `cl-net::csp`, `cl-webapi::csp` | wpt/content-security-policy | todo | |
| html #origin, #site | origin/site model | `cl-browser::site` | wpt/html/browsers/origin | todo | |
| html #cross-origin-opener-policies, #coep | COOP/COEP | `cl-browser::navigation`, `cl-net` | wpt/html/cross-origin-opener-policy, cross-origin-embedder-policy | todo | |
| wai-aria-1.2, html-aam | accessibility mapping | `cl-a11y` | wpt/accname, manual | todo | |
| ecma-262 / ecma-402 | ECMAScript | V8 via `cl-js` | test262 qualification set | todo | V8 semantics = Chrome |
| wasm core / js-api | WebAssembly | V8 via `cl-js` | wpt/wasm | todo | |
| CDP (Chromium) | DevTools protocol subset | `cl-devtools` | integration | todo | tip-of-tree drift — pin version |
| sync.proto (Chromium) | sync client/server | `cl-sync`, `cl-sync-server` | integration | todo | protocol snapshot pinned per Chromium release |
| MV3 (Chromium) | extensions runtime | `cl-extensions` | integration + curated extension corpus | todo | Chrome-specific; document unsupported APIs |
| CRX3 (Chromium) | extension package verification | `cl-extensions::crx` | unit + fuzz | todo | |

Добавляй строки в PR вместе с фичей. Строки с `deviation` обязаны иметь ссылку на WPT-тест, который мы намеренно fail-им, и bug ID.
