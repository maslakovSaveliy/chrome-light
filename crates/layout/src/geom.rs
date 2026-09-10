//! Geometry primitives built on [`crate::au::Au`], and the CSS value shapes
//! [`crate::style_adapt`] maps stylo's computed values onto.
//!
//! Everything here is a plain data type: no layout algorithm lives in this module (that is
//! Task 17 onward), just the vocabulary later tasks share.

use crate::au::Au;

/// A point in 2D space, in app units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Point {
    /// Horizontal offset.
    pub x: Au,
    /// Vertical offset.
    pub y: Au,
}

/// A width/height pair, in app units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Size {
    /// Width.
    pub w: Au,
    /// Height.
    pub h: Au,
}

/// An axis-aligned rectangle: an origin plus a size, in app units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    /// The rectangle's top-left corner.
    pub origin: Point,
    /// The rectangle's width and height.
    pub size: Size,
}

/// A CSS box's four sides (top, right, bottom, left — CSS's own clockwise-from-top order,
/// matching how `margin`/`padding`/`border-width` shorthands enumerate their four values),
/// each holding one `T`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Sides<T> {
    /// The top side.
    pub top: T,
    /// The right side.
    pub right: T,
    /// The bottom side.
    pub bottom: T,
    /// The left side.
    pub left: T,
}

impl<T> Sides<T> {
    /// Builds a `Sides` with all four sides set to the same value.
    pub fn uniform(value: T) -> Sides<T>
    where
        T: Clone,
    {
        Sides {
            top: value.clone(),
            right: value.clone(),
            bottom: value.clone(),
            left: value,
        }
    }
}

/// A resolved CSS `<length>`, `<percentage>`, or the `auto` keyword — the computed-value
/// shape shared by `width`, `height`, `margin`, `padding`, `inset` and friends.
///
/// A length is always `Au`: percentages stay symbolic (resolved against a containing block
/// by Task 17's layout algorithm, not here), and `auto` is its own variant rather than a
/// magic length, since which of the three a property holds changes how it lays out (an
/// `auto` margin absorbs extra space; an `auto` width fills the containing block).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Length {
    /// The `auto` keyword — the initial value of `width`/`height`/the inset properties, and
    /// the most useful "nothing was specified" default for a `Length` field.
    #[default]
    Auto,
    /// An absolute length, already resolved to app units.
    Px(Au),
    /// A percentage, still relative to a containing block that layout has not established
    /// yet. Stored as CSS percentage points (`50.0` for `50%`), not a `0.0..=1.0` fraction.
    Percent(f32),
}

/// The subset of CSS `display` this box tree distinguishes: whether a box participates in
/// its container as a block or as inline content, or generates no box at all.
///
/// M1a's CSS scope is exactly `display: block | inline | none`
/// (`docs/superpowers/plans/2026-09-07-m1a-static-pipeline.md`, "Global Constraints"); every
/// other computed `display` (`inline-block`, `flex`, `table`, `list-item`, …) is folded onto
/// one of these three by [`crate::style_adapt::adapt`] — see that function's docs for how.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Display {
    /// A block-level box.
    Block,
    /// An inline-level box.
    #[default]
    Inline,
    /// No box at all: the element and its descendants are removed from the box tree.
    None,
}

/// The subset of CSS `position` this box tree distinguishes.
///
/// M1a implements only `static` and `relative` (abs/fixed positioning is out of scope for
/// M1a per the plan's Global Constraints); [`crate::style_adapt::adapt`] folds every other
/// computed `position` (`absolute`, `fixed`, `sticky`) onto `Static`, i.e. treats them as
/// still participating in normal in-flow layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Position {
    /// The element stays in its normal flow position.
    #[default]
    Static,
    /// The element stays in its normal flow position, then is offset by `LayoutStyle::offset`
    /// without affecting where anything else lays out.
    Relative,
}

/// The subset of CSS `white-space` this box tree distinguishes.
///
/// M1a implements `normal` and `pre` (the plan's CSS scope); see [`crate::style_adapt::adapt`]
/// for how the other `white-space` values (`nowrap`, `pre-wrap`, `pre-line`, `break-spaces`)
/// fold onto these two.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WhiteSpace {
    /// Whitespace sequences collapse to a single space; text wraps at box edges.
    #[default]
    Normal,
    /// Whitespace is preserved verbatim; text only breaks on an explicit line break.
    Pre,
}

/// The subset of CSS `text-align` this box tree distinguishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAlign {
    /// Left-aligned (and the fold target for `start`, per M1a's LTR-only scope — see
    /// [`crate::style_adapt::adapt`]).
    #[default]
    Left,
    /// Right-aligned (and the fold target for `end`).
    Right,
    /// Centered.
    Center,
}

/// CSS `box-sizing`: whether `width`/`height` describe the content box or the border box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BoxSizing {
    /// `width`/`height` set the content box; padding and border add to it.
    #[default]
    ContentBox,
    /// `width`/`height` set the border box; padding and border are carved out of it.
    BorderBox,
}

/// The subset of CSS `overflow` this box tree distinguishes.
///
/// M1a implements `visible` and `hidden` (the latter also covering `clip`, per the plan's
/// CSS scope); see [`crate::style_adapt::adapt`] for how `scroll`/`auto` (no scrolling
/// support in M1a) fold onto `Visible`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Overflow {
    /// Content is never clipped to the box.
    #[default]
    Visible,
    /// Content outside the box's padding edge is clipped away.
    Hidden,
}

/// A straight (non-premultiplied) 8-bit-per-channel RGBA colour — the resolved form of every
/// CSS color this layer reads (`currentcolor` and other indirections are already resolved by
/// [`crate::style_adapt::adapt`]; see its docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rgba8 {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
    /// Alpha channel (0 = fully transparent, 255 = fully opaque).
    pub a: u8,
}

impl Rgba8 {
    /// Opaque black — the initial value of the CSS `color` property, and the fallback for
    /// every color field this crate cannot otherwise resolve.
    pub const BLACK: Rgba8 = Rgba8 {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };

    /// Fully transparent black — the initial value of `background-color`.
    pub const TRANSPARENT: Rgba8 = Rgba8 {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };
}

/// The multiplier [`normal_line_height`] uses to approximate `line-height: normal`.
///
/// CSS leaves the exact value UA- and font-dependent; `1.2` is the commonly used
/// approximation (and close to what the fonts M1a bundles — Ahem, Noto Sans — resolve to in
/// practice). Task 18's real line-box construction is free to refine this once font metrics
/// (`parley`/`swash`) are available; until then this is the one place the number lives.
const NORMAL_LINE_HEIGHT_RATIO: f32 = 1.2;

/// Approximates `line-height: normal` for a given `font-size`, already in app units.
///
/// Shared by [`LayoutStyle::initial`] and [`crate::style_adapt::adapt`], so the two paths
/// that can produce a `normal` line-height (an element stylo never touched, and one whose
/// cascaded `line-height` genuinely computed to `normal`) agree.
#[must_use]
pub fn normal_line_height(font_size: Au) -> Au {
    font_size.mul_by_f32(NORMAL_LINE_HEIGHT_RATIO)
}

/// The layout-relevant subset of one element's computed style: every CSS longhand M1a's
/// layout algorithms (Task 17 onward) read, already resolved from stylo's `ComputedValues`
/// into this crate's own vocabulary by [`crate::style_adapt::adapt`].
///
/// Every field here is a *computed* value, not yet a *used* value: percentages are still
/// symbolic (`Length::Percent`), and `auto` is still `auto` — resolving either against a
/// containing block is a layout-algorithm concern (Task 17+), not this adapter's.
#[derive(Debug, Clone, PartialEq)]
pub struct LayoutStyle {
    /// `display`, folded onto block/inline/none — see [`Display`].
    pub display: Display,
    /// `position`, folded onto static/relative — see [`Position`].
    pub position: Position,
    /// `width`.
    pub width: Length,
    /// `height`.
    pub height: Length,
    /// `min-width`.
    pub min_width: Length,
    /// `max-width`. `None` for `max-width: none` (no upper bound) and for the intrinsic-size
    /// keywords M1a does not implement (`min-content`, `max-content`, `fit-content`, …).
    pub max_width: Option<Length>,
    /// `min-height`.
    pub min_height: Length,
    /// `max-height`. See [`LayoutStyle::max_width`] for what `None` means.
    pub max_height: Option<Length>,
    /// `margin-top`/`-right`/`-bottom`/`-left`.
    pub margin: Sides<Length>,
    /// `padding-top`/`-right`/`-bottom`/`-left`.
    pub padding: Sides<Length>,
    /// The *used* border width for each side: `0` whenever that side's `border-style` is
    /// `none`/`hidden`, regardless of what `border-*-width` computed to. See
    /// [`crate::style_adapt::adapt`]'s docs for why this matters.
    pub border_width: Sides<Au>,
    /// `border-top-color`/`-right-color`/`-bottom-color`/`-left-color`, with `currentcolor`
    /// already resolved against [`LayoutStyle::color`].
    pub border_color: Sides<Rgba8>,
    /// Whether each side's `border-style` is `solid` (`true`) or anything else, including
    /// `none`/`hidden` (`false`) — M1a's CSS scope implements only `solid`/`none`.
    pub border_solid: Sides<bool>,
    /// `box-sizing`.
    pub box_sizing: BoxSizing,
    /// `overflow` (both axes folded onto one value — see [`crate::style_adapt::adapt`]).
    pub overflow: Overflow,
    /// `top`/`right`/`bottom`/`left`, meaningful only when [`LayoutStyle::position`] is
    /// [`Position::Relative`].
    pub offset: Sides<Length>,
    /// `color`.
    pub color: Rgba8,
    /// `background-color`.
    pub background: Rgba8,
    /// `font-family`, as the raw list of family names and generic keywords (`"serif"`,
    /// `"sans-serif"`, …) in specified order — font *selection* is a `cl-fonts`/Task 18
    /// concern, not this adapter's.
    pub font_family: Vec<String>,
    /// `font-size`.
    pub font_size: Au,
    /// `font-weight`, as its CSS numeric value (`1..=1000`; `400` is `normal`, `700` is
    /// `bold`).
    pub font_weight: u16,
    /// Whether `font-style` computed to `italic` or `oblique` (M1a does not distinguish the
    /// two — see [`crate::style_adapt::adapt`]).
    pub font_italic: bool,
    /// `line-height`, already resolved to an absolute length: a `<number>` is multiplied by
    /// `font-size` and `normal` is approximated via [`normal_line_height`] — see
    /// [`crate::style_adapt::adapt`].
    pub line_height: Au,
    /// `text-align`, folded onto left/right/center — see [`TextAlign`].
    pub text_align: TextAlign,
    /// `white-space`, folded onto normal/pre — see [`WhiteSpace`].
    pub white_space: WhiteSpace,
}

impl LayoutStyle {
    /// The CSS initial values for every field above, as a concrete [`LayoutStyle`].
    ///
    /// Used where there is no `ComputedValues` to adapt: [`crate::box_tree::build`]'s
    /// synthetic root box for a document with no root element, and as the non-inherited half
    /// of [`LayoutStyle::anonymous_block`].
    #[must_use]
    pub fn initial() -> LayoutStyle {
        let font_size = Au::from_px(16.0);
        LayoutStyle {
            display: Display::Inline,
            position: Position::Static,
            width: Length::Auto,
            height: Length::Auto,
            min_width: Length::Auto,
            max_width: None,
            min_height: Length::Auto,
            max_height: None,
            margin: Sides::uniform(Length::Px(Au::ZERO)),
            padding: Sides::uniform(Length::Px(Au::ZERO)),
            border_width: Sides::uniform(Au::ZERO),
            border_color: Sides::uniform(Rgba8::BLACK),
            border_solid: Sides::uniform(false),
            box_sizing: BoxSizing::ContentBox,
            overflow: Overflow::Visible,
            offset: Sides::uniform(Length::Auto),
            color: Rgba8::BLACK,
            background: Rgba8::TRANSPARENT,
            font_family: vec![String::from("sans-serif")],
            font_size,
            font_weight: 400,
            font_italic: false,
            line_height: normal_line_height(font_size),
            text_align: TextAlign::Left,
            white_space: WhiteSpace::Normal,
        }
    }

    /// The style of an anonymous block box wrapping a run of inline-level boxes
    /// ([`crate::box_tree::build`]'s job when a block container mixes block- and
    /// inline-level children).
    ///
    /// Per CSS 2.1 §9.2.2.1, an anonymous box takes its *inherited* properties from the
    /// element that would otherwise be its parent, and the *initial* value for everything
    /// non-inherited (it has no `margin`, no `border`, is never positioned, …) — so this
    /// copies exactly the inherited fields (`color`, the font fields, `line-height`,
    /// `text-align`, `white-space`) from `parent` and takes [`LayoutStyle::initial`] for the
    /// rest.
    #[must_use]
    pub fn anonymous_block(parent: &LayoutStyle) -> LayoutStyle {
        LayoutStyle {
            display: Display::Block,
            color: parent.color,
            font_family: parent.font_family.clone(),
            font_size: parent.font_size,
            font_weight: parent.font_weight,
            font_italic: parent.font_italic,
            line_height: parent.line_height,
            text_align: parent.text_align,
            white_space: parent.white_space,
            ..LayoutStyle::initial()
        }
    }
}
