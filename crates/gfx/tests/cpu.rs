//! Behavioural tests for [`cl_gfx::cpu::rasterize`]: exact pixel values on a small canvas.
//!
//! Every assertion here names concrete pixels rather than a hash or a golden file: the CPU
//! backend's whole job in M1a is to be *exactly* predictable (anti-aliasing off for rects and
//! borders, whole-pixel glyph placement, no system fonts), so a test that cannot say which
//! byte it expects would not be testing that property at all.
#![allow(
    clippy::expect_used,
    reason = "a failed setup step in a test should abort that test, loudly"
)]
#![allow(
    clippy::unreadable_literal,
    reason = "pixel coordinates and 8-bit channel values read better bare"
)]

use cl_fonts::FontDb;
use cl_gfx::{GfxError, Pixmap, cpu};
use cl_layout::{Au, Glyph, GlyphRun, Point, Rect, Rgba8, Sides};
use cl_paint::{DisplayItem, DisplayList, DisplayListError};

/// The canvas every test in this file rasterises against, in CSS pixels.
const CANVAS: u32 = 32;

/// Opaque white: the canvas colour [`cl_gfx::cpu::rasterize`] starts from.
const WHITE: (u8, u8, u8, u8) = (255, 255, 255, 255);

/// Wraps `items` in a [`DisplayList`] whose bounds are the whole test canvas.
fn list(items: Vec<DisplayItem>) -> DisplayList {
    DisplayList {
        items,
        #[allow(
            clippy::cast_precision_loss,
            reason = "CANVAS is 32; every value here is exact in f32"
        )]
        bounds: Rect::from_px(0.0, 0.0, CANVAS as f32, CANVAS as f32),
    }
}

/// Reads one pixel back as straight (demultiplied) RGBA, the form the test's expectations
/// are written in.
fn px(pixmap: &Pixmap, x: u32, y: u32) -> (u8, u8, u8, u8) {
    let p = pixmap
        .pixel(x, y)
        .expect("pixel coordinates must be inside the canvas")
        .demultiply();
    (p.red(), p.green(), p.blue(), p.alpha())
}

/// Straight RGBA helper, so a test reads `rgba(255, 0, 0, 255)` rather than a struct literal.
fn rgba(r: u8, g: u8, b: u8, a: u8) -> Rgba8 {
    Rgba8 { r, g, b, a }
}

/// Rasterises `items` on the standard canvas with the bundled fonts.
fn raster(items: Vec<DisplayItem>) -> Pixmap {
    let fonts = FontDb::bundled().expect("bundled font db");
    cpu::rasterize(&list(items), CANVAS, CANVAS, &fonts).expect("rasterize")
}

/// Ahem's glyph id for `x` (U+0078) in the bundled asset (`assets/SHA256SUMS` pins the file;
/// `cl_fonts`'s own `ahem_glyph_should_be_square_em` test proves that glyph is the full em
/// square). Hard-coded rather than looked up because `cl-gfx`'s integration tests do not link
/// a font-introspection crate of their own; if the bundled asset is ever replaced this test
/// fails loudly rather than silently drawing a different glyph.
const AHEM_X_GLYPH: u16 = 90;

#[test]
fn rect_should_fill_exact_pixels() {
    let pixmap = raster(vec![DisplayItem::Rect {
        rect: Rect::from_px(5.0, 5.0, 10.0, 10.0),
        color: rgba(255, 0, 0, 255),
    }]);

    assert_eq!(px(&pixmap, 5, 5), (255, 0, 0, 255), "top-left of the rect");
    assert_eq!(px(&pixmap, 4, 4), WHITE, "just outside the top-left");
    assert_eq!(
        px(&pixmap, 14, 14),
        (255, 0, 0, 255),
        "bottom-right of the rect"
    );
    assert_eq!(px(&pixmap, 15, 15), WHITE, "just outside the bottom-right");
}

#[test]
fn clip_should_restrict_children() {
    let pixmap = raster(vec![
        DisplayItem::PushClip {
            rect: Rect::from_px(0.0, 0.0, 10.0, 10.0),
        },
        DisplayItem::Rect {
            rect: Rect::from_px(0.0, 0.0, 20.0, 20.0),
            color: rgba(0, 0, 255, 255),
        },
        DisplayItem::PopClip,
    ]);

    assert_eq!(px(&pixmap, 5, 5), (0, 0, 255, 255), "inside the clip");
    assert_eq!(
        px(&pixmap, 9, 9),
        (0, 0, 255, 255),
        "last pixel inside the clip"
    );
    assert_eq!(px(&pixmap, 10, 10), WHITE, "first pixel past the clip");
    assert_eq!(px(&pixmap, 15, 15), WHITE, "well past the clip");
}

#[test]
fn clip_should_be_restored_by_pop() {
    let pixmap = raster(vec![
        DisplayItem::PushClip {
            rect: Rect::from_px(0.0, 0.0, 10.0, 10.0),
        },
        DisplayItem::PopClip,
        DisplayItem::Rect {
            rect: Rect::from_px(0.0, 0.0, 20.0, 20.0),
            color: rgba(0, 0, 255, 255),
        },
    ]);

    assert_eq!(
        px(&pixmap, 15, 15),
        (0, 0, 255, 255),
        "the popped clip must not still be in force"
    );
}

#[test]
fn border_should_paint_four_sides() {
    // A 16x16 border box at (4, 4) with 2px sides. Corners belong to the top and bottom
    // strips (M1a: no mitres) -- see `cl_gfx::cpu`'s docs.
    let pixmap = raster(vec![DisplayItem::Border {
        rect: Rect::from_px(4.0, 4.0, 16.0, 16.0),
        widths: Sides::uniform(Au::from_px(2.0)),
        colors: Sides {
            top: rgba(255, 0, 0, 255),
            right: rgba(0, 255, 0, 255),
            bottom: rgba(0, 0, 255, 255),
            left: rgba(255, 0, 255, 255),
        },
    }]);

    assert_eq!(px(&pixmap, 10, 4), (255, 0, 0, 255), "top strip, outer row");
    assert_eq!(px(&pixmap, 10, 5), (255, 0, 0, 255), "top strip, inner row");
    assert_eq!(
        px(&pixmap, 18, 10),
        (0, 255, 0, 255),
        "right strip, inner column"
    );
    assert_eq!(
        px(&pixmap, 19, 10),
        (0, 255, 0, 255),
        "right strip, outer column"
    );
    assert_eq!(
        px(&pixmap, 10, 19),
        (0, 0, 255, 255),
        "bottom strip, outer row"
    );
    assert_eq!(
        px(&pixmap, 10, 18),
        (0, 0, 255, 255),
        "bottom strip, inner row"
    );
    assert_eq!(
        px(&pixmap, 4, 10),
        (255, 0, 255, 255),
        "left strip, outer column"
    );
    assert_eq!(
        px(&pixmap, 5, 10),
        (255, 0, 255, 255),
        "left strip, inner column"
    );

    assert_eq!(
        px(&pixmap, 4, 4),
        (255, 0, 0, 255),
        "top-left corner is top"
    );
    assert_eq!(
        px(&pixmap, 19, 4),
        (255, 0, 0, 255),
        "top-right corner is top"
    );
    assert_eq!(
        px(&pixmap, 4, 19),
        (0, 0, 255, 255),
        "bottom-left corner is bottom"
    );
    assert_eq!(
        px(&pixmap, 19, 19),
        (0, 0, 255, 255),
        "bottom-right corner is bottom"
    );

    assert_eq!(px(&pixmap, 12, 12), WHITE, "the border box's inside");
    assert_eq!(px(&pixmap, 3, 10), WHITE, "just outside the left edge");
    assert_eq!(px(&pixmap, 20, 10), WHITE, "just outside the right edge");
}

#[test]
fn border_should_skip_zero_width_sides() {
    let pixmap = raster(vec![DisplayItem::Border {
        rect: Rect::from_px(4.0, 4.0, 16.0, 16.0),
        widths: Sides {
            top: Au::from_px(2.0),
            right: Au::ZERO,
            bottom: Au::ZERO,
            left: Au::ZERO,
        },
        colors: Sides::uniform(rgba(255, 0, 0, 255)),
    }]);

    assert_eq!(px(&pixmap, 10, 4), (255, 0, 0, 255), "top strip is painted");
    assert_eq!(
        px(&pixmap, 4, 10),
        WHITE,
        "zero-width left side paints none"
    );
    assert_eq!(
        px(&pixmap, 19, 10),
        WHITE,
        "zero-width right side paints none"
    );
    assert_eq!(
        px(&pixmap, 10, 19),
        WHITE,
        "zero-width bottom side paints none"
    );
}

/// One Ahem glyph at 16 px, with the run origin (the baseline) at `(0, 16)`.
///
/// Ahem's em box runs from `+0.8 em` above the baseline to `-0.2 em` below it, so at 16 px
/// the square spans `y in [16 - 12.8, 16 + 3.2] = [3.2, 19.2]`: **16 px tall, but not
/// aligned to the pixel grid at any whole-pixel baseline**, since `0.8 * 16 = 12.8` is not an
/// integer. The rasteriser therefore composites a 16x17 alpha mask whose 15 interior rows are
/// fully covered and whose two fringe rows carry exactly the fractional coverage (`0.8` ->
/// alpha 204, `0.2` -> alpha 52). Black over white at those alphas is `255 - alpha`. See
/// `ahem_square_should_be_exact_when_the_em_box_is_pixel_aligned` for the same glyph at a
/// size where the em box *is* grid-aligned and the square comes out exactly.
#[test]
fn text_should_paint_ahem_square() {
    let fonts = FontDb::bundled().expect("bundled font db");
    let font = fonts.key_for("Ahem").expect("Ahem is bundled");
    let pixmap = raster(vec![DisplayItem::Text {
        run: GlyphRun {
            font,
            size: Au::from_px(16.0),
            origin: Point {
                x: Au::ZERO,
                y: Au::from_px(16.0),
            },
            glyphs: vec![Glyph {
                id: AHEM_X_GLYPH,
                x: Au::ZERO,
                y: Au::ZERO,
                advance: Au::from_px(16.0),
            }],
            color: Rgba8::BLACK,
        },
    }]);

    // The 15 fully covered rows, 3 + 1 ..= 3 + 15, over the full 16-column width.
    for y in 4..=18 {
        for x in 0..16 {
            assert_eq!(
                px(&pixmap, x, y),
                (0, 0, 0, 255),
                "solid square at ({x},{y})"
            );
        }
    }
    // The two fractional-coverage fringe rows.
    for x in 0..16 {
        assert_eq!(
            px(&pixmap, x, 3),
            (51, 51, 51, 255),
            "top fringe at ({x},3)"
        );
        assert_eq!(
            px(&pixmap, x, 19),
            (203, 203, 203, 255),
            "bottom fringe at ({x},19)"
        );
    }
    // All four edges: nothing is painted outside the glyph's 16x17 box.
    for x in 0..17 {
        assert_eq!(px(&pixmap, x, 2), WHITE, "row above the glyph at x={x}");
        assert_eq!(px(&pixmap, x, 20), WHITE, "row below the glyph at x={x}");
    }
    for y in 2..21 {
        assert_eq!(
            px(&pixmap, 16, y),
            WHITE,
            "column right of the glyph at y={y}"
        );
    }
}

/// The same Ahem glyph at 20 px, where `0.8 em = 16 px` and `0.2 em = 4 px` are both whole
/// pixels: the em box lands exactly on the grid and the rasteriser produces an exactly
/// 20x20 fully black square with no anti-aliased fringe at all. This is what proves the
/// fringe in `text_should_paint_ahem_square` comes from Ahem's `12.8 px` ascent and not from
/// the rasteriser positioning the glyph imprecisely.
#[test]
fn ahem_square_should_be_exact_when_the_em_box_is_pixel_aligned() {
    let fonts = FontDb::bundled().expect("bundled font db");
    let font = fonts.key_for("Ahem").expect("Ahem is bundled");
    let pixmap = raster(vec![DisplayItem::Text {
        run: GlyphRun {
            font,
            size: Au::from_px(20.0),
            origin: Point {
                x: Au::ZERO,
                y: Au::from_px(20.0),
            },
            glyphs: vec![Glyph {
                id: AHEM_X_GLYPH,
                x: Au::ZERO,
                y: Au::ZERO,
                advance: Au::from_px(20.0),
            }],
            color: Rgba8::BLACK,
        },
    }]);

    for y in 4..24 {
        for x in 0..20 {
            assert_eq!(px(&pixmap, x, y), (0, 0, 0, 255), "square at ({x},{y})");
        }
    }
    for x in 0..21 {
        assert_eq!(px(&pixmap, x, 3), WHITE, "row above the square at x={x}");
        assert_eq!(px(&pixmap, x, 24), WHITE, "row below the square at x={x}");
    }
    for y in 3..25 {
        assert_eq!(
            px(&pixmap, 20, y),
            WHITE,
            "column right of the square at y={y}"
        );
    }
}

#[test]
fn glyph_cache_should_hit_on_second_use() {
    let fonts = FontDb::bundled().expect("bundled font db");
    let font = fonts.key_for("Ahem").expect("Ahem is bundled");
    let items = vec![DisplayItem::Text {
        run: GlyphRun {
            font,
            size: Au::from_px(16.0),
            origin: Point {
                x: Au::ZERO,
                y: Au::from_px(16.0),
            },
            glyphs: vec![
                Glyph {
                    id: AHEM_X_GLYPH,
                    x: Au::ZERO,
                    y: Au::ZERO,
                    advance: Au::from_px(16.0),
                },
                Glyph {
                    id: AHEM_X_GLYPH,
                    x: Au::from_px(16.0),
                    y: Au::ZERO,
                    advance: Au::from_px(16.0),
                },
            ],
            color: Rgba8::BLACK,
        },
    }];

    let (_pixmap, stats) =
        cpu::rasterize_with_stats(&list(items), CANVAS, CANVAS, &fonts).expect("rasterize");
    assert_eq!(
        stats.glyphs_rendered, 1,
        "the same glyph at the same size must only be rasterised once"
    );
    assert_eq!(
        stats.cache_hits, 1,
        "the second use of that glyph must come from the cache"
    );
}

#[test]
fn rasterize_should_reject_invalid_list() {
    let fonts = FontDb::bundled().expect("bundled font db");
    let err = cpu::rasterize(&list(vec![DisplayItem::PopClip]), CANVAS, CANVAS, &fonts)
        .expect_err("a lone PopClip must be rejected");
    assert!(
        matches!(
            err,
            GfxError::Invalid(DisplayListError::PopWithoutPush { index: 0 })
        ),
        "expected Invalid(PopWithoutPush), got {err:?}"
    );
}

#[test]
fn oversized_glyph_should_be_skipped_not_allocated() {
    let fonts = FontDb::bundled().expect("bundled font db");
    let font = fonts.key_for("Ahem").expect("Ahem is bundled");
    let pixmap = raster(vec![DisplayItem::Text {
        run: GlyphRun {
            font,
            // Far past MAX_GLYPH_PX: a hostile display list must not make the raster process
            // allocate a 10000x10000 alpha mask.
            size: Au::from_px(10_000.0),
            origin: Point {
                x: Au::ZERO,
                y: Au::from_px(16.0),
            },
            glyphs: vec![Glyph {
                id: AHEM_X_GLYPH,
                x: Au::ZERO,
                y: Au::ZERO,
                advance: Au::from_px(10_000.0),
            }],
            color: Rgba8::BLACK,
        },
    }]);

    for y in 0..CANVAS {
        for x in 0..CANVAS {
            assert_eq!(
                px(&pixmap, x, y),
                WHITE,
                "canvas must be untouched at ({x},{y})"
            );
        }
    }
}

/// `cl_paint::validate` accepts a `Border` whose `top` and `bottom` widths are each no larger
/// than the box's height but whose *sum* exceeds it (`border_widths_valid` checks each side
/// individually, by design — see its docs), so a hostile or merely odd list can reach
/// `fill_border` with no middle band at all. It must paint the two horizontal strips, skip the
/// left/right strips entirely rather than computing a negative band height, and not panic.
#[test]
fn border_should_handle_top_plus_bottom_exceeding_height() {
    let red = rgba(255, 0, 0, 255);
    let green = rgba(0, 255, 0, 255);
    let blue = rgba(0, 0, 255, 255);
    let magenta = rgba(255, 0, 255, 255);
    let items = vec![DisplayItem::Border {
        // A 16px-tall box with 12px top and 12px bottom borders: 12 <= 16 on each side, so the
        // list is valid, but 12 + 12 > 16 leaves no room between them.
        rect: Rect::from_px(4.0, 4.0, 16.0, 16.0),
        widths: Sides {
            top: Au::from_px(12.0),
            right: Au::from_px(2.0),
            bottom: Au::from_px(12.0),
            left: Au::from_px(2.0),
        },
        colors: Sides {
            top: red,
            right: green,
            bottom: blue,
            left: magenta,
        },
    }];
    let dl = list(items.clone());
    assert_eq!(
        cl_paint::validate(&dl, dl.bounds),
        Ok(()),
        "the premise: this list is one validate accepts"
    );

    let pixmap = raster(items);

    assert_eq!(
        px(&pixmap, 10, 4),
        (255, 0, 0, 255),
        "top strip's first row"
    );
    assert_eq!(
        px(&pixmap, 10, 7),
        (255, 0, 0, 255),
        "top strip, above where the bottom strip starts"
    );
    assert_eq!(
        px(&pixmap, 10, 8),
        (0, 0, 255, 255),
        "the bottom strip starts at y = bottom() - 12 = 8 and paints over the top strip"
    );
    assert_eq!(
        px(&pixmap, 10, 19),
        (0, 0, 255, 255),
        "bottom strip's last row"
    );
    assert_eq!(
        px(&pixmap, 4, 10),
        (0, 0, 255, 255),
        "no middle band, so the left strip is never painted - the bottom strip spans the \
         box's full width here"
    );
    assert_eq!(px(&pixmap, 3, 10), WHITE, "just outside the box");
    assert_eq!(px(&pixmap, 20, 10), WHITE, "just outside the box");
}

/// `cl_paint::validate` deliberately treats a [`cl_fonts::FontKey`] as opaque data (a key is
/// valid for exactly one process's font database, which only that process can check), so a
/// glyph run naming a key this process does not know is a *raster*-time condition: expected
/// from M1b on, when the key crosses IPC from a renderer holding a stale database.
/// `rasterize` must report it as [`GfxError::FontNotBundled`] rather than silently dropping
/// the text or panicking.
///
/// Needs `cl_fonts::FontKey::from_raw` (doc-hidden, `arbitrary`-feature-gated, enabled here as
/// a dev-dependency feature): every key a real `FontDb` hands out resolves by construction, so
/// this path has no in-process input otherwise.
#[test]
fn rasterize_should_report_font_not_bundled() {
    let fonts = FontDb::bundled().expect("bundled font db");
    // The bundled database registers exactly two faces, at indices 0 and 1.
    let stale = cl_fonts::FontKey::from_raw(999);
    let dl = list(vec![DisplayItem::Text {
        run: GlyphRun {
            font: stale,
            size: Au::from_px(16.0),
            origin: Point {
                x: Au::ZERO,
                y: Au::from_px(16.0),
            },
            glyphs: vec![Glyph {
                id: AHEM_X_GLYPH,
                x: Au::ZERO,
                y: Au::ZERO,
                advance: Au::from_px(16.0),
            }],
            color: Rgba8::BLACK,
        },
    }]);

    let err = cpu::rasterize(&dl, CANVAS, CANVAS, &fonts)
        .expect_err("a stale font key must be reported, not ignored");
    assert!(
        matches!(err, GfxError::FontNotBundled { key } if key == stale),
        "expected FontNotBundled({stale:?}), got {err:?}"
    );
}

#[test]
fn rasterize_should_reject_impossible_canvas_sizes() {
    let fonts = FontDb::bundled().expect("bundled font db");
    let empty = list(Vec::new());
    for (w, h) in [(0, 32), (32, 0), (16_385, 32), (32, 16_385)] {
        let err = cpu::rasterize(&empty, w, h, &fonts).expect_err("{w}x{h} must be rejected");
        assert!(
            matches!(err, GfxError::PixmapTooLarge { width, height } if width == w && height == h),
            "expected PixmapTooLarge for {w}x{h}, got {err:?}"
        );
    }
}

#[test]
fn rasterize_should_be_deterministic() {
    let fonts = FontDb::bundled().expect("bundled font db");
    let font = fonts.key_for("Ahem").expect("Ahem is bundled");
    let items = vec![
        DisplayItem::Rect {
            rect: Rect::from_px(0.0, 0.0, 32.0, 32.0),
            color: rgba(200, 220, 240, 255),
        },
        DisplayItem::PushClip {
            rect: Rect::from_px(2.0, 2.0, 24.0, 24.0),
        },
        DisplayItem::Border {
            rect: Rect::from_px(3.0, 3.0, 20.0, 20.0),
            widths: Sides::uniform(Au::from_px(1.0)),
            colors: Sides::uniform(rgba(10, 20, 30, 255)),
        },
        DisplayItem::Text {
            run: GlyphRun {
                font,
                size: Au::from_px(16.0),
                origin: Point {
                    x: Au::from_px(4.0),
                    y: Au::from_px(20.0),
                },
                glyphs: vec![Glyph {
                    id: AHEM_X_GLYPH,
                    x: Au::ZERO,
                    y: Au::ZERO,
                    advance: Au::from_px(16.0),
                }],
                color: Rgba8::BLACK,
            },
        },
        DisplayItem::PopClip,
    ];
    let dl = list(items);

    let a = cpu::rasterize(&dl, CANVAS, CANVAS, &fonts).expect("first raster");
    let b = cpu::rasterize(&dl, CANVAS, CANVAS, &fonts).expect("second raster");
    assert_eq!(
        a.data(),
        b.data(),
        "the same display list must give the same bytes"
    );
}
