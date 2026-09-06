# ADR-0011: Расширения — только MV3, свой runtime `chrome.*`, установка из Chrome Web Store

**Status:** Accepted
**Date:** 2026-09-07
**Deciders:** владелец проекта

## Context

MV2 отключён в Chrome 138 (июль 2025), Web Store очищен 2026-08-31. MV3: declarative permissions, background service worker, content scripts в isolated world, `declarativeNetRequest`, запрет remote code. CRX3 и Web Store update URL публичны — Chromium-форки устанавливают оттуда.

## Decision

- Поддерживаем **только MV3**. MV2 — non-goal.
- `cl-extensions`: manifest parser + permission model, CRX3 verify (RSA/ECDSA подпись + id = SHA-256 публичного ключа), установка по Web Store update URL, background service worker (наш SW runtime, M5+), content scripts в isolated world (отдельный V8 context в renderer сайта, capability только на DOM), `chrome.*` API host-side в browser process с валидацией каждого вызова против grant, `declarativeNetRequest` в network process, extension pages в отдельном extension renderer.
- Порядок API: `runtime`, `storage`, `tabs`, `scripting`, `declarativeNetRequest`, `action`, `contextMenus`, `webNavigation`, `cookies` (с permission), `alarms`, `notifications`. Остальное — по реестру; неподдерживаемые API возвращают `undefined` + warning в консоль расширения (как Firefox для Chrome-only API).
- Тест-корпус: uBlock Origin Lite, Bitwarden, Dark Reader, Vimium, React DevTools — приёмка M6.

## Options Considered

- **WebExtensions cross-vendor подмножество** — MV3 Chrome — де-факто стандарт; делаем Chrome-семантику.
- **Свой формат расширений** — ноль экосистемы.

## Consequences

- Легче: пользователь ставит свои расширения; список из Chrome-профиля переустанавливается автоматически (ADR-0007).
- Труднее: `chrome.*` API огромен; каждая API — граница безопасности.
- Пересмотреть когда: Google изменит формат/подпись CRX или закроет update URL для сторонних → альтернатива: sideload + подпись.

## Action Items

1. [ ] Реестр `chrome.*` API со статусом в SPEC_REGISTRY (M6).
2. [ ] CRX3 парсер + fuzz (M6).
