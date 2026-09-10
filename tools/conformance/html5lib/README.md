# html5lib-tests tree-construction conformance corpus

`tree-construction/*.dat` is vendored from the
[html5lib-tests](https://github.com/html5lib/html5lib-tests) repository's
`tree-construction/` directory — the shared corpus every conformant HTML5 parser (html5ever,
Servo, browsers' own test suites) checks tree-construction behaviour against. It is pinned to
the commit recorded in `PINNED_COMMIT`, not tracked live, so the harness in
`crates/html/tests/tree_construction.rs` sees a stable, reproducible corpus.

## Why a specific pinned commit, not `master`

As of when this was vendored, `html5lib-tests`' `master` branch **no longer contains
`tree-construction/`** — the directory was migrated into
[web-platform-tests](https://github.com/web-platform-tests/wpt) (`wpt`'s `html/syntax/`
tree). Pointing `fetch.sh` at `master` would silently vendor nothing.

`PINNED_COMMIT` is instead the commit that
[servo/html5ever](https://github.com/servo/html5ever) itself vendors as a git submodule at
`rcdom/html5lib-tests` (see html5ever's `.gitmodules`) — i.e. the exact tree-construction
corpus html5ever's own test suite runs against. That makes it a natural, well-precedented
choice: html5ever is the tree-construction engine `cl_html` is built on, so testing against the
same fixed corpus html5ever itself uses keeps the comparison meaningful.

Found via GitHub's contents API (no local clone needed):

```sh
curl -s "https://api.github.com/repos/servo/html5ever/contents/rcdom/html5lib-tests" \
  | python3 -c 'import json,sys; print(json.load(sys.stdin)["sha"])'
# -> f994590f528ac8b6073665791ddb1ed85c66dfb2
```

The API response's `type` is `"submodule"` and its `sha` is the pinned commit inside
`html5lib/html5lib-tests` that submodule entry points at — confirmed by fetching
`tree-construction/` at that commit (`.../contents/tree-construction?ref=<sha>`) and finding
every file this harness needs still present there.

## Vendored files

`tests1.dat` … `tests26.dat` (note: **`tests13.dat` has never existed** in this corpus — 13 was
skipped by upstream, not a gap introduced here), `doctype01.dat`, `entities01.dat`,
`entities02.dat`, `comments01.dat`, `adoption01.dat`, `adoption02.dat`, `tables01.dat`,
`template.dat`, `tricky01.dat`, `webkit01.dat`, `webkit02.dat` — the exact list from the M1a
plan's Task 9. `scripted/` (needs a JS engine) and every other `.dat` file in the upstream
directory (`blocks.dat`, `foreign-fragment.dat`, `ruby.dat`, `svg.dat`, `math.dat`, ...) are not
vendored; they were out of this task's scope.

`LICENSE` is the corpus's own MIT license text, fetched from the same pinned commit's repository
root.

## `.dat` format

Each file is a sequence of test cases separated by a blank line. A case has, in order:

- `#data` — the HTML source to parse. May itself contain blank lines (a case whose entire input
  *is* the empty string renders as `#data` immediately followed by one blank line, then
  `#errors`); the section runs until the next `#`-prefixed heading line, not until the next
  blank line.
- `#errors` — one parse-error diagnostic per line (not checked by this harness — see below).
- `#new-errors` (optional) — a newer-format restatement of the same diagnostics.
- `#document-fragment <context>` (optional) — the context element for fragment parsing.
  Fragment parsing is not implemented in M1a, so any case with this section is skipped, not
  failed.
- `#script-on` / `#script-off` (optional) — some cases exist in both a scripting-enabled and a
  scripting-disabled variant with the same `#data` (typically `<noscript>` behaviour, which the
  HTML spec defines differently depending on the scripting flag). `cl_html::parse_document_str`
  always parses with scripting disabled, so a `#script-on` case is skipped (its `#script-off` —
  or unmarked, which means the same thing — twin is run instead); an unmarked or `#script-off`
  case is run normally.
- `#document` — the expected parse tree, in the format `cl_dom::serialize::html5lib_tree`
  produces (see that module's doc comments for the exact grammar: `| ` prefix, two spaces per
  depth, sorted attributes, etc.). Runs until the blank line that separates cases (or EOF).

`crates/html/tests/tree_construction.rs`'s `parse_dat` reimplements this format directly against
real vendored files (not from memory) and matches the reference Python parser
(`html5lib/tests/support.py`'s `TestData` class, upstream in the `html5lib-python` repository)
line for line, including its very permissive "any line starting with `#` is a new section
heading" rule — see that function's doc comment for the full reasoning, including why the
corpus's per-case blank-line separator never corrupts a genuine `#document` block.

This harness does not check `#errors`/`#new-errors` against `ParseOutput::parse_errors` — HTML5
tree construction is defined to always recover from a parse error rather than abort, so a
mismatched error list is not itself a correctness bug the way a mismatched tree is; only the
`#document` block is compared, per the M1a Task 9 spec.

## Refreshing

```sh
tools/conformance/html5lib/fetch.sh [<commit-sha>]
```

Unlike `tools/conformance/url/fetch.sh`, there is **no branch-tip fallback** — see "Why a
specific pinned commit" above for why fetching `master` would be silently wrong. With no
argument, `fetch.sh` re-fetches whatever commit is currently recorded in `PINNED_COMMIT`. To
move to a different commit (e.g. because html5lib-tests eventually moves `tree-construction/`
back, or a newer commit is found to be more appropriate), pass its sha explicitly. Then:

1. Diff the new `tree-construction/*.dat` files against the previous versions (`git diff`) to
   see what changed.
2. Update `PINNED_COMMIT` to the sha you fetched.
3. Run `cargo test -p cl-html --test tree_construction -- --nocapture` and read the summary
   line.
4. Update `expectations.txt`: delete entries for cases that now pass (a stale expectation would
   silently hide a future regression), add entries with honest `# reason` comments — grouped by
   root cause, per the header comment convention already in the file — for any newly-failing
   cases.
5. Commit the refreshed corpus, `PINNED_COMMIT`, and `expectations.txt` together.
