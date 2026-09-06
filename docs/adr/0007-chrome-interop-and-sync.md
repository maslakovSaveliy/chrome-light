# ADR-0007: Chrome-интероп — импорт/зеркало локального профиля и свой sync по `sync.proto`

**Status:** Accepted
**Date:** 2026-09-07
**Deciders:** владелец проекта

## Context

Требование «полная синхронизация с Chrome на устройстве». Факты (RESEARCH §3): Google закрыл Chrome Sync API для сторонних сборок 2021-03-15 — легального пути нет. Протокол `sync.proto` открыт; Brave `go-sync` реализует сервер. Локальный профиль Chrome читаем: Bookmarks/History/Login Data/Preferences/Extensions; пароли и cookies зашифрованы платформенно; на Windows с Chrome 127 — App-Bound Encryption (v20), сторонний процесс расшифровать не может.

Владелец выбрал: импорт/зеркало локального профиля + свой sync-сервер.

## Decision

1. **`cl-chrome-import`** — односторонний (Chrome → мы), read-only доступ к профилю Chrome: копия файлов под lock, парсинг Bookmarks (JSON), History/Web Data (SQLite), Preferences (подмножество: search engine, homepage, языки, zoom), Login Data (расшифровка через Keychain «Chrome Safe Storage» на macOS / libsecret-kwallet на Linux / DPAPI `v10` на Windows; `v20` ABE — недоступно → UI предлагает экспорт CSV из Chrome), Extensions (id + версия → переустановка из Web Store через ADR-0011). Режим **зеркало**: file watcher на профиль Chrome + периодический diff, применяется в наш профиль с пометкой источника; конфликт — наш локальный edit побеждает, лог в UI.
2. **Обратная запись в профиль Chrome — запрещена** (гонки с запущенным Chrome, повреждение профиля, ToS).
3. **`cl-sync`** — клиент протокола Chromium Sync: `.proto` из Chromium (pinned commit), `prost`; типы v1: bookmarks, history, passwords, preferences, open tabs, extensions list. Шифрование — клиентское E2E (passphrase → Argon2id → AES-GCM), сервер видит только blob-ы.
4. **`cl-sync-server`** — self-hosted (axum + SQLite/Postgres), Docker-образ, совместимость с Brave `go-sync` wire-форматом как цель (не гарантия). Публичный hosted-сервис — не в v1.
5. **Google Chrome Sync — non-goal навсегда.** UI не создаёт впечатления, что «синхронизируется с Google-аккаунтом».

## Options Considered

- **Обход Google API (флаги/ключи)** — отвергнут: нарушение ToS, отзыв ключей, риск для пользователей.
- **Только импорт один раз** — недостаточно для «синхронизации на устройстве»; зеркало добавляет живость без записи в Chrome.
- **Свой протокол sync вместо `sync.proto`** — проще, но теряем совместимость с существующими серверами/инструментами и знания из Chromium-клиента.

## Consequences

- Легче: пользователь Chrome получает свои данные сразу; между нашими устройствами — sync под контролем пользователя.
- Труднее: `sync.proto` объёмен и меняется с Chromium; поддержка форматов профиля Chrome при каждом релизе; Windows ABE — деградированный UX.
- Пересмотреть когда: Google расширит ABE на все данные на всех ОС → импорт только через официальный экспорт.

## Action Items

1. [ ] Фикстуры профиля Chrome (генератор) для трёх ОС (M6).
2. [ ] Pin Chromium `components/sync/protocol` commit + скрипт обновления (M6).
3. [ ] Threat model sync: passphrase recovery отсутствует по дизайну — задокументировать в UI.
