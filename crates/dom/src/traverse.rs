//! Iterative DOM traversal: direct children, pre-order descendants, and ancestor chains.
//!
//! Every iterator here is a small state machine driven entirely by [`Node::first_child`],
//! [`Node::next_sibling`] and [`Node::parent`] lookups through [`Document::get`] — no
//! recursion (see the crate-level docs), so none of them can stack-overflow no matter how
//! deep a hostile document nests. Each iterator also caps its own total yield count at
//! [`Document::len`]: a well-formed tree never comes close to that bound (every node is
//! visited at most once), but it guarantees every iterator here *terminates* even if the
//! arena's sibling/child links were somehow corrupted into a cycle, rather than looping
//! forever.

use crate::document::Document;
use crate::node::{Node, NodeId};

/// Iterator over a node's direct children, in document order.
///
/// Returned by [`Document::children`].
#[derive(Debug)]
pub struct Children<'a> {
    doc: &'a Document,
    next: Option<NodeId>,
    remaining: usize,
}

impl Iterator for Children<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        if self.remaining == 0 {
            return None;
        }
        let current = self.next?;
        self.remaining -= 1;
        self.next = self.doc.get(current).and_then(Node::next_sibling);
        Some(current)
    }
}

/// Iterator over a node's descendants in pre-order (a node before its children, children
/// visited left-to-right), excluding the node itself.
///
/// Returned by [`Document::descendants`]. Walks with an explicit stack rather than
/// recursion — see the module docs.
#[derive(Debug)]
pub struct Descendants<'a> {
    doc: &'a Document,
    stack: Vec<NodeId>,
    remaining: usize,
}

impl Iterator for Descendants<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        if self.remaining == 0 {
            return None;
        }
        let current = self.stack.pop()?;
        self.remaining -= 1;
        // Push in reverse so the first child is popped (and thus visited) next, giving
        // pre-order / document-order traversal. `Document::children` is itself bounded by
        // `doc.len()`, so this single call always terminates even on a corrupted sibling
        // chain; the outer `remaining` counter bounds the *whole* walk, including the case
        // where corrupted parent/child links form a cycle across multiple nodes.
        let mut children: Vec<NodeId> = self.doc.children(current).collect();
        children.reverse();
        self.stack.extend(children);
        Some(current)
    }
}

/// Iterator over a node's ancestor chain — parent, grandparent, and so on up to and
/// including the document root — excluding the node itself.
///
/// Returned by [`Document::ancestors`].
#[derive(Debug)]
pub struct Ancestors<'a> {
    doc: &'a Document,
    next: Option<NodeId>,
    remaining: usize,
}

impl Iterator for Ancestors<'_> {
    type Item = NodeId;

    fn next(&mut self) -> Option<NodeId> {
        if self.remaining == 0 {
            return None;
        }
        let current = self.next?;
        self.remaining -= 1;
        self.next = self.doc.get(current).and_then(Node::parent);
        Some(current)
    }
}

impl Document {
    /// Iterates `id`'s direct children, in document order.
    ///
    /// Empty if `id` is unknown or has no children. Never panics or loops forever, even on
    /// a corrupted arena — see the module docs.
    #[must_use]
    pub fn children(&self, id: NodeId) -> Children<'_> {
        let next = self.get(id).and_then(Node::first_child);
        Children {
            doc: self,
            next,
            remaining: self.len(),
        }
    }

    /// Iterates `id`'s descendants in pre-order, excluding `id` itself.
    ///
    /// Empty if `id` is unknown or has no children. Iterative (explicit stack), so this
    /// never overflows the call stack no matter how deep the subtree nests. Never panics or
    /// loops forever, even on a corrupted arena — see the module docs.
    #[must_use]
    pub fn descendants(&self, id: NodeId) -> Descendants<'_> {
        let mut stack: Vec<NodeId> = self.children(id).collect();
        stack.reverse();
        Descendants {
            doc: self,
            stack,
            remaining: self.len(),
        }
    }

    /// Iterates `id`'s ancestor chain — parent, grandparent, and so on up to and including
    /// the document root — excluding `id` itself.
    ///
    /// Empty if `id` is unknown or is itself the document root (which has no parent).
    /// Never panics or loops forever, even on a corrupted arena — see the module docs.
    #[must_use]
    pub fn ancestors(&self, id: NodeId) -> Ancestors<'_> {
        let next = self.get(id).and_then(Node::parent);
        Ancestors {
            doc: self,
            next,
            remaining: self.len(),
        }
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use crate::element::Element;
    use crate::node::NodeKind;
    use crate::{QualName, ns};

    fn el(doc: &mut Document, name: &str) -> NodeId {
        let qn = QualName::new(None, ns!(html), name.into());
        doc.create(NodeKind::Element(Element {
            name: qn,
            attrs: Vec::new(),
            template_contents: None,
        }))
    }

    /// Builds `<html><head></head><body><p>hi</p></body></html>` under `doc`'s root and
    /// returns `(html, head, body, p, text)`.
    fn build_tree(doc: &mut Document) -> (NodeId, NodeId, NodeId, NodeId, NodeId) {
        let root = doc.root();
        let html = el(doc, "html");
        let head = el(doc, "head");
        let body = el(doc, "body");
        let p = el(doc, "p");
        let text = doc.create(NodeKind::Text("hi".into()));
        doc.append_child(root, html).expect("html");
        doc.append_child(html, head).expect("head");
        doc.append_child(html, body).expect("body");
        doc.append_child(body, p).expect("p");
        doc.append_child(p, text).expect("text");
        (html, head, body, p, text)
    }

    #[test]
    fn children_should_yield_direct_children_in_order() {
        let mut doc = Document::new("about:blank");
        let (html, head, body, ..) = build_tree(&mut doc);
        assert_eq!(doc.children(html).collect::<Vec<_>>(), vec![head, body]);
    }

    #[test]
    fn children_should_be_empty_for_leaf_and_unknown_id() {
        let mut doc = Document::new("about:blank");
        let (_, head, ..) = build_tree(&mut doc);
        assert_eq!(doc.children(head).collect::<Vec<_>>(), Vec::<NodeId>::new());
        assert_eq!(
            doc.children(NodeId::from_index(9_999)).collect::<Vec<_>>(),
            Vec::<NodeId>::new()
        );
    }

    #[test]
    fn descendants_should_be_preorder_excluding_self() {
        let mut doc = Document::new("about:blank");
        let root = doc.root();
        let (html, head, body, p, text) = build_tree(&mut doc);
        assert_eq!(
            doc.descendants(root).collect::<Vec<_>>(),
            vec![html, head, body, p, text]
        );
        assert_eq!(doc.descendants(p).collect::<Vec<_>>(), vec![text]);
        assert_eq!(
            doc.descendants(text).collect::<Vec<_>>(),
            Vec::<NodeId>::new()
        );
    }

    #[test]
    fn ancestors_should_walk_parent_chain_excluding_self() {
        let mut doc = Document::new("about:blank");
        let root = doc.root();
        let (html, _head, body, p, _text) = build_tree(&mut doc);
        assert_eq!(doc.ancestors(p).collect::<Vec<_>>(), vec![body, html, root]);
        assert_eq!(
            doc.ancestors(root).collect::<Vec<_>>(),
            Vec::<NodeId>::new()
        );
    }

    #[test]
    fn descendants_should_not_overflow_stack_on_deep_chain() {
        // A hostile document can nest thousands of elements deep; a recursive walk would
        // blow the call stack on input like this. Build a chain of 5000 elements (well past
        // any plausible native stack depth at a few dozen bytes per would-be frame) and
        // confirm the iterative walk still runs to completion in the right order.
        const DEPTH: usize = 5_000;
        let mut doc = Document::new("about:blank");
        let root = doc.root();
        let mut ids = Vec::with_capacity(DEPTH);
        let mut parent = root;
        for _ in 0..DEPTH {
            let child = el(&mut doc, "div");
            doc.append_child(parent, child).expect("append in chain");
            ids.push(child);
            parent = child;
        }
        let visited: Vec<NodeId> = doc.descendants(root).collect();
        assert_eq!(visited, ids);

        // Same guarantee for the ancestor walk from the bottom of the chain back up to the
        // root.
        let deepest = *ids.last().expect("chain is non-empty");
        // `ids` in reverse, minus its own last entry (`deepest`, whose ancestors these
        // are), then the root — i.e. `deepest`'s parent, grandparent, ..., then root.
        let mut expected: Vec<NodeId> = ids.iter().rev().skip(1).copied().collect();
        expected.push(root);
        assert_eq!(doc.ancestors(deepest).collect::<Vec<_>>(), expected);
    }
}
