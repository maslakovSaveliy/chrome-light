# Build times

Время сборки `cl-style` (stylo 0.20 + вся его зависимость: cssparser, selectors, servo_arc,
euclid, string_cache/markup5ever, icu4x-семейство через `url`/`idna`, ...). Замер сделан в
task-2 (stylo build spike, ADR-0015) как часть risk-reduction — если cold build окажется
слишком долгим, это риск для CI-кэша (`actions/cache` на `~/.cargo` + `target/`).

Метод: `cargo clean` (полная очистка `target/`, не только `-p cl-style`, чтобы замер отражал
реальный холодный CI-раннер без кэша) → `time cargo build -p cl-style`. Warm — повторный
`cargo build -p cl-style` без изменений исходников сразу после cold.

| Дата | Машина | `cargo build -p cl-style` cold | warm (без изменений) | warm (правка своего файла) | `target/` после cold |
|---|---|---|---|---|---|
| 2026-09-08 | macOS 26.5.2, Apple Silicon, 8 cores, 16 GB | **18m 37s** (`184.11s user 36.03s system 19% cpu 18:38.90 total`) | 13.16s (`0.23s user 0.40s system` — cargo re-хэширует граф из ~140 крейтов) | 0.96s (`0.08s user 0.20s system` — только релинк `cl-style`) | 765 MB (debug, только `cl-style`+deps, без тестов) |

**Риск подтверждён**: cold build (`cargo clean` → `cargo build -p cl-style`) занял **18m 37s**,
что превышает порог 10 минут из брифа task-2. Средняя загрузка CPU за это время — всего 19%
(184s user+system CPU против 1118s wall), т.е. большая часть времени НЕ параллелится по
ядрам — узкое место, вероятно, в однопоточном фронтенде `rustc` на чек ноль огромного
mako-сгенерированного `stylo::properties` (тысячи строк из `properties.mako.rs`), а не в
самом Python-кодогене (тот отрабатывает за секунды при `cargo build --timings`, не измерялось
отдельно в этом прогоне). Для CI (task 2 брифа: "controller pushes the feature branch after
this task specifically to see the 3-OS build result") это означает: обязательно кэшировать
`~/.cargo/registry` и `target/` между запусками (`actions/cache`), иначе каждый job будет
терять ~19 минут на голую сборку одного `cl-style`. Windows/Linux раннеры в CI могут быть
медленнее/быстрее — реальные числа появятся после первого push (см. task-2-report.md).

Полный `time` вывод и полный список скомпилированных крейтов — в отчёте task-2
(`.superpowers/sdd/2026-09-07-m1a-static-pipeline/task-2-report.md`).
