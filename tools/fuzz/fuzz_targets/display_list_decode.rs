#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // The gpu process decodes the renderer's display-list bytes (copied out of shm) with
    // `cl_paint::wire::decode_display_list`, then validates the result against the surface
    // size (docs/adr/0016-m1b-raster-service-and-frame-transport.md §1, §4). Neither step may
    // panic, and the decoder must stay within its item and glyph limits whatever the bytes.
    if let Ok(dl) =
        cl_paint::wire::decode_display_list(data, cl_paint::wire::MAX_DISPLAY_LIST_BYTES)
    {
        let _ = cl_paint::validate(&dl, cl_layout::Rect::from_px(0.0, 0.0, 800.0, 600.0));
    }
});
