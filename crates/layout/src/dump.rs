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

use crate::au::Au;
use crate::box_tree::{BoxId, BoxKind, BoxTree, LayoutBox};
use crate::geom::{
    BoxSizing, Display, Length, Overflow, Position, Rgba8, Sides, TextAlign, WhiteSpace,
};

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
