# Архитектура chrome-light

Версия документа: 2026-09-07. Источник решений — `docs/adr/`. Здесь — связная картина.

## 1. Границы и терминология

- **Продукт (browser)** — окна, вкладки, omnibox, профили, история/закладки/пароли, загрузки, разрешения, sync, импорт из Chrome, расширения, DevTools, обновления.
- **Движок (engine)** — всё от URL до пикселей: сеть, HTML/DOM, CSS/style, layout, paint, compositor, GPU, Web IDL bindings, Web API.
- **JS/Wasm VM** — V8 (crate `v8`), обёрнут `cl-js`. DOM не часть VM: bindings принадлежат движку.
- **Платформенные сервисы** — ОС-абстракция, процессы, sandbox, IPC, хранилище, accessibility.

Правило: Chromium ≠ Blink ≠ V8. У нас: chrome-light ≠ cl-engine ≠ V8.

## 2. Процессная модель (ADR-0005)

```
┌──────────────────────────────── browser process (privileged) ────────────────────────────────┐
│ cl-browser  cl-shell-ui  cl-storage  cl-chrome-import  cl-sync  cl-extensions(host)  cl-devtools │
│ cl-process (spawn, sandbox policy, crash recovery)   cl-ipc (broker, capability handles)         │
└───────┬─────────────────────┬──────────────────────────┬──────────────────────┬────────────────┘
        │                     │                          │                      │
 ┌──────▼──────┐       ┌──────▼──────┐            ┌──────▼──────┐        ┌──────▼──────┐
 │ renderer    │  ...  │ renderer    │            │ network     │        │ gpu         │
 │ site A      │       │ site B      │            │ cl-net      │        │ cl-gfx      │
 │ (sandboxed) │       │ (sandboxed) │            │ (sandboxed, │        │ cl-compositor│
 │ cl-renderer │       │             │            │  weaker)    │        │ (sandboxed, │
 └─────────────┘       └─────────────┘            └─────────────┘        │  weaker)    │
        utility processes: image decode, font parse, media demux/decode  └─────────────┘
```

**Принципы:**

1. Browser process не парсит недоверенные байты. Никогда. Даже favicon декодируется в utility process.
2. Renderer — один на **site** (scheme + eTLD+1) в рамках профиля; cross-site iframe — out-of-process (OOPIF) начиная с M4. До M4 cross-site iframe рендерится в том же процессе, но это документированное ограничение и блокер публичной беты.
3. Renderer не имеет: сокетов, файлов, keychain, clipboard-write без user gesture, доступа к другим renderer-ам. Всё через capability handles от broker-а.
4. Network process держит TLS-ключи сессий, cookies, кэш. Renderer получает только `FetchHandle` на конкретный запрос с уже применёнными CORS/CSP/cookie-политиками, проверенными в browser process.
5. GPU process получает display lists / command buffers через shared memory; валидирует всё; падение GPU process → перезапуск без потери вкладок.
6. Любой child может упасть: browser process показывает «страница упала», перезапускает. Crash dump без чувствительных данных (ADR-0005 §crash).

**Sandbox по платформам** (cl-process):

| ОС | Механизм | Заметки |
|---|---|---|
| macOS | `sandbox_init` (seatbelt profiles, SBPL) + отдельный user-less процесс, entitlements | профили per process type в `cl-process/sandbox/macos/*.sb` |
| Linux | user+pid+net namespaces, seccomp-bpf allowlist, `no_new_privs`, chroot-в-пустоту | fallback без user-ns → отказ загружать untrusted content, не «тихий» режим |
| Windows | restricted token + job object + AppContainer + Win32k lockdown (`ProcessSystemCallDisablePolicy`) | самая сложная; отдельный owner-milestone |

## 3. IPC (cl-ipc)

- Транспорт: `ipc-channel` (unix domain sockets / named pipes) для сообщений; shared memory (`cl-platform::shm`) для frames, display lists, больших ресурсов.
- Схема: сообщения — Rust enum-ы с `serde` + `postcard`; **каждый** тип имеет `validate(&self, ctx: &ReceiverCtx) -> Result<(), IpcViolation>`; версия протокола в handshake.
- Capabilities: непередаваемые `Handle<T>` выдаются broker-ом; renderer не может «сконструировать» handle. Handle содержит origin/site и срок жизни.
- Только async. Единственный допустимый sync-канал — renderer→browser для `window.alert`-класса модальных операций, и тот с таймаутом.
- IPC decoders — fuzz-target с первого дня (`fuzz/ipc_*`).

## 4. Pipeline документа (renderer process)

```
FetchHandle bytes ──► encoding sniff ──► cl-html (html5ever tokenizer/tree builder)
        │                                           │ sink
        │                                           ▼
        │                                        cl-dom (Node arena, events, shadow DOM, mutation records)
        │                                           ▲ ▼ generated bindings (cl-bindings, Web IDL → V8 glue)
        │                                        cl-js (V8 isolate per renderer, context per Window, event loop, tasks/microtasks)
        │                                           │
        ▼                                           ▼
   cl-style: stylo (cascade, computed values, invalidation) ──► ComputedStyle per element
                                                    │
                                                    ▼
   cl-layout: box tree → fragment tree (immutable per pass, LayoutNG-style)
              block/inline (own), flex/grid math (taffy), tables (own), text (parley/swash), fragmentation later
                                                    │
                                                    ▼
   cl-paint: display list (own format), stacking contexts, clips, transforms, effects
                                                    │ commit (shared memory)
                                                    ▼
   ─────────────── gpu process ───────────────
   cl-compositor: property trees (transform/clip/effect/scroll), layerization, tiles, damage, async scroll/animation
   cl-gfx: vello scene → wgpu; CPU fallback (vello_cpu/tiny-skia) для headless/reftests/без GPU
                                                    │
                                                    ▼
   present → окно (winit surface) / PNG (testshell)
```

**Инвалидация:** style → layout → paint → raster помечают только затронутые поддеревья. Полная перерисовка допустима только в M1 и помечена `// M1-ONLY: full relayout`.

**Event loop (cl-js + cl-dom):** реализация HTML §event loop: task sources, microtask checkpoint, rendering opportunity, `requestAnimationFrame`, timers с throttling для background-вкладок. Workers — отдельные threads с собственными isolate-ами внутри того же renderer.

## 5. Сеть (network process, cl-net)

- URL: crate `url` (WHATWG). DNS: свой async resolver поверх `hickory-resolver` (см. DEPENDENCIES). TLS: `rustls` + `rustls-platform-verifier` (системные root stores). HTTP/1.1, HTTP/2: `hyper`. HTTP/3: `quinn` + `h3`.
- Fetch (WHATWG): реализуется **в network process** как state machine: redirects, CORS (включая preflight), credentials mode, referrer policy, CSP `connect-src`, mixed content, service worker (позже), ORB-подобная фильтрация тел для `no-cors`.
- HTTP cache: RFC 9111, диск (`cl-storage` cache backend) + память; ключ включает top-level site (partitioning).
- Cookies: RFC 6265bis: `Secure`, `HttpOnly`, `SameSite`, `__Host-`/`__Secure-` префиксы, CHIPS `Partitioned`. Хранилище — SQLite в profile dir, зашифрованное платформенным ключом.
- Downloads: в browser process (политика, UI), байты — через network process в файл с quarantine attribute (macOS `com.apple.quarantine`, Windows MOTW).

## 6. Хранилище (cl-storage)

Единый `StorageKey = (origin, top-level site, ancestor-bit)` как в WHATWG Storage. Backends:

| Данные | Backend | Процесс |
|---|---|---|
| history, bookmarks, prefs, permissions, site data index | SQLite | browser |
| cookies, HTTP cache index | SQLite | network |
| localStorage | SQLite (per StorageKey, async commit; sync API в renderer через локальный кэш + IPC) | browser (storage service) |
| sessionStorage | память browser process, namespace per tab | browser |
| IndexedDB | SQLite (одна БД на StorageKey), транзакции — RFC-style журнал | browser (storage service) |
| Cache Storage | файлы + SQLite index | browser |
| OPFS | директории в profile dir с квотой | browser |

Квоты и eviction — по WHATWG Storage. Private mode — memory-only backends того же интерфейса.

## 7. Продукт (browser process)

- **cl-browser:** `Tab`, `NavigationController` (history entries, back/forward, bfcache — позже), `SiteInstance` assignment, permissions (`Permission` state machine + UI prompts), downloads, session restore, crash recovery, memory pressure policy (freeze → discard → hibernate-to-disk, ADR-0012).
- **cl-shell-ui:** egui на wgpu: tab strip, omnibox (с точным отображением origin, security state), диалоги, settings. Позже — privileged web UI на своём движке (ADR-0008).
- **cl-chrome-import (ADR-0007):** читает профиль Chrome на устройстве (read-only): `Bookmarks` (JSON), `History` (SQLite, копия под lock), `Login Data` (SQLite + расшифровка: macOS Keychain «Chrome Safe Storage», Linux libsecret/kwallet/basic, Windows DPAPI для legacy `v10`, а `v20` App-Bound — только через пользовательский экспорт CSV), `Preferences`, `Extensions/` (манифесты + CRX id → переустановка из Web Store), `Web Data` (autofill). Режим «зеркало»: file watcher + периодический diff, только Chrome → нам.
- **cl-sync (ADR-0007):** клиент протокола Chromium sync (`components/sync/protocol/*.proto` → `prost`), типы: bookmarks, history, passwords, preferences, tabs, extensions. Сервер `cl-sync-server` (axum + SQLite/Postgres), self-hosted; шифрование — passphrase-derived key на клиенте (как Brave/custom passphrase в Chrome).
- **cl-extensions (ADR-0011):** MV3: manifest parse, permissions model, service worker background, content scripts в isolated world (отдельный V8 context в renderer), `chrome.*` API host-side в browser process с валидацией; declarativeNetRequest в network process; CRX3 verify + установка из Chrome Web Store update URL.
- **cl-devtools:** CDP-совместимый сервер (домены Runtime, Debugger через V8 Inspector, DOM, CSS, Network, Page, Log, Target). Frontend — Chrome DevTools frontend (BSD) как отдельная загрузка либо собственный минимальный.
- **updater:** подписанные пакеты (ed25519 + code signing ОС), staged rollout, rollback. Без него — нет публичной беты.

## 8. Карта crate-ов

```
Cargo.toml (workspace, [workspace.lints], [workspace.dependencies])
crates/
  platform/      cl-platform     threads, clocks, files, shm, keychain, quarantine, fonts discovery (unsafe allowed)
  process/       cl-process      spawn, sandbox policies, crash handling (unsafe allowed)
  ipc/           cl-ipc          schema, validation, capability broker
  net/           cl-net          fetch state machine, http stack, cache, cookies
  html/          cl-html         html5ever integration → DOM sink, encoding sniffing
  dom/           cl-dom          nodes, events, ranges, shadow DOM, mutation, custom elements
  style/         cl-style        stylo integration, invalidation, computed style access
  layout/        cl-layout       box/fragment trees, formatting contexts, text, tables
  paint/         cl-paint        display list format and builder, hit testing
  compositor/    cl-compositor   property trees, layers, tiles, damage, async scroll
  gfx/           cl-gfx          vello/wgpu backend, CPU backend, gpu process main (unsafe allowed)
  js/            cl-js           V8 embedding, JsRuntime trait, event loop (unsafe allowed)
  bindings/      cl-bindings     Web IDL parser + codegen (build-time), runtime glue
  webapi/        cl-webapi       Fetch API, Storage APIs, Workers, Canvas, timers… (feature-gated modules)
  storage/       cl-storage      SQLite backends, quota, StorageKey
  a11y/          cl-a11y         DOM → accesskit tree
  renderer/      cl-renderer     renderer process main
  browser/       cl-browser      browser process core
  shell-ui/      cl-shell-ui     egui chrome UI
  chrome-import/ cl-chrome-import
  sync/          cl-sync         sync client
  sync-server/   cl-sync-server  self-hosted server binary
  extensions/    cl-extensions   MV3 runtime
  devtools/      cl-devtools     CDP server
  testshell/     cl-testshell    headless deterministic shell (WPT product, reftests, PNG)
apps/
  chromelight/                   main binary (browser process entry; child processes — тот же бинарник с `--type=`)
tools/
  wpt/           wptrunner product adapter, expectations metadata
  test262/       harness
  bench/         memory/perf harness, corpus
  fuzz/          cargo-fuzz targets
  chrome-corpus/ curated site list for compat acceptance
docs/
```

Правило зависимостей между crate-ами (проверяется `cargo deny` bans + `tools/check-deps.sh`): `cl-dom` не зависит от `cl-layout`; `cl-layout` не зависит от `cl-js`; ничего в renderer не зависит от `cl-browser`; `cl-platform` — лист.

## 9. Бюджеты памяти (ADR-0012, гипотезы до измерений)

| Метрика | Цель v1 | Ориентир Chrome 140+ (из открытых источников, 2026) |
|---|---|---|
| Пустой браузер, 1 пустая вкладка | ≤ 120 МБ RSS суммарно по процессам | ~300–400 МБ |
| Типичная новостная страница, активная | ≤ 70 МБ на renderer | 150–300 МБ |
| 10 активных вкладок | ≤ 600 МБ суммарно | ~1.4 ГБ |
| Фоновая вкладка после freeze | ≤ 15 МБ (heap snapshot на диск, V8 isolate disposed) | «до 80% меньше» после discard |
| Время до первого кадра (cold start) | ≤ 400 мс на M1 | — |

Тактики: один isolate на renderer с lazy context; V8 flags для фона (`--lazy`, `--optimize-for-size`, no sparkplug/turbofan во фоне); shared glyph/image caches в GPU process; hibernate вкладки на диск (сериализация DOM+state); отсутствие лишних сервисных процессов (network in-process **запрещено** — но GPU и network — по одному на профиль, не на окно).

Измерение: `tools/bench` — фиксированный corpus, RSS/PSS по процессам, `cargo bench` + CI-гейт на регрессию >5%.

## 10. Observability

`tracing` во всех crate-ах; структурированные события; `tracing-chrome` экспорт для Perfetto UI; crash handling — minidump через `minidumper`/`crash-handler` (Rust), без содержимого страниц; feature flags через `cl-browser::flags` (compile-time + runtime toggles).

## 11. Что пересматриваем при росте

- OOPIF и Site Isolation granularity (origin vs site) — M4.
- Собственный JS VM — не раньше v2, только если V8 ограничивает бюджет памяти или sandbox.
- Privileged web UI вместо egui — M5.
- Service workers, bfcache, WebGPU, WebRTC — по feature matrix.
