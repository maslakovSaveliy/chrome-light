//! A stable, human-readable rendering of a [`crate::box_tree::BoxTree`], for snapshot tests
//! and for eyeballing what box generation actually produced.
//!
//! [`box_tree_dump`] takes only a `&BoxTree` (no `cl_dom::Document`, unlike
//! `cl_style::dump::computed_style_dump`), so it cannot print a box's originating tag name or
//! a text box's content — those live in the `Document` this tree was built from, which the
//! tree itself does not borrow (see `cl_layout::box_tree::build`'s docs: a `BoxTree` outlives
//! the `StyledDocument` it was built from no differently than any other owned value). Each
//! box is instead labelled by its [`crate::box_tree::BoxKind`] plus the `cl_dom::NodeId`
//! index it was generated from, when it has one — enough to cross-reference against a
//! `computed_style_dump`/`html5lib_tree` dump of the same document if needed, without this
//! module depending on either.
//!
//! The layout otherwise mirrors `cl_style::dump::computed_style_dump`: a `#boxtree` header,
//! the same `| ` marker, two spaces per depth, one line per box followed by its style fields
//! one level deeper.

use std::fmt::Write as _;

use cl_dom::{Document, Element, ns};
use cl_fonts::FontDb;

use crate::au::Au;
use crate::box_tree::{BoxId, BoxKind, BoxTree, LayoutBox};
use crate::fragment::{Fragment, FragmentId, FragmentKind, FragmentTree};
use crate::geom::{
    BoxSizing, Display, Length, Overflow, Position, Rect, Rgba8, Sides, TextAlign, WhiteSpace,
};
use crate::text::GlyphRun;

/// Renders every box of `tree` with its kind and layout-relevant style fields.
///
/// # Format
///
/// ```text
/// #boxtree
/// | Block node=1
/// |   display: block
/// |   ...
/// |   Inline node=2
/// |     display: inline
/// |     ...
/// ```
///
/// The walk uses one explicit stack and is bounded by [`BoxTree::len`], the same
/// hostile-input safeguard `computed_style_dump`'s walk uses: a tree built from an
/// attacker-controlled document can be very large, but never contains a cycle (every
/// `BoxId` a child list holds was allocated strictly before its parent — see
/// `crate::box_tree::build`), so the bound is never actually reached; it exists so a future
/// change to `BoxTree`'s construction that *did* introduce a cycle would make this function
/// stop rather than loop forever.
#[must_use]
pub fn box_tree_dump(tree: &BoxTree) -> String {
    let mut lines = vec![String::from("#boxtree")];
    let mut stack: Vec<(BoxId, usize)> = vec![(tree.root, 0)];
    let mut remaining = tree.len();

    // A plain `Vec` used as a stack, popped from the end: to visit children in order, they
    // are pushed in reverse (the same trick `cl_style::dump`'s walk uses).
    while let Some((id, depth)) = stack.pop() {
        if remaining == 0 {
            break;
        }
        remaining -= 1;

        let Some(b) = tree.get(id) else {
            continue;
        };

        lines.push(format!("| {}{}", indent(depth), label(b)));
        for line in style_lines(b) {
            lines.push(format!("| {}{line}", indent(depth + 1)));
        }

        let mut children = b.children.clone();
        children.reverse();
        stack.extend(children.into_iter().map(|child| (child, depth + 1)));
    }

    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(line);
    }
    out
}

/// Two spaces per level, matching `cl_style::dump`/`cl_dom::serialize`'s indent.
fn indent(depth: usize) -> String {
    "  ".repeat(depth)
}

/// A box's own line: its `BoxKind` plus, when it has one, its originating node's arena index.
fn label(b: &LayoutBox) -> String {
    let kind = match &b.kind {
        BoxKind::Block => "Block",
        BoxKind::Inline => "Inline",
        BoxKind::InlineText(_) => "InlineText",
        BoxKind::AnonymousBlock => "AnonymousBlock",
        BoxKind::AnonymousInline => "AnonymousInline",
        BoxKind::LineBreak => "LineBreak",
    };
    match b.node {
        Some(node) => format!("{kind} node={}", node_index(node)),
        None => kind.to_owned(),
    }
}

/// `cl_dom::NodeId`'s arena index, for display — `NodeId` itself does not implement
/// `Display`, only the `index()` accessor this crate is allowed to call on it.
fn node_index(node: cl_dom::NodeId) -> usize {
    node.index()
}

/// One box's style fields, as `name: value` lines, in a fixed order covering every
/// [`crate::geom::LayoutStyle`] field layout reads.
fn style_lines(b: &LayoutBox) -> Vec<String> {
    let s = &b.style;
    vec![
        format!("display: {}", display_str(s.display)),
        format!("position: {}", position_str(s.position)),
        format!("width: {}", length_str(s.width)),
        format!("height: {}", length_str(s.height)),
        format!("min-width: {}", length_str(s.min_width)),
        format!("max-width: {}", opt_length_str(s.max_width)),
        format!("min-height: {}", length_str(s.min_height)),
        format!("max-height: {}", opt_length_str(s.max_height)),
        format!("margin: {}", sides_str(&s.margin, length_str)),
        format!("padding: {}", sides_str(&s.padding, length_str)),
        format!("border-width: {}", sides_str(&s.border_width, px_str)),
        format!("border-color: {}", sides_str(&s.border_color, rgba_str)),
        format!("border-solid: {}", sides_str(&s.border_solid, bool_str)),
        format!("box-sizing: {}", box_sizing_str(s.box_sizing)),
        format!("overflow: {}", overflow_str(s.overflow)),
        format!("offset: {}", sides_str(&s.offset, length_str)),
        format!("color: {}", rgba_str(s.color)),
        format!("background: {}", rgba_str(s.background)),
        format!("font-family: {}", s.font_family.join(", ")),
        format!("font-size: {}", px_str(s.font_size)),
        format!("font-weight: {}", s.font_weight),
        format!("font-italic: {}", s.font_italic),
        format!("line-height: {}", px_str(s.line_height)),
        format!("text-align: {}", text_align_str(s.text_align)),
        format!("white-space: {}", white_space_str(s.white_space)),
    ]
}

fn display_str(d: Display) -> &'static str {
    match d {
        Display::Block => "block",
        Display::Inline => "inline",
        Display::None => "none",
    }
}

fn position_str(p: Position) -> &'static str {
    match p {
        Position::Static => "static",
        Position::Relative => "relative",
    }
}

fn box_sizing_str(b: BoxSizing) -> &'static str {
    match b {
        BoxSizing::ContentBox => "content-box",
        BoxSizing::BorderBox => "border-box",
    }
}

fn overflow_str(o: Overflow) -> &'static str {
    match o {
        Overflow::Visible => "visible",
        Overflow::Hidden => "hidden",
    }
}

fn text_align_str(t: TextAlign) -> &'static str {
    match t {
        TextAlign::Left => "left",
        TextAlign::Right => "right",
        TextAlign::Center => "center",
    }
}

fn white_space_str(w: WhiteSpace) -> &'static str {
    match w {
        WhiteSpace::Normal => "normal",
        WhiteSpace::Pre => "pre",
    }
}

fn length_str(l: Length) -> String {
    match l {
        Length::Auto => String::from("auto"),
        Length::Px(au) => px_str(au),
        Length::Percent(pct) => format!("{pct}%"),
    }
}

fn opt_length_str(l: Option<Length>) -> String {
    match l {
        Some(l) => length_str(l),
        None => String::from("none"),
    }
}

fn px_str(au: Au) -> String {
    format!("{}px", au.to_px())
}

fn bool_str(b: bool) -> String {
    b.to_string()
}

fn rgba_str(c: Rgba8) -> String {
    format!("rgba({}, {}, {}, {})", c.r, c.g, c.b, c.a)
}

/// Renders a `Sides<T>` as its four values in CSS clockwise order (top right bottom left),
/// space-separated on one line.
fn sides_str<T: Copy>(sides: &Sides<T>, mut render: impl FnMut(T) -> String) -> String {
    let mut out = String::new();
    let _ = write!(
        out,
        "{} {} {} {}",
        render(sides.top),
        render(sides.right),
        render(sides.bottom),
        render(sides.left)
    );
    out
}

/// Renders every fragment of `tree` with its kind, originating tag (when it has one) and
/// geometry, for snapshot tests and for eyeballing what [`crate::block::layout`] actually
/// produced.
///
/// `doc` is the [`Document`] the [`cl_style::StyledDocument`] passed to
/// [`crate::block::layout`] was built from — needed to print a `Block`/`AnonymousBlock`
/// fragment's originating tag, the one thing [`box_tree_dump`] never needed (it only ever
/// printed a node's raw index; see that function's docs for why). This is otherwise the same
/// deliberate deviation `computed_style_dump` takes for the same reason: a [`FragmentTree`]
/// does not borrow the `Document` it was laid out from any more than a
/// [`crate::box_tree::BoxTree`] does (see `crate::box_tree::build`'s docs), so a dump that
/// wants a tag name must be handed one to look it up in.
///
/// `fonts` is the same [`FontDb`] the tree was laid out with — needed to turn a glyph run's
/// `cl_fonts::FontKey` (an opaque index, meaningful only against the database that issued it)
/// back into a family name.
///
/// # Format
///
/// ```text
/// #fragments
/// | Block <html> border=(0.00, 0.00, 800.00, 22.00) content=(0.00, 0.00, 800.00, 22.00)
/// |   Block <body> border=(8.00, 8.00, 784.00, 6.00) content=(8.00, 8.00, 784.00, 6.00)
/// |     Line border=(8.00, 8.00, 32.00, 19.20) content=(8.00, 8.00, 32.00, 19.20)
/// |       Text runs=1 border=(8.00, 8.00, 32.00, 19.20) content=(8.00, 8.00, 32.00, 19.20)
/// |         Run font=Ahem size=16.00 origin=(8.00, 20.80) glyphs=2 advance=32.00
/// ```
///
/// A `Text` fragment's line carries its run count (`runs=N`), and each of its runs gets one
/// line of its own, indented one further level: the family the run was shaped with, its font
/// size, its origin (the left end of its baseline, absolute like every other coordinate
/// here), how many glyphs it holds, and their total advance — all in CSS pixels to two
/// decimals. Individual glyphs are deliberately *not* printed: a glyph-per-line dump would
/// bury the geometry these goldens exist to check under hundreds of lines, and glyph ids are
/// font-build-specific in a way the rest of this format is not. `cl-layout`'s
/// `tests/inline.rs` asserts per-glyph positions directly instead.
///
/// Every rect is `(x, y, w, h)`, each printed as CSS pixels with two decimal places, and —
/// unlike a typical fragment tree, and worth calling out explicitly — **absolute**: relative
/// to the viewport origin, never to the fragment's own parent (see [`Fragment::border_box`]'s
/// docs). Only `border_box` and `content_box` are shown (`padding_box` sits exactly between
/// the two and does not earn a third column in an already-wide line). A `Block`/
/// `AnonymousBlock`/`Line`/`Text` fragment's kind name always appears; only `Block` prints a
/// tag (an element fragment — `<html>`/`<body>` included), matching the controller ruling
/// that shaped this format. The walk uses one explicit stack, bounded by [`FragmentTree::len`]
/// like [`box_tree_dump`]'s, for the same hostile-input reason.
#[must_use]
pub fn fragment_tree_dump(tree: &FragmentTree, doc: &Document, fonts: &FontDb) -> String {
    let mut lines = vec![String::from("#fragments")];
    let mut stack: Vec<(FragmentId, usize)> = vec![(tree.root, 0)];
    let mut remaining = tree.len();

    while let Some((id, depth)) = stack.pop() {
        if remaining == 0 {
            break;
        }
        remaining -= 1;

        let Some(f) = tree.get(id) else {
            continue;
        };

        lines.push(format!("| {}{}", indent(depth), fragment_label(f, doc)));
        if let FragmentKind::Text { runs } = &f.kind {
            for run in runs {
                lines.push(format!("| {}{}", indent(depth + 1), run_line(run, fonts)));
            }
        }

        let mut children = f.children.clone();
        children.reverse();
        stack.extend(children.into_iter().map(|child| (child, depth + 1)));
    }

    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(line);
    }
    out
}

/// One fragment's dump line: its kind (plus tag, for a `Block`; plus run count, for a
/// `Text`) and its `border`/`content` rects.
fn fragment_label(f: &Fragment, doc: &Document) -> String {
    let kind = match &f.kind {
        FragmentKind::Block => "Block",
        FragmentKind::AnonymousBlock => "AnonymousBlock",
        FragmentKind::Line => "Line",
        FragmentKind::Text { .. } => "Text",
    };
    let name = match (&f.kind, f.node) {
        (FragmentKind::Block, Some(node)) => match doc.element(node) {
            Some(element) => format!("{kind} {}", tag(element)),
            None => kind.to_owned(),
        },
        (FragmentKind::Text { runs }, _) => format!("{kind} runs={}", runs.len()),
        _ => kind.to_owned(),
    };
    format!(
        "{name} border={} content={}",
        rect_str(f.border_box),
        rect_str(f.content_box)
    )
}

/// An element's rendering: `<tag>`, or `<svg tag>` / `<math tag>` for the two foreign
/// namespaces — the same convention `cl_style::dump::computed_style_dump` and
/// `cl_dom::serialize::html5lib_tree` use.
fn tag(element: &Element) -> String {
    let local = &element.name.local;
    if element.name.ns == ns!(svg) {
        format!("<svg {local}>")
    } else if element.name.ns == ns!(mathml) {
        format!("<math {local}>")
    } else {
        format!("<{local}>")
    }
}

/// One rect as `(x, y, w, h)`, each coordinate a CSS pixel value with two decimal places —
/// see [`fragment_tree_dump`]'s docs for why two decimals (not the bare, variable-precision
/// `{}px` [`px_str`] uses for a box-tree dump's style values): a geometry dump benefits from
/// column-like alignment a variable-width float render would not give it.
fn rect_str(r: Rect) -> String {
    format!(
        "({:.2}, {:.2}, {:.2}, {:.2})",
        r.origin.x.to_px(),
        r.origin.y.to_px(),
        r.size.w.to_px(),
        r.size.h.to_px()
    )
}

/// One glyph run's dump line — see [`fragment_tree_dump`]'s format docs.
///
/// A run whose [`cl_fonts::FontKey`] does not resolve in `fonts` prints `font=?`: the dump is
/// a debugging aid and must stay total even for a hand-assembled tree, and
/// [`crate::block::layout`] itself reports that condition as
/// [`crate::error::LayoutError::FontNotBundled`] rather than producing such a run.
fn run_line(run: &GlyphRun, fonts: &FontDb) -> String {
    let family = fonts.face(run.font).map_or("?", |face| face.family);
    let advance = run
        .glyphs
        .iter()
        .fold(Au::ZERO, |sum, glyph| sum.saturating_add(glyph.advance));
    format!(
        "Run font={family} size={:.2} origin=({:.2}, {:.2}) glyphs={} advance={:.2}",
        run.size.to_px(),
        run.origin.x.to_px(),
        run.origin.y.to_px(),
        run.glyphs.len(),
        advance.to_px()
    )
}
