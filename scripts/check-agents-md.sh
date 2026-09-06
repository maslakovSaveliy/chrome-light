#!/usr/bin/env bash
# CI gate: AGENTS.md must be byte-identical to CLAUDE.md (CLAUDE.md §0).
set -euo pipefail
cd "$(dirname "$0")/.."
if ! cmp -s CLAUDE.md AGENTS.md; then
  echo "AGENTS.md differs from CLAUDE.md — run scripts/sync-agents-md.sh" >&2
  exit 1
fi
echo "AGENTS.md in sync"
