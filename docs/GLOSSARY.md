# Glossary

| Term | Meaning in the project |
|---|---|
| **Browser process** | The only privileged process: UI, navigation, profiles, broker. Does not parse untrusted bytes. |
| **Renderer process** | Sandboxed process with the engine + V8 for one **site**. Considered hostile. |
| **Site** | `scheme + eTLD+1` (registrable domain). Unit of process isolation. Larger than an origin. |
| **Origin** | `scheme + host + port`. Unit of the Same-Origin Policy. |
| **StorageKey** | `(origin, top-level site, ancestor-bit)` — the key for all storage; implements partitioning. |
| **SiteInstance** | Binding of a document to a renderer process in the browser process. |
| **OOPIF** | Out-of-process iframe: a cross-site frame in another renderer. M4. |
| **Capability handle** | Non-transferable token from the broker that grants the renderer the right to one specific operation (fetch, storage area…). |
| **Broker** | Part of `cl-ipc` in the browser process that issues handles and validates requests. |
| **Sandbox<Applied>** | Type-state: renderer main starts only with the sandbox policy applied. |
| **Fetch** | The WHATWG Fetch algorithm; implemented in the network process. Not an HTTP client, but a policy engine on top of one. |
| **ORB** | Opaque Response Blocking: filtering of `no-cors` response bodies before they reach the renderer. |
| **Display list** | Our format for paint commands from the renderer to the GPU process. Validated by the receiver. |
| **Property trees** | transform/clip/effect/scroll trees in the compositor; enable async scroll/animation without layout. |
| **Fragment tree** | Immutable result of a layout pass (LayoutNG-style). |
| **Isolate / Context** | V8: isolate — heap+VM per thread; context — the global object of one `Window`. One isolate per renderer main thread, one context per document. |
| **Web IDL** | DSL for describing DOM APIs; `cl-bindings` generates Rust↔V8 glue. Hand-written bindings are forbidden. |
| **Freeze / Discard / Hibernate** | Three levels of memory saving for a background tab: JS stopped; renderer killed, tab stays in the UI; state serialized to disk and restorable. |
| **MV3** | Chrome Extensions Manifest V3. The only supported extension format. |
| **CRX3** | Signed Chrome extension package format. |
| **CDP** | Chrome DevTools Protocol. We implement a subset for the DevTools frontend. |
| **WebDriver BiDi** | Standard automation protocol; for E2E tests. |
| **sync.proto** | The Chromium Sync protocol; our client+server speak it. Not tied to a Google account. |
| **App-Bound Encryption (ABE)** | Encryption of Chrome cookies/passwords on Windows since Chrome 127, bound to Chrome. Blocks third-party import. |
| **WPT** | web-platform-tests — cross-browser web platform test suite. |
| **Test262** | ECMAScript conformance suite. |
| **Reftest** | Test that compares the rendering of a test document against a reference document. |
| **Golden test** | Snapshot of a structure (parse tree, style, fragments) via `insta`. |
| **M0–M6** | Milestones. See FEATURE_MATRIX; details — the plan. |
| **Gate S0/S1/S2** | Security gates from SECURITY.md §5. |
| **`M1-ONLY`** | Marker for temporary code removed when the milestone closes. |
| **`SPEC-DEVIATION`** | Marker for an intentional spec deviation, with a link to the registry. |
