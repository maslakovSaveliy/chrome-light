#!/usr/bin/env bash
# Sample RSS of every ChromeLight process while the browser idles. macOS/Linux; Windows: M1 (ps1).
set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
cd "$(dirname "$0")/../.."

IDLE="${IDLE_SECONDS:-5}"
cargo build --profile release-dev -p chromelight --quiet
BIN="$(pwd)/target/release-dev/chromelight"
mkdir -p tools/bench/results
OUT="tools/bench/results/mem-$(date +%Y%m%d-%H%M%S).json"

"$BIN" --no-sandbox --idle-seconds "$IDLE" &
BROWSER_PID=$!
sleep 2

TOTAL=0
ROWS=""
for p in $(pgrep -f "$BIN" || true); do
  rss=$(ps -o rss= -p "$p" 2>/dev/null | tr -d ' ' || echo 0)
  cmd=$(ps -o args= -p "$p" 2>/dev/null | sed 's/"/\\"/g' || echo "?")
  [ -z "$rss" ] && continue
  TOTAL=$((TOTAL + rss))
  ROWS="${ROWS}{\"pid\":$p,\"rss_kb\":$rss,\"cmd\":\"$cmd\"},"
done

wait "$BROWSER_PID"
printf '{"timestamp":"%s","total_rss_kb":%d,"processes":[%s]}\n' \
  "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$TOTAL" "${ROWS%,}" > "$OUT"
cat "$OUT"
