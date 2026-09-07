#!/usr/bin/env bash
# CI gate: platform cfgs only in cl-platform, cl-process, cl-gfx (ADR-0009).
set -euo pipefail
cd "$(dirname "$0")/.."
ALLOWED='^(crates/platform/|crates/process/|crates/gfx/)'
HITS=$(grep -rnE 'cfg\((target_os|windows|unix)' --include='*.rs' crates apps 2>/dev/null | grep -vE "$ALLOWED" || true)
if [ -n "$HITS" ]; then
  echo "platform-specific cfg outside allowed crates:" >&2
  echo "$HITS" >&2
  exit 1
fi
echo "platform cfg check ok"
