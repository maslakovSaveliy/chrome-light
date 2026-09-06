#!/usr/bin/env bash
# AGENTS.md is a verbatim copy of CLAUDE.md. Run after editing CLAUDE.md. CI checks they match.
set -euo pipefail
cd "$(dirname "$0")/.."
cp CLAUDE.md AGENTS.md
echo "AGENTS.md synced from CLAUDE.md"
