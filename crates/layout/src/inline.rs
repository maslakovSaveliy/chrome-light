//! The inline formatting context: turns one block box's inline-level children into `Line`
//! and `Text` fragments (CSS 2.1 §9.4.2, CSS Text 3 §4.1.1).
//!
//! This module is the glue between the box tree and [`crate::text::TextShaper`]. It
//!
//! 1. flattens the block's inline-level box-tree children into a document-order list of text
//!    runs and `<br>`s, walking *into* nested `Inline` boxes (with an explicit stack, never
//!    recursion — a document's nesting depth is attacker-controlled);
//! 2. whitespace-processes each text node against its own `white-space`, threading the
//!    collapse state across run boundaries so `<span>a </span><span> b</span>` becomes
//!    `"a b"` (see [`crate::whitespace`]);
//! 3. hands the result to the shaper, which shapes it, breaks it into lines against the
//!    container's content-box width and aligns it;
//! 4. turns each line into one `Line` fragment whose children are `Text` fragments — one per
//!    source text node's contribution to that line, each carrying that node's own
//!    [`crate::geom::LayoutStyle`] and its [`crate::text::GlyphRun`]s.
//!
//! # What an `Inline` box contributes
//!
//! Its descendants' text, with *its* inherited style — `<p>a<b>b</b>c</p>` produces three
//! `Text` fragments, the middle one with `font-weight: 700`. An `Inline` box generates no
//! fragment of its own, and M1a paints no inline borders, padding or backgrounds: those
//! would need inline *fragments* with their own box model, which is a later task. Any
//! `background-color`/`border` on a `<span>` is therefore read into its style, reachable via
//! the `Text` fragment's `StyleId`, but drawn by nobody.
//!
//! # Line boxes
//!
//! A line box is as wide as its containing block and stacks directly under the previous one
//! (CSS 2.1 §9.4.2), so a `Line` fragment's three rects are all the same and all span the
//! container's content box horizontally: `x`/`w` are the container's content-box `x`/width,
//! `y` is one line height per preceding line below the container's content-box top, and `h`
//! is the *container*'s `line-height` (M1a implements neither per-inline line heights nor
//! vertical alignment, so there is no per-line maximum of differing inline heights to take).
//!
//! Where the text actually sits *within* that full-width box is `text-align`'s business, and
//! it is carried by the runs: every [`crate::text::GlyphRun`] has its own absolute origin, and
//! each `Text` child fragment spans exactly its own runs. So a centred line is a full-width
//! `Line` fragment whose `Text` children start half the free space in.
//!
//! # Content that collapses to nothing
//!
//! A block whose inline content processes to an empty string — a whitespace-only text node
//! between two block elements is the common case — produces **no** line box at all and
//! contributes zero height, which is what CSS 2.1 §9.2.1.1/§9.4.2 mean by white space that
//! "collapses away". (Task 17's placeholder instead gave such an anonymous block one full
//! `line-height`; the goldens show the difference.)

use cl_dom::{Document, NodeId, NodeKind};

use crate::au::Au;
use crate::box_tree::{BoxId, BoxKind, BoxTree};
use crate::error::LayoutError;
use crate::fragment::{Fragment, FragmentId, FragmentKind, StyleId};
use crate::geom::{LayoutStyle, Point, Rect, Size};
use crate::text::{GlyphRun, InlineItem, LineBox, TextShaper};
use crate::whitespace;

/// The long-lived context one layout pass threads through inline layout.
pub(crate) struct InlineContext<'a> {
    /// The box tree being laid out.
    pub(crate) box_tree: &'a BoxTree,
    /// The document the box tree was built from — the only place a text node's characters
    /// live (a [`BoxTree`] does not borrow it; see [`crate::box_tree::build`]'s docs).
    pub(crate) document: &'a Document,
    /// The shaper, reused across every block in the document.
    pub(crate) shaper: &'a mut TextShaper,
}

/// One block's inline content and the box it is being laid out into.
pub(crate) struct InlineArgs<'a> {
    /// The block's own box-tree children, all inline-level.
    pub(crate) children: &'a [BoxId],
    /// The block's style: the source of `line-height`, `text-align`, and the default font.
    pub(crate) container_style: &'a LayoutStyle,
    /// The block's absolute content-box origin.
    pub(crate) content_origin: Point,
    /// The block's content-box width — what lines break against.
    pub(crate) content_width: Au,
}

/// Lays out one block's inline content, appending its `Line`/`Text` fragments to `fragments`.
///
/// Returns the new `Line` fragments' ids (in order) and the total height they contribute to
/// the container's content box.
///
/// # Errors
/// Propagates [`LayoutError::FontNotBundled`] from the shaper.
pub(crate) fn layout(
    ctx: &mut InlineContext<'_>,
    args: &InlineArgs<'_>,
    fragments: &mut Vec<Fragment>,
    styles: &mut Vec<LayoutStyle>,
) -> Result<(Vec<FragmentId>, Au), LayoutError> {
    let pieces = collect_pieces(ctx.box_tree, ctx.document, args.children);
    let items: Vec<InlineItem<'_>> = pieces
        .iter()
        .map(|piece| match piece {
            Piece::Text(text) => InlineItem::Text {
                text: &text.text,
                style: &text.style,
                node: text.node,
            },
            Piece::Break => InlineItem::Break,
        })
        .collect();

    let lines = ctx
        .shaper
        .layout_inline(&items, args.container_style, args.content_width)?;

    Ok(build_fragments(&lines, &items, args, fragments, styles))
}

/// One piece of flattened inline content, owning its processed text so
/// [`crate::text::InlineItem`] can borrow it.
enum Piece {
    /// A text node's contribution. Boxed: a [`LayoutStyle`] is a few hundred bytes, and a
    /// `Vec<Piece>` of mostly-`Break` entries should not each pay for one.
    Text(Box<TextPiece>),
    /// A `<br>`.
    Break,
}

/// The owned half of [`Piece::Text`].
struct TextPiece {
    /// The text after whitespace processing — possibly empty.
    text: String,
    /// The inherited style the text node's box carries.
    style: LayoutStyle,
    /// The text node itself.
    node: NodeId,
}

/// Flattens `children` into document-order [`Piece`]s, whitespace-processing each text node
/// as it goes.
///
/// Walks into nested `Inline`/`AnonymousInline` boxes with an explicit stack (the box tree's
/// invariant is that their children are inline-level too), never recursing; the walk is
/// bounded by the box tree's size, the same hostile-input safeguard [`crate::dump`]'s walks
/// use — never actually reached, since a box tree has no cycles.
///
/// The whitespace collapse state is threaded across pieces in document order and reset after
/// every `<br>`: a collapsible space at the start of a block, or just after a forced break,
/// is removed entirely (CSS Text 3 §4.1.1). A collapsible space at the *end* of a line is not
/// removed here — it cannot be, since lines are not known until the shaper has broken them —
/// but hangs, and the shaper drops its glyphs from the line's runs.
fn collect_pieces(box_tree: &BoxTree, document: &Document, children: &[BoxId]) -> Vec<Piece> {
    let mut pieces = Vec::new();
    let mut stack: Vec<BoxId> = children.iter().rev().copied().collect();
    let mut remaining = box_tree.len();
    let mut suppress_space = true;

    while let Some(id) = stack.pop() {
        if remaining == 0 {
            break;
        }
        remaining -= 1;
        let Some(b) = box_tree.get(id) else {
            continue;
        };
        match &b.kind {
            BoxKind::InlineText(node) => {
                let source = text_of(document, *node);
                let processed = whitespace::process(source, b.style.white_space, suppress_space);
                suppress_space = processed.suppress_next_space;
                pieces.push(Piece::Text(Box::new(TextPiece {
                    text: processed.text,
                    style: b.style.clone(),
                    node: *node,
                })));
            }
            BoxKind::LineBreak => {
                pieces.push(Piece::Break);
                suppress_space = true;
            }
            BoxKind::Inline | BoxKind::AnonymousInline => {
                stack.extend(b.children.iter().rev().copied());
            }
            // Never a child of a block whose children are inline-level, per the box tree's
            // own invariant — skipped defensively rather than trusted, in case a
            // hand-assembled `BoxTree` breaks it.
            BoxKind::Block | BoxKind::AnonymousBlock => {}
        }
    }

    pieces
}

/// One text node's characters, or `""` if `node` is not a text node of `document` (which
/// [`crate::box_tree::build`] never produces, but a hand-assembled tree could).
fn text_of(document: &Document, node: NodeId) -> &str {
    match document.get(node).map(|n| &n.kind) {
        Some(NodeKind::Text(text)) => text,
        _ => "",
    }
}

/// Turns the shaper's [`LineBox`]es into `Line` fragments with `Text` children, shifting
/// every run's origin from container-content-box-relative to absolute.
fn build_fragments(
    lines: &[LineBox],
    items: &[InlineItem<'_>],
    args: &InlineArgs<'_>,
    fragments: &mut Vec<Fragment>,
    styles: &mut Vec<LayoutStyle>,
) -> (Vec<FragmentId>, Au) {
    let origin = args.content_origin;
    let mut line_ids = Vec::with_capacity(lines.len());
    let mut cursor_y = Au::ZERO;

    for line in lines {
        let line_top = origin.y.saturating_add(cursor_y);
        // CSS 2.1 §9.4.2: a line box is as wide as its containing block, whatever the text
        // inside it does — `text-align` moves the *content* within the line box, not the box.
        let line_rect = Rect {
            origin: Point {
                x: origin.x,
                y: line_top,
            },
            size: Size {
                w: args.content_width,
                h: line.height,
            },
        };

        let mut children = Vec::new();
        for group in group_runs(line) {
            let (node, style) = match items.get(group.item) {
                Some(InlineItem::Text { node, style, .. }) => (Some(*node), (*style).clone()),
                _ => (None, args.container_style.clone()),
            };
            let baseline_y = line_top.saturating_add(line.baseline);
            let runs: Vec<GlyphRun> = group
                .runs
                .iter()
                .map(|run| GlyphRun {
                    origin: Point {
                        x: origin.x.saturating_add(run.origin.x),
                        y: baseline_y.saturating_add(run.origin.y),
                    },
                    ..run.clone()
                })
                .collect();
            let rect = Rect {
                origin: Point {
                    x: runs.first().map_or(origin.x, |r| r.origin.x),
                    y: line_top,
                },
                size: Size {
                    w: runs_width(&runs),
                    h: line.height,
                },
            };
            let style_id = push_style(styles, style);
            children.push(push_fragment(
                fragments,
                Fragment {
                    node,
                    kind: FragmentKind::Text { runs },
                    border_box: rect,
                    padding_box: rect,
                    content_box: rect,
                    style: style_id,
                    children: Vec::new(),
                },
            ));
        }

        let line_style_id = push_style(styles, args.container_style.clone());
        line_ids.push(push_fragment(
            fragments,
            Fragment {
                node: None,
                kind: FragmentKind::Line,
                border_box: line_rect,
                padding_box: line_rect,
                content_box: line_rect,
                style: line_style_id,
                children,
            },
        ));
        cursor_y = cursor_y.saturating_add(line.height);
    }

    (line_ids, cursor_y)
}

/// One `Text` fragment's worth of runs: every consecutive run of a line that came from the
/// same source text node.
struct RunGroup<'a> {
    /// Index into the `pieces` slice of the text node these runs came from.
    item: usize,
    runs: &'a [GlyphRun],
}

/// Splits a line's runs into [`RunGroup`]s, one per source text node contribution.
///
/// Runs are already grouped by source item within a line (the shaper starts a new run
/// whenever the item changes), so this only has to find the boundaries.
fn group_runs(line: &LineBox) -> Vec<RunGroup<'_>> {
    let mut groups: Vec<RunGroup<'_>> = Vec::new();
    let mut start = 0usize;
    for index in 0..line.runs.len() {
        let item = line.run_items.get(index).copied().unwrap_or(usize::MAX);
        let next = line.run_items.get(index + 1).copied();
        if next != Some(item) {
            if let Some(runs) = line.runs.get(start..=index) {
                groups.push(RunGroup { item, runs });
            }
            start = index + 1;
        }
    }
    groups
}

/// The inline extent covered by `runs`: from the first run's origin to the end of the last
/// run's last glyph.
fn runs_width(runs: &[GlyphRun]) -> Au {
    let (Some(first), Some(last)) = (runs.first(), runs.last()) else {
        return Au::ZERO;
    };
    let end = last
        .glyphs
        .iter()
        .fold(last.origin.x, |x, glyph| x.saturating_add(glyph.advance));
    end.saturating_sub(first.origin.x)
}

/// Appends `style` to the tree's style table and returns its id — mirrors
/// `crate::block`'s own helper of the same name (both exist so the two modules can build
/// fragments independently without either owning the other's arena).
fn push_style(styles: &mut Vec<LayoutStyle>, style: LayoutStyle) -> StyleId {
    let id = StyleId::from_index(styles.len());
    styles.push(style);
    id
}

/// Appends `fragment` to the arena and returns its id.
fn push_fragment(fragments: &mut Vec<Fragment>, fragment: Fragment) -> FragmentId {
    let id = FragmentId::from_index(fragments.len());
    fragments.push(fragment);
    id
}
