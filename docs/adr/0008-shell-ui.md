# ADR-0008: Shell UI — egui сейчас, privileged web UI на своём движке позже

**Status:** Accepted (revisit at M5)
**Date:** 2026-09-07
**Deciders:** владелец проекта

## Context

Browser chrome (tab strip, omnibox, диалоги, settings) нужен рано для dogfooding, но движок в M1–M3 не готов рендерить свой UI. Firefox рендерит UI своим движком (privileged documents); Chrome — Views (C++). Rust GUI 2026: egui (immediate, wgpu, быстро), iced, Slint (DSL, коммерческая), Xilem (не production).

## Decision

- **M1–M4:** `cl-shell-ui` на **egui** (`egui-wgpu`, `egui-winit`), тот же wgpu-device, что compositor; UI в browser process. Минимум: tab strip, omnibox с честным origin/security state, диалоги permissions/cert, settings, downloads.
- **M5+:** миграция на **privileged web UI**: HTML/CSS/JS страницы `chrome-light://` в отдельном privileged renderer (не sandbox-escape: отдельный процесс с capability на UI-API, строго изолированный от web-контента; никогда не в browser process). Dogfooding движка + l10n/a11y бесплатно из веб-платформы.
- Omnibox security UI — всегда в browser process, даже после миграции (защита от spoofing).

## Options Considered

- **Сразу privileged web UI** — блокирует shell до M4+.
- **Native toolkit per OS (AppKit/WinUI/GTK)** — тройная работа, противоречит соло.
- **iced/Slint** — сопоставимы; egui выбран за скорость итерации и wgpu-интеграцию; лицензия Slint — минус.

## Consequences

- Легче: рабочий браузер для dogfooding в M1.
- Труднее: две реализации UI за жизнь проекта; egui выглядит «не нативно» — приемлемо для альфы.
- Пересмотреть на M5 по готовности движка (forms, focus, a11y) — решение о миграции.

## Action Items

1. [ ] `cl-shell-ui` skeleton на egui (M1).
2. [ ] Omnibox origin display по спецификации URL display guidelines (M2).
