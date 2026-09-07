# Bench

`mem.sh` — idle RSS of all ChromeLight processes (browser + renderer) using the `release-dev` profile
(release optimisations + debug assertions so `--no-sandbox` exists until real sandboxes land).

    ./tools/bench/mem.sh            # IDLE_SECONDS=5 by default
    IDLE_SECONDS=20 ./tools/bench/mem.sh

Results land in `tools/bench/results/` (gitignored). Monthly comparison against Chrome goes to
`docs/history/bench-YYYY-MM.md` (ADR-0012). Windows script and page corpus arrive in M1.
