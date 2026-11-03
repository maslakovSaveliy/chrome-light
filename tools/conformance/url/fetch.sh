#!/usr/bin/env bash
# Refresh the vendored WPT URL conformance corpus.
#
# Downloads `url/resources/urltestdata.json` (and the WPT license text) from a pinned
# web-platform-tests commit and overwrites the copies in this directory. Does NOT update
# `PINNED_COMMIT` or `expectations.txt` for you — after running this, diff the new JSON
# against the old one, bump `PINNED_COMMIT` to the sha you fetched, re-run the harness
# (`cargo test -p cl-net --test urltestdata`), and update `expectations.txt` (remove
# stale entries, add reasons for any new failures) before committing.
set -euo pipefail
cd "$(dirname "$0")"

# Pass a commit sha to fetch a specific commit; otherwise fetch the current wpt `master`
# tip and print its sha so you can record it in PINNED_COMMIT.
SHA="${1:-}"
if [ -z "$SHA" ]; then
  SHA=$(curl -sf "https://api.github.com/repos/web-platform-tests/wpt/commits/master" | python3 -c 'import json,sys; print(json.load(sys.stdin)["sha"])')
  echo "no sha given; using current wpt master tip: $SHA" >&2
fi

curl -sf "https://raw.githubusercontent.com/web-platform-tests/wpt/$SHA/url/resources/urltestdata.json" -o urltestdata.json
curl -sf "https://raw.githubusercontent.com/web-platform-tests/wpt/$SHA/LICENSE.md" -o LICENSE

echo "fetched urltestdata.json + LICENSE at $SHA" >&2
echo "next: update PINNED_COMMIT to $SHA, run the harness, refresh expectations.txt" >&2
