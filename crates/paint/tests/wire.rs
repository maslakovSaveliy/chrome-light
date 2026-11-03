//! M1b Task 6: the renderer → gpu display-list wire format. The decoder faces a hostile
//! renderer, so beyond round-tripping it must refuse — while decoding, before the memory is
//! spent — any list over [`MAX_ITEMS`], [`MAX_GLYPHS_PER_RUN`] or [`MAX_TOTAL_GLYPHS`], any
//! input over the size cap, and any bytes left after the list.
#![cfg(feature = "serde")]
#![allow(
    clippy::expect_used,
    reason = "a failed setup step in a test should abort that test, loudly"
)]

use cl_fonts::FontDb;
use cl_layout::{Au, Glyph, GlyphRun, Point, Rect, Rgba8, Sides};
use cl_paint::wire::{MAX_DISPLAY_LIST_BYTES, WireError, decode_display_list, encode_display_list};
use cl_paint::{DisplayItem, DisplayList, MAX_GLYPHS_PER_RUN, MAX_ITEMS, MAX_TOTAL_GLYPHS};

fn bounds() -> Rect {
    Rect::from_px(0.0, 0.0, 800.0, 600.0)
}

fn font_key() -> cl_fonts::FontKey {
    let db = FontDb::bundled().expect("bundled font db");
    db.key_for("Ahem").expect("Ahem key")
}

fn glyphs(count: usize) -> Vec<Glyph> {
    vec![
        Glyph {
            id: 0,
            x: Au::ZERO,
            y: Au::ZERO,
            advance: Au::ZERO,
        };
        count
    ]
}

fn text(glyph_count: usize) -> DisplayItem {
    DisplayItem::Text {
        run: GlyphRun {
            font: font_key(),
            size: Au::from_px(16.0),
            origin: Point {
                x: Au::from_px(10.0),
                y: Au::from_px(20.0),
            },
            glyphs: glyphs(glyph_count),
            color: Rgba8::BLACK,
        },
    }
}

fn list(items: Vec<DisplayItem>) -> DisplayList {
    DisplayList {
        items,
        bounds: bounds(),
    }
}

/// One item of every kind, with non-trivial field values.
fn every_kind() -> DisplayList {
    let rect = Rect::from_px(1.0, 2.0, 30.0, 40.0);
    list(vec![
        DisplayItem::Rect {
            rect,
            color: Rgba8 {
                r: 1,
                g: 2,
                b: 3,
                a: 4,
            },
        },
        DisplayItem::Border {
            rect,
            widths: Sides {
                top: Au::from_px(1.0),
                right: Au::from_px(2.0),
                bottom: Au::from_px(3.0),
                left: Au::from_px(4.0),
            },
            colors: Sides {
                top: Rgba8::BLACK,
                right: Rgba8::TRANSPARENT,
                bottom: Rgba8::BLACK,
                left: Rgba8::TRANSPARENT,
            },
        },
        DisplayItem::PushClip { rect },
        text(3),
        DisplayItem::PopClip,
    ])
}

/// The postcard varint encoding of `n` (LEB128, as `postcard` writes a sequence length).
fn varint(mut n: u64) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let byte = u8::try_from(n & 0x7f).expect("seven bits fit in a byte");
        n >>= 7;
        if n == 0 {
            out.push(byte);
            return out;
        }
        out.push(byte | 0x80);
    }
}

/// `PopClip`'s variant index in `DisplayItem` — one byte per item on the wire.
const POP_CLIP: u8 = 4;

#[test]
fn every_item_kind_should_roundtrip() {
    let dl = every_kind();
    let bytes = encode_display_list(&dl).expect("encode");
    let decoded = decode_display_list(&bytes, MAX_DISPLAY_LIST_BYTES).expect("decode");
    assert_eq!(decoded, dl);
}

#[test]
fn decoder_should_read_the_derived_postcard_encoding() {
    let dl = every_kind();
    let bytes = postcard::to_allocvec(&dl).expect("derived encode");
    assert_eq!(encode_display_list(&dl).expect("encode"), bytes);
    let decoded = decode_display_list(&bytes, MAX_DISPLAY_LIST_BYTES).expect("decode");
    assert_eq!(decoded, dl);
}

#[test]
fn popclip_flood_should_fail_during_decode() {
    // Declares 10^9 items, then carries one PopClip byte more than MAX_ITEMS allows: the
    // derived decoder would grow a Vec item by item; this one must stop at MAX_ITEMS.
    let mut bytes = varint(1_000_000_000);
    bytes.extend(std::iter::repeat_n(POP_CLIP, MAX_ITEMS + 1));
    assert_eq!(
        decode_display_list(&bytes, MAX_DISPLAY_LIST_BYTES),
        Err(WireError::LimitsExceeded)
    );
}

#[test]
fn declared_item_count_over_the_limit_should_fail_before_reading_items() {
    // The declared count fits the remaining bytes (so postcard reports it), but exceeds
    // MAX_ITEMS: refused up front.
    let declared = MAX_ITEMS + 1;
    let mut bytes = varint(u64::try_from(declared).expect("fits u64"));
    bytes.extend(std::iter::repeat_n(POP_CLIP, declared + 64));
    assert_eq!(
        decode_display_list(&bytes, MAX_DISPLAY_LIST_BYTES),
        Err(WireError::LimitsExceeded)
    );
}

#[test]
fn exactly_max_items_should_decode() {
    let dl = list(vec![DisplayItem::PopClip; MAX_ITEMS]);
    let bytes = postcard::to_allocvec(&dl).expect("derived encode");
    let decoded = decode_display_list(&bytes, MAX_DISPLAY_LIST_BYTES).expect("decode");
    assert_eq!(decoded.items.len(), MAX_ITEMS);
}

#[test]
fn items_over_max_items_should_fail_during_decode() {
    let dl = list(vec![DisplayItem::PopClip; MAX_ITEMS + 1]);
    let bytes = postcard::to_allocvec(&dl).expect("derived encode");
    assert_eq!(
        decode_display_list(&bytes, MAX_DISPLAY_LIST_BYTES),
        Err(WireError::LimitsExceeded)
    );
}

#[test]
fn glyph_run_at_the_per_run_limit_should_decode() {
    let dl = list(vec![text(MAX_GLYPHS_PER_RUN)]);
    let bytes = postcard::to_allocvec(&dl).expect("derived encode");
    assert_eq!(
        decode_display_list(&bytes, MAX_DISPLAY_LIST_BYTES).expect("decode"),
        dl
    );
}

#[test]
fn glyph_vector_over_limit_should_fail_during_decode() {
    let dl = list(vec![text(MAX_GLYPHS_PER_RUN + 1)]);
    let bytes = postcard::to_allocvec(&dl).expect("derived encode");
    assert_eq!(
        decode_display_list(&bytes, MAX_DISPLAY_LIST_BYTES),
        Err(WireError::LimitsExceeded)
    );
}

#[test]
fn total_glyphs_over_limit_should_fail_during_decode() {
    // Every run is within MAX_GLYPHS_PER_RUN; only the sum is over MAX_TOTAL_GLYPHS.
    let runs = MAX_TOTAL_GLYPHS / MAX_GLYPHS_PER_RUN + 1;
    let dl = list((0..runs).map(|_| text(MAX_GLYPHS_PER_RUN)).collect());
    let bytes = postcard::to_allocvec(&dl).expect("derived encode");
    assert!(
        bytes.len() <= MAX_DISPLAY_LIST_BYTES,
        "the input itself is within the cap"
    );
    assert_eq!(
        decode_display_list(&bytes, MAX_DISPLAY_LIST_BYTES),
        Err(WireError::LimitsExceeded)
    );
}

#[test]
fn total_glyphs_at_the_limit_should_decode() {
    let runs = MAX_TOTAL_GLYPHS / MAX_GLYPHS_PER_RUN;
    let dl = list((0..runs).map(|_| text(MAX_GLYPHS_PER_RUN)).collect());
    let bytes = postcard::to_allocvec(&dl).expect("derived encode");
    let decoded = decode_display_list(&bytes, MAX_DISPLAY_LIST_BYTES).expect("decode");
    assert_eq!(decoded.items.len(), runs);
}

#[test]
fn trailing_bytes_should_fail() {
    let mut bytes = encode_display_list(&every_kind()).expect("encode");
    bytes.extend_from_slice(&[0, 0, 0]);
    assert_eq!(
        decode_display_list(&bytes, MAX_DISPLAY_LIST_BYTES),
        Err(WireError::Trailing(3))
    );
}

#[test]
fn oversize_input_should_fail_before_decode() {
    let bytes = vec![0_u8; MAX_DISPLAY_LIST_BYTES + 1];
    assert_eq!(
        decode_display_list(&bytes, MAX_DISPLAY_LIST_BYTES),
        Err(WireError::TooLarge {
            size: MAX_DISPLAY_LIST_BYTES + 1,
            max: MAX_DISPLAY_LIST_BYTES,
        })
    );
}

#[test]
fn input_over_the_callers_cap_should_fail_before_decode() {
    let bytes = encode_display_list(&every_kind()).expect("encode");
    let max = bytes.len() - 1;
    assert_eq!(
        decode_display_list(&bytes, max),
        Err(WireError::TooLarge {
            size: bytes.len(),
            max,
        })
    );
}

#[test]
fn truncated_input_should_be_undecodable() {
    let bytes = encode_display_list(&every_kind()).expect("encode");
    let truncated = bytes.get(..bytes.len() - 1).expect("non-empty");
    assert!(matches!(
        decode_display_list(truncated, MAX_DISPLAY_LIST_BYTES),
        Err(WireError::Undecodable(_))
    ));
}

#[test]
fn unknown_item_variant_should_be_undecodable() {
    let mut bytes = varint(1);
    bytes.push(5);
    assert!(matches!(
        decode_display_list(&bytes, MAX_DISPLAY_LIST_BYTES),
        Err(WireError::Undecodable(_))
    ));
}

#[test]
fn encoder_should_refuse_a_list_the_decoder_would_refuse() {
    // A renderer must learn before sending that its list is over a limit — otherwise the
    // gpu reports it and the browser kills the renderer for a bad message.
    let too_many_items = list(vec![DisplayItem::PopClip; MAX_ITEMS + 1]);
    assert_eq!(
        encode_display_list(&too_many_items),
        Err(WireError::LimitsExceeded)
    );
    let too_many_glyphs = list(vec![text(MAX_GLYPHS_PER_RUN + 1)]);
    assert_eq!(
        encode_display_list(&too_many_glyphs),
        Err(WireError::LimitsExceeded)
    );
}
