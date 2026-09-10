//! Adapts stylo's `ComputedValues` into [`crate::geom::LayoutStyle`].
//!
//! This is the **only** file outside `crates/style/**` allowed to name a stylo
//! (`style::`/`servo_arc::`/`selectors::`/`cssparser::`) type (ADR-0015 §2, enforced by
//! `tools/check-stylo-scope.sh`). Everything downstream of [`adapt`] — `box_tree`, `dump`,
//! and every task from Task 17 on — sees only [`crate::geom::LayoutStyle`] and the rest of
//! `crate::geom`'s vocabulary.
//!
//! Accessor names below were read from the vendored stylo source
//! (`~/.cargo/registry/src/*/stylo-0.20.0/`) and the generated `properties.rs`
//! (`target/debug/build/stylo-*/out/properties.rs`, built from
//! `properties/{longhands.toml,*.mako.rs}` — there is no plain `.rs` file with the final
//! accessor list, hence reading the build output directly), not guessed: every
//! `get_*()`/`clone_*()` call here is a real inherent method on `ComputedValues` or one of
//! its style structs, and every computed-value type named here is a real type at the path
//! given.

use cl_style::ComputedValues;
use style::color::AbsoluteColor;
use style::properties::longhands::box_sizing::computed_value::T as StyloBoxSizing;
use style::properties::longhands::white_space_collapse::computed_value::T as WhiteSpaceCollapse;
use style::values::computed::font::{FontFamily as StyloFontFamily, GenericFontFamily, SingleFontFamily};
use style::values::computed::{
    BorderSideWidth, BorderStyle, Color as StyloColor, Display as StyloDisplay, FontStyle,
    FontWeight as StyloFontWeight, Inset, Length as StyloLength, LengthPercentage,
    LineHeight as StyloLineHeight, Margin as StyloMargin, MaxSize, Overflow as StyloOverflow,
    PositionProperty, Size as StyloSize, TextAlign as StyloTextAlign,
};

use crate::au::Au;
use crate::geom::{
    BoxSizing, Display, Length, LayoutStyle, Overflow, Position, Rgba8, Sides, TextAlign,
    WhiteSpace, normal_line_height,
};

/// Adapts one element's stylo `ComputedValues` into a [`LayoutStyle`].
///
/// Total over any `ComputedValues` stylo can produce: every computed-value shape read here
/// has a documented fold onto `cl_layout`'s narrower vocabulary, including values M1a's CSS
/// scope does not implement (an unimplemented keyword, `calc()`, an anchor-positioning
/// function, …) — see "What is dropped" below. Reads twenty-two longhands across eight style
/// structs (`get_box`, `get_position`, `get_margin`, `get_padding`, `get_border`,
/// `get_background`, `get_font`, `get_inherited_text`) plus the `color` shortcut; `get_text()`
/// is not used — `text-align` and the `white-space` longhands both live in
/// `get_inherited_text()`'s `InheritedText` style struct, not in `get_text()`'s `Text` struct
/// (which holds `text-overflow`/`text-decoration-*`, out of M1a's scope).
///
/// # The border-width/border-style trap (Task 13's handoff)
///
/// stylo computes `border-*-width` to its specified value (`medium`, i.e. `3px`, when no
/// declaration set one) **regardless** of `border-*-style`: zeroing the width for
/// `border-style: none`/`hidden` is a *used*-value step, which is this adapter's job, not
/// the cascade's (`cl_style::dump`'s module docs hit the same fact from the dump-format
/// side). So every side's width is read alongside that side's style and forced to
/// [`Au::ZERO`] whenever [`BorderStyle::none_or_hidden`] is true — never copied from stylo
/// blindly. See `adapt_border_side` below.
///
/// # What is dropped
///
/// - **`calc()` mixing a length and a percentage**: `LengthPercentage::to_length`/
///   `to_percentage` both return `None` for it (only a pure length or pure percentage
///   matches either), so `adapt_length_percentage` falls back to resolving against a zero
///   percentage basis — exact for the length-only contribution, drops the percentage one.
///   Deterministic and panic-free either way. `calc()` is not in M1a's CSS scope.
/// - **CSS anchor-positioning** (`anchor()`/`anchor-size()`) on `margin`/`inset`: not in
///   M1a's scope (there is no anchor-element concept yet); folded onto `auto` as if
///   unspecified. A `calc()` proven to *contain* an anchor function
///   (`AnchorContainingCalcFunction`) is instead routed through the same
///   `adapt_length_percentage` fallback as ordinary `calc()`, since it is still, structurally,
///   a length-percentage.
/// - **Intrinsic-sizing keywords** on `width`/`height`/`min-*`/`max-*` (`min-content`,
///   `max-content`, `fit-content`, `stretch`, …): not in M1a's scope; folded onto `auto`
///   (`max-width`/`max-height`: `None`, meaning "no upper bound").
/// - **`display` values other than `block`/`inline`/`none`** (`inline-block`, `flex`,
///   `grid`, `table*`, `list-item`, `contents`, …): folded onto [`Display::Inline`] when the
///   computed value's `<display-outside>` is `inline` and `<display-inside>` is `flow` (i.e.
///   exactly plain `inline`, via `style::values::computed::Display::is_inline_flow`),
///   [`Display::Block`] otherwise. `cl-layout`'s box tree does not implement any of these
///   formatting contexts in M1a; block/inline is the closest fallback that keeps every
///   element in a well-defined box rather than dropping it.
/// - **`position` values other than `static`/`relative`** (`absolute`, `fixed`, `sticky`):
///   M1a has no out-of-flow positioning, so these fold onto [`Position::Static`].
/// - **`overflow-y`**: [`LayoutStyle::overflow`] is one value, not a per-axis pair;
///   `overflow-x` is read as the representative (the `overflow` shorthand — the only form
///   any M1a-scope stylesheet uses — sets both axes identically).
/// - **`overflow: scroll`/`auto`**: M1a has no scrolling; folded onto [`Overflow::Visible`]
///   (never clipping content the page author expected to remain reachable via a scrollbar is
///   closer to correct than silently hiding it, which [`Overflow::Hidden`] would do).
/// - **`text-align: justify`/`start`/`end`/`-moz-*`**: M1a implements only
///   `left`/`right`/`center`; `start`/`-moz-left` fold onto `Left`, `end`/`-moz-right` onto
///   `Right` (M1a has no bidi support, so `start`/`end` are treated as `left`/`right` under
///   an LTR-only assumption), and `justify`/`-moz-center` fold onto `Left`.
/// - **`white-space` values other than `normal`/`pre`** (`nowrap`, `pre-wrap`, `pre-line`,
///   `break-spaces`): CSS Text 4 splits `white-space` into `white-space-collapse` and
///   `text-wrap-mode` longhands (`cl_style::dump`'s docs cover the same split); this adapter
///   reads only `white-space-collapse` and maps `preserve` to [`WhiteSpace::Pre`] and every
///   other value (`collapse`, `preserve-breaks`, `break-spaces`) to [`WhiteSpace::Normal`],
///   which reproduces `normal` and `pre` exactly and is the closest one-bit approximation for
///   the rest.
/// - **`font-style: oblique <angle>` vs `italic`**: both set [`LayoutStyle::font_italic`];
///   M1a does not implement synthetic oblique slanting by angle.
/// - **`line-height: normal`**: CSS leaves the used value UA/font-dependent; approximated via
///   [`normal_line_height`] (documented there).
#[must_use]
pub fn adapt(cv: &ComputedValues) -> LayoutStyle {
    let self_color = cv.clone_color();

    let box_style = cv.get_box();
    let position_style = cv.get_position();
    let margin = cv.get_margin();
    let padding = cv.get_padding();
    let border = cv.get_border();
    let background = cv.get_background();
    let font = cv.get_font();
    let inherited_text = cv.get_inherited_text();

    let top_border = adapt_border_side(
        &border.clone_border_top_width(),
        border.clone_border_top_style(),
        &border.clone_border_top_color(),
        self_color,
    );
    let right_border = adapt_border_side(
        &border.clone_border_right_width(),
        border.clone_border_right_style(),
        &border.clone_border_right_color(),
        self_color,
    );
    let bottom_border = adapt_border_side(
        &border.clone_border_bottom_width(),
        border.clone_border_bottom_style(),
        &border.clone_border_bottom_color(),
        self_color,
    );
    let left_border = adapt_border_side(
        &border.clone_border_left_width(),
        border.clone_border_left_style(),
        &border.clone_border_left_color(),
        self_color,
    );

    let font_size_px = font.clone_font_size().computed_size().px();
    let font_size = Au::from_px(font_size_px);

    LayoutStyle {
        display: adapt_display(box_style.clone_display()),
        position: adapt_position(box_style.clone_position()),
        width: adapt_size(&position_style.clone_width()),
        height: adapt_size(&position_style.clone_height()),
        min_width: adapt_size(&position_style.clone_min_width()),
        max_width: adapt_max_size(&position_style.clone_max_width()),
        min_height: adapt_size(&position_style.clone_min_height()),
        max_height: adapt_max_size(&position_style.clone_max_height()),
        margin: Sides {
            top: adapt_margin(&margin.clone_margin_top()),
            right: adapt_margin(&margin.clone_margin_right()),
            bottom: adapt_margin(&margin.clone_margin_bottom()),
            left: adapt_margin(&margin.clone_margin_left()),
        },
        padding: Sides {
            top: adapt_length_percentage(&padding.clone_padding_top().0),
            right: adapt_length_percentage(&padding.clone_padding_right().0),
            bottom: adapt_length_percentage(&padding.clone_padding_bottom().0),
            left: adapt_length_percentage(&padding.clone_padding_left().0),
        },
        border_width: Sides {
            top: bt_w,
            right: br_w,
            bottom: bb_w,
            left: bl_w,
        },
        border_color: Sides {
            top: bt_c,
            right: br_c,
            bottom: bb_c,
            left: bl_c,
        },
        border_solid: Sides {
            top: bt_s,
            right: br_s,
            bottom: bb_s,
            left: bl_s,
        },
        box_sizing: adapt_box_sizing(position_style.clone_box_sizing()),
        overflow: adapt_overflow(box_style.clone_overflow_x()),
        offset: Sides {
            top: adapt_inset(&position_style.clone_top()),
            right: adapt_inset(&position_style.clone_right()),
            bottom: adapt_inset(&position_style.clone_bottom()),
            left: adapt_inset(&position_style.clone_left()),
        },
        color: to_rgba8(self_color),
        background: to_rgba8(
            background
                .clone_background_color()
                .resolve_to_absolute(&self_color),
        ),
        font_family: adapt_font_family(&font.clone_font_family()),
        font_size,
        font_weight: adapt_font_weight(font.clone_font_weight()),
        font_italic: font.clone_font_style() != FontStyle::NORMAL,
        line_height: adapt_line_height(&font.clone_line_height(), font_size),
        text_align: adapt_text_align(inherited_text.clone_text_align()),
        white_space: adapt_white_space(inherited_text.clone_white_space_collapse()),
    }
}

/// `display` → [`Display`]. See [`adapt`]'s "What is dropped" for the fold rule.
fn adapt_display(display: StyloDisplay) -> Display {
    if display == StyloDisplay::None {
        Display::None
    } else if display.is_inline_flow() {
        Display::Inline
    } else {
        Display::Block
    }
}

/// `position` → [`Position`]. See [`adapt`]'s "What is dropped" for the fold rule.
fn adapt_position(position: PositionProperty) -> Position {
    match position {
        PositionProperty::Relative => Position::Relative,
        PositionProperty::Static
        | PositionProperty::Absolute
        | PositionProperty::Fixed
        | PositionProperty::Sticky => Position::Static,
    }
}

/// `box-sizing` → [`BoxSizing`]. Exhaustive: stylo's computed `box-sizing` has exactly these
/// two values.
fn adapt_box_sizing(box_sizing: StyloBoxSizing) -> BoxSizing {
    match box_sizing {
        StyloBoxSizing::ContentBox => BoxSizing::ContentBox,
        StyloBoxSizing::BorderBox => BoxSizing::BorderBox,
    }
}

/// `overflow-x` → [`Overflow`]. See [`adapt`]'s "What is dropped" for the fold rule (and for
/// why `overflow-x` rather than `overflow-y`).
fn adapt_overflow(overflow: StyloOverflow) -> Overflow {
    match overflow {
        StyloOverflow::Visible | StyloOverflow::Scroll | StyloOverflow::Auto => {
            Overflow::Visible
        }
        StyloOverflow::Hidden | StyloOverflow::Clip => Overflow::Hidden,
    }
}

/// `text-align` → [`TextAlign`]. See [`adapt`]'s "What is dropped" for the fold rule.
fn adapt_text_align(align: StyloTextAlign) -> TextAlign {
    match align {
        StyloTextAlign::Left | StyloTextAlign::Start | StyloTextAlign::MozLeft => {
            TextAlign::Left
        }
        StyloTextAlign::Right | StyloTextAlign::End | StyloTextAlign::MozRight => {
            TextAlign::Right
        }
        StyloTextAlign::Center | StyloTextAlign::MozCenter => TextAlign::Center,
        StyloTextAlign::Justify => TextAlign::Left,
    }
}

/// `white-space-collapse` → [`WhiteSpace`]. See [`adapt`]'s "What is dropped" for the fold
/// rule (and why only this one of the two `white-space` longhands is read).
fn adapt_white_space(collapse: WhiteSpaceCollapse) -> WhiteSpace {
    match collapse {
        WhiteSpaceCollapse::Preserve => WhiteSpace::Pre,
        WhiteSpaceCollapse::Collapse
        | WhiteSpaceCollapse::PreserveBreaks
        | WhiteSpaceCollapse::BreakSpaces => WhiteSpace::Normal,
    }
}

/// `font-weight`'s numeric value, clamped to the `u16` range `1..=1000` CSS defines for it.
fn adapt_font_weight(weight: StyloFontWeight) -> u16 {
    // `FontWeight::value()` is already clamped to `[1, 1000]` by stylo (the property's own
    // parse-time range), so the cast below never truncates in practice; it is still written
    // as a saturating round rather than a bare `as u16` so a future stylo version relaxing
    // that range could not turn this into a silent wraparound.
    weight.value().round().clamp(1.0, 1000.0) as u16
}

/// `width`/`height`/`min-width`/`min-height` → [`Length`]. See [`adapt`]'s "What is dropped"
/// for the fold rule for the intrinsic-sizing keywords.
fn adapt_size(size: &StyloSize) -> Length {
    match size {
        StyloSize::LengthPercentage(lp) => adapt_length_percentage(&lp.0),
        StyloSize::Auto
        | StyloSize::MaxContent
        | StyloSize::MinContent
        | StyloSize::FitContent
        | StyloSize::WebkitFillAvailable
        | StyloSize::Stretch
        | StyloSize::FitContentFunction(_)
        | StyloSize::AnchorSizeFunction(_)
        | StyloSize::AnchorContainingCalcFunction(_) => Length::Auto,
    }
}

/// `max-width`/`max-height` → `Option<Length>`. `None` means "no upper bound" — both for
/// `max-*: none` and for the intrinsic-sizing keywords M1a does not implement. See [`adapt`]'s
/// "What is dropped".
fn adapt_max_size(size: &MaxSize) -> Option<Length> {
    match size {
        MaxSize::LengthPercentage(lp) => Some(adapt_length_percentage(&lp.0)),
        MaxSize::None
        | MaxSize::MaxContent
        | MaxSize::MinContent
        | MaxSize::FitContent
        | MaxSize::WebkitFillAvailable
        | MaxSize::Stretch
        | MaxSize::FitContentFunction(_)
        | MaxSize::AnchorSizeFunction(_)
        | MaxSize::AnchorContainingCalcFunction(_) => None,
    }
}

/// `margin-*` → [`Length`]. See [`adapt`]'s "What is dropped" for the anchor-positioning
/// fold rule.
fn adapt_margin(margin: &StyloMargin) -> Length {
    match margin {
        StyloMargin::LengthPercentage(lp) | StyloMargin::AnchorContainingCalcFunction(lp) => {
            adapt_length_percentage(lp)
        }
        StyloMargin::Auto | StyloMargin::AnchorSizeFunction(_) => Length::Auto,
    }
}

/// `top`/`right`/`bottom`/`left` → [`Length`]. See [`adapt`]'s "What is dropped" for the
/// anchor-positioning fold rule.
fn adapt_inset(inset: &Inset) -> Length {
    match inset {
        Inset::LengthPercentage(lp) | Inset::AnchorContainingCalcFunction(lp) => {
            adapt_length_percentage(lp)
        }
        Inset::Auto | Inset::AnchorFunction(_) | Inset::AnchorSizeFunction(_) => Length::Auto,
    }
}

/// A `<length-percentage>` → [`Length`]: `Px` for a pure length, `Percent` for a pure
/// percentage (as CSS percentage points, `50.0` for `50%`), and a zero-basis `resolve` for
/// anything else (`calc()` mixing both) — see [`adapt`]'s "What is dropped".
fn adapt_length_percentage(lp: &LengthPercentage) -> Length {
    if let Some(len) = lp.to_length() {
        Length::Px(Au::from_px(len.px()))
    } else if let Some(pct) = lp.to_percentage() {
        Length::Percent(pct.0 * 100.0)
    } else {
        let resolved = lp.resolve(StyloLength::new(0.0));
        Length::Px(Au::from_px(resolved.px()))
    }
}

/// `line-height` → [`Au`]: a `<length>` is used as-is, a `<number>` multiplies `font_size`
/// (both per CSS's definition of the property), and `normal` is approximated by
/// [`normal_line_height`].
fn adapt_line_height(line_height: &StyloLineHeight, font_size: Au) -> Au {
    match line_height {
        StyloLineHeight::Normal => normal_line_height(font_size),
        StyloLineHeight::Number(n) => font_size.mul_by_f32(n.0),
        StyloLineHeight::Length(len) => Au::from_px(len.0.px()),
    }
}

/// `font-family` → the raw list of family names and generic keywords, in specified order.
/// Font *selection* is a `cl-fonts`/Task 18 concern, not this adapter's.
fn adapt_font_family(family: &StyloFontFamily) -> Vec<String> {
    family.families.iter().map(single_family_name).collect()
}

/// One `<family-name>` or `<generic-family>` → its CSS-source-text-compatible name.
fn single_family_name(family: &SingleFontFamily) -> String {
    match family {
        SingleFontFamily::FamilyName(name) => name.name.to_string(),
        SingleFontFamily::Generic(generic) => generic_family_name(*generic).to_owned(),
    }
}

/// A `<generic-family>` keyword's CSS source-text spelling.
fn generic_family_name(generic: GenericFontFamily) -> &'static str {
    match generic {
        GenericFontFamily::Serif => "serif",
        GenericFontFamily::SansSerif => "sans-serif",
        GenericFontFamily::Monospace => "monospace",
        GenericFontFamily::Cursive => "cursive",
        GenericFontFamily::Fantasy => "fantasy",
        GenericFontFamily::SystemUi => "system-ui",
        // `None` is stylo's "no generic family specified" internal sentinel — never produced
        // by parsing a real `font-family` declaration. Falls back to the CSS-wide default
        // rather than an empty string, so a `LayoutStyle::font_family` entry is never blank.
        GenericFontFamily::None => "sans-serif",
    }
}

/// One border side's stylo values → the used width (zeroed for `none`/`hidden`, the
/// border-width/border-style trap [`adapt`] documents), the resolved color (`currentcolor`
/// against `self_color`), and whether the side is `solid`.
fn adapt_border_side(
    width: BorderSideWidth,
    style: BorderStyle,
    color: StyloColor,
    self_color: AbsoluteColor,
) -> (Au, Rgba8, bool) {
    let used_width = if style.none_or_hidden() {
        Au::ZERO
    } else {
        Au(width.0.0)
    };
    let solid = style == BorderStyle::Solid;
    let resolved_color = to_rgba8(color.resolve_to_absolute(&self_color));
    (used_width, resolved_color, solid)
}

/// An [`AbsoluteColor`] → [`Rgba8`]: converts to the legacy sRGB syntax (plain 0..=1 r/g/b/a
/// components, no `none` keyword, matching every color M1a's CSS scope can produce — `rgb()`,
/// hex, named colors) and scales each channel to a byte, clamping components outside `0..=1`
/// (an out-of-gamut wide-color-space value, not reachable from M1a's CSS scope, but not
/// something to panic on either) and treating a non-finite component as `0`.
fn to_rgba8(color: AbsoluteColor) -> Rgba8 {
    let legacy = color.into_srgb_legacy();
    let [r, g, b, a] = *legacy.raw_components();
    Rgba8 {
        r: unit_to_u8(r),
        g: unit_to_u8(g),
        b: unit_to_u8(b),
        a: unit_to_u8(a),
    }
}

/// A single `0.0..=1.0` color/alpha component → its nearest `u8`, saturating and never
/// panicking on a non-finite or out-of-range input.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "clamped to [0, 255] just above, so the f32 -> u8 cast below never truncates \
               or loses a sign; kept as a checked clamp rather than relying solely on the \
               (also saturating) `as` cast, so the intent reads directly at the call site"
)]
fn unit_to_u8(component: f32) -> u8 {
    let component = if component.is_finite() { component } else { 0.0 };
    (component.clamp(0.0, 1.0) * 255.0).round() as u8
}
