# ADR-0008: Shell UI — egui now, privileged web UI on our own engine later

**Status:** Accepted (revisit at M5)
**Date:** 2026-09-07
**Deciders:** project owner

## Context

Browser chrome (tab strip, omnibox, dialogs, settings) is needed early for dogfooding, but in M1–M3 the engine is not ready to render its own UI. Firefox renders its UI with its own engine (privileged documents); Chrome — Views (C++). Rust GUI in 2026: egui (immediate, wgpu, fast), iced, Slint (DSL, commercial), Xilem (not production).

## Decision

- **M1–M4:** `cl-shell-ui` on **egui** (`egui-wgpu`, `egui-winit`), the same wgpu device as the compositor; UI in the browser process. Minimum: tab strip, omnibox with honest origin/security state, permissions/cert dialogs, settings, downloads.
- **M5+:** migration to a **privileged web UI**: HTML/CSS/JS pages at `chrome-light://` in a separate privileged renderer (not a sandbox escape: a separate process with a capability for the UI API, strictly isolated from web content; never in the browser process). Engine dogfooding + l10n/a11y for free from the web platform.
- Omnibox security UI — always in the browser process, even after the migration (spoofing protection).

## Options Considered

- **Privileged web UI right away** — blocks the shell until M4+.
- **Native toolkit per OS (AppKit/WinUI/GTK)** — triple the work, at odds with a solo project.
- **iced/Slint** — comparable; egui chosen for iteration speed and wgpu integration; Slint's license is a minus.

## Consequences

- Easier: a working browser for dogfooding in M1.
- Harder: two UI implementations over the project's lifetime; egui looks "non-native" — acceptable for an alpha.
- Revisit at M5 based on engine readiness (forms, focus, a11y) — decision on the migration.

## Action Items

1. [ ] `cl-shell-ui` skeleton on egui (M1).
2. [ ] Omnibox origin display per the URL display guidelines spec (M2).
