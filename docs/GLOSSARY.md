# Глоссарий

| Термин | Значение в проекте |
|---|---|
| **Browser process** | Единственный привилегированный процесс: UI, навигация, профили, broker. Не парсит недоверенные байты. |
| **Renderer process** | Sandboxed процесс с движком + V8 для одного **site**. Считается враждебным. |
| **Site** | `scheme + eTLD+1` (registrable domain). Единица изоляции процессов. Крупнее origin. |
| **Origin** | `scheme + host + port`. Единица Same-Origin Policy. |
| **StorageKey** | `(origin, top-level site, ancestor-bit)` — ключ всех хранилищ; реализует partitioning. |
| **SiteInstance** | Привязка документа к renderer-процессу в browser process. |
| **OOPIF** | Out-of-process iframe: cross-site фрейм в другом renderer. M4. |
| **Capability handle** | Непередаваемый токен от broker-а, дающий renderer-у право на одну конкретную операцию (fetch, storage area…). |
| **Broker** | Часть `cl-ipc` в browser process, выдающая handles и валидирующая запросы. |
| **Sandbox<Applied>** | Type-state: renderer main запускается только с применённой sandbox-политикой. |
| **Fetch** | Алгоритм WHATWG Fetch; реализован в network process. Не HTTP-клиент, а policy engine над ним. |
| **ORB** | Opaque Response Blocking: фильтрация тел `no-cors` ответов до попадания в renderer. |
| **Display list** | Наш формат paint-команд из renderer в GPU process. Валидируется получателем. |
| **Property trees** | transform/clip/effect/scroll деревья в compositor; позволяют async scroll/animation без layout. |
| **Fragment tree** | Иммутабельный результат layout-прохода (LayoutNG-стиль). |
| **Isolate / Context** | V8: isolate — heap+VM на поток; context — глобальный объект одного `Window`. Один isolate на renderer main thread, context на документ. |
| **Web IDL** | DSL описания DOM API; `cl-bindings` генерирует Rust↔V8 glue. Рукописные bindings запрещены. |
| **Freeze / Discard / Hibernate** | Три уровня экономии памяти фоновой вкладки: JS остановлен; renderer убит, вкладка в UI; состояние сериализовано на диск и восстановимо. |
| **MV3** | Chrome Extensions Manifest V3. Единственный поддерживаемый формат расширений. |
| **CRX3** | Формат пакета расширений Chrome с подписью. |
| **CDP** | Chrome DevTools Protocol. Мы реализуем подмножество для DevTools frontend. |
| **WebDriver BiDi** | Стандартный протокол автоматизации; для E2E-тестов. |
| **sync.proto** | Протокол Chromium Sync; наш клиент+сервер говорят на нём. Не связано с Google-аккаунтом. |
| **App-Bound Encryption (ABE)** | Шифрование cookies/паролей Chrome на Windows с Chrome 127, привязанное к Chrome. Блокирует сторонний импорт. |
| **WPT** | web-platform-tests — кросс-браузерный набор тестов веб-платформы. |
| **Test262** | Conformance-набор ECMAScript. |
| **Reftest** | Тест сравнения рендера тестового и эталонного документа. |
| **Golden test** | Снимок структуры (parse tree, style, fragments) через `insta`. |
| **M0–M6** | Milestones. См. FEATURE_MATRIX; детали — план. |
| **Gate S0/S1/S2** | Security-гейты из SECURITY.md §5. |
| **`M1-ONLY`** | Маркер временного кода, удаляемого при закрытии milestone. |
| **`SPEC-DEVIATION`** | Маркер намеренного отклонения от спецификации со ссылкой на реестр. |
