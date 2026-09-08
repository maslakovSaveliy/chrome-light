//! The html5lib tree-construction `#document` dump format.
//!
//! [`html5lib_tree`] renders a [`Document`] exactly the way the reference test corpus
//! (`html5lib-tests/tree-construction/*.dat`, the block after each test's `#document`
//! line) renders its expected parse trees. Task 9's conformance harness diffs a real
//! parser's output against that corpus line-for-line, so every character here — the `| `
//! marker, the two-space-per-depth indent, the exact punctuation around each node kind —
//! is load-bearing, not a style choice.
//!
//! Verified directly against the corpus (see the Task 4 report for the fetched commit and
//! files): plain elements (`tests1.dat`), attribute sorting (`webkit01.dat`), doctypes with
//! and without public/system ids (`doctype01.dat`), the `<svg …>`/`<math …>` tag prefix
//! (`svg.dat`, `math.dat`), `<template>` content nesting (`template.dat`), and the
//! `prefix local="value"` form taken by an adjusted foreign attribute inside SVG/`MathML`
//! content, as opposed to a literal colon-containing attribute name on a plain HTML element
//! (`tests9.dat`, the `xlink:href` vs. `xlink href` contrast).
//!
//! No recursion: the walk uses one explicit `(NodeId, depth)` stack (see the crate-level
//! docs on why — hostile input can nest arbitrarily deep) and is bounded by
//! [`Document::len`], so it terminates even on a corrupted arena.

use crate::document::Document;
use crate::element::Attr;
use crate::node::{Doctype, Node, NodeId, NodeKind};
use crate::{Namespace, ns};

/// Renders `doc` in the html5lib tree-construction `#document` format.
///
/// # Format
///
/// - The first line is `#document`.
/// - Every other node is one line: `| ` followed by two spaces per depth level (a direct
///   child of the document root is depth 1), then the node's own rendering.
/// - An element renders as `<tag>`; an element in the SVG namespace as `<svg tag>`; in the
///   `MathML` namespace as `<math tag>`. Its attributes each get their own line, one level
///   deeper than the element, sorted by `(namespace, local name)`. A plain attribute
///   renders as `name="value"`; one with a namespace prefix (only possible for an "adjusted
///   foreign attribute" inside SVG/`MathML` content — `xlink:href`, `xml:lang`, and the like)
///   renders as `prefix local="value"` (space-separated, no colon) — see the module docs
///   for the corpus example this is verified against.
/// - A text node renders as `"text"`; a comment as `<!-- text -->`. Neither escapes
///   characters in its content (including embedded `"`) — this matches the corpus, which
///   writes the raw source text back out unmodified.
/// - A doctype renders as `<!DOCTYPE name>` when both its public and system ids are empty,
///   otherwise `<!DOCTYPE name "public" "system">`.
/// - A `<template>` element's contents are not its own DOM children (see
///   [`crate::Element::template_contents`]); they render under a synthetic `content` line
///   one level deeper than the `<template>` element itself, with the actual contents one
///   level deeper again.
///
/// A [`NodeKind::ProcessingInstruction`] is not part of this contract: HTML5 tree
/// construction never produces one (a `<?...?>` in HTML source becomes a bogus comment —
/// verified against `comments01.dat`), so no corpus line exists to match against. It
/// renders as `<?target data>` on the same best-effort basis as any node kind that can only
/// arise from direct DOM construction rather than HTML parsing.
#[must_use]
pub fn html5lib_tree(doc: &Document) -> String {
    let mut lines = vec![String::from("#document")];
    let mut stack: Vec<(NodeId, usize)> = Vec::new();
    push_children(doc, doc.root(), 1, &mut stack);

    let mut remaining = doc.len();
    while let Some((id, depth)) = stack.pop() {
        if remaining == 0 {
            break;
        }
        remaining -= 1;
        let Some(node) = doc.get(id) else { continue };
        push_node_lines(node, depth, &mut lines);

        match &node.kind {
            NodeKind::Element(element) => {
                if let Some(contents) = element.template_contents {
                    lines.push(format!("| {}content", "  ".repeat(depth)));
                    push_children(doc, contents, depth + 2, &mut stack);
                } else {
                    push_children(doc, id, depth + 1, &mut stack);
                }
            }
            // A bare document fragment should never occur outside `template_contents` (see
            // above), which is handled without ever pushing the fragment node itself onto
            // the stack. Handled anyway, defensively, so a fragment reached some other way
            // still contributes its subtree instead of being silently dropped.
            NodeKind::DocumentFragment => push_children(doc, id, depth, &mut stack),
            NodeKind::Document
            | NodeKind::Doctype(_)
            | NodeKind::Text(_)
            | NodeKind::Comment(_)
            | NodeKind::ProcessingInstruction { .. } => {}
        }
    }

    lines.join("\n")
}

/// Renders `doc` for use as an `insta` snapshot golden.
///
/// Currently just [`html5lib_tree`]: the same exact-format dump is both what Task 9 diffs
/// against the html5lib corpus and what later tasks snapshot-test their own trees against,
/// so there is (so far) nothing to add. Kept as a separate name so a future divergence
/// between "the html5lib contract" and "what's convenient in a snapshot" doesn't require
/// renaming every call site that only wants the latter.
#[must_use]
pub fn dom_dump(doc: &Document) -> String {
    html5lib_tree(doc)
}

/// Appends `parent`'s children, in reverse document order, to `stack` at `depth` — so
/// popping `stack` yields them in forward document order (first child first), the standard
/// trick for turning a recursive pre-order walk into an iterative one with an explicit
/// stack. `Document::children` is itself bounded by `doc.len()` (see its docs), so this
/// call always terminates, even if `parent`'s sibling chain were somehow corrupted into a
/// cycle.
fn push_children(doc: &Document, parent: NodeId, depth: usize, stack: &mut Vec<(NodeId, usize)>) {
    let mut children: Vec<NodeId> = doc.children(parent).collect();
    children.reverse();
    stack.extend(children.into_iter().map(|id| (id, depth)));
}

/// Appends the line(s) for one node — its own line, plus one per attribute for an element —
/// to `lines`. `depth` follows the convention documented on [`html5lib_tree`] (a direct
/// child of the document root is depth 1), so the node's own line is indented `depth - 1`
/// two-space groups and its attribute lines (one level deeper) `depth` groups. Does not
/// touch `node`'s children; the caller pushes those onto the walk's stack separately (see
/// [`html5lib_tree`]).
fn push_node_lines(node: &Node, depth: usize, lines: &mut Vec<String>) {
    let indent = "  ".repeat(depth.saturating_sub(1));
    match &node.kind {
        NodeKind::Element(element) => {
            lines.push(format!(
                "| {indent}<{}{}>",
                tag_prefix(&element.name.ns),
                element.name.local
            ));
            let mut attrs: Vec<&Attr> = element.attrs.iter().collect();
            attrs.sort_by(|a, b| (&a.name.ns, &a.name.local).cmp(&(&b.name.ns, &b.name.local)));
            let attr_indent = "  ".repeat(depth);
            for attr in attrs {
                lines.push(format!("| {attr_indent}{}", format_attr(attr)));
            }
        }
        NodeKind::Text(text) => lines.push(format!("| {indent}\"{text}\"")),
        NodeKind::Comment(text) => lines.push(format!("| {indent}<!-- {text} -->")),
        NodeKind::Doctype(doctype) => lines.push(format!("| {indent}{}", format_doctype(doctype))),
        NodeKind::ProcessingInstruction { target, data } => {
            lines.push(format!("| {indent}<?{target} {data}>"));
        }
        // The document root is never pushed onto the walk (traversal starts from its
        // children), and a document fragment contributes only its own children's lines,
        // never a line of its own — see `html5lib_tree`.
        NodeKind::Document | NodeKind::DocumentFragment => {}
    }
}

/// The html5lib namespace prefix for an element's tag line: `svg `/`math ` for the SVG and
/// `MathML` namespaces, empty for HTML (and anything else — namespaced elements outside
/// SVG/`MathML` do not occur in HTML tree construction, so no corpus example covers them; an
/// empty prefix is the same "omit anything we can't verify" choice as
/// [`html5lib_tree`]'s handling of `ProcessingInstruction`).
fn tag_prefix(ns: &Namespace) -> &'static str {
    if *ns == ns!(svg) {
        "svg "
    } else if *ns == ns!(mathml) {
        "math "
    } else {
        ""
    }
}

/// Renders one attribute as `name="value"`, or `prefix local="value"` when the attribute
/// has a namespace prefix (only possible for an adjusted foreign attribute inside SVG/
/// `MathML` content, e.g. `xlink href="…"` — see the module docs).
fn format_attr(attr: &Attr) -> String {
    attr.name.prefix.as_ref().map_or_else(
        || format!("{}=\"{}\"", attr.name.local, attr.value),
        |prefix| format!("{prefix} {}=\"{}\"", attr.name.local, attr.value),
    )
}

/// Renders a doctype as `<!DOCTYPE name>` when both ids are empty, else
/// `<!DOCTYPE name "public" "system">`.
fn format_doctype(doctype: &Doctype) -> String {
    if doctype.public_id.is_empty() && doctype.system_id.is_empty() {
        format!("<!DOCTYPE {}>", doctype.name)
    } else {
        format!(
            "<!DOCTYPE {} \"{}\" \"{}\">",
            doctype.name, doctype.public_id, doctype.system_id
        )
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use markup5ever::Prefix;

    use super::*;
    use crate::element::Element;
    use crate::node::{Doctype, NodeKind};
    use crate::{LocalName, QualName, ns};

    fn el(doc: &mut Document, ns: Namespace, name: &str) -> NodeId {
        let qn = QualName::new(None, ns, LocalName::from(name));
        doc.create(NodeKind::Element(Element {
            name: qn,
            attrs: Vec::new(),
            template_contents: None,
        }))
    }

    fn html_el(doc: &mut Document, name: &str) -> NodeId {
        el(doc, ns!(html), name)
    }

    fn attr(name: &str, value: &str) -> Attr {
        Attr {
            name: QualName::new(None, ns!(), LocalName::from(name)),
            value: value.into(),
        }
    }

    /// `<!DOCTYPE html><html><head></head><body><p class="x" id="y">hi<!--c--></p></body></html>`,
    /// built by hand through the public `Document` API — the exact tree from the Task 4
    /// brief.
    fn hand_built_tree() -> Document {
        let mut doc = Document::new("about:blank");
        let root = doc.root();

        let doctype = doc.create(NodeKind::Doctype(Doctype {
            name: "html".into(),
            public_id: "".into(),
            system_id: "".into(),
        }));
        let html = html_el(&mut doc, "html");
        let head = html_el(&mut doc, "head");
        let body = html_el(&mut doc, "body");
        let p = html_el(&mut doc, "p");
        let text = doc.create(NodeKind::Text("hi".into()));
        let comment = doc.create(NodeKind::Comment("c".into()));

        // Attributes added out of (sorted) order, to exercise attribute sorting.
        if let Some(NodeKind::Element(element)) = doc.get_mut(p).map(|n| &mut n.kind) {
            element.attrs.push(attr("id", "y"));
            element.attrs.push(attr("class", "x"));
        }

        doc.append_child(root, doctype).expect("doctype");
        doc.append_child(root, html).expect("html");
        doc.append_child(html, head).expect("head");
        doc.append_child(html, body).expect("body");
        doc.append_child(body, p).expect("p");
        doc.append_child(p, text).expect("text");
        doc.append_child(p, comment).expect("comment");

        doc
    }

    #[test]
    fn html5lib_tree_should_match_brief_example_exactly() {
        let doc = hand_built_tree();
        let expected = "#document\n\
             | <!DOCTYPE html>\n\
             | <html>\n\
             |   <head>\n\
             |   <body>\n\
             |     <p>\n\
             |       class=\"x\"\n\
             |       id=\"y\"\n\
             |       \"hi\"\n\
             |       <!-- c -->";
        assert_eq!(html5lib_tree(&doc), expected);
    }

    #[test]
    fn dom_dump_should_delegate_to_html5lib_tree() {
        let doc = hand_built_tree();
        assert_eq!(dom_dump(&doc), html5lib_tree(&doc));
    }

    #[test]
    fn svg_and_mathml_elements_should_get_namespace_prefix() {
        let mut doc = Document::new("about:blank");
        let root = doc.root();
        let svg = el(&mut doc, ns!(svg), "svg");
        let math = el(&mut doc, ns!(mathml), "math");
        doc.append_child(root, svg).expect("svg");
        doc.append_child(root, math).expect("math");
        assert_eq!(html5lib_tree(&doc), "#document\n| <svg svg>\n| <math math>");
    }

    #[test]
    fn namespaced_attribute_should_render_as_prefix_local() {
        // Mirrors the html5lib corpus's `xlink:href` (plain HTML, literal colon in the
        // local name) vs. `xlink href` (inside SVG/`MathML`, an adjusted foreign attribute
        // with a real namespace prefix) contrast from `tree-construction/tests9.dat`.
        let mut doc = Document::new("about:blank");
        let root = doc.root();
        let math = el(&mut doc, ns!(mathml), "mi");
        if let Some(NodeKind::Element(element)) = doc.get_mut(math).map(|n| &mut n.kind) {
            element.attrs.push(Attr {
                name: QualName::new(
                    Some(Prefix::from("xlink")),
                    Namespace::from("http://www.w3.org/1999/xlink"),
                    LocalName::from("href"),
                ),
                value: "foo".into(),
            });
        }
        doc.append_child(root, math).expect("math");
        assert_eq!(
            html5lib_tree(&doc),
            "#document\n| <math mi>\n|   xlink href=\"foo\""
        );
    }

    #[test]
    fn doctype_with_public_and_system_ids_should_render_both() {
        let mut doc = Document::new("about:blank");
        let root = doc.root();
        let doctype = doc.create(NodeKind::Doctype(Doctype {
            name: "html".into(),
            public_id: "-//W3C//DTD HTML 4.01//EN".into(),
            system_id: "http://www.w3.org/TR/html4/strict.dtd".into(),
        }));
        doc.append_child(root, doctype).expect("doctype");
        assert_eq!(
            html5lib_tree(&doc),
            "#document\n| <!DOCTYPE html \"-//W3C//DTD HTML 4.01//EN\" \"http://www.w3.org/TR/html4/strict.dtd\">"
        );
    }

    #[test]
    fn template_contents_should_nest_under_content_line() {
        let mut doc = Document::new("about:blank");
        let root = doc.root();
        let template = html_el(&mut doc, "template");
        let contents = doc.create(NodeKind::DocumentFragment);
        let text = doc.create(NodeKind::Text("Hello".into()));
        doc.append_child(contents, text)
            .expect("text into contents");
        if let Some(NodeKind::Element(element)) = doc.get_mut(template).map(|n| &mut n.kind) {
            element.template_contents = Some(contents);
        }
        doc.append_child(root, template).expect("template");
        assert_eq!(
            html5lib_tree(&doc),
            "#document\n| <template>\n|   content\n|     \"Hello\""
        );
    }

    #[test]
    fn attributes_should_be_sorted_by_name_regardless_of_insertion_order() {
        let mut doc = Document::new("about:blank");
        let root = doc.root();
        let div = html_el(&mut doc, "div");
        if let Some(NodeKind::Element(element)) = doc.get_mut(div).map(|n| &mut n.kind) {
            element.attrs.push(attr("zebra", "1"));
            element.attrs.push(attr("apple", "2"));
            element.attrs.push(attr("mango", "3"));
        }
        doc.append_child(root, div).expect("div");
        assert_eq!(
            html5lib_tree(&doc),
            "#document\n| <div>\n|   apple=\"2\"\n|   mango=\"3\"\n|   zebra=\"1\""
        );
    }

    #[test]
    fn html5lib_tree_should_not_overflow_stack_on_deep_chain() {
        // Same hostile-input concern as the traversal iterators: a recursive serializer
        // would blow the call stack on a document nested thousands of elements deep.
        const DEPTH: usize = 5_000;
        let mut doc = Document::new("about:blank");
        let root = doc.root();
        let mut parent = root;
        for _ in 0..DEPTH {
            let child = html_el(&mut doc, "div");
            doc.append_child(parent, child).expect("append in chain");
            parent = child;
        }
        let out = html5lib_tree(&doc);
        assert_eq!(out.lines().count(), 1 + DEPTH);
        // The deepest `<div>` sits at depth `DEPTH` (a direct child of the document root is
        // depth 1), rendered with `DEPTH - 1` two-space indent groups.
        assert!(out.ends_with(&format!("{}<div>", "  ".repeat(DEPTH - 1))));
    }
}
