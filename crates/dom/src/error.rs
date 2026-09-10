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
