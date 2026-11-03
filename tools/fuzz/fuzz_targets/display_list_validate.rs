#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|dl: cl_paint::DisplayList| {
    // A `DisplayList` crosses the renderer -> gpu process IPC boundary from M1b onward; the
    // gpu process runs `cl_paint::validate` on it before touching any of its geometry
    // (docs/SECURITY.md §3's "renderer -> gpu" trust boundary). `Arbitrary` (the `arbitrary`
    // feature on cl-paint/cl-layout/cl-fonts) builds a structured, adversarial `DisplayList`
    // straight from the fuzzer's bytes rather than this target hand-rolling one field at a
    // time — `validate` must never panic on any of them, whatever `Ok`/`Err` it returns.
    let _ = cl_paint::validate(&dl, cl_layout::Rect::from_px(0.0, 0.0, 800.0, 600.0));
});
