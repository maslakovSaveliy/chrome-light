//! Errors returned by [`crate::Document`]'s tree-mutation methods.

use crate::node::NodeId;

/// Failure modes for [`crate::Document`]'s tree-mutation methods.
///
/// Every variant carries the offending [`NodeId`](s) so callers can report a useful
/// diagnostic. None of these are ever produced by indexing or unwrapping untrusted
/// input — see `docs/CODING_STANDARDS.md` §2 ("untrusted input never panics"); malformed
/// requests come back as `Err`, not a panic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DomError {
    /// `NodeId` does not name a node in this document: it was never allocated by
    /// [`crate::Document::create`], or its arena index is out of range.
    #[error("no such node: {0:?}")]
    NoSuchNode(NodeId),
    /// Attaching `descendant`'s children (or `descendant` itself) under `ancestor` would
    /// make `ancestor` a descendant of itself, since `ancestor` already lies inside
    /// `descendant`'s current subtree.
    #[error("cycle: {ancestor:?} is already an ancestor of {descendant:?}")]
    Cycle {
        /// The node that is already an ancestor of `descendant`.
        ancestor: NodeId,
        /// The node the operation tried to attach `ancestor` under (directly or as the
        /// target of a `reparent_children` move).
        descendant: NodeId,
    },
    /// The node being attached already has a parent. Detach it first (or the caller
    /// picked the wrong node).
    #[error("node {0:?} already has a parent; detach it first")]
    NotDetached(NodeId),
    /// The operation targets or requires the document root, which is neither attachable
    /// to a parent nor a valid insertion anchor (it has no parent to insert relative to).
    #[error("operation not valid on the document root")]
    IsDocument,
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "a failed setup step in a test should abort that test, loudly"
)]
mod tests {
    use super::DomError;
    use crate::{Document, Element, LocalName, NodeKind, QualName, ns};

    /// A fresh, detached `<div>` in `doc`.
    fn el(doc: &mut Document) -> crate::NodeId {
        doc.create(NodeKind::Element(Element {
            name: QualName::new(None, ns!(html), LocalName::from("div")),
            attrs: Vec::new(),
            template_contents: None,
        }))
    }

    /// `reparent_children` validates both of its arguments before touching anything: a
    /// `NodeId` from another document (or a stale one) is [`DomError::NoSuchNode`], naming the
    /// argument that was wrong — not a silent no-op and not a panic.
    #[test]
    fn reparent_children_should_reject_an_unknown_node() {
        let mut doc = Document::new("about:blank");
        let real = el(&mut doc);
        // A `NodeId` from another, larger document: its arena index is past the end of
        // `doc`'s own arena, so it names no node here. (Taking the *first* id of another
        // document would not do — arena indices start over per document, so it would collide
        // with a real node of `doc`.)
        let foreign = {
            let mut other = Document::new("about:blank");
            let mut last = other.root();
            for _ in 0..8 {
                last = el(&mut other);
            }
            last
        };
        assert!(
            doc.get(foreign).is_none(),
            "the premise: this id names no node in `doc`"
        );

        assert!(matches!(
            doc.reparent_children(foreign, real),
            Err(DomError::NoSuchNode(id)) if id == foreign
        ));
        assert!(matches!(
            doc.reparent_children(real, foreign),
            Err(DomError::NoSuchNode(id)) if id == foreign
        ));
    }

    /// Moving a node's children *into* its own subtree would make one of them its own
    /// ancestor, so it is [`DomError::Cycle`] — the one rejection `reparent_children` still
    /// pays the full ancestor walk for (`check_attach`'s childless short-circuit does not
    /// apply: these children are populated subtrees being moved, not fresh empty nodes).
    #[test]
    fn reparent_children_should_reject_a_target_inside_its_own_subtree() {
        let mut doc = Document::new("about:blank");
        let outer = el(&mut doc);
        let inner = el(&mut doc);
        doc.append_child(doc.root(), outer).expect("attach outer");
        doc.append_child(outer, inner).expect("attach inner");

        assert!(matches!(
            doc.reparent_children(outer, inner),
            Err(DomError::Cycle { ancestor, descendant })
                if ancestor == outer && descendant == inner
        ));
    }

    /// `insert_before` needs a parent to insert *under*, which it takes from the anchor
    /// sibling. A detached anchor — and the document root, which never has a parent — gives it
    /// none, so both are [`DomError::IsDocument`] rather than a panic or a stray append.
    #[test]
    fn insert_before_should_reject_an_anchor_with_no_parent() {
        let mut doc = Document::new("about:blank");
        let detached_anchor = el(&mut doc);
        let new_a = el(&mut doc);
        let new_b = el(&mut doc);

        assert!(matches!(
            doc.insert_before(detached_anchor, new_a),
            Err(DomError::IsDocument)
        ));
        assert!(matches!(
            doc.insert_before(doc.root(), new_b),
            Err(DomError::IsDocument)
        ));
    }
}
