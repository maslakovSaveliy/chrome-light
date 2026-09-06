# Безопасность

Принцип: **defense in depth**. Ни один слой не заменяет другой. Memory-safe язык снижает класс memory-багов, но не лечит origin-логику, IPC confused-deputy, JIT, `unsafe`-FFI, драйверы, supply chain.

## 1. Модель угроз

**Атакующий может:** заставить пользователя открыть враждебный сайт; полностью контролировать HTML/CSS/JS/Wasm/медиа и сетевые ответы; найти баг в парсере/VM/декодере и скомпрометировать renderer; использовать спекулятивные side-channels; поставить вредоносное расширение; атаковать цепочку обновлений.

**Вне модели:** скомпрометированная ОС/аккаунт пользователя, подменённый root store, физический доступ, добровольная отдача секрета фишингу, серверные уязвимости сайтов.

## 2. Защищаемые активы

1. Файлы, keychain, clipboard, устройства (камера/микрофон/геолокация) пользователя.
2. Cookies, токены, пароли, autofill, история — включая импортированные из Chrome.
3. Данные одного origin/site от другого.
4. Browser process и privileged UI (omnibox правдивость, диалоги разрешений).
5. Целостность кода: бинарник, обновления, расширения, sync-данные.
6. Приватность: fingerprint surface, cross-site linkage, sync-содержимое (E2E-шифрование).
7. Доступность: CPU/RAM/GPU/диск — DoS одной вкладки не роняет браузер.

## 3. Границы доверия

```
Интернет ──TLS──► network process ──validated IPC──► browser process ◄──validated IPC── renderer (site A)
                        ▲                                  │                                  ▲
                        │ FetchHandle                      │ capability handles               │
                        └──────────────────────────────────┴──────────────────────────────────┘
                                          gpu process ◄── display lists (shm, validated)
```

| Граница | Кто доверяет кому | Enforcement |
|---|---|---|
| renderer → browser | browser не доверяет ничему | `cl-ipc::validate` на каждом сообщении; origin/site из `SiteInstance`, не из payload |
| renderer → network | network доверяет только `FetchHandle` от browser | handle несёт site, credentials mode, CSP-снимок |
| renderer → gpu | gpu валидирует display list (bounds, resource ids, размеры) | лимиты размеров, ids из таблицы выделенных |
| browser → ОС | browser привилегирован | минимизировать код: нет парсеров, нет JS в browser process (до ADR-0008 revisit) |
| расширение → browser | по manifest permissions | `chrome.*` host в browser process, каждый вызов проверяет grant |
| sync client → server | сервер не читает данные | E2E: passphrase-derived key; сервер хранит blob-ы |
| updater → бинарник | только подписанные пакеты | ed25519 (наш ключ) + OS code signing; rollback protection по версии |

## 4. Матрица защит

| Угроза | Поверхность входа | Защищаемый актив | Контроль | Точка применения | Остаточный риск |
|---|---|---|---|---|---|
| RCE в renderer | HTML/CSS/JS/Wasm/image/font | ОС, файлы, keychain | sandbox (seatbelt / seccomp+ns / AppContainer), least privilege | `cl-process` до загрузки первого байта | sandbox escape через kernel/IPC-баг; GPU/network sandbox слабее |
| Compromised renderer читает чужой сайт | cross-site iframe, Spectre | данные site B | Site-per-process, OOPIF (M4), ORB-фильтрация тел, CORP/COEP/COOP | browser (assignment), network (ORB) | subdomain = один site; до M4 iframes in-process |
| Confused deputy через IPC | враждебные сообщения | privileged операции | schema validation, capability handles, no sync IPC, fuzz IPC | receiver в browser/network/gpu | логические ошибки в валидаторах |
| XSS/DOM injection | сайт | сессия origin | SOP, CSP L3, Trusted Types (позже), cookies `HttpOnly/SameSite` | `cl-webapi`, `cl-net` | opt-in сайта |
| Cross-origin read | fetch/XHR | ответы | CORS (preflight, credentials), ORB, Fetch Metadata headers | network process | сервер misconfig |
| MITM | сеть | трафик, identity | rustls + platform verifier, HSTS (preload list), mixed content block, cert error interstitial без click-through для HSTS | network + browser UI | скомпрометированный CA/root |
| Скрытый доступ к устройствам | Permissions API | камера/мик/гео | secure context gate, Permissions Policy, prompt в browser process, индикаторы, OS permission | browser + cl-platform | prompt fatigue |
| Tracking | third-party context | privacy | storage partitioning по top-level site, third-party cookie blocking по умолчанию, CHIPS, referrer policy `strict-origin-when-cross-origin` | network/storage key | fingerprinting |
| Вредоносное расширение | CRX | профиль, вкладки | MV3 permissions, isolated worlds, CRX3 signature, host permissions runtime-grant, no remote code | browser (host API), renderer (isolated world) | пользователь выдал широкие права |
| Опасная загрузка | download | ОС | quarantine/MOTW, file-type policy, подтверждение, (Safe Browsing — non-goal v1, документируем) | browser | нет reputation-сервиса |
| N-day после патча | старые версии | всё | подписанные автообновления, staged rollout, rollback, 2-недельный релизный ритм под Chrome/V8 | updater | пользователь отключил обновления |
| Memory bugs в `unsafe`/FFI | V8, wgpu, OS API | процесс | `unsafe` только в 5 crate-ах, SAFETY-комментарии, miri, careful, fuzz + ASan для FFI-модулей | review + CI | V8 — C++, наследуем его баги; обновлять V8 в течение 7 дней после релиза Chrome |
| JIT-эксплуатация | V8 | renderer | V8 hardening flags, W^X, JIT off в фоне и для untrusted-by-policy сайтов (опция «JIT-less mode» как в Edge Super Duper Secure Mode) | `cl-js` | производительность |
| Supply chain (crates) | `cargo` | бинарник | `cargo deny`, `cargo vet`, `Cargo.lock`, SBOM, prebuilt V8 по checksum | CI | компромисс upstream |
| DevTools как RCE | `--remote-debugging-port` | всё | только loopback, token в URL, выключено по умолчанию, отдельный target isolation | `cl-devtools` | локальный malware |
| Импорт Chrome-профиля | файлы Chrome | пароли | read-only, копия под lock, расшифровка только с согласия пользователя (Keychain prompt), никаких кэшей расшифрованного на диске | `cl-chrome-import` | Windows ABE — недоступно, только CSV-экспорт пользователем |

## 5. Гейты

- **Gate S0 (до M2):** ни одна сборка не открывает произвольный URL без применённого sandbox для renderer. `Sandbox<Applied>` type-state — единственный способ построить `RendererMain`.
- **Gate S1 (до первой публичной альфы):** Site-per-process, IPC-fuzzing ≥ 30 дней без crash class ≥ medium, cert validation через WPT `wpt/tls`-подобные тесты + `badssl.com` матрица, подписанный updater, crash reporting без PII.
- **Gate S2 (до беты):** OOPIF, внешний security review, bug bounty / disclosure policy, JIT-hardening, `cargo vet` полный.

## 6. Политика `unsafe` и FFI

См. `docs/CODING_STANDARDS.md` §4. Дополнительно: V8 API вызывается только через тонкий слой `cl-js::raw`; никакой другой crate не импортирует `v8::` напрямую. То же для `wgpu` (через `cl-gfx::backend`) и OS API (через `cl-platform`).

## 7. Секреты и данные

- Пароли/cookies на диске — SQLite + шифрование ключом из platform keystore (Keychain / DPAPI+ABE-аналог / libsecret). На Windows ключ привязываем к нашему бинарнику (path + signature) по аналогии с ABE.
- Логи/трейсы/дампы: `tracing` field-фильтр запрещает `url.query`, `cookie`, `authorization`, содержимое DOM; `#[derive(Debug)]` на типах с секретами заменяется ручным `Debug` с редактированием.
- Sync: клиентское E2E; сервер — только blob + метаданные версии.

## 8. Раскрытие уязвимостей

До публичного релиза — private issues. После: `SECURITY.md` в корне с PGP-ключом, SLA ответа 72 ч, фикс критических ≤ 7 дней, релиз в ближайший 2-недельный train или hotfix. Advisory публикуется после rollout ≥ 80%.
