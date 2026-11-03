#!/usr/bin/env bash
# CI gate: stylo types stay inside cl-style and cl-layout's adapter (ADR-0015 §2).
set -euo pipefail
cd "$(dirname "$0")/.."
ALLOWED='^(crates/style/|crates/layout/src/style_adapt\.rs)'
HITS=$(grep -rnE '\b(servo_arc|style::|stylo|selectors::|cssparser)\b' --include='*.rs' crates apps 2>/dev/null \
  | grep -vE '^[^:]+:[0-9]+:\s*//' | grep -vE "$ALLOWED" || true)
if [ -n "$HITS" ]; then echo "stylo types outside cl-style/style_adapt:" >&2; echo "$HITS" >&2; exit 1; fi
echo "stylo scope check ok"
