//! A stable, human-readable rendering of a [`StyledDocument`]'s computed values, for
//! snapshot tests and for eyeballing what the cascade actually produced.
//!
//! The layout deliberately mirrors `cl_dom::serialize::html5lib_tree`: same `#document`
//! header, same `| ` marker, same two-spaces-per-depth indent, so a tree dump and a style
//! dump of the same document line up when read side by side. Each element gets one line for
//! its tag and then one line per property, indented one level deeper — exactly where
//! `html5lib_tree` puts an element's attributes.
//!
//! Only elements appear. Text, comments and doctypes have no computed style of their own,
//! and printing them would double the size of every golden for no information.

use std::fmt::Write as _;

use cl_dom::{Document, Element, NodeId, ns};
use style::properties::LonghandId;

use crate::engine::StyledDocument;

/// The longhands every element's dump reports, in this order.
///
/// Chosen to cover what `cl-layout` reads in M1a (box type, geometry, colours, the font
/// and text bits that drive line breaking) while staying short enough that a golden diff is
/// readable. The names are the CSS property names, so a dump line can be pasted straight
/// back into a stylesheet.
///
/// `white-space` is absent because it is no longer a longhand: CSS Text 4 split it into
/// `white-space-collapse` and `text-wrap-mode`, and stylo 0.20 follows (`LonghandId` has
/// both; `white-space` is a `ShorthandId`). Both halves are dumped instead, which is
/// strictly more information than the shorthand carries.
const DUMPED_LONGHANDS: &[(&str, LonghandId)] = &[
    ("display", LonghandId::Display),
    ("position", LonghandId::Position),
    ("width", LonghandId::Width),
    ("height", LonghandId::Height),
    ("margin-top", LonghandId::MarginTop),
    ("margin-right", LonghandId::MarginRight),
    ("margin-bottom", LonghandId::MarginBottom),
    ("margin-left", LonghandId::MarginLeft),
    ("padding-top", LonghandId::PaddingTop),
    ("padding-right", LonghandId::PaddingRight),
    ("padding-bottom", LonghandId::PaddingBottom),
    ("padding-left", LonghandId::PaddingLeft),
    ("border-top-width", LonghandId::BorderTopWidth),
    ("border-right-width", LonghandId::BorderRightWidth),
    ("border-bottom-width", LonghandId::BorderBottomWidth),
    ("border-left-width", LonghandId::BorderLeftWidth),
    ("color", LonghandId::Color),
    ("background-color", LonghandId::BackgroundColor),
    ("font-size", LonghandId::FontSize),
    ("font-weight", LonghandId::FontWeight),
    ("line-height", LonghandId::LineHeight),
    ("text-align", LonghandId::TextAlign),
    ("white-space-collapse", LonghandId::WhiteSpaceCollapse),
    ("text-wrap-mode", LonghandId::TextWrapMode),
];

/// Renders every element of `styled` with its tag and [`DUMPED_LONGHANDS`] computed values.
///
/// # Format
///
/// ```text
/// #document
/// | <html>
/// |   display: block
/// |   ...
/// |   <body>
/// |     display: block
/// ```
///
/// The first line is `#document`. Each element is `| ` + two spaces per level *above* its
/// depth (a direct child of the document node is depth 1 and gets no indent) + `<tag>`,
/// exactly where `html5lib_tree` puts an element; SVG and `MathML` elements
/// render as `<svg tag>` / `<math tag>`, the same way `cl_dom::serialize::html5lib_tree`
/// distinguishes them. Its property lines sit one level deeper, as `name: value`, where
/// `value` is the property's computed value serialized by stylo itself
/// (`ComputedValues::computed_or_resolved_value` with no resolution context, so what is
/// printed is the computed value, not a used value that would need layout to exist).
///
/// Computed, not used, is worth taking literally for `border-*-width`: stylo keeps the
/// specified width (`medium`, i.e. `3px`, when nothing set one) whatever `border-*-style`
/// says, and zeroing it for `border-style: none` is a used-value step the consumer does.
/// So a dump showing `border-top-width: 3px` on an element with no border is correct, and
/// `cl-layout` must consult `border-*-style` rather than trusting the width alone.
///
/// An element the traversal never reached — the descendants of a `display: none` element,
/// which stylo skips on purpose — gets a single `(no computed style)` line instead of the
/// property block, so the golden records the skip rather than silently omitting it.
///
/// The walk uses one explicit stack and is bounded by `Document::len()`, like
/// `html5lib_tree`'s: hostile input can nest arbitrarily deep, and neither recursion nor an
/// unbounded loop is acceptable here.
#[must_use]
pub fn computed_style_dump(styled: &StyledDocument) -> String {
    let doc = styled.document();
    let mut lines = vec![String::from("#document")];
    let mut stack: Vec<(NodeId, usize)> = Vec::new();
    push_children(doc, doc.root(), 1, &mut stack);

    let mut remaining = doc.len();
    while let Some((id, depth)) = stack.pop() {
        if remaining == 0 {
            break;
        }
        remaining -= 1;

        let Some(element) = doc.element(id) else {
            // A non-element node contributes no line, but its subtree still might: keep the
            // depth so the elements below it stay at their true tree depth.
            push_children(doc, id, depth, &mut stack);
            continue;
        };

        lines.push(format!(
            "| {}{}",
            indent(depth.saturating_sub(1)),
            tag(element)
        ));
        match styled.computed(id) {
            Some(values) => {
                for (name, longhand) in DUMPED_LONGHANDS {
                    let mut value = String::new();
                    // Writing into a `String` cannot fail; the `Result` is stylo's generic
                    // `fmt::Write` signature. A failure would leave `value` empty, which is
                    // still a printable line.
                    let _ = values.computed_or_resolved_value(*longhand, None, &mut value);
                    let mut line = format!("| {}{name}: ", indent(depth));
                    line.push_str(&value);
                    lines.push(line);
                }
            }
            None => lines.push(format!("| {}(no computed style)", indent(depth))),
        }

        push_children(doc, id, depth + 1, &mut stack);
    }

    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let _ = write!(out, "{line}");
    }
    out
}

/// Two spaces per level, the indent `cl_dom::serialize::html5lib_tree` uses.
fn indent(depth: usize) -> String {
    "  ".repeat(depth)
}

/// An element's rendering: `<tag>`, or `<svg tag>` / `<math tag>` for the two foreign
/// namespaces, matching `html5lib_tree`.
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

/// Pushes `parent`'s children onto `stack` in reverse document order at `depth`, so popping
/// yields them front to back. Same trick (and same `Document::len()` bound) as
/// `cl_dom::serialize`'s walk.
fn push_children(doc: &Document, parent: NodeId, depth: usize, stack: &mut Vec<(NodeId, usize)>) {
    let mut children: Vec<NodeId> = doc.children(parent).collect();
    children.reverse();
    stack.extend(children.into_iter().map(|id| (id, depth)));
}
