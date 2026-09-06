# ADR-0013: Кодовое имя `chrome-light` и торговая марка

**Status:** Accepted
**Date:** 2026-09-07
**Deciders:** владелец проекта

## Context

Директория и рабочее имя — `chrome-light`. «Chrome» и «Google Chrome» — зарегистрированные торговые марки Google. Продукт, имитирующий Chrome по имени, создаёт риск претензий и вводит пользователей в заблуждение (особенно при импорте профиля и «sync»).

## Decision

- Имя продукта: **ChromeLight** (решение владельца 2026-09-07). Репозиторий/workspace — `chrome-light`, бинарник `chromelight`, схема `chromelight://`, префикс crate-ов `cl-`.
- **Риск принят владельцем:** «Chrome» — торговая марка Google; имя ChromeLight может получить претензию (trademark dilution/confusion). Смягчение: в UI/документации — «ChromeLight is not affiliated with Google»; никаких логотипов/цветов Chrome; формулировки «импорт данных из Google Chrome», «совместимость с расширениями Chrome Web Store». `tools/rename-checklist.md` (M5) остаётся на случай вынужденного переименования.
- В UI и документации никогда не утверждать «аналог Chrome» или «синхронизация с Chrome».

## Consequences

- Легче: разработка без блокировки на нейминг.
- Труднее: переименование перед релизом — рутинная, но обязательная работа.

## Action Items

1. [x] Владелец выбрал ChromeLight.
2. [ ] `tools/rename-checklist.md` (M5) — на случай претензии.
3. [ ] Disclaimer «not affiliated with Google» в About и README до публичного релиза.
