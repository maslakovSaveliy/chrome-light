#!/usr/bin/env bash
# Re-downloads the bundled fonts from their pinned upstream commits and verifies
# them against SHA256SUMS. Run from anywhere; paths are resolved relative to this
# script's own directory so `crates/fonts/assets/fetch.sh` works from the repo root.
#
# These assets are pinned to specific commits (not `master`/`main`) so a re-fetch
# is reproducible: the same bytes today and a year from now. If you need to pick up
# a newer upstream font, update the SHA/URLs below *and* SHA256SUMS together, and
# re-check the license text in LICENSE-Ahem / LICENSE-OFL still matches.
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$script_dir"

ahem_sha="986881aaf27ffc441f67dd9e5595e797141b1f40"
ahem_url="https://raw.githubusercontent.com/web-platform-tests/wpt/${ahem_sha}/fonts/Ahem.ttf"

noto_sans_sha="28b15b4b43b7bed62b5cf6e6b0b5ff5846270535"
noto_sans_url="https://raw.githubusercontent.com/notofonts/notofonts.github.io/${noto_sans_sha}/fonts/NotoSans/hinted/ttf/NotoSans-Regular.ttf"

echo "fetching Ahem.ttf from pinned commit ${ahem_sha}..."
curl -sSL --fail -o Ahem.ttf "$ahem_url"

echo "fetching NotoSans-Regular.ttf from pinned commit ${noto_sans_sha}..."
curl -sSL --fail -o NotoSans-Regular.ttf "$noto_sans_url"

echo "verifying against SHA256SUMS..."
shasum -a 256 -c SHA256SUMS

echo "OK: assets match pinned checksums."
