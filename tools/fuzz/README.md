# Fuzz targets

Nightly-only workspace (excluded from root). Run:

    rustup toolchain install nightly
    cargo +nightly fuzz run --fuzz-dir tools/fuzz ipc_decode -- -max_total_time=60   # from the repo root
    cargo +nightly fuzz run --fuzz-dir tools/fuzz url_parse -- -max_total_time=60
    cargo +nightly fuzz run --fuzz-dir tools/fuzz html_parse -- -max_total_time=60
    cargo +nightly fuzz run --fuzz-dir tools/fuzz css_stylesheet -- -max_total_time=60

Targets: `ipc_decode` (cl-ipc codec, both directions), `url_parse` (cl-net `Url::parse` and
`Url::parse_with_base`), `html_parse` (cl-html `parse_document`: encoding sniff, html5ever
tokenizer/tree builder, arena DOM, then cl-dom's html5lib serializer over the result),
`css_stylesheet` (cl-style `StyleEngine::add_author_sheet`: cssparser tokenizing and stylo
rule/selector parsing over an author stylesheet). Every new parser/decoder adds a target in the
same PR (CLAUDE.md §3.1).
