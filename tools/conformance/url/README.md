# WPT URL conformance corpus

`urltestdata.json` is vendored from the [web-platform-tests](https://github.com/web-platform-tests/wpt)
repository's `url/resources/urltestdata.json` — the shared test data used by every browser
engine (and the `url`/`servo_url` Rust crates) to check WHATWG URL Standard conformance. It is
pinned to the commit recorded in `PINNED_COMMIT`, not tracked live, so the harness in
`crates/net/tests/urltestdata.rs` sees a stable, reproducible corpus.

## Format

A JSON array. Each entry is either:

- a `string` — a comment, describing the section below it. Skipped by the harness.
- an `object` with (at least):
  - `input` (string) — the URL string to parse.
  - `base` (string or `null`) — the base URL to resolve `input` against, or `null` to parse
    `input` as an absolute URL with no base.
  - `failure` (bool, optional) — `true` means parsing `input` (against `base`) must fail.
  - `href`, `origin`, `protocol`, `username`, `password`, `host`, `hostname`, `port`,
    `pathname`, `search`, `hash` — the expected serialized components when parsing succeeds.
    ChromeLight's harness only checks `href` (against `Url::as_str()`); `cl_net::Url` does not
    yet expose the individual component accessors.

See the upstream WPT `url/README.md` (not vendored here) for the authoritative format
description.

## License

`LICENSE` is the WPT project's BSD-3-Clause license text, fetched from the same pinned commit.
It covers `urltestdata.json`.

## Refreshing

```sh
tools/conformance/url/fetch.sh [<commit-sha>]
```

With no argument, fetches the current wpt `master` tip and prints its sha. Then:

1. Diff the new `urltestdata.json` against the previous version (`git diff`) to see what
   changed.
2. Update `PINNED_COMMIT` to the sha you fetched.
3. Run `cargo test -p cl-net --test urltestdata -- --nocapture` and read the summary line.
4. Update `expectations.txt`: delete entries for cases that now pass (a stale expectation
   would silently hide a future regression), add entries with honest `# reason` comments for
   any newly-failing cases.
5. Commit the refreshed corpus, `PINNED_COMMIT`, and `expectations.txt` together.
