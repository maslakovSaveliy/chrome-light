# ADR-0007: Chrome interop — import/mirror of the local profile and own sync over `sync.proto`

**Status:** Accepted
**Date:** 2026-09-07
**Deciders:** project owner

## Context

Requirement: "full sync with Chrome on the device". Facts (RESEARCH §3): Google closed the Chrome Sync API to third-party builds on 2021-03-15 — there is no legal path. The `sync.proto` protocol is open; Brave `go-sync` implements a server. The local Chrome profile is readable: Bookmarks/History/Login Data/Preferences/Extensions; passwords and cookies are encrypted with platform mechanisms; on Windows since Chrome 127 — App-Bound Encryption (v20), which a third-party process cannot decrypt.

The owner chose: import/mirror of the local profile + our own sync server.

## Decision

1. **`cl-chrome-import`** — one-way (Chrome → us), read-only access to the Chrome profile: copy the files under a lock, parse Bookmarks (JSON), History/Web Data (SQLite), Preferences (subset: search engine, homepage, languages, zoom), Login Data (decryption via the Keychain "Chrome Safe Storage" on macOS / libsecret-kwallet on Linux / DPAPI `v10` on Windows; `v20` ABE — unavailable → the UI offers a CSV export from Chrome), Extensions (id + version → reinstall from the Web Store via ADR-0011). **Mirror** mode: a file watcher on the Chrome profile + periodic diff, applied to our profile with the source marked; on conflict our local edit wins, logged in the UI.
2. **Writing back to the Chrome profile is forbidden** (races with a running Chrome, profile corruption, ToS).
3. **`cl-sync`** — a client for the Chromium Sync protocol: `.proto` from Chromium (pinned commit), `prost`; v1 types: bookmarks, history, passwords, preferences, open tabs, extensions list. Encryption — client-side E2E (passphrase → Argon2id → AES-GCM); the server sees only blobs.
4. **`cl-sync-server`** — self-hosted (axum + SQLite/Postgres), Docker image, compatibility with the Brave `go-sync` wire format as a goal (not a guarantee). A public hosted service — not in v1.
5. **Google Chrome Sync — a permanent non-goal.** The UI does not create the impression that it "syncs with a Google account".

## Options Considered

- **Bypassing the Google API (flags/keys)** — rejected: ToS violation, key revocation, risk to users.
- **One-time import only** — not enough for "sync on the device"; the mirror adds liveness without writing to Chrome.
- **Our own sync protocol instead of `sync.proto`** — simpler, but we lose compatibility with existing servers/tools and the knowledge embedded in the Chromium client.

## Consequences

- Easier: a Chrome user gets their data right away; between our devices — sync under the user's control.
- Harder: `sync.proto` is large and changes with Chromium; Chrome profile formats need support on every release; Windows ABE — degraded UX.
- Revisit when: Google extends ABE to all data on all OSes → import only via the official export.

## Action Items

1. [ ] Chrome profile fixtures (generator) for three OSes (M6).
2. [ ] Pin the Chromium `components/sync/protocol` commit + update script (M6).
3. [ ] Sync threat model: there is no passphrase recovery by design — document it in the UI.
