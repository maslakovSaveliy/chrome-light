//! A single arena slot ([`Node`]) and its id ([`NodeId`]).
//!
//! Tree edges are `NodeId` links stored inline on each `Node`, not `Rc`/`RefCell`
//! pointers (`docs/CODING_STANDARDS.md` §1): the tree lives in one flat `Vec<Node>`
//! owned by [`crate::Document`], and nodes refer to each other by index.

use crate::StrTendril;
use crate::element::Element;

/// An index into a [`crate::Document`]'s node arena.
///
/// `NodeId`s are only meaningful relative to the `Document` that produced them via
/// [`crate::Document::create`] — mixing ids from two different documents is a logic bug
/// (it will not panic; it will just look up the wrong node, or no node, in the other
/// document).
///
/// The arena is capped at `u32::MAX` (2^32 − 1) real nodes: `u32::MAX` itself is reserved
/// as a saturation marker (see [`NodeId::from_index`]) and is never assigned to a real
/// node by [`crate::Document::create`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId(u32);

impl NodeId {
    /// Builds a `NodeId` from a raw arena index.
    ///
    /// If `index` does not fit in a `u32` (i.e. the arena has somehow grown past its
    /// `u32::MAX`-node cap), this saturates to `NodeId(u32::MAX)` rather than panicking.
    /// `crate::Document::create` never assigns that value to a real node, so a saturated
    /// id behaves exactly like any other unknown id: every `Document` lookup on it
    /// returns `None` instead of aliasing some other node.
    #[must_use]
    pub fn from_index(index: usize) -> Self {
        Self(u32::try_from(index).unwrap_or(u32::MAX))
    }

    /// Returns the raw arena index this id names.
    #[must_use]
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

/// A `DOCTYPE` declaration, e.g. `<!DOCTYPE html>`.
#[derive(Debug, Clone)]
pub struct Doctype {
    /// The declared root element name, e.g. `"html"`.
    pub name: StrTendril,
    /// The public identifier, or empty if the doctype had none.
    pub public_id: StrTendril,
    /// The system identifier, or empty if the doctype had none.
    pub system_id: StrTendril,
}

/// The kind of a [`Node`] and the data specific to that kind.
#[derive(Debug, Clone)]
pub enum NodeKind {
    /// The document root. Exactly one exists per [`crate::Document`], at
    /// [`crate::Document::root`] (arena index 0).
    Document,
    /// A `DOCTYPE` declaration.
    Doctype(Doctype),
    /// An element: a tag name, its attributes, and (for `<template>`) its contents.
    Element(Element),
    /// A run of character data.
    Text(StrTendril),
    /// A `<!-- comment -->`.
    Comment(StrTendril),
    /// A `<?target data?>` processing instruction.
    ProcessingInstruction {
        /// The instruction target (the part before the first whitespace).
        target: StrTendril,
        /// The instruction data (the rest of the instruction).
        data: StrTendril,
    },
    /// A document fragment root: a detached subtree with no single owning parent, e.g. a
    /// `<template>`'s contents ([`Element::template_contents`]) or the result of
    /// `document.createDocumentFragment()`.
    DocumentFragment,
}

/// A node in a [`crate::Document`]'s arena.
///
/// Tree edges (`parent`, `first_child`, `last_child`, `prev_sibling`, `next_sibling`) are
/// private to the crate: read them through the getters below, and mutate them only
/// through [`crate::Document`]'s tree-mutation methods (`append_child`, `insert_before`,
/// `detach`, `reparent_children`), which keep all of a node's neighbours' links
/// consistent on every call. There is deliberately no public way to set a link directly.
#[derive(Debug, Clone)]
pub struct Node {
    /// This node's kind-specific data.
    pub kind: NodeKind,
    pub(crate) parent: Option<NodeId>,
    pub(crate) first_child: Option<NodeId>,
    pub(crate) last_child: Option<NodeId>,
    pub(crate) prev_sibling: Option<NodeId>,
    pub(crate) next_sibling: Option<NodeId>,
}

impl Node {
    /// Creates a fresh, fully detached node (no parent, no children, no siblings).
    pub(crate) fn new(kind: NodeKind) -> Self {
        Self {
            kind,
            parent: None,
            first_child: None,
            last_child: None,
            prev_sibling: None,
            next_sibling: None,
        }
    }

    /// This node's parent, or `None` if it is the document root or currently detached.
    #[must_use]
    pub fn parent(&self) -> Option<NodeId> {
        self.parent
    }

    /// This node's first child in document order, or `None` if it has none.
    #[must_use]
    pub fn first_child(&self) -> Option<NodeId> {
        self.first_child
    }

    /// This node's last child in document order, or `None` if it has none.
    #[must_use]
    pub fn last_child(&self) -> Option<NodeId> {
        self.last_child
    }

    /// The sibling immediately before this node under their shared parent, or `None` if
    /// this is the first child (or has no parent).
    #[must_use]
    pub fn prev_sibling(&self) -> Option<NodeId> {
        self.prev_sibling
    }

    /// The sibling immediately after this node under their shared parent, or `None` if
    /// this is the last child (or has no parent).
    #[must_use]
    pub fn next_sibling(&self) -> Option<NodeId> {
        self.next_sibling
    }
}
