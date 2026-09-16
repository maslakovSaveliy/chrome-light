#!/usr/bin/env bash
# CI gate: `unsafe` only in cl-platform, cl-process, cl-gfx, cl-style (ADR-0002 + ADR-0015).
set -euo pipefail
cd "$(dirname "$0")/.."
ALLOWED='^(crates/platform/|crates/process/|crates/gfx/|crates/style/src/(store|handle|stylo_dom)\.rs)'
HITS=$(grep -rnE '\bunsafe\b' --include='*.rs' crates apps 2>/dev/null \
  | grep -vE '^[^:]+:[0-9]+:\s*//' \
  | grep -vE 'forbid\(unsafe_code\)|deny\(unsafe_code\)|allow\(unsafe_code\)|unsafe_op_in_unsafe_fn|undocumented_unsafe_blocks' \
  | grep -vE "$ALLOWED" || true)
if [ -n "$HITS" ]; then echo "unsafe outside allowed scope:" >&2; echo "$HITS" >&2; exit 1; fi
echo "unsafe scope check ok"
