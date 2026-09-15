//! Integration tests for [`cl_layout::style_adapt::adapt`]: does a real stylo
//! `ComputedValues`, produced by the same cascade `cl-style`'s own tests exercise, map onto
//! the exact [`cl_layout::LayoutStyle`] fields the M1a plan promises.
#![allow(
    clippy::expect_used,
    reason = "a failed setup step in a test should abort that test, loudly"
)]

#[path = "common/mod.rs"]
mod common;

use cl_dom::local_name;
use cl_layout::style_adapt::adapt;
use cl_layout::{Au, Length, Rgba8, Sides};

/// `<div style="width:50%;margin:0 auto;border:2px solid #f00">`: a percentage `width`, an
/// `auto` half of the `margin` shorthand, and a solid, non-default-width, non-black
/// `border` all round-trip through `adapt` intact.
#[test]
fn adapt_should_map_display_and_lengths() {
    let styled = common::styled_document(concat!(
        "<!DOCTYPE html><html><body>",
        r#"<div style="width:50%;margin:0 auto;border:2px solid #f00"></div>"#,
        "</body></html>",
    ));
    let div = common::find_element(styled.document(), |el| el.name.local == local_name!("div"))
        .expect("fixture has a <div>");
    let style = adapt(styled.computed(div).expect("<div> is styled"));

    assert_eq!(style.width, Length::Percent(50.0));
    // `margin: 0 auto` sets top/bottom to `0` and left/right to `auto`.
    assert_eq!(style.margin.top, Length::Px(Au::ZERO));
    assert_eq!(style.margin.left, Length::Auto);
    assert_eq!(style.margin.right, Length::Auto);
    // `border: 2px solid #f00` is 2px = 120 app units on every side.
    assert_eq!(style.border_width.left, Au(120));
    assert_eq!(
        style.border_color.left,
        Rgba8 {
            r: 255,
            g: 0,
            b: 0,
            a: 255
        }
    );
    assert!(style.border_solid.left);
}

/// The border-width/border-style trap (Task 13's handoff, restated in [`adapt`]'s docs):
/// stylo computes `border-*-width` to its specified value regardless of `border-*-style`, so
/// `adapt` must force the *used* width to zero whenever that side's style is not
/// `solid`/some other paintable style — i.e. whenever it is `none` (the initial value, which
/// is what `border-width` alone, with no `border-style`, leaves every side at).
#[test]
fn adapt_should_zero_border_width_when_style_is_none() {
    let no_style = common::styled_document(concat!(
        "<!DOCTYPE html><html><body>",
        r#"<div style="border-width:5px"></div>"#,
        "</body></html>",
    ));
    let div = common::find_element(no_style.document(), |el| {
        el.name.local == local_name!("div")
    })
    .expect("fixture has a <div>");
    let style = adapt(no_style.computed(div).expect("<div> is styled"));
    assert_eq!(style.border_width, Sides::uniform(Au::ZERO));
    assert_eq!(style.border_solid, Sides::uniform(false));

    let solid = common::styled_document(concat!(
        "<!DOCTYPE html><html><body>",
        r#"<div style="border:2px solid #f00"></div>"#,
        "</body></html>",
    ));
    let div = common::find_element(solid.document(), |el| el.name.local == local_name!("div"))
        .expect("fixture has a <div>");
    let style = adapt(solid.computed(div).expect("<div> is styled"));
    assert_eq!(style.border_width, Sides::uniform(Au(120)));
    assert_eq!(style.border_solid, Sides::uniform(true));
}
