# Стандарты кода (Rust)

База — Apollo Rust Best Practices (skill `rust-best-practices`) + специфика браузера. Правила ниже — обязательные; исключения — через `#[expect(lint, reason = "…")]`, не `#[allow]`.

## 1. Ownership и типы

- Параметры функций: `&str`, `&[T]`, `&Path`, `impl AsRef<…>`; `String`/`Vec<T>`/`PathBuf` — только при передаче владения.
- `Copy`-типы ≤ 24 байт — по значению. Всё крупнее — по ссылке.
- `Cow<'_, str>` там, где владение зависит от входа (нормализация URL, декодирование entity).
- `.clone()` в горячем пути (parser loop, style resolution, layout, paint) — только с комментарием `// clone: <why>`.
- Никаких `Rc<RefCell<…>>` в DOM/layout: DOM — arena (`NodeId` индексы в `Vec`/slab), layout tree — immutable fragment tree на проход. Ссылки между деревьями — индексы, не указатели.
- Newtypes для всех ID и единиц: `NodeId`, `SiteId`, `Px(f32)`, `Au(i32)` (app units для layout), `OriginKey`. Голый `u32`/`f32` в публичном API — reject.
- `Send`/`Sync`: всё, что пересекает поток, — явно `Send`. DOM-объекты `!Send` (main thread renderer); их индексы `Send`.

## 2. Ошибки

- Библиотечные crate-ы: `thiserror`, иерархия per crate (`cl_net::Error`, `cl_dom::Error`), с `#[source]`.
- Бинарники (`apps/`, `cl-sync-server`, `tools/`): `anyhow` допустим.
- **Недоверенный вход никогда не паникует.** `unwrap`/`expect`/`indexing[]` на данных из сети/страницы/IPC/файла профиля — bug класса security. Использовать `.get()`, `checked_*`, `try_from`.
- `unwrap()`/`expect()` разрешены только в `#[cfg(test)]`, `build.rs`, и для инвариантов, доказанных строкой выше, с `expect("invariant: …")`.
- `Result` пробрасывать `?`; не `match` ради переупаковки.
- Panic policy: `panic = "abort"` в release для child-процессов (crash → чистый dump, без unwinding через FFI).

## 3. Clippy и lints

Workspace `Cargo.toml`:

```toml
[workspace.lints.rust]
unsafe_code = "deny"            # forbid нельзя переопределить; каждый не-FFI crate добавляет #![forbid(unsafe_code)] в lib.rs
missing_docs = "warn"
unused_must_use = "deny"
rust_2018_idioms = "warn"

[workspace.lints.clippy]
all = "warn"
pedantic = "warn"
perf = "deny"
unwrap_used = "deny"
expect_used = "warn"
indexing_slicing = "warn"       # deny в cl-net, cl-html, cl-ipc, cl-bindings
panic = "deny"
todo = "deny"
dbg_macro = "deny"
print_stdout = "deny"           # логирование только через tracing
large_enum_variant = "warn"
redundant_clone = "warn"
needless_collect = "warn"
module_name_repetitions = "allow"
must_use_candidate = "allow"
```

`clippy.toml`: `too-many-arguments-threshold = 8`, `type-complexity-threshold = 300`, `cognitive-complexity-threshold = 30`, `disallowed-methods` для `std::process::exit` вне `apps/`, `std::env::var` вне `cl-platform`.

## 4. `unsafe`

Разрешён **только** в: `cl-platform`, `cl-process`, `cl-gfx` (backend модули), `cl-js` (V8 FFI), `cl-bindings/runtime`. Там: `#![deny(unsafe_code)]` на уровне crate + `#[allow(unsafe_code)]` на конкретном модуле; `#![deny(unsafe_op_in_unsafe_fn)]`; `#![deny(clippy::undocumented_unsafe_blocks)]`.

Каждый блок:

```rust
// SAFETY: `ptr` получен из `v8::Local` в текущем HandleScope, живёт до конца scope;
// мы не сохраняем его дольше (см. lifetime 's).
unsafe { ... }
```

Все `unsafe` изменения — review чек-лист в PR-шаблоне; `cargo miri` для чистых unsafe-модулей без FFI; `cargo +nightly careful` в nightly CI.

## 5. Производительность

- Профилировать до оптимизации: `samply`/Instruments на macOS, `perf` на Linux, ETW на Windows. Цифры — в коммит.
- Итераторы вместо индексных циклов; без промежуточных `collect()`.
- Аллокации в горячем пути — `SmallVec`, арены (`bumpalo`) для per-pass данных (layout pass, paint pass), `Box<str>` вместо `String` для иммутабельных строк в DOM.
- `Box` большие варианты enum-ов (`large_enum_variant`).
- Строки DOM: атомы (`string_cache`/`markup5ever` `Atom`) для тегов/атрибутов; текстовые узлы — `Tendril`/`Box<str>`.
- Не оптимизировать до M3 ничего, что не показано профилем. Reference-path сохранять для differential-тестов.

## 6. Generics и dispatch

- Статический dispatch в движке. `dyn Trait` — только на границах: `JsRuntime`, `GfxBackend`, `StorageBackend`, `SandboxPolicy`, `PlatformFs`.
- Не боксовать внутри crate-а «для удобства»; боксовать на API-границе.
- Type-state для протоколов: `IpcConnection<Handshaking>` → `IpcConnection<Ready>`; `Fetch<Pending>` → `Fetch<Redirected>` → `Fetch<Done>`; `Sandbox<Unapplied>` → `Sandbox<Applied>` (renderer main не запускается без `Applied`).

## 7. Документация и комментарии

- `///` на каждом pub item: что, инварианты, паника-контракт («никогда не паникует на любом входе»), ссылка на спецификацию: `/// Implements <https://html.spec.whatwg.org/#tokenization> §13.2.5.1`.
- `//` — только *почему*: workaround, security-обоснование, отклонение от спеки со ссылкой на WPT/issue.
- `// TODO(#123): …` — только с issue. `todo!()` запрещён (clippy deny).
- `// SPEC-DEVIATION(<spec>#<anchor>): <reason>; tracked in SPEC_REGISTRY` — обязательный маркер для любого отклонения.
- `// M1-ONLY:` — код, который допустим только до указанного milestone; grep-гейт в CI при закрытии milestone.

## 8. Тесты

- Имена: `tokenizer_should_emit_eof_when_input_empty`. Одно утверждение на тест где возможно.
- Golden/snapshot — `insta` (`cargo insta review`): parse trees, computed style, fragment trees, display lists.
- Property-тесты — `proptest` для URL, cookies, cache keys, IPC (де)сериализации.
- Fuzz — `cargo-fuzz` targets в `tools/fuzz/`, `arbitrary` для структурированных входов.
- Никаких сетевых тестов без локального сервера (`tools/testserver`).
- Флак — баг. Тест с `sleep` — reject; использовать детерминированный clock из `cl-platform::Clock`.

## 9. Стиль

- `rustfmt.toml`: `edition = "2024"`, `max_width = 100`, `imports_granularity = "Crate"`, `group_imports = "StdExternalCrate"`.
- Модули: один концепт — один файл; `mod.rs` не используем (`foo.rs` + `foo/`).
- Публичный API crate-а — в `lib.rs` через `pub use`, внутренности `pub(crate)`.
- Feature flags — только для необязательных Web API (`webapi/canvas`, `webapi/workers`), не для платформ.

## 10. Зависимости

- Добавление — `cargo deny check` + строка в `docs/DEPENDENCIES.md` (crate, версия, лицензия, зачем, альтернативы).
- Предпочитать crate-ы с: >1 мейнтейнер, релиз за последний год, без `unsafe` либо с аудитом (`cargo vet`/RustSec).
- Версии закреплены в `[workspace.dependencies]`; `Cargo.lock` коммитится; обновление — отдельный PR с changelog-ссылками.
- Запрещены: `openssl` (rustls), `native-tls`, `reqwest` в движке (свой Fetch), любые crate-ы с сетевыми build-скриптами кроме `v8` (prebuilt download с checksum).
