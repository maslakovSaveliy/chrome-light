//! The arena-backed DOM tree: a flat `Vec<Node>` addressed by [`NodeId`], plus the
//! tree-mutation operations every later crate (`cl-html`'s tree builder, `cl-style`'s
//! cascade, `cl-layout`'s box generation) builds on.
//!
//! No `Rc`/`RefCell` anywhere (`docs/CODING_STANDARDS.md` §1): tree edges are `NodeId`
//! links stored inline on each [`Node`]. No recursion in any algorithm here: a hostile or
//! merely very deep document can nest arbitrarily far, and a recursive walk would be a
//! stack-overflow `DoS` on that input, so every traversal below (ancestor walks, child-list
//! walks) is an explicit iterative loop. No `[]` indexing into the arena: all lookups go
//! through [`Document::get`]/[`Document::get_mut`], which return `Option` instead of
//! panicking on an out-of-range or foreign [`NodeId`].

use markup5ever::LocalName;

use crate::element::Element;
use crate::error::DomError;
use crate::node::{Node, NodeId, NodeKind};

/// Which of the three HTML "quirks" rendering modes a document was parsed in.
///
/// Set by the HTML tree-construction algorithm (`cl-html`) once it has seen the
/// `DOCTYPE` (or the lack of one); read by `cl-style`'s cascade, which changes several
/// matching rules in quirks mode (e.g. case-insensitive `class`/`id` matching).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuirksMode {
    /// Standards mode.
    NoQuirks,
    /// "Almost standards" mode.
    LimitedQuirks,
    /// Full quirks mode.
    Quirks,
}

/// An arena-backed DOM tree.
///
/// Nodes live in one flat `Vec<Node>` and are addressed by [`NodeId`] (their index).
/// Arena index 0 always exists and is the [`NodeKind::Document`] root ([`Document::root`]).
/// Nodes are never freed once created: [`Document::detach`] unlinks a node from the tree
/// but keeps its slot (and its own subtree) alive and reachable by id, so a detached
/// subtree can be reattached elsewhere (e.g. `<template>` contents, or a node moved by
/// the HTML "adoption agency" algorithm) without losing identity.
#[derive(Debug, Clone)]
pub struct Document {
    nodes: Vec<Node>,
    quirks: QuirksMode,
    base_url: String,
}

impl Document {
    /// Creates a new document containing only its root node (arena index 0,
    /// [`NodeKind::Document`]), with quirks mode [`QuirksMode::NoQuirks`].
    #[must_use]
    pub fn new(base_url: &str) -> Self {
        Self {
            nodes: vec![Node::new(NodeKind::Document)],
            quirks: QuirksMode::NoQuirks,
            base_url: base_url.to_owned(),
        }
    }

    /// The id of the document root. Always `NodeId::from_index(0)`.
    ///
    /// Takes `&self` (rather than being a bare associated function) to match the rest of
    /// this API and stay future-proof if the root's index ever stops being a fixed `0`;
    /// today's implementation happens not to need any instance data for it.
    #[must_use]
    #[allow(clippy::unused_self)]
    pub fn root(&self) -> NodeId {
        NodeId::from_index(0)
    }

    /// Looks up a node by id. `None` if `id` is out of range or from another document.
    #[must_use]
    pub fn get(&self, id: NodeId) -> Option<&Node> {
        self.nodes.get(id.index())
    }

    /// Looks up a node by id, mutably. `None` if `id` is out of range or from another
    /// document.
    #[must_use]
    pub fn get_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.get_mut(id.index())
    }

    /// The number of nodes in the arena, including detached ones (nodes are never freed;
    /// see the [`Document`] docs).
    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Whether the arena has no nodes at all. Always `false` in practice — [`Document::new`]
    /// seeds the root node and nothing ever removes it — but required for API symmetry
    /// with [`Document::len`] (clippy's `len_without_is_empty`).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Allocates a new, detached node (no parent, no children, no siblings) holding
    /// `kind`, and returns its id.
    ///
    /// The arena is capped at `u32::MAX` (2^32 − 1) real nodes (see the [`NodeId`] docs).
    /// In the practically unreachable case where it is already at that cap — which would
    /// already require hundreds of gigabytes just for the `Node` slots — `create` does
    /// not panic or grow the arena further: it returns the reserved sentinel id
    /// (`NodeId::from_index(usize::MAX)`, which saturates to `NodeId(u32::MAX)`), and
    /// every `Document` lookup on that id returns `None`, exactly as for any other
    /// unknown id.
    #[must_use]
    pub fn create(&mut self, kind: NodeKind) -> NodeId {
        if self.nodes.len() >= u32::MAX as usize {
            return NodeId::from_index(usize::MAX);
        }
        let id = NodeId::from_index(self.nodes.len());
        self.nodes.push(Node::new(kind));
        id
    }

    /// Appends `child` as `parent`'s new last child.
    ///
    /// Rejects, without mutating anything: an unknown `parent` or `child`
    /// ([`DomError::NoSuchNode`]); `child` being the document root, which can never be
    /// attached anywhere ([`DomError::IsDocument`]); attaching `child` under one of its
    /// own descendants, which would create a cycle ([`DomError::Cycle`]); and `child`
    /// already having a parent ([`DomError::NotDetached`] — detach it first).
    pub fn append_child(&mut self, parent: NodeId, child: NodeId) -> Result<(), DomError> {
        self.check_attach(parent, child)?;
        self.link_last(parent, child);
        Ok(())
    }

    /// Inserts `new` immediately before `sibling`, under `sibling`'s current parent.
    ///
    /// Rejects the same cases as [`Document::append_child`] (with `new` playing the role
    /// of `child`), plus [`DomError::IsDocument`] if `sibling` has no parent — either
    /// because it is the document root, or because it is itself currently detached —
    /// since there is then no parent to insert `new` under.
    pub fn insert_before(&mut self, sibling: NodeId, new: NodeId) -> Result<(), DomError> {
        if self.get(new).is_none() {
            return Err(DomError::NoSuchNode(new));
        }
        let parent = self
            .get(sibling)
            .ok_or(DomError::NoSuchNode(sibling))?
            .parent()
            .ok_or(DomError::IsDocument)?;
        self.check_attach(parent, new)?;
        self.link_before(parent, sibling, new);
        Ok(())
    }

    /// Detaches `id` from its current parent and siblings, if any. `id`'s own children
    /// move with it (it becomes the root of its own, now-independent subtree again).
    ///
    /// Detaching a node that already has no parent — because it was already detached, or
    /// because it is the document root, which never has one — is a no-op: the
    /// postcondition ("`id` has no parent") already holds, so `detach` is idempotent
    /// rather than an error. This lets callers detach speculatively (e.g. "make sure this
    /// node isn't attached anywhere before reusing it") without a separate attachment
    /// check first.
    ///
    /// Only an unknown `id` is rejected, with [`DomError::NoSuchNode`].
    pub fn detach(&mut self, id: NodeId) -> Result<(), DomError> {
        if self.get(id).is_none() {
            return Err(DomError::NoSuchNode(id));
        }
        self.unlink(id);
        Ok(())
    }

    /// Moves all of `from`'s children, in order, to become `to`'s last children
    /// (appended after `to`'s existing children). `from` ends up with no children.
    ///
    /// A no-op if `from == to`. Rejects an unknown `from` or `to`
    /// ([`DomError::NoSuchNode`]), and rejects `to` being `from` itself or lying inside
    /// `from`'s own subtree ([`DomError::Cycle`]) — moving `from`'s children there would
    /// make one of them an ancestor of itself.
    pub fn reparent_children(&mut self, from: NodeId, to: NodeId) -> Result<(), DomError> {
        if self.get(from).is_none() {
            return Err(DomError::NoSuchNode(from));
        }
        if self.get(to).is_none() {
            return Err(DomError::NoSuchNode(to));
        }
        if from == to {
            return Ok(());
        }
        if self.is_ancestor_or_self(from, to) {
            return Err(DomError::Cycle {
                ancestor: from,
                descendant: to,
            });
        }
        while let Some(child) = self.get(from).and_then(Node::first_child) {
            self.unlink(child);
            self.link_last(to, child);
        }
        Ok(())
    }

    /// The document's current quirks mode. Defaults to [`QuirksMode::NoQuirks`].
    #[must_use]
    pub fn quirks_mode(&self) -> QuirksMode {
        self.quirks
    }

    /// Sets the document's quirks mode.
    pub fn set_quirks_mode(&mut self, q: QuirksMode) {
        self.quirks = q;
    }

    /// The document's base URL, used to resolve relative URLs found in attributes and
    /// stylesheets.
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Convenience: `id`'s [`Element`] data, or `None` if `id` is unknown or is not an
    /// element node.
    #[must_use]
    pub fn element(&self, id: NodeId) -> Option<&Element> {
        match &self.get(id)?.kind {
            NodeKind::Element(element) => Some(element),
            _ => None,
        }
    }

    /// Convenience: the value of `id`'s attribute named `local` (in any namespace), or
    /// `None` if `id` is unknown, is not an element, or has no such attribute.
    #[must_use]
    pub fn attr(&self, id: NodeId, local: &LocalName) -> Option<&str> {
        self.element(id)?
            .attrs
            .iter()
            .find(|attr| &attr.name.local == local)
            .map(|attr| &*attr.value)
    }

    /// Validates that `child` may be freshly attached under `parent` (used by both
    /// `append_child` and `insert_before`, which differ only in *where* under `parent`
    /// the child ends up). Checked in this order: both ids exist; `child` is not the
    /// document root; attaching would not create a cycle; `child` is not already
    /// attached somewhere. Cycle is checked before "already attached" so that
    /// re-attaching an existing ancestor lower in its own subtree is reported as the
    /// more specific `Cycle`, not `NotDetached`.
    fn check_attach(&self, parent: NodeId, child: NodeId) -> Result<(), DomError> {
        if self.get(parent).is_none() {
            return Err(DomError::NoSuchNode(parent));
        }
        let child_node = self.get(child).ok_or(DomError::NoSuchNode(child))?;
        if child == self.root() {
            return Err(DomError::IsDocument);
        }
        if self.is_ancestor_or_self(child, parent) {
            return Err(DomError::Cycle {
                ancestor: child,
                descendant: parent,
            });
        }
        if child_node.parent().is_some() {
            return Err(DomError::NotDetached(child));
        }
        Ok(())
    }

    /// Iteratively walks `node`'s ancestor chain, including `node` itself, looking for
    /// `candidate`. No recursion (see module docs): worst case this is one step per node
    /// currently in the document, still `O(depth)` and never re-visits a node because the
    /// tree the walk runs over has no cycles by construction (every mutation that could
    /// introduce one is rejected by this same check first).
    fn is_ancestor_or_self(&self, candidate: NodeId, mut node: NodeId) -> bool {
        loop {
            if node == candidate {
                return true;
            }
            match self.get(node).and_then(Node::parent) {
                Some(p) => node = p,
                None => return false,
            }
        }
    }

    /// Removes `id` from wherever it currently sits (its parent's child list), fixing up
    /// to four neighbouring links (parent's first/last child, prev's next, next's prev),
    /// and clears `id`'s own parent/prev/next links. Does not touch `id`'s children. A
    /// no-op if `id` has no parent. Caller must ensure `id` names a real node.
    fn unlink(&mut self, id: NodeId) {
        let Some(node) = self.get(id) else { return };
        let (parent, prev, next) = (node.parent(), node.prev_sibling(), node.next_sibling());
        let Some(parent) = parent else { return };

        match prev {
            Some(prev_id) => {
                if let Some(prev_node) = self.get_mut(prev_id) {
                    prev_node.next_sibling = next;
                }
            }
            None => {
                if let Some(parent_node) = self.get_mut(parent) {
                    parent_node.first_child = next;
                }
            }
        }
        match next {
            Some(next_id) => {
                if let Some(next_node) = self.get_mut(next_id) {
                    next_node.prev_sibling = prev;
                }
            }
            None => {
                if let Some(parent_node) = self.get_mut(parent) {
                    parent_node.last_child = prev;
                }
            }
        }
        if let Some(node) = self.get_mut(id) {
            node.parent = None;
            node.prev_sibling = None;
            node.next_sibling = None;
        }
    }

    /// Links `child` — assumed currently fully unlinked (no parent/prev/next) — as
    /// `parent`'s new last child. Caller must ensure both ids name real nodes.
    fn link_last(&mut self, parent: NodeId, child: NodeId) {
        let old_last = self.get(parent).and_then(Node::last_child);
        if let Some(child_node) = self.get_mut(child) {
            child_node.parent = Some(parent);
            child_node.prev_sibling = old_last;
            child_node.next_sibling = None;
        }
        match old_last {
            Some(last_id) => {
                if let Some(last_node) = self.get_mut(last_id) {
                    last_node.next_sibling = Some(child);
                }
            }
            None => {
                if let Some(parent_node) = self.get_mut(parent) {
                    parent_node.first_child = Some(child);
                }
            }
        }
        if let Some(parent_node) = self.get_mut(parent) {
            parent_node.last_child = Some(child);
        }
    }

    /// Links `new` — assumed currently fully unlinked (no parent/prev/next) —
    /// immediately before `sibling`, under `parent`. Caller must ensure `sibling` is
    /// currently one of `parent`'s children.
    fn link_before(&mut self, parent: NodeId, sibling: NodeId, new: NodeId) {
        let old_prev = self.get(sibling).and_then(Node::prev_sibling);
        if let Some(new_node) = self.get_mut(new) {
            new_node.parent = Some(parent);
            new_node.prev_sibling = old_prev;
            new_node.next_sibling = Some(sibling);
        }
        match old_prev {
            Some(prev_id) => {
                if let Some(prev_node) = self.get_mut(prev_id) {
                    prev_node.next_sibling = Some(new);
                }
            }
            None => {
                if let Some(parent_node) = self.get_mut(parent) {
                    parent_node.first_child = Some(new);
                }
            }
        }
        if let Some(sibling_node) = self.get_mut(sibling) {
            sibling_node.prev_sibling = Some(new);
        }
    }
}

/// Checks that `doc`'s sibling/child/parent links are mutually consistent.
///
/// For every node: walking `first_child -> next_sibling` reaches exactly the set of
/// nodes that report this node as their `parent()`, each exactly once; `last_child` is
/// the last node in that walk; and walking backwards via `prev_sibling` from `last_child`
/// reproduces the exact reverse of the forward walk. No recursion — see the module docs.
///
/// Test-only: used by the unit tests below and by the `append_detach_preserves_invariants`
/// property test to assert the invariant holds after every mutation in a random sequence.
#[cfg(test)]
pub(crate) fn check_invariants(doc: &Document) -> Result<(), String> {
    use std::collections::HashMap;

    // How many times each node was found while walking some *other* node's children.
    // Should end up at exactly 1 for every node that reports a parent, 0 otherwise.
    let mut child_visits: HashMap<NodeId, u32> = HashMap::new();

    for i in 0..doc.len() {
        let id = NodeId::from_index(i);
        let Some(node) = doc.get(id) else {
            continue;
        };

        let mut forward = Vec::new();
        let mut cursor = node.first_child();
        let mut prev = None;
        while let Some(child_id) = cursor {
            if forward.len() > doc.len() {
                return Err(format!(
                    "{id:?}'s first_child/next_sibling chain does not terminate"
                ));
            }
            let Some(child) = doc.get(child_id) else {
                return Err(format!("{id:?} links to nonexistent child {child_id:?}"));
            };
            if child.parent() != Some(id) {
                return Err(format!(
                    "{child_id:?} is in {id:?}'s child chain but child.parent() = {:?}",
                    child.parent()
                ));
            }
            if child.prev_sibling() != prev {
                return Err(format!(
                    "{child_id:?}.prev_sibling() = {:?}, expected {prev:?}",
                    child.prev_sibling()
                ));
            }
            *child_visits.entry(child_id).or_insert(0) += 1;
            forward.push(child_id);
            prev = Some(child_id);
            cursor = child.next_sibling();
        }
        if node.last_child() != forward.last().copied() {
            return Err(format!(
                "{id:?}.last_child() = {:?}, expected {:?}",
                node.last_child(),
                forward.last()
            ));
        }

        let mut backward = Vec::new();
        let mut cursor = node.last_child();
        while let Some(child_id) = cursor {
            if backward.len() > doc.len() {
                return Err(format!(
                    "{id:?}'s last_child/prev_sibling chain does not terminate"
                ));
            }
            let Some(child) = doc.get(child_id) else {
                return Err(format!(
                    "{id:?} links to nonexistent child {child_id:?} via last_child"
                ));
            };
            backward.push(child_id);
            cursor = child.prev_sibling();
        }
        backward.reverse();
        if backward != forward {
            return Err(format!(
                "{id:?}'s forward child chain {forward:?} disagrees with backward chain {backward:?}"
            ));
        }
    }

    for i in 0..doc.len() {
        let id = NodeId::from_index(i);
        let Some(node) = doc.get(id) else {
            continue;
        };
        let visits = child_visits.get(&id).copied().unwrap_or(0);
        match node.parent() {
            Some(p) if visits != 1 => {
                return Err(format!(
                    "{id:?} claims parent {p:?} but was visited {visits} time(s) while walking children (expected 1)"
                ));
            }
            None if visits != 0 => {
                return Err(format!(
                    "{id:?} has no parent but was visited {visits} time(s) while walking some parent's children"
                ));
            }
            _ => {}
        }
    }

    Ok(())
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::{NodeKind, QualName, ns};

    fn el(doc: &mut Document, name: &str) -> NodeId {
        let qn = QualName::new(None, ns!(html), LocalName::from(name));
        doc.create(NodeKind::Element(Element {
            name: qn,
            attrs: Vec::new(),
            template_contents: None,
        }))
    }

    #[test]
    fn new_document_should_have_only_root() {
        let doc = Document::new("about:blank");
        assert_eq!(doc.len(), 1);
        assert!(matches!(
            doc.get(doc.root()).map(|n| &n.kind),
            Some(NodeKind::Document)
        ));
    }

    #[test]
    fn append_child_should_link_parent_and_siblings() {
        let mut doc = Document::new("about:blank");
        let root = doc.root();
        let a = el(&mut doc, "a");
        let b = el(&mut doc, "b");
        doc.append_child(root, a).expect("append a");
        doc.append_child(root, b).expect("append b");
        let r = doc.get(root).expect("root");
        assert_eq!((r.first_child(), r.last_child()), (Some(a), Some(b)));
        assert_eq!(doc.get(a).expect("a").next_sibling(), Some(b));
        assert_eq!(doc.get(b).expect("b").prev_sibling(), Some(a));
        assert_eq!(doc.get(b).expect("b").parent(), Some(root));
        assert_eq!(check_invariants(&doc), Ok(()));
    }

    #[test]
    fn insert_before_should_place_node_between_siblings() {
        let mut doc = Document::new("about:blank");
        let root = doc.root();
        let a = el(&mut doc, "a");
        let c = el(&mut doc, "c");
        let b = el(&mut doc, "b");
        doc.append_child(root, a).expect("a");
        doc.append_child(root, c).expect("c");
        doc.insert_before(c, b).expect("insert");
        let order: Vec<NodeId> =
            std::iter::successors(doc.get(root).and_then(Node::first_child), |&id| {
                doc.get(id).and_then(Node::next_sibling)
            })
            .collect();
        assert_eq!(order, vec![a, b, c]);
        assert_eq!(check_invariants(&doc), Ok(()));
    }

    #[test]
    fn detach_should_unlink_and_keep_node_alive() {
        let mut doc = Document::new("about:blank");
        let root = doc.root();
        let a = el(&mut doc, "a");
        let b = el(&mut doc, "b");
        doc.append_child(root, a).expect("a");
        doc.append_child(root, b).expect("b");
        doc.detach(a).expect("detach");
        assert_eq!(doc.get(root).expect("root").first_child(), Some(b));
        assert_eq!(doc.get(b).expect("b").prev_sibling(), None);
        assert_eq!(doc.get(a).expect("a").parent(), None);
        assert_eq!(doc.len(), 3);
        assert_eq!(check_invariants(&doc), Ok(()));
    }

    #[test]
    fn append_child_should_reject_cycle() {
        let mut doc = Document::new("about:blank");
        let root = doc.root();
        let a = el(&mut doc, "a");
        let b = el(&mut doc, "b");
        doc.append_child(root, a).expect("a");
        doc.append_child(a, b).expect("b");
        assert!(matches!(
            doc.append_child(b, a),
            Err(DomError::Cycle { .. })
        ));
        assert_eq!(check_invariants(&doc), Ok(()));
    }

    #[test]
    fn append_child_should_reject_attached_node() {
        let mut doc = Document::new("about:blank");
        let root = doc.root();
        let a = el(&mut doc, "a");
        let b = el(&mut doc, "b");
        doc.append_child(root, a).expect("a");
        doc.append_child(root, b).expect("b");
        assert!(matches!(
            doc.append_child(a, b),
            Err(DomError::NotDetached(_))
        ));
        assert_eq!(check_invariants(&doc), Ok(()));
    }

    #[test]
    fn reparent_children_should_move_all_children_in_order() {
        let mut doc = Document::new("about:blank");
        let root = doc.root();
        let from = el(&mut doc, "from");
        let to = el(&mut doc, "to");
        let x = el(&mut doc, "x");
        let y = el(&mut doc, "y");
        doc.append_child(root, from).expect("from");
        doc.append_child(root, to).expect("to");
        doc.append_child(from, x).expect("x");
        doc.append_child(from, y).expect("y");
        doc.reparent_children(from, to).expect("reparent");
        assert_eq!(doc.get(from).expect("from").first_child(), None);
        assert_eq!(doc.get(to).expect("to").first_child(), Some(x));
        assert_eq!(doc.get(y).expect("y").parent(), Some(to));
        assert_eq!(check_invariants(&doc), Ok(()));
    }

    #[test]
    fn get_should_return_none_for_unknown_id() {
        let doc = Document::new("about:blank");
        assert!(doc.get(NodeId::from_index(99)).is_none());
    }

    proptest! {
        #[test]
        fn append_and_detach_sequences_preserve_invariants(
            ops in proptest::collection::vec((any::<bool>(), any::<usize>(), any::<usize>()), 0..=50)
        ) {
            let mut doc = Document::new("about:blank");
            let mut ids = vec![doc.root()];

            for (is_append, a, b) in ops {
                if is_append {
                    let parent_idx = a % ids.len();
                    let parent = *ids.get(parent_idx).expect("invariant: parent_idx < ids.len()");
                    let child = el(&mut doc, "x");
                    doc.append_child(parent, child)
                        .expect("invariant: a freshly created node is always detached and never the root, so append_child cannot fail");
                    ids.push(child);
                } else {
                    let target_idx = b % ids.len();
                    let target = *ids.get(target_idx).expect("invariant: target_idx < ids.len()");
                    doc.detach(target)
                        .expect("invariant: every id in `ids` was created in this document, so detach cannot fail");
                }
                let invariants = check_invariants(&doc);
                prop_assert!(invariants.is_ok(), "invariants violated: {:?}", invariants);
            }
        }
    }
}
