#![no_main]

use cl_paint::wire::{MAX_DISPLAY_LIST_BYTES, WireError, decode_display_list};
use cl_paint::{DisplayItem, DisplayList, MAX_GLYPHS_PER_RUN, MAX_ITEMS, MAX_TOTAL_GLYPHS};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // The gpu process decodes the renderer's display-list bytes (copied out of shm) with
    // `cl_paint::wire::decode_display_list`, then validates the result against the surface
    // size (docs/adr/0016-m1b-raster-service-and-frame-transport.md §1, §4). Neither step may
    // panic, a decoded list is within the item and glyph limits, and the bounded decoder
    // agrees with the derived postcard decoding on every input it does not refuse for a
    // limit. libFuzzer's default -max_len (4096) keeps the inputs far below the limits, so
    // the limit paths themselves are covered by crates/paint/tests/wire.rs, not here.
    let ours = decode_display_list(data, MAX_DISPLAY_LIST_BYTES);
    let derived = postcard::take_from_bytes::<DisplayList>(data)
        .ok()
        .filter(|(_, rest)| rest.is_empty())
        .map(|(dl, _)| dl);
    match ours {
        Ok(dl) => {
            assert_eq!(
                derived.as_ref(),
                Some(&dl),
                "the derived decoding disagrees"
            );
            assert!(dl.items.len() <= MAX_ITEMS);
            assert!(!over_limits(&dl));
            let _ = cl_paint::validate(&dl, cl_layout::Rect::from_px(0.0, 0.0, 800.0, 600.0));
        }
        Err(WireError::Undecodable(_) | WireError::Trailing(_)) => {
            assert!(
                derived.is_none(),
                "the derived decoding accepts what we reject"
            );
        }
        Err(WireError::LimitsExceeded) => {
            // Refused for a limit: the derived decoding must fail too, or produce a list that
            // really is over one — never a legitimate list.
            if let Some(dl) = &derived {
                assert!(over_limits(dl), "a list within the limits was refused");
            }
        }
        Err(WireError::TooLarge { .. }) => {}
    }
});

/// Whether `dl` is over `MAX_ITEMS`, `MAX_GLYPHS_PER_RUN` or `MAX_TOTAL_GLYPHS`.
fn over_limits(dl: &DisplayList) -> bool {
    let mut total: usize = 0;
    for item in &dl.items {
        if let DisplayItem::Text { run } = item {
            if run.glyphs.len() > MAX_GLYPHS_PER_RUN {
                return true;
            }
            total = total.saturating_add(run.glyphs.len());
        }
    }
    dl.items.len() > MAX_ITEMS || total > MAX_TOTAL_GLYPHS
}
