#!/usr/bin/env bash
# Refresh the vendored html5lib-tests tree-construction conformance corpus.
#
# IMPORTANT: html5lib/html5lib-tests `master` no longer contains `tree-construction/` — it
# was migrated into web-platform-tests. This script does NOT support fetching a branch tip
# (unlike tools/conformance/url/fetch.sh's wpt fetcher) for exactly that reason: pointing it
# at `master` would silently vendor nothing. It only fetches a specific, already-known commit
# sha that still has the directory — such as the one html5ever vendors as a git submodule
# (`rcdom/html5lib-tests`, see servo/html5ever's `.gitmodules`). See README.md for how
# PINNED_COMMIT's current value was found.
#
# Downloads the `.dat` files this harness runs (see the list below) plus the corpus's MIT
# `LICENSE`, from the pinned commit, into this directory. Does NOT update `PINNED_COMMIT` or
# `expectations.txt` for you — after running this, diff the new files against the old ones,
# bump `PINNED_COMMIT` to the sha you fetched, re-run the harness
# (`cargo test -p cl-html --test tree_construction`), and update `expectations.txt` (remove
# stale entries, add reasons for any new failures) before committing.
set -euo pipefail
cd "$(dirname "$0")"

# A commit sha is required — see the note above about why there is no branch-tip fallback.
SHA="${1:-}"
if [ -z "$SHA" ]; then
  SHA=$(cat PINNED_COMMIT)
  echo "no sha given; re-fetching the currently pinned commit: $SHA" >&2
fi

# tests1.dat .. tests26.dat, except tests13.dat, which has never existed in this corpus
# (skipped by upstream, not a gap on our side) -- plus the specific extra files the M1a plan
# calls for.
FILES=(
  tests1.dat tests2.dat tests3.dat tests4.dat tests5.dat tests6.dat tests7.dat tests8.dat
  tests9.dat tests10.dat tests11.dat tests12.dat tests14.dat tests15.dat tests16.dat
  tests17.dat tests18.dat tests19.dat tests20.dat tests21.dat tests22.dat tests23.dat
  tests24.dat tests25.dat tests26.dat
  doctype01.dat entities01.dat entities02.dat comments01.dat adoption01.dat adoption02.dat
  tables01.dat template.dat tricky01.dat webkit01.dat webkit02.dat
)

mkdir -p tree-construction
BASE="https://raw.githubusercontent.com/html5lib/html5lib-tests/$SHA/tree-construction"
for f in "${FILES[@]}"; do
  curl -sf "$BASE/$f" -o "tree-construction/$f"
done
curl -sf "https://raw.githubusercontent.com/html5lib/html5lib-tests/$SHA/LICENSE" -o LICENSE

echo "fetched ${#FILES[@]} tree-construction/*.dat files + LICENSE at $SHA" >&2
echo "next: update PINNED_COMMIT to $SHA, run the harness, refresh expectations.txt" >&2
