//! Shaped-text data types ([`Glyph`], [`GlyphRun`]) and the `parley` shaper that fills them
//! in (`TextShaper`).
//!
//! [`Glyph`] and [`GlyphRun`] are plain data: no `parley`, `fontique`, `swash` or `skrifa`
//! type appears in either, so everything downstream of this crate (Task 19's paint pass,
//! Task 21's rasteriser) reads shaped text without depending on the shaping stack. That is a
//! hard constraint, not a convenience: `parley`/`fontique` types never leave `cl-layout`, and
//! within it they never leave this module and `crate::inline`.
//!
//! # What this module does
//!
//! `TextShaper::layout_inline` takes one block's inline content — already whitespace-
//! processed by `crate::whitespace`, already flattened into a document-order list of
//! `InlineItem`s by `crate::inline` — plus the container's style and its content-box
//! width, and returns one `LineBox` per line `parley` broke the content into. Everything
//! `parley` knows about (shaping, font fallback, the Unicode line breaking algorithm,
//! `text-align`) happens here; everything CSS knows about (which whitespace survives, what a
//! line box is worth in `Au`, which fragment a run belongs to) happens in `crate::inline`.
//!
//! # `f32` stops here
//!
//! `parley` works in `f32` CSS pixels; this crate works in [`Au`] (1/60 px integers). Every
//! value crossing that boundary is converted exactly once, with [`Au::from_px`], at the point
//! it becomes part of a `LineBox` or a [`GlyphRun`] — never twice, never accumulated after
//! conversion. No `f32` reaches `crate::inline` or [`crate::block`].
//!
//! # Determinism
//!
//! The `fontique` collection this builds is a clone of [`FontDb`]'s, which by construction
//! contains exactly the two bundled families and never enumerates host fonts (see
//! `cl_fonts::FontDb`'s docs). Generic families (`serif`, `sans-serif`, `monospace`, …) are
//! all pointed at Noto Sans, and every font stack gets Noto Sans appended as a last resort,
//! so font selection is a pure function of the document — no host state, no `HashMap`
//! iteration order, no clock.
//!
//! # Not implemented in M1a
//!
//! Vertical writing modes, bidi (the `text-align` mapping below is LTR-only, matching
//! [`crate::geom::TextAlign`]'s own M1a scope), `letter-spacing`/`word-spacing`,
//! `text-indent`, `text-decoration`, inline boxes with their own borders/padding/backgrounds
//! (an `Inline` box contributes only its descendants' text — see `crate::inline`), and
//! per-inline `line-height` (every line box in a block is the *container*'s `line-height`
//! tall).

use std::borrow::Cow;

use cl_dom::NodeId;
use cl_fonts::{FontDb, FontFace, FontKey};
use parley::{
    Alignment, AlignmentOptions, FontContext, FontData, FontFamily, FontFamilyName, FontStyle,
    FontWeight, GenericFamily, LayoutContext, LineHeight, StyleProperty, TextWrapMode,
};

use crate::au::Au;
use crate::error::LayoutError;
use crate::geom::{LayoutStyle, Point, Rgba8, TextAlign, WhiteSpace};

/// One positioned glyph within a [`GlyphRun`].
///
/// Coordinates are relative to [`GlyphRun::origin`], in app units, matching every other
/// geometry type in this crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Glyph {
    /// The glyph id within [`GlyphRun::font`] (a `skrifa`/`swash` glyph index, not a
    /// Unicode code point).
    pub id: u16,
    /// Horizontal offset from [`GlyphRun::origin`] to this glyph's origin.
    pub x: Au,
    /// Vertical offset from [`GlyphRun::origin`] to this glyph's origin.
    pub y: Au,
    /// How far this glyph advances the pen, in app units.
    pub advance: Au,
}

/// One run of shaped glyphs: a contiguous stretch of text rendered in a single font, size
/// and color (a shaper never mixes fonts or sizes within a run — a style or font-fallback
/// change starts a new one).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "arbitrary", derive(arbitrary::Arbitrary))]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct GlyphRun {
    /// The font face every glyph in this run is drawn from.
    pub font: FontKey,
    /// The font size this run was shaped at, in app units.
    pub size: Au,
    /// The run's origin (its text baseline's left end), in the same absolute coordinate
    /// space as [`crate::fragment::Fragment`]'s rects.
    pub origin: Point,
    /// The run's glyphs, in visual (left-to-right) order.
    pub glyphs: Vec<Glyph>,
    /// The color the glyphs paint with (the CSS `color` in effect where the text was
    /// generated).
    pub color: Rgba8,
}

/// One piece of a block's flattened inline content, in document order.
///
/// Built by [`crate::inline`] from the box tree: one [`InlineItem::Text`] per
/// [`crate::box_tree::BoxKind::InlineText`] box (its `text` already whitespace-processed,
/// its `style` the *inherited* style that box carries — so a nested `<b>`'s text arrives
/// with its own `font-weight`), and one [`InlineItem::Break`] per
/// [`crate::box_tree::BoxKind::LineBreak`] (`<br>`) box.
#[derive(Debug, Clone, Copy)]
pub(crate) enum InlineItem<'a> {
    /// A run of text and the style it inherited.
    Text {
        /// The already-whitespace-processed text (see [`crate::whitespace`]).
        text: &'a str,
        /// The style this text inherited from its containing element.
        style: &'a LayoutStyle,
        /// The text node this text came from — carried onto the `Text` fragment so a paint
        /// or hit-test pass can find its way back to the DOM.
        node: NodeId,
    },
    /// A `<br>`: forces a line break here.
    Break,
}

/// One line box `parley` broke a block's inline content into.
///
/// Geometry is *relative to the container's content-box origin*: [`crate::inline`] adds the
/// absolute origin when it turns these into fragments, so this module never needs to know
/// where on the page the block ended up.
///
/// The task brief's sketch of this type is `{ height, baseline, runs, width }`. There is no
/// `width` (nor a line-offset) field here: a line box is as wide as its containing block (CSS
/// 2.1 §9.4.2), so the `Line` fragment's width comes from the container, and the line's own
/// occupied extent and its `text-align` offset are already baked into the runs' origins and
/// advances — storing either a second time would be a second source of truth for the same
/// geometry. `run_items` *is* added, because the fragment builder needs it and nothing else
/// can recover it: it says which [`InlineItem`] each run's text came from, and therefore which
/// DOM node and which `LayoutStyle` its `Text` fragment carries.
pub(crate) struct LineBox {
    /// The line box's height: the container's `line-height` (see the module docs — M1a does
    /// not implement per-inline line heights).
    pub(crate) height: Au,
    /// The baseline's offset from the line box's top.
    pub(crate) baseline: Au,
    /// The line's glyph runs, in visual (left-to-right) order. Origins are relative to the
    /// *line box's* top-left corner: `x` already includes the `text-align` offset, and `y` is
    /// always zero — [`crate::inline`] adds `baseline` (and the block's absolute content
    /// origin) when it places the runs, so the baseline lives in exactly one place.
    pub(crate) runs: Vec<GlyphRun>,
    /// For each entry of `runs`, the index into `layout_inline`'s `items` slice of the
    /// [`InlineItem::Text`] that produced it. Always the same length as `runs`.
    pub(crate) run_items: Vec<usize>,
}

/// The upper bound this module clamps a `font-size` to before handing it to `parley`, in CSS
/// pixels.
///
/// A document is attacker-controlled: `font-size: 1e9px` is valid CSS, and a font size that
/// large makes glyph metrics (and every `f32` derived from them) meaningless or non-finite.
/// Clamping keeps shaping total — a document can make text absurdly large, but never make
/// layout panic or produce `NaN` geometry. `4096` is comfortably past any real page and far
/// inside `f32`'s exact-integer range even after multiplication by a font's units-per-em.
const MAX_FONT_SIZE_PX: f32 = 4096.0;

/// The family every unresolvable font stack falls back to — the only non-test face
/// `ChromeLight` bundles besides Ahem. See the module docs' "Determinism" section.
const FALLBACK_FAMILY: &str = cl_fonts::bundled::NOTO_SANS_FAMILY;

/// The `parley` shaping pipeline, owned by [`crate::block::layout`] for the duration of one
/// layout pass.
///
/// Holds `parley`'s two long-lived resources — a `FontContext` (the font database) and a
/// `LayoutContext` (reusable scratch space) — plus the [`FontFace`] table needed to map a
/// shaped run's font back to a [`FontKey`].
pub(crate) struct TextShaper {
    font_cx: FontContext,
    layout_cx: LayoutContext<Rgba8>,
    /// Every face in the [`FontDb`] this shaper was built from, in key order. Two entries in
    /// M1a (Ahem, Noto Sans), scanned linearly by [`TextShaper::key_for_font`].
    faces: Vec<FontFace>,
}

impl TextShaper {
    /// Builds a shaper over `fonts`.
    ///
    /// The `fontique` collection is cloned out of the [`FontDb`] rather than borrowed:
    /// `parley::FontContext` owns its collection, and cloning shares the registered fonts'
    /// `Blob`s by `Arc` rather than copying the font bytes — which is also what makes
    /// [`TextShaper::key_for_font`]'s pointer comparison work (see its docs).
    ///
    /// Every CSS generic family is then pointed at Noto Sans. `fontique` has no generic
    /// mapping of its own once system fonts are switched off (the platform backends are what
    /// normally populate it), so without this a `font-family: sans-serif` — the initial value
    /// of every element's `font-family`, see [`LayoutStyle::initial`] — would resolve to no
    /// family at all.
    pub(crate) fn new(fonts: &mut FontDb) -> TextShaper {
        let families: Vec<&'static str> = fonts.families().collect();
        let mut faces = Vec::with_capacity(families.len());
        for family in families {
            if let Some(face) = fonts.key_for(family).and_then(|key| fonts.face(key)) {
                faces.push(*face);
            }
        }

        let (db_collection, db_source_cache) = fonts.fontique();
        let mut collection = db_collection.clone();
        let source_cache = db_source_cache.clone();
        if let Some(fallback) = collection.family_id(FALLBACK_FAMILY) {
            for generic in GenericFamily::all() {
                collection.set_generic_families(*generic, std::iter::once(fallback));
            }
        }

        TextShaper {
            font_cx: FontContext {
                collection,
                source_cache,
            },
            layout_cx: LayoutContext::new(),
            faces,
        }
    }

    /// Shapes and line-breaks one block's inline content.
    ///
    /// `items` is the block's flattened, already-whitespace-processed inline content in
    /// document order; `style` is the *container*'s style (the source of the line height, the
    /// alignment, and the default font); `available_width` is the container's content-box
    /// width, which is what lines break against.
    ///
    /// Returns one [`LineBox`] per line, top to bottom. Content that processes to nothing at
    /// all (a whitespace-only text node between two block elements, say) produces **no**
    /// lines — CSS 2.1 §9.4.2: white space that collapses away generates no line box, so such
    /// an anonymous block contributes no height.
    ///
    /// # Errors
    /// [`LayoutError::FontNotBundled`] if `parley` shaped a run with a font this crate cannot
    /// map back to a [`FontKey`]. Unreachable in practice — the font stack always ends in a
    /// bundled family — and deliberately an error rather than a silently dropped run: a glyph
    /// whose face is unknown cannot be rasterised later, so losing it quietly would turn a
    /// font-plumbing bug into missing text.
    pub(crate) fn layout_inline(
        &mut self,
        items: &[InlineItem<'_>],
        style: &LayoutStyle,
        available_width: Au,
    ) -> Result<Vec<LineBox>, LayoutError> {
        let (text, spans) = concatenate(items);
        if text.is_empty() {
            return Ok(Vec::new());
        }

        let layout = self.build_layout(&text, items, &spans, style, available_width);
        self.collect_lines(&layout, &text, &spans, items, style)
    }

    /// Builds the `parley` layout for `text`: pushes the container's style as the default,
    /// one style span per [`InlineItem::Text`], then breaks and aligns the lines.
    fn build_layout(
        &mut self,
        text: &str,
        items: &[InlineItem<'_>],
        spans: &[Span],
        style: &LayoutStyle,
        available_width: Au,
    ) -> parley::Layout<Rgba8> {
        // `scale` is 1.0 and `quantize` is false: this crate lays out in CSS pixels (device
        // pixel ratio is a paint/compositor concern, Task 19+), and rounding to whole device
        // pixels would fight `Au`'s own 1/60px rounding rather than help it.
        let mut builder = self
            .layout_cx
            .ranged_builder(&mut self.font_cx, text, 1.0, false);
        builder.push_default(StyleProperty::FontSize(font_size_px(style)));
        builder.push_default(StyleProperty::LineHeight(LineHeight::Absolute(
            line_height_px(style),
        )));
        builder.push_default(StyleProperty::FontFamily(font_family(style)));
        builder.push_default(StyleProperty::FontWeight(font_weight(style)));
        builder.push_default(StyleProperty::FontStyle(font_style(style)));
        builder.push_default(StyleProperty::Brush(style.color));
        builder.push_default(StyleProperty::TextWrapMode(wrap_mode(style)));

        for span in spans {
            let Some(InlineItem::Text {
                style: item_style, ..
            }) = items.get(span.item)
            else {
                continue;
            };
            let range = span.start..span.end;
            builder.push(
                StyleProperty::FontSize(font_size_px(item_style)),
                range.clone(),
            );
            builder.push(
                StyleProperty::FontFamily(font_family(item_style)),
                range.clone(),
            );
            builder.push(
                StyleProperty::FontWeight(font_weight(item_style)),
                range.clone(),
            );
            builder.push(
                StyleProperty::FontStyle(font_style(item_style)),
                range.clone(),
            );
            builder.push(StyleProperty::Brush(item_style.color), range.clone());
            builder.push(StyleProperty::TextWrapMode(wrap_mode(item_style)), range);
        }

        let mut layout = builder.build(text);
        // A zero or negative available width is legitimate (`width: 0`), and must break at
        // every break opportunity rather than panic or loop. `white-space: pre` content has no
        // soft-wrap opportunities at all (`TextWrapMode::NoWrap`, pushed above), so it
        // overflows this width instead of wrapping — which is what `pre` means.
        layout.break_all_lines(Some(available_width.to_px().max(0.0)));
        layout.align(alignment(style.text_align), AlignmentOptions::default());
        layout
    }

    /// Turns a broken, aligned `parley` layout into [`LineBox`]es.
    fn collect_lines(
        &self,
        layout: &parley::Layout<Rgba8>,
        text: &str,
        spans: &[Span],
        items: &[InlineItem<'_>],
        style: &LayoutStyle,
    ) -> Result<Vec<LineBox>, LayoutError> {
        let line_height = style.line_height.max(Au::ZERO);
        let line_count = layout.len();
        let mut lines = Vec::with_capacity(line_count);

        for (index, line) in layout.lines().enumerate() {
            let mut clusters = self.line_clusters(&line, spans, items, style)?;
            trim_trailing_spaces(&mut clusters, text, spans, items);

            // Text ending in a forced break (a `<br>` at the end of a paragraph, a trailing
            // `\n` in a `<pre>`) makes `parley` emit one more, entirely empty line after it —
            // a caret position, not a line box (`parley`'s own line breaker excludes that
            // line's height from `Layout::height`). CSS 2.1 §9.4.2 generates no line box
            // there, so a *last* line that holds nothing is dropped. An empty line in the
            // middle (`a<br><br>b`) is a real line box and is kept.
            if index + 1 == line_count && clusters.is_empty() {
                continue;
            }

            let metrics = line.metrics();
            let line_x = metrics.offset + metrics.inline_min_coord;
            let baseline = Au::from_px(metrics.baseline - metrics.block_min_coord);

            let (runs, run_items) = build_runs(&clusters, line_x);
            lines.push(LineBox {
                height: line_height,
                baseline,
                runs,
                run_items,
            });
        }

        Ok(lines)
    }

    /// Flattens one line into owned [`ClusterRecord`]s, in visual order.
    ///
    /// Owned rather than borrowed because trailing-space trimming and run grouping both need
    /// to look across `parley`'s run boundaries, and a `parley::Cluster` borrows the `Run` it
    /// came from for that run's whole scope.
    fn line_clusters(
        &self,
        line: &parley::Line<'_, Rgba8>,
        spans: &[Span],
        items: &[InlineItem<'_>],
        style: &LayoutStyle,
    ) -> Result<Vec<ClusterRecord>, LayoutError> {
        let mut out = Vec::new();
        for run in line.runs() {
            let font_key =
                self.key_for_font(run.font())
                    .ok_or_else(|| LayoutError::FontNotBundled {
                        family: style.font_family.join(", "),
                    })?;
            let size = Au::from_px(run.font_size());
            for cluster in run.visual_clusters() {
                let text_start = cluster.text_range().start;
                let item = span_item(spans, text_start);
                let color = match items.get(item.unwrap_or(usize::MAX)) {
                    Some(InlineItem::Text { style, .. }) => style.color,
                    _ => cluster.first_style().brush,
                };
                let glyphs = cluster
                    .glyphs()
                    .map(|g| RawGlyph {
                        id: u16::try_from(g.id).unwrap_or(0),
                        x: g.x,
                        y: g.y,
                        advance: g.advance,
                    })
                    .collect();
                out.push(ClusterRecord {
                    text_start,
                    item,
                    advance: cluster.advance(),
                    font: font_key,
                    size,
                    color,
                    glyphs,
                });
            }
        }
        Ok(out)
    }

    /// Maps a font `parley` shaped with back to the [`FontKey`] it came from.
    ///
    /// Identity is the font *blob* plus the face index, compared by data pointer and length
    /// rather than by family name: the collection this shaper queries is a clone of
    /// [`FontDb`]'s, and cloning it shares each registered blob's `Arc` rather than copying
    /// the bytes, so a face `parley` resolved is byte-for-byte the same `&'static [u8]`
    /// `cl_fonts::bundled` embedded. That makes the mapping exact and deterministic — no name
    /// matching, no `HashMap`, no dependence on how `fontique` happened to normalize a family
    /// name. `None` means `parley` shaped with a face this crate never registered, which
    /// [`TextShaper::layout_inline`] turns into [`LayoutError::FontNotBundled`].
    fn key_for_font(&self, font: &FontData) -> Option<FontKey> {
        let data = font.data.data();
        self.faces
            .iter()
            .find(|face| {
                face.index == font.index
                    && face.data.len() == data.len()
                    && std::ptr::eq(face.data.as_ptr(), data.as_ptr())
            })
            .map(|face| face.key)
    }
}

/// One [`InlineItem::Text`]'s byte range within the concatenated shaping string.
#[derive(Debug, Clone, Copy)]
struct Span {
    start: usize,
    end: usize,
    /// Index into the `items` slice.
    item: usize,
}

/// One glyph as `parley` reported it, before conversion to [`Au`].
struct RawGlyph {
    id: u16,
    x: f32,
    y: f32,
    advance: f32,
}

/// One shaped cluster of a line, flattened out of `parley`'s borrowed run/cluster types.
struct ClusterRecord {
    /// Byte offset of this cluster's first character in the shaping string.
    text_start: usize,
    /// Index into the `items` slice of the [`InlineItem::Text`] this cluster came from, or
    /// `None` for a cluster no item owns (a `<br>`'s newline, which shapes to nothing).
    item: Option<usize>,
    advance: f32,
    font: FontKey,
    size: Au,
    color: Rgba8,
    glyphs: Vec<RawGlyph>,
}

/// Concatenates `items` into the string `parley` shapes, recording where each
/// [`InlineItem::Text`] landed.
///
/// An [`InlineItem::Break`] contributes a `\n`, which the Unicode line breaking algorithm
/// treats as a mandatory break; `parley` gives a newline cluster no glyphs and no advance, so
/// a `<br>` never paints anything of its own.
fn concatenate(items: &[InlineItem<'_>]) -> (String, Vec<Span>) {
    let mut text = String::new();
    let mut spans = Vec::new();
    for (index, item) in items.iter().enumerate() {
        match item {
            InlineItem::Text { text: run, .. } => {
                let start = text.len();
                text.push_str(run);
                if text.len() > start {
                    spans.push(Span {
                        start,
                        end: text.len(),
                        item: index,
                    });
                }
            }
            InlineItem::Break => text.push('\n'),
        }
    }
    (text, spans)
}

/// The index into `items` of the [`InlineItem::Text`] covering byte `offset`, if any.
///
/// Linear rather than binary: a block's item count is its inline element count, and every
/// caller walks the clusters of one line, so the scan is over a handful of entries.
fn span_item(spans: &[Span], offset: usize) -> Option<usize> {
    spans
        .iter()
        .find(|s| offset >= s.start && offset < s.end)
        .map(|s| s.item)
}

/// Drops the clusters of a line's trailing collapsible spaces (CSS Text 3 §4.1.1: a
/// collapsible space at the end of a line hangs — it is not rendered and does not count
/// toward the line's width).
///
/// "Collapsible" is decided from the source: the character really is U+0020 in the shaping
/// string *and* the item it came from is `white-space: normal`. A `white-space: pre` line's
/// trailing spaces are preserved, glyphs and advance included.
///
/// Clusters that draw and measure nothing are stepped over rather than stopping the trim:
/// the mandatory-break cluster at the end of a `<br>`-terminated line is exactly that (see
/// [`concatenate`]), and the collapsible space that precedes it must still hang.
fn trim_trailing_spaces(
    clusters: &mut Vec<ClusterRecord>,
    text: &str,
    spans: &[Span],
    items: &[InlineItem<'_>],
) {
    while clusters.last().is_some_and(|c| {
        (c.glyphs.is_empty() && c.advance <= 0.0)
            || is_collapsible_space(c.text_start, text, spans, items)
    }) {
        clusters.pop();
    }
}

/// Whether the character at `offset` in `text` is a collapsible space — see
/// [`trim_trailing_spaces`].
fn is_collapsible_space(
    offset: usize,
    text: &str,
    spans: &[Span],
    items: &[InlineItem<'_>],
) -> bool {
    if text.get(offset..).and_then(|rest| rest.chars().next()) != Some(' ') {
        return false;
    }
    match span_item(spans, offset).and_then(|index| items.get(index)) {
        Some(InlineItem::Text { style, .. }) => style.white_space == WhiteSpace::Normal,
        _ => false,
    }
}

/// Groups a line's clusters into [`GlyphRun`]s, positioning each glyph.
///
/// A new run starts whenever the font, the size, the color or the source item changes — the
/// first three because a run is by definition uniform in them, the fourth so every run
/// belongs to exactly one `Text` fragment (and therefore one DOM node).
///
/// `line_x` is the line's left edge relative to the container's content box (alignment
/// included); the returned run origins are relative to the line box's top-left corner, and
/// each glyph's `x` is relative to its own run's origin.
fn build_runs(clusters: &[ClusterRecord], line_x: f32) -> (Vec<GlyphRun>, Vec<usize>) {
    let mut runs: Vec<GlyphRun> = Vec::new();
    let mut run_items: Vec<usize> = Vec::new();
    let mut current: Option<(FontKey, Au, Rgba8, Option<usize>)> = None;
    let mut run_start_x = line_x;
    let mut pen_x = line_x;

    for cluster in clusters {
        let key = (cluster.font, cluster.size, cluster.color, cluster.item);
        if current != Some(key) {
            current = Some(key);
            run_start_x = pen_x;
            runs.push(GlyphRun {
                font: cluster.font,
                size: cluster.size,
                origin: Point {
                    x: Au::from_px(pen_x),
                    y: Au::ZERO,
                },
                glyphs: Vec::new(),
                color: cluster.color,
            });
            run_items.push(cluster.item.unwrap_or(usize::MAX));
        }
        let mut glyph_x = pen_x;
        if let Some(run) = runs.last_mut() {
            for glyph in &cluster.glyphs {
                run.glyphs.push(Glyph {
                    id: glyph.id,
                    x: Au::from_px(glyph_x + glyph.x - run_start_x),
                    y: Au::from_px(glyph.y),
                    advance: Au::from_px(glyph.advance),
                });
                glyph_x += glyph.advance;
            }
        }
        pen_x += cluster.advance;
    }

    // A cluster with no glyphs at all (a `<br>`'s newline) can leave an empty run behind.
    let mut kept_items = Vec::with_capacity(run_items.len());
    let mut kept_runs = Vec::with_capacity(runs.len());
    for (run, item) in runs.into_iter().zip(run_items) {
        if !run.glyphs.is_empty() {
            kept_runs.push(run);
            kept_items.push(item);
        }
    }
    (kept_runs, kept_items)
}

/// One style's `font-size`, in CSS pixels, clamped to a shapeable range — see
/// [`MAX_FONT_SIZE_PX`].
fn font_size_px(style: &LayoutStyle) -> f32 {
    style.font_size.to_px().clamp(0.0, MAX_FONT_SIZE_PX)
}

/// One style's `line-height`, in CSS pixels, clamped to non-negative (a negative
/// `line-height` is invalid CSS, but a hand-assembled [`LayoutStyle`] can still carry one).
fn line_height_px(style: &LayoutStyle) -> f32 {
    style.line_height.to_px().max(0.0)
}

/// One style's `font-family` list as a `parley` font stack, with Noto Sans appended.
///
/// Each CSS generic keyword (`serif`, `sans-serif`, `monospace`, …) becomes a
/// `FontFamilyName::Generic`, which [`TextShaper::new`] has already pointed at Noto Sans;
/// every other name is matched by name against the bundled families and simply contributes
/// nothing if it does not match (`fontique` resolves an unknown name to no family). The
/// unconditional Noto Sans at the end is what guarantees *some* bundled face is always
/// selected — including for a stack of nothing but unbundled web fonts — and also serves as
/// the fallback face for a character the requested family lacks.
fn font_family(style: &LayoutStyle) -> FontFamily<'static> {
    let mut names: Vec<FontFamilyName<'static>> = style
        .font_family
        .iter()
        .map(|name| match GenericFamily::parse(name) {
            Some(generic) => FontFamilyName::Generic(generic),
            None => FontFamilyName::Named(Cow::Owned(name.clone())),
        })
        .collect();
    names.push(FontFamilyName::Named(Cow::Borrowed(FALLBACK_FAMILY)));
    FontFamily::List(Cow::Owned(names))
}

/// One style's `font-weight` as `parley`'s.
fn font_weight(style: &LayoutStyle) -> FontWeight {
    FontWeight::new(f32::from(style.font_weight))
}

/// One style's `font-style` as `parley`'s — M1a does not distinguish `italic` from
/// `oblique` (see [`crate::style_adapt::adapt`]), so an italic style is always `Italic`.
fn font_style(style: &LayoutStyle) -> FontStyle {
    if style.font_italic {
        FontStyle::Italic
    } else {
        FontStyle::Normal
    }
}

/// One style's `white-space` as `parley`'s soft-wrap setting.
///
/// `white-space: pre` means "break only at a forced break": it has no soft-wrap opportunities
/// at all, so a `pre` line wider than its containing block overflows rather than wrapping
/// (CSS Text 3 §3 `white-space`, §5 line breaking). `normal` wraps as usual. Pushed both as
/// the container's default and per text span, since a nested inline can carry its own
/// `white-space`.
fn wrap_mode(style: &LayoutStyle) -> TextWrapMode {
    match style.white_space {
        WhiteSpace::Normal => TextWrapMode::Wrap,
        WhiteSpace::Pre => TextWrapMode::NoWrap,
    }
}

/// [`TextAlign`] as `parley`'s alignment.
///
/// The physical variants (`Left`/`Right`) rather than the logical ones (`Start`/`End`):
/// [`TextAlign`] has already folded `start`/`end` onto physical values for M1a's LTR-only
/// scope, so re-deriving direction here would only risk disagreeing with that fold.
fn alignment(align: TextAlign) -> Alignment {
    match align {
        TextAlign::Left => Alignment::Left,
        TextAlign::Right => Alignment::Right,
        TextAlign::Center => Alignment::Center,
    }
}
