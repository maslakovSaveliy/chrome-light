# ADR-0005: Multi-process, typed IPC, sandbox с первого milestone

**Status:** Accepted
**Date:** 2026-09-07
**Deciders:** владелец проекта

## Context

Референс: «сначала функции, потом безопасность» = позднее перепроектирование ownership и API. Renderer обрабатывает враждебные байты; V8 — C++. Единственная защита ОС при компрометации renderer — sandbox; единственная защита других сайтов — process isolation; единственная защита browser process — валидация IPC.

## Decision

1. **Процессы с M1:** browser (privileged) + renderer per site + network + gpu + utility (decoders). Один бинарник, `--type=`. `--single-process` только в dev-сборках.
2. **Gate S0:** untrusted URL не загружается без `Sandbox<Applied>` (type-state) в renderer. До готовности sandbox на платформе — только `file://` тест-фикстуры и localhost.
3. **Sandbox:** macOS seatbelt SBPL профили; Linux user/pid/net namespaces + seccomp-bpf allowlist + `no_new_privs`; Windows restricted token + job + AppContainer + Win32k lockdown. Network/GPU — более слабые, но отдельные профили.
4. **IPC (`cl-ipc`):** транспорт `ipc-channel` + shm; сообщения — serde enum-ы через `postcard`; `validate()` на каждом типе на стороне получателя; версия в handshake; capability handles от broker-а; **только async** (единственный sync-путь — модальные диалоги renderer→browser с таймаутом).
5. **Site assignment** в browser process с M2 (site-per-process), OOPIF — M4.
6. **Crash:** minidump в child; browser перезапускает; UI «вкладка упала»; дампы без содержимого страниц.

## Options Considered

- **Single-process до M3, потом разделить** — отвергнут: переделка ownership (DOM ↔ network ↔ storage) дороже, чем сделать сразу; референс и опыт Chromium/Firefox/WebKit это подтверждают.
- **Thread-isolation вместо процессов** — не защищает от memory-багов V8/FFI и Spectre.
- **Mojo-подобный IDL с кодогенерацией** — позже, если enum+serde станет узким местом; сейчас избыточно.

## Consequences

- Легче: security-модель ясна с первого коммита; тесты сразу multiprocess ловят IPC-баги.
- Труднее: M1 медленнее на ~месяц (spawn/IPC/shm инфраструктура); Windows sandbox — отдельная сложность.
- Пересмотреть когда: стоимость процессов бьёт по бюджету памяти → renderer reuse policy (ADR-0012), не отказ от изоляции.

## Action Items

1. [ ] `cl-process` spawn + handshake на 3 ОС (M0/M1).
2. [ ] Sandbox профили renderer на macOS и Linux (M1), Windows (M2).
3. [ ] `fuzz/ipc_*` targets в PR, который вводит первое сообщение.
4. [ ] `tests/security/sandbox_*`: из renderer `open("/etc/passwd")` → EPERM на всех ОС.
