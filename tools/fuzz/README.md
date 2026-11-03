# Fuzz targets

Nightly-only workspace (excluded from root). Run:

    rustup toolchain install nightly
    cargo +nightly fuzz run --fuzz-dir tools/fuzz ipc_decode -- -max_total_time=60   # from the repo root
    cargo +nightly fuzz run --fuzz-dir tools/fuzz url_parse -- -max_total_time=60
    cargo +nightly fuzz run --fuzz-dir tools/fuzz html_parse -- -max_total_time=60
    cargo +nightly fuzz run --fuzz-dir tools/fuzz css_stylesheet -- -max_total_time=60
    cargo +nightly fuzz run --fuzz-dir tools/fuzz display_list_validate -- -max_total_time=60
    cargo +nightly fuzz run --fuzz-dir tools/fuzz display_list_rasterize -- -max_total_time=60
    cargo +nightly fuzz run --fuzz-dir tools/fuzz display_list_decode -- -max_total_time=60

Targets: `ipc_decode` (cl-ipc codec, both directions), `url_parse` (cl-net `Url::parse` and
`Url::parse_with_base`), `html_parse` (cl-html `parse_document`: encoding sniff, html5ever
tokenizer/tree builder, arena DOM, then cl-dom's html5lib serializer over the result),
`css_stylesheet` (cl-style `StyleEngine::add_author_sheet`: cssparser tokenizing and stylo
rule/selector parsing over an author stylesheet), `display_list_validate` (cl-paint
`validate`: schema-checks a structured, `Arbitrary`-generated `DisplayList` — the renderer ->
gpu process boundary's receiver-side check, Task 20 — against a fixed 800x600 bounds; built
with cl-paint's `arbitrary` feature, which forwards through cl-layout to cl-fonts so every
geometry/text/font type a `DisplayList` embeds derives `arbitrary::Arbitrary`),
`display_list_rasterize` (cl-gfx `cpu::rasterize` over the same `Arbitrary`-generated
`DisplayList` on a 64x64 canvas — the *consumer* behind that boundary check: `rasterize` runs
`validate` itself and refuses an invalid list, so this target exercises the arithmetic past the
gate, e.g. border strips whose top and bottom widths each validate but together exceed the box
height. The bundled `FontDb` is built once per thread, not per iteration: measured 4.3 us to
build against 4.2 us per `rasterize`, so caching it roughly doubles the iteration rate), `display_list_decode` (cl-paint
`wire::decode_display_list` over raw bytes, then `validate` on whatever decodes — the gpu's
receive path for the renderer's display list, M1b Task 6; built with cl-paint's `serde`
feature; asserts a decoded list is within the item/glyph limits and that the result agrees
with the derived postcard decoding, except where the bounded decoder refuses for a limit.
The default `-max_len` (4096) keeps inputs far below the limits, whose paths are covered by
`crates/paint/tests/wire.rs`). Every
new parser/decoder adds a target in the same PR (CLAUDE.md §3.1).
