# Security

Principle: **defense in depth**. No layer replaces another. A memory-safe language shrinks the class of memory bugs, but does not cure origin logic, IPC confused-deputy, JIT, `unsafe`-FFI, drivers, supply chain.

## 1. Threat model

**The attacker can:** make the user open a hostile site; fully control HTML/CSS/JS/Wasm/media and network responses; find a bug in a parser/VM/decoder and compromise the renderer; use speculative side-channels; install a malicious extension; attack the update chain.

**Outside the model:** a compromised OS/user account, a tampered root store, physical access, the user voluntarily handing a secret to phishing, server-side vulnerabilities of sites.

## 2. Protected assets

1. The user's files, keychain, clipboard, devices (camera/microphone/geolocation).
2. Cookies, tokens, passwords, autofill, history — including those imported from Chrome.
3. Data of one origin/site from another.
4. Browser process and privileged UI (omnibox truthfulness, permission dialogs).
5. Code integrity: binary, updates, extensions, sync data.
6. Privacy: fingerprint surface, cross-site linkage, sync contents (E2E encryption).
7. Availability: CPU/RAM/GPU/disk — a DoS in one tab does not bring down the browser.

## 3. Trust boundaries

```
Internet ──TLS──► network process ──validated IPC──► browser process ◄──validated IPC── renderer (site A)
                        ▲                                  │                                  ▲
                        │ FetchHandle                      │ capability handles               │
                        └──────────────────────────────────┴──────────────────────────────────┘
                                          gpu process ◄── display lists (shm, validated)
```

| Boundary | Who trusts whom | Enforcement |
|---|---|---|
| renderer → browser | browser trusts nothing | `cl-ipc::validate` on every message; origin/site from `SiteInstance`, not from the payload |
| renderer → network | network trusts only a `FetchHandle` from browser | the handle carries site, credentials mode, CSP snapshot |
| renderer → gpu | gpu validates the display list (bounds, resource ids, sizes) | size limits, ids from the table of allocated ones; in `cl-paint` this is `cl_paint::validate` (item/glyph/clip-depth limits, bounds ± 4096px) with the fuzz target `display_list_validate` (Task 20) |
| browser → OS | browser is privileged | minimize code: no parsers, no JS in browser process (until ADR-0008 revisit) |
| extension → browser | per manifest permissions | `chrome.*` host in browser process, every call checks the grant |
| sync client → server | server does not read the data | E2E: passphrase-derived key; server stores blobs |
| updater → binary | only signed packages | ed25519 (our key) + OS code signing; rollback protection by version |

### Known gaps

Boundaries the table above claims but the code does not yet hold. Each one blocks the gate named.

| Gap | Where | Effect | Blocks | Tracking |
|---|---|---|---|---|
| The `ipc-channel` receive path panics or allocates on hostile input: Linux short-datagram `set_len` underflow, `pop().unwrap()`, peer-sized `reserve_exact` (servo/ipc-channel#457); Windows `expect`/`unwrap` on peer data and peer-named `PROCESS_DUP_HANDLE`; channel ends attached to a byte message dropped behind `debug_assert!` | every child → browser channel (M0 renderer → browser today; renderer/gpu → browser in M1b) | a compromised child crashes (panic = abort) or corrupts the browser | Gate S0 (M2) — before the first untrusted content (owner, 2026-11-03) | ADR-0016 Context fact 3, `docs/DEPENDENCIES.md` (`ipc-channel` row) |
| Unsealed memfd shm on Linux | gpu → browser frames (M1b) | a sender that shrinks the memfd during the copy causes SIGBUS in the receiver | Gate S0 (M2) | ADR-0016 Context fact 3 |
| No receive-size cap before allocation (macOS copies a peer-sized out-of-line payload; Linux `reserve_exact`, Windows `reserve` on a peer-chosen size) | every child → browser channel | a child forces large allocations in the browser; allocation failure aborts | Gate S0 (M2) | ADR-0016 Context fact 3 |
| A forged `MACH_NOTIFY_NO_SENDERS` message id (macOS) | every child → browser channel | a child fakes its own disconnect and leaks the message's ports/memory | Gate S0 (M2) | ADR-0016 Context fact 3 |
| Sending shm or ports on Windows needs `PROCESS_DUP_HANDLE` on the receiver | gpu → browser frames once Windows children are sandboxed | the sandbox would have to grant a handle-duplication right on the browser (a sandbox-escape primitive) | the Windows sandbox | ADR-0016 Context fact 3 |

## 4. Defense matrix

| Threat | Entry surface | Protected asset | Control | Enforcement point | Residual risk |
|---|---|---|---|---|---|
| RCE in renderer | HTML/CSS/JS/Wasm/image/font | OS, files, keychain | sandbox (seatbelt / seccomp+ns / AppContainer), least privilege | `cl-process` before the first byte is loaded | sandbox escape via kernel/IPC bug; GPU/network sandbox is weaker |
| Compromised renderer reads another site | cross-site iframe, Spectre | site B data | Site-per-process, OOPIF (M4), ORB filtering of bodies, CORP/COEP/COOP | browser (assignment), network (ORB) | subdomain = one site; until M4 iframes in-process |
| Confused deputy via IPC | hostile messages | privileged operations | schema validation, capability handles, no sync IPC, fuzz IPC | receiver in browser/network/gpu | logic errors in validators |
| XSS/DOM injection | site | origin session | SOP, CSP L3, Trusted Types (later), cookies `HttpOnly/SameSite` | `cl-webapi`, `cl-net` | site opt-in |
| Cross-origin read | fetch/XHR | responses | CORS (preflight, credentials), ORB, Fetch Metadata headers | network process | server misconfig |
| MITM | network | traffic, identity | rustls + platform verifier, HSTS (preload list), mixed content block, cert error interstitial without click-through for HSTS | network + browser UI | compromised CA/root |
| Covert device access | Permissions API | camera/mic/geo | secure context gate, Permissions Policy, prompt in browser process, indicators, OS permission | browser + cl-platform | prompt fatigue |
| Tracking | third-party context | privacy | storage partitioning by top-level site, third-party cookie blocking by default, CHIPS, referrer policy `strict-origin-when-cross-origin` | network/storage key | fingerprinting |
| Malicious extension | CRX | profile, tabs | MV3 permissions, isolated worlds, CRX3 signature, host permissions runtime-grant, no remote code | browser (host API), renderer (isolated world) | user granted broad permissions |
| Dangerous download | download | OS | quarantine/MOTW, file-type policy, confirmation, (Safe Browsing — non-goal v1, we document it) | browser | no reputation service |
| N-day after patch | old versions | everything | signed auto-updates, staged rollout, rollback, 2-week release cadence tracking Chrome/V8 | updater | user disabled updates |
| Memory bugs in `unsafe`/FFI | V8, wgpu, OS API | process | `unsafe` only in 5 crates, SAFETY comments, miri, careful, fuzz + ASan for FFI modules | review + CI | V8 is C++, we inherit its bugs; update V8 within 7 days of a Chrome release |
| JIT exploitation | V8 | renderer | V8 hardening flags, W^X, JIT off in background and for untrusted-by-policy sites ("JIT-less mode" option as in Edge Super Duper Secure Mode) | `cl-js` | performance |
| Supply chain (crates) | `cargo` | binary | `cargo deny`, `cargo vet`, `Cargo.lock`, SBOM, prebuilt V8 by checksum | CI | upstream compromise |
| DevTools as RCE | `--remote-debugging-port` | everything | loopback only, token in URL, off by default, separate target isolation | `cl-devtools` | local malware |
| Chrome profile import | Chrome files | passwords | read-only, copy under lock, decryption only with user consent (Keychain prompt), no caches of decrypted data on disk | `cl-chrome-import` | Windows ABE — unavailable, only CSV export by the user |

## 5. Gates

- **Gate S0 (before M2):** no build opens an arbitrary URL without an applied sandbox for the renderer. `Sandbox<Applied>` type-state is the only way to construct `RendererMain`.
- **Gate S1 (before the first public alpha):** Site-per-process, IPC fuzzing ≥ 30 days without a crash of class ≥ medium, cert validation via WPT `wpt/tls`-like tests + `badssl.com` matrix, signed updater, crash reporting without PII.
- **Gate S2 (before beta):** OOPIF, external security review, bug bounty / disclosure policy, JIT hardening, full `cargo vet`.

## 6. `unsafe` and FFI policy

See `docs/CODING_STANDARDS.md` §4. Additionally: the V8 API is called only through the thin `cl-js::raw` layer; no other crate imports `v8::` directly. Same for `wgpu` (via `cl-gfx::backend`) and OS API (via `cl-platform`).

## 7. Secrets and data

- Passwords/cookies on disk — SQLite + encryption with a key from the platform keystore (Keychain / DPAPI+ABE analog / libsecret). On Windows we bind the key to our binary (path + signature), analogous to ABE.
- Logs/traces/dumps: a `tracing` field filter forbids `url.query`, `cookie`, `authorization`, DOM content; `#[derive(Debug)]` on types holding secrets is replaced with a manual `Debug` that redacts.
- Sync: client-side E2E; server — only blob + version metadata.

## 8. Vulnerability disclosure

Before public release — private issues. After: `SECURITY.md` at the root with a PGP key, response SLA 72 h, fix for criticals ≤ 7 days, release in the next 2-week train or a hotfix. The advisory is published after rollout ≥ 80%.
