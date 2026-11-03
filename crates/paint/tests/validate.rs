//! Behavioural tests for [`cl_paint::validate`]: exact error variant and index for every
//! rule the module docs describe, plus the "every golden `build()` output validates clean"
//! constraint (ruling 5 of Task 20's controller review).
#![allow(
    clippy::expect_used,
    reason = "a failed setup step in a test should abort that test, loudly"
)]

#[path = "common/mod.rs"]
mod common;

use cl_fonts::FontDb;
use cl_layout::{Au, Glyph, GlyphRun, Point, Rect, Rgba8, Sides, Size};
use cl_paint::{
    DisplayItem, DisplayList, DisplayListError, MAX_CLIP_DEPTH, MAX_GLYPHS_PER_RUN, MAX_ITEMS,
    build, validate,
};

/// The bounds every test validates against, matching the fuzz target's own viewport.
fn bounds() -> Rect {
    Rect::from_px(0.0, 0.0, 800.0, 600.0)
}

/// A real [`cl_fonts::FontKey`] from the bundled database — `FontKey`'s inner field is
/// private to `cl-fonts`, so a hand-built [`GlyphRun`] needs one of these rather than a
/// literal.
fn font_key() -> cl_fonts::FontKey {
    let db = FontDb::bundled().expect("bundled font db");
    db.key_for("Ahem").expect("Ahem key")
}

/// A minimal, otherwise-valid glyph run at `origin`, for tests that only care about one
/// specific rule.
fn text_run(origin: Point, glyphs: Vec<Glyph>) -> GlyphRun {
    GlyphRun {
        font: font_key(),
        size: Au::from_px(16.0),
        origin,
        glyphs,
        color: Rgba8::BLACK,
    }
}

#[test]
fn valid_list_should_pass() {
    let dl = DisplayList {
        items: vec![
            DisplayItem::Rect {
                rect: Rect::from_px(10.0, 10.0, 100.0, 50.0),
                color: Rgba8::BLACK,
            },
            DisplayItem::Border {
                rect: Rect::from_px(10.0, 10.0, 100.0, 50.0),
                widths: Sides::uniform(Au::from_px(2.0)),
                colors: Sides::uniform(Rgba8::BLACK),
            },
            DisplayItem::PushClip {
                rect: Rect::from_px(10.0, 10.0, 100.0, 50.0),
            },
            DisplayItem::Text {
                run: text_run(
                    Point {
                        x: Au::from_px(12.0),
                        y: Au::from_px(30.0),
                    },
                    vec![Glyph {
                        id: 1,
                        x: Au::ZERO,
                        y: Au::ZERO,
                        advance: Au::from_px(10.0),
                    }],
                ),
            },
            DisplayItem::PopClip,
        ],
        bounds: bounds(),
    };

    assert_eq!(validate(&dl, bounds()), Ok(()));
}

#[test]
fn pop_without_push_should_fail() {
    let dl = DisplayList {
        items: vec![
            DisplayItem::Rect {
                rect: Rect::from_px(0.0, 0.0, 10.0, 10.0),
                color: Rgba8::BLACK,
            },
            DisplayItem::PopClip,
        ],
        bounds: bounds(),
    };

    assert_eq!(
        validate(&dl, bounds()),
        Err(DisplayListError::PopWithoutPush { index: 1 })
    );
}

#[test]
fn unbalanced_push_should_fail() {
    let dl = DisplayList {
        items: vec![DisplayItem::PushClip {
            rect: Rect::from_px(0.0, 0.0, 10.0, 10.0),
        }],
        bounds: bounds(),
    };

    assert_eq!(
        validate(&dl, bounds()),
        Err(DisplayListError::UnbalancedClip { depth_at_end: 1 })
    );
}

#[test]
fn negative_size_should_fail() {
    let dl = DisplayList {
        items: vec![DisplayItem::Rect {
            rect: Rect {
                origin: Point::default(),
                size: Size {
                    w: Au(-1),
                    h: Au::from_px(10.0),
                },
            },
            color: Rgba8::BLACK,
        }],
        bounds: bounds(),
    };

    assert_eq!(
        validate(&dl, bounds()),
        Err(DisplayListError::InvalidRect { index: 0 })
    );
}

#[test]
fn overflowing_rect_should_fail_not_panic() {
    let dl = DisplayList {
        items: vec![DisplayItem::Rect {
            rect: Rect {
                origin: Point {
                    x: Au(i32::MAX),
                    y: Au::ZERO,
                },
                size: Size {
                    w: Au::from_px(1.0),
                    h: Au::from_px(1.0),
                },
            },
            color: Rgba8::BLACK,
        }],
        bounds: bounds(),
    };

    assert_eq!(
        validate(&dl, bounds()),
        Err(DisplayListError::InvalidRect { index: 0 })
    );
}

#[test]
fn too_many_items_should_fail() {
    let dl = DisplayList {
        items: vec![DisplayItem::PopClip; MAX_ITEMS + 1],
        bounds: bounds(),
    };

    assert_eq!(
        validate(&dl, bounds()),
        Err(DisplayListError::TooManyItems {
            count: MAX_ITEMS + 1,
            max: MAX_ITEMS,
        })
    );
}

#[test]
fn too_many_glyphs_should_fail() {
    let glyphs = vec![
        Glyph {
            id: 1,
            x: Au::ZERO,
            y: Au::ZERO,
            advance: Au::from_px(1.0),
        };
        MAX_GLYPHS_PER_RUN + 1
    ];
    let dl = DisplayList {
        items: vec![DisplayItem::Text {
            run: text_run(Point::default(), glyphs),
        }],
        bounds: bounds(),
    };

    assert_eq!(
        validate(&dl, bounds()),
        Err(DisplayListError::TooManyGlyphs { index: 0 })
    );
}

#[test]
fn too_deep_clip_should_fail() {
    let mut items = Vec::new();
    for _ in 0..=MAX_CLIP_DEPTH {
        items.push(DisplayItem::PushClip {
            rect: Rect::from_px(0.0, 0.0, 10.0, 10.0),
        });
    }
    let dl = DisplayList {
        items,
        bounds: bounds(),
    };

    assert_eq!(
        validate(&dl, bounds()),
        Err(DisplayListError::TooDeepClip {
            index: MAX_CLIP_DEPTH,
            max: MAX_CLIP_DEPTH,
        })
    );
}

#[test]
fn invalid_bounds_should_fail() {
    let dl = DisplayList {
        items: Vec::new(),
        bounds: Rect::from_px(0.0, 0.0, 0.0, 600.0),
    };

    assert_eq!(
        validate(&dl, Rect::from_px(0.0, 0.0, 0.0, 600.0)),
        Err(DisplayListError::InvalidBounds)
    );
}

#[test]
fn border_width_exceeding_rect_should_fail() {
    let rect = Rect::from_px(0.0, 0.0, 10.0, 10.0);
    let dl = DisplayList {
        items: vec![DisplayItem::Border {
            rect,
            widths: Sides {
                top: Au::from_px(20.0),
                right: Au::ZERO,
                bottom: Au::ZERO,
                left: Au::ZERO,
            },
            colors: Sides::uniform(Rgba8::BLACK),
        }],
        bounds: bounds(),
    };

    assert_eq!(
        validate(&dl, bounds()),
        Err(DisplayListError::InvalidRect { index: 0 })
    );
}

#[test]
fn rect_beyond_the_offscreen_margin_should_fail() {
    let dl = DisplayList {
        items: vec![DisplayItem::Rect {
            rect: Rect::from_px(100_000.0, 0.0, 10.0, 10.0),
            color: Rgba8::BLACK,
        }],
        bounds: bounds(),
    };

    assert_eq!(
        validate(&dl, bounds()),
        Err(DisplayListError::OutOfBounds { index: 0 })
    );
}

#[test]
fn rect_within_the_offscreen_margin_should_pass() {
    // Just inside bounds (0,0,800,600) inflated by 4096px on every side.
    let dl = DisplayList {
        items: vec![DisplayItem::Rect {
            rect: Rect::from_px(-4000.0, -4000.0, 10.0, 10.0),
            color: Rgba8::BLACK,
        }],
        bounds: bounds(),
    };

    assert_eq!(validate(&dl, bounds()), Ok(()));
}

/// A rect that starts inside the inflated bounds and runs far past them is legitimate
/// content, not a malformed item: a `background` on a 100 000px-tall container is ordinary
/// markup, and the rasteriser clips it to the canvas anyway. `validate`'s bounds check is
/// therefore an **intersection** test, not a containment one — requiring containment made
/// every page taller than `viewport + 2 × 4096px` unrenderable, and forced `build` to cull
/// the item (dropping visible content) to keep its own output validatable.
#[test]
fn rect_extending_far_past_the_offscreen_margin_should_pass() {
    let dl = DisplayList {
        items: vec![DisplayItem::Rect {
            rect: Rect::from_px(0.0, 0.0, 800.0, 100_000.0),
            color: Rgba8::BLACK,
        }],
        bounds: bounds(),
    };

    assert_eq!(validate(&dl, bounds()), Ok(()));
}

/// The other half of [`rect_extending_far_past_the_offscreen_margin_should_pass`]: an
/// intersection test still rejects a rect with *no* overlap at all. This one sits entirely
/// below the inflated bounds (`600 + 4096 = 4696px`), where the previous containment test
/// only ever exercised a rect entirely to the right of them.
#[test]
fn rect_entirely_outside_the_offscreen_margin_should_fail() {
    let dl = DisplayList {
        items: vec![DisplayItem::Rect {
            rect: Rect::from_px(0.0, 5000.0, 800.0, 10.0),
            color: Rgba8::BLACK,
        }],
        bounds: bounds(),
    };

    assert_eq!(
        validate(&dl, bounds()),
        Err(DisplayListError::OutOfBounds { index: 0 })
    );
}

/// Cross-area Q4: the UA stylesheet gives `<hr>` a 1px solid border all round and
/// `height: 0`, so its border box is 2px tall and its top+bottom border widths (1px each)
/// sum to exactly that. `validate`'s `border_widths_valid` checks each width against the
/// rect's corresponding dimension *individually* (`1 ≤ 2`), which this satisfies — the
/// concern was that `build` might emit the border on the *content* box (0px tall), where
/// `1 ≤ 0` would fail. It emits it on the border box, so a page with an `<hr>` validates.
#[test]
fn hr_border_should_validate() {
    let html = "<!DOCTYPE html><html><head><style>html, body { margin: 0; padding: 0 }</style></head><body><hr></body></html>";
    let (tree, styled) = common::layout_html(html);

    let dl = build(&tree, styled.document());

    let hr_border = dl
        .items
        .iter()
        .find_map(|item| match item {
            DisplayItem::Border { rect, widths, .. } => Some((*rect, *widths)),
            _ => None,
        })
        .expect("<hr> must emit a Border item");
    assert_eq!(
        hr_border.0.size,
        Size {
            w: Au::from_px(800.0),
            h: Au::from_px(2.0)
        },
        "the Border rect must be the 2px-tall border box, not the 0px-tall content box"
    );
    assert_eq!(hr_border.1, Sides::uniform(Au::from_px(1.0)));
    assert_eq!(validate(&dl, dl.bounds), Ok(()));
}

/// Ruling 5: `build()`'s output must itself validate — run over the same five golden
/// fixtures `crates/paint/tests/display_list_goldens.rs` snapshots.
#[test]
fn built_lists_should_validate() {
    let fixtures = [
        include_str!("../../style/tests/golden/minimal.html"),
        include_str!("../../style/tests/golden/nested-divs.html"),
        include_str!("../../style/tests/golden/inline-spans.html"),
        include_str!("../../style/tests/golden/pre.html"),
        include_str!("../../style/tests/golden/cascade-conflict.html"),
    ];

    for html in fixtures {
        let (tree, styled) = common::layout_html(html);
        let dl = build(&tree, styled.document());
        assert_eq!(validate(&dl, dl.bounds), Ok(()));
    }
}
