# ADR-0015: Интеграция stylo — unsafe в cl-style, Python 3 на сборке, pinned версия

**Status:** Accepted
**Date:** 2026-09-07
**Deciders:** владелец проекта
**Amends:** ADR-0002 (список unsafe-crates), ADR-0010 (action item 1 — WPT adapter → M2)

## Context
ADR-0003 выбрал stylo как CSS-движок. Разведка (2026-09-07): stylo 0.20 требует `python3` в `build.rs` (Mako-кодогенерация свойств); embedder реализует `style::dom::{TNode,TElement,...}` на `Copy`-хэндлах, а `TElement::{ensure_data, clear_data, set_dirty_descendants, ...}` — `unsafe fn`, хранилище `ElementData` требует interior mutability; Blitz (референс) держит сырой `*mut` на арену (открытая проблема DioxusLabs/blitz#151). Владелец подтвердил stylo с первого дня.

## Decision
1. `cl-style` — пятый crate с разрешённым `unsafe`, только в модулях `store.rs`, `handle.rs`, `stylo_dom.rs`; хэндл — `Copy`-тройка `(&Document, &StyleStore, NodeId)`, не сырой указатель; `ElementData` — side table на один проход стиля (`Box<[UnsafeCell<Option<ElementData>>]>`), пересоздаётся каждый `resolve()`; traversal только последовательный (`traverse_dom(.., None)`); `debug_assert` проверки потока в каждом `unsafe` входе.
2. `servo_arc`/`style::*` не покидают `crates/style/**` и `crates/layout/src/style_adapt.rs` (скрипт `tools/check-stylo-scope.sh`).
3. Python 3 ≥ 3.10 — build-зависимость; CI ставит `actions/setup-python@v5` во всех job-ах.
4. Версии закреплены в `[workspace.dependencies]`: stylo 0.20.0, stylo_traits 0.20.0, stylo_dom 0.20.0, stylo_atoms 0.20.0, selectors 0.40, cssparser 0.37, html5ever/markup5ever 0.39 (одна версия markup5ever в дереве — проверка `cargo tree -d`). Обновление — только целиком, отдельным PR.
5. `cargo miri` для cl-style невозможен (build.rs/кодоген); вместо него nightly `cargo careful test -p cl-style` + `careful` в CI nightly.
6. **Option B (fallback):** если задача интеграции трейтов не проходит тест `p_with_color_rule_should_resolve_to_red` после 3 попыток / 5 рабочих дней — минимальный собственный cascade за тем же API `cl_style::{StyleEngine, StyledDocument, computed()}` (cssparser + selectors, ~20 longhand-свойств, без @media/custom properties); stylo возвращается в M2. Решение фиксируется здесь как «Option B taken» с датой.

## Consequences
- Легче: Firefox-grade cascade, selectors, @media, custom properties бесплатно.
- Труднее: время сборки, Python в CI, unsafe-review для трёх модулей, Windows-сборка stylo.
- Пересмотреть: при смене мажорной версии stylo или если Blitz#151 даст safe-паттерн — перейти на него.

## Amendment 2026-09-09 (по итогам Task 11)

Пункт 1 Decision исходил из того, что per-node хранилище `ElementData` придётся строить как
`Box<[UnsafeCell<Option<ElementData>>]>`, потому что stylo вызывает `unsafe fn ensure_data(&self)`
через `Copy`-хэндл. **Это оказалось неверно и, более того, неосуществимо** против stylo 0.20:
`ElementDataMut`/`ElementDataRef` имеют приватные поля и создаются исключительно
`ElementDataWrapper::{borrow, borrow_mut}` (`stylo-0.20.0/style/data.rs`), а сам `ElementDataWrapper`
уже содержит нужную interior mutability (плюс debug-трекер `AtomicRefCell`). Собственный `UnsafeCell`
здесь не только лишний — его нечем наполнить.

Фактическая реализация: `StyleStore` — безопасные `Cell`-ы и один `ElementDataWrapper` на узел.
**В продакшн-коде нет ни одного блока `unsafe`.** `store.rs` и `handle.rs` сохранили
`#![deny(unsafe_code)]`; исключение `#![allow(unsafe_code)]` нужно только в `stylo_dom.rs`, и только
потому, что *реализация* пяти `unsafe fn`-методов трейта `TElement` сама по себе считается
`unsafe_code` — тел с `unsafe { }` нет. Инвариант одного потока сохранён как `debug_assert_eq!` по
владеющему потоку на каждой мутирующей точке входа, `StyleStore` не `Sync`.

Что это меняет:
- Пункт 1 Decision читать как «хранилище per-pass, доступ только через API stylo; `unsafe` разрешён,
  но по факту не понадобился».
- Пункт 5 (невозможность `cargo miri`) остаётся в силе из-за `build.rs`-кодогенерации stylo, но
  ценность miri для `cl-style` теперь околонулевая: проверять нечего.
- Option B (пункт 6) **не активирован**: гейт `p_with_color_rule_should_resolve_to_red` ещё впереди
  (Task 13), но трейты, матчинг селекторов и хранилище работают.
