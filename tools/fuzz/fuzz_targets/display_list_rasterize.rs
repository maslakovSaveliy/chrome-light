#![no_main]

use std::cell::OnceCell;

use libfuzzer_sys::fuzz_target;

thread_local! {
    /// The bundled font database, built once per fuzzing thread rather than per iteration.
    ///
    /// Measured on this machine (release, macOS arm64): `cl_fonts::FontDb::bundled` costs
    /// 4.3 us per call and one `cl_gfx::cpu::rasterize` of a small list on a 64x64 canvas
    /// costs 4.2 us. Construction does *not* dominate — it is roughly one to one — but it is
    /// still half of every iteration's work for no information gained, since the database is
    /// identical every time and `rasterize` only borrows it, so building it once roughly
    /// doubles the iteration rate. `thread_local` rather than a `static`: `FontDb` wraps
    /// `fontique` types that are not `Sync`, and libFuzzer drives one thread anyway.
    static FONTS: OnceCell<Option<cl_fonts::FontDb>> = const { OnceCell::new() };
}

fuzz_target!(|dl: cl_paint::DisplayList| {
    // The other half of the renderer -> gpu trust boundary (docs/SECURITY.md §3): the sibling
    // target `display_list_validate` proves the schema check itself never panics, and this one
    // proves the thing that check exists to protect never panics either.
    // `cl_gfx::cpu::rasterize` runs `cl_paint::validate` first and refuses an invalid list, so
    // many inputs stop at the gate — what this target hunts is the arithmetic *past* it: border
    // strips whose top and bottom widths each pass validation but together exceed the box
    // height, clip push/pop stacks, glyph masks sized from an attacker-chosen `font-size`, and
    // rects near `i32`'s extremes.
    FONTS.with(|cell| {
        let built = cell.get_or_init(|| cl_fonts::FontDb::bundled().ok());
        if let Some(fonts) = built.as_ref() {
            let _ = cl_gfx::cpu::rasterize(&dl, 64, 64, fonts);
        }
    });
});
