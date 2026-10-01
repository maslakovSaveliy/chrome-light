# ADR-0011: Extensions — MV3 only, own `chrome.*` runtime, installation from the Chrome Web Store

**Status:** Accepted
**Date:** 2026-09-07
**Deciders:** project owner

## Context

MV2 was disabled in Chrome 138 (Jul 2025); the Web Store was purged on 2026-08-31. MV3: declarative permissions, background service worker, content scripts in an isolated world, `declarativeNetRequest`, no remote code. CRX3 and the Web Store update URL are public — Chromium forks install from there.

## Decision

- We support **MV3 only**. MV2 — non-goal.
- `cl-extensions`: manifest parser + permission model, CRX3 verify (RSA/ECDSA signature + id = SHA-256 of the public key), installation via the Web Store update URL, background service worker (our SW runtime, M5+), content scripts in an isolated world (a separate V8 context in the site's renderer, capability on the DOM only), `chrome.*` API host-side in the browser process with every call validated against the grant, `declarativeNetRequest` in the network process, extension pages in a separate extension renderer.
- API order: `runtime`, `storage`, `tabs`, `scripting`, `declarativeNetRequest`, `action`, `contextMenus`, `webNavigation`, `cookies` (with permission), `alarms`, `notifications`. The rest — per the registry; unsupported APIs return `undefined` + a warning in the extension's console (as Firefox does for Chrome-only APIs).
- Test corpus: uBlock Origin Lite, Bitwarden, Dark Reader, Vimium, React DevTools — M6 acceptance.

## Options Considered

- **WebExtensions cross-vendor subset** — Chrome MV3 is the de facto standard; we implement Chrome semantics.
- **Our own extension format** — zero ecosystem.

## Consequences

- Easier: users install their own extensions; the list from the Chrome profile is reinstalled automatically (ADR-0007).
- Harder: the `chrome.*` API is huge; every API is a security boundary.
- Revisit when: Google changes the CRX format/signature or closes the update URL to third parties → alternative: sideload + signing.

## Action Items

1. [ ] Registry of `chrome.*` APIs with status in SPEC_REGISTRY (M6).
2. [ ] CRX3 parser + fuzz (M6).
