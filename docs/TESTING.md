# Тестирование

Pass rate — не единственный KPI. Unsupported, timeout, crash и ошибочный baseline — разные вещи; дашборд показывает их раздельно.

## 1. Пирамида

| Уровень | Инструмент | Что покрывает | Гейт |
|---|---|---|---|
| Unit / property | `cargo nextest`, `proptest` | URL, tokenizer, selectors, cascade helpers, length math, cookie parsing, cache keys, IPC (де)сериализация | PR |
| Golden / snapshot | `insta` | parse trees, computed style, fragment trees, display lists | PR |
| Reftest / pixel | `cl-testshell --png` + сравнение с reference HTML; perceptual tolerance только для растровых различий | layout/paint | PR (затронутые), nightly (все) |
| WPT | `wptrunner` + product adapter `tools/wpt/product/chrome_light.py` | наблюдаемое поведение web-платформы | PR: затронутые директории + smoke shard; nightly: полный run на 3 ОС |
| Test262 | `tools/test262/` harness | ECMAScript (V8) — qualification set при каждом обновлении V8 или изменении bindings | при обновлении V8 |
| Integration | `tests/integration/` с `tools/testserver` | redirects, navigation races, history, process crash/restart, storage eviction, permissions | PR |
| Security | `tests/security/` | origin/CORS/CSP матрицы, malicious IPC corpus, sandbox policy assertions (попытка open(2) из renderer → EPERM), cert edge cases | PR |
| Fuzz | `cargo-fuzz` (`tools/fuzz/`) | HTML/CSS/URL/cookie/image/font/IPC decoders, display list validator, DOM mutation sequences | PR 60 с; nightly 30 мин |
| Differential | `tools/diff/` | один input → наш testshell vs headless Chrome (CDP screenshot/DOM dump) | nightly, triage-сигнал, не proof |
| Performance / memory | `tools/bench/` | cold start, first frame, RSS/PSS по процессам на corpus, scroll fps, input latency | PR с меткой `perf`, nightly |
| UI E2E | WebDriver BiDi (свой endpoint в `cl-devtools`) + OS-level (AX API) | omnibox, диалоги, updater, a11y | nightly |
| Real-site corpus | `tools/chrome-corpus/sites.toml` | product acceptance: список сайтов × сценариев (load, login form, scroll, video) | milestone gate |

## 2. Детерминизм

- Фиксированные шрифты в testshell (bundled test fonts, никаких системных), DPR=1, viewport 800×600, отключены анимации, фиксированный `Clock`, фиксированный RNG seed, `Math.random` детерминирован в test mode.
- `cl-testshell` — один процесс? **Нет:** multiprocess по умолчанию даже в тестах, чтобы ловить IPC-баги; `--single-process` только для отладки.

## 3. WPT интеграция

1. Product adapter: запуск бинарника, профиль во временной директории, порты/сертификаты WPT, таймауты, cleanup, детекция crash по exit code и dump.
2. Порядок включения директорий: `url`, `encoding`, `dom`, `html/syntax`, `css/css-cascade`, `css/CSS2`, `fetch`, `xhr`, `html/webappapis`, затем `css/css-flexbox`, `css/css-grid`, `css/css-text`, `workers`, `IndexedDB`, `service-workers`…
3. Expected failures — versioned metadata `tools/wpt/expectations/**/*.ini` с bug ID и датой пересмотра. Переписывать expectation для скрытия регрессии запрещено.
4. Каждый compat-фикс → минимальный regression test; если поведение спеки — upstream в WPT.
5. Дашборд: pass / expected-fail / crash / timeout по директории и коммиту; отдельно новые регрессии.

## 4. Test262

V8 уже проходит Test262; мы гоняем **qualification set** (~500 тестов, покрытие: modules, realms, Intl, host hooks, Atomics) при каждом bump V8 и при изменении `cl-bindings`/event loop, потому что наши host hooks и job queue — наш код.

## 5. Fuzzing

- Targets появляются в том же PR, что и парсер.
- Структурированные входы через `arbitrary`; корпуса в `tools/fuzz/corpus/` (git LFS позже).
- Sanitizers: для чистого Rust — miri на unsafe-модулях; для FFI (V8, wgpu) — ASan сборка в nightly Linux job.
- Найденный crash → минимизация → regression test → фикс → закрытие с ссылкой.

## 6. Производительность и память

- Corpus: 20 страниц (статичные копии, `tools/bench/corpus/`, лицензия проверена) + 5 синтетических стрессов (10k DOM nodes, deep nesting, huge table, flex reflow, 1000 images).
- Метрики: cold start → first frame; navigation → LCP-аналог; RSS/PSS каждого процесса на 1/5/10 вкладках; после freeze; после hibernate; scroll fps на длинной странице; input latency.
- Гейт: регрессия памяти >5% или времени >10% блокирует merge без объяснения в коммите.
- Ежемесячно — сравнение с актуальным Chrome на том же corpus, той же машине; цифры — в `docs/history/bench-YYYY-MM.md`.

## 7. Определение «готово» для фичи

- строка в `SPEC_REGISTRY.md` со ссылкой на спецификацию и тесты;
- WPT директория включена, expectations зафиксированы;
- fuzz target (если парсер/декодер);
- reftest (если layout/paint);
- нет `M*-ONLY` маркеров старше текущего milestone;
- бюджеты памяти не нарушены.
