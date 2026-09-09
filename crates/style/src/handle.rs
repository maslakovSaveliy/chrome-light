//! The `Copy` handles stylo's DOM traits are implemented on.
//!
//! ADR-0015 §1 requires a borrow-based handle rather than Blitz's `*mut NodeTree`
//! (DioxusLabs/blitz#151), and that is what this is — but it is **one machine word**, not
//! the three-word `(&Document, &StyleStore, NodeId)` triple the ADR sketched.
//!
//! # Why the handle has to be pointer-sized (ADR-0015 §1 amendment, Task 13)
//!
//! stylo's style-sharing cache keeps its LRU allocation in a thread-local typed as
//! `SharingCacheBase<FakeCandidate>` and `transmute`s it to `SharingCacheBase<
//! StyleSharingCandidate<E>>` per element type. `FakeCandidate` declares the element field
//! as a bare `usize` (registry `stylo-0.20.0/sharing/mod.rs:324`), and
//! `StyleSharingCache::new` — which `ThreadLocalStyleContext::new` calls unconditionally,
//! so *every* traversal hits it — opens with a hard
//!
//! ```text
//! assert_eq!(mem::size_of::<SharingCache<E>>(), mem::size_of::<TypelessSharingCache>());
//! ```
//!
//! (`sharing/mod.rs:611`). A 24-byte handle therefore aborts every style pass with
//! `left: 10000, right: 9488` — 32 cache entries × the 16 bytes by which the triple
//! overshoots a `usize`. **`size_of::<ElementHandle>() == size_of::<usize>()` is a hard
//! requirement stylo 0.20 places on any embedder**, and it is why Blitz's handle is a raw
//! pointer. The `const` assertion below pins it so the next change to these types fails to
//! compile rather than at run time.
//!
//! The triple is still there; it just moved one level down. A style pass allocates one
//! [`NodeArena`] holding a [`NodeSlot`] per arena node — that is where `(&Document,
//! &StyleStore, NodeId)` lives — and a [`NodeHandle`] is a `&NodeSlot` into it. Tree
//! navigation needs to reach *other* slots, so each slot also carries a shared reference to
//! the whole slot array. That reference cannot exist when the array is built, hence the
//! `Cell` in [`NodeSlot::siblings`] and [`NodeArena::link`]: interior mutability is what
//! lets the array reference itself in **safe** Rust, where Blitz needs a raw pointer.
//! Everything below is still safe Rust; the handles exist precisely so that the
//! interesting invariants are expressed as lifetimes instead of as `unsafe`.
//!
//! [`ElementHandle`] and [`DocumentHandle`] are newtypes over `NodeHandle` because stylo
//! wants three distinct types (`TElement`, `TDocument`, `TNode`) that can be converted
//! into one another. Neither newtype is a *proof* that the node really is an element or
//! the document — see [`NodeHandle::as_element`] / [`NodeHandle::as_document`], the only
//! places they are constructed from a fresh id, and [`ElementHandle::element`], which is
//! total and never panics.

use std::cell::Cell;
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::sync::OnceLock;

use cl_dom::{Document, Element, LocalName, Node, NodeId, NodeKind, QualName, ns};

use crate::store::StyleStore;

/// stylo requires element handles to be exactly pointer-sized — see the module docs for the
/// `assert_eq!` in `StyleSharingCache::new` that enforces it at run time on every traversal.
/// Checking it here turns that run-time abort into a compile error.
const _: () = assert!(
    size_of::<ElementHandle<'static>>() == size_of::<usize>(),
    "stylo transmutes its style-sharing cache through a usize-sized element field \
     (stylo-0.20.0/sharing/mod.rs:324,611); an element handle must stay one word wide"
);

/// The qualified name reported for a node that is not an element.
///
/// `TElement::{local_name, namespace}` and `selectors::Element::has_local_name` return
/// borrowed names and cannot fail, but an [`ElementHandle`] is only *conventionally*
/// element-shaped. Rather than `unwrap` (forbidden outside tests) or panic on a
/// mis-constructed handle, those methods fall back to this empty-namespace, empty-local
/// name, which matches no selector.
fn placeholder_name() -> &'static QualName {
    static PLACEHOLDER: OnceLock<QualName> = OnceLock::new();
    PLACEHOLDER.get_or_init(|| QualName::new(None, ns!(), LocalName::from("")))
}

/// The per-pass record for one arena node: the `(&Document, &StyleStore, NodeId)` triple
/// ADR-0015 §1 specifies, plus the back-reference tree navigation needs.
///
/// Lives in a [`NodeArena`], one slot per [`NodeId`], for exactly one style pass. It is
/// never handled directly: a [`NodeHandle`] is a `&NodeSlot`, and the fields are read
/// through that handle's [`Deref`].
pub(crate) struct NodeSlot<'a> {
    /// The document this node lives in. Shared, hence immutable for the whole pass.
    pub doc: &'a Document,
    /// The side table holding this pass's `ElementData` and friends.
    pub store: &'a StyleStore,
    /// The node's arena index.
    pub id: NodeId,
    /// Every slot of the [`NodeArena`] this slot belongs to, itself included — how
    /// [`NodeHandle::with`] gets from one node to another.
    ///
    /// A `Cell` because an array cannot hold a reference to itself at the moment it is
    /// built: [`NodeArena::link`] fills every slot in once the array exists. `None` only
    /// between those two moments, which no handle can observe — [`NodeArena::node`] links
    /// before it hands out its first handle.
    siblings: Cell<Option<&'a [NodeSlot<'a>]>>,
}

/// Every [`NodeSlot`] of one style pass, indexed by [`NodeId`].
///
/// Built once per `StyleEngine::resolve` alongside the [`StyleStore`] and dropped with it.
/// Handles borrow from it, so it must outlive the traversal.
pub(crate) struct NodeArena<'a> {
    /// `slots[id.index()]` is the slot for node `id`.
    slots: Box<[NodeSlot<'a>]>,
    /// Whether [`NodeArena::link`] has already run.
    linked: Cell<bool>,
}

impl<'a> NodeArena<'a> {
    /// Builds one slot per node of `doc`, backed by `store`.
    ///
    /// # Panics
    /// If `store` was not built for `doc` — i.e. `StyleStore::new` was given a length other
    /// than this document's `len()`. This is a hard assert rather than a `debug_assert!`
    /// because a store sized from a stale `Document::len()` silently collapses several real
    /// elements onto the store's single scratch slot, and two live `ElementDataMut` on one
    /// slot is undefined behaviour in a release build (`store.rs`'s `scratch` docs). It can
    /// only fire on a programming error inside this crate — never on document content — and
    /// runs once per pass, not per node.
    pub(crate) fn new(doc: &'a Document, store: &'a StyleStore) -> Self {
        assert_eq!(
            store.slot_count(),
            doc.len(),
            "StyleStore was built for a different document than the handles borrow"
        );
        let slots = (0..doc.len())
            .map(|index| NodeSlot {
                doc,
                store,
                id: NodeId::from_index(index),
                siblings: Cell::new(None),
            })
            .collect();
        Self {
            slots,
            linked: Cell::new(false),
        }
    }

    /// Points every slot at the whole array, so [`NodeHandle::with`] can navigate.
    ///
    /// Idempotent and O(n); runs on the first [`NodeArena::node`] call and never again.
    fn link(&'a self) {
        if self.linked.replace(true) {
            return;
        }
        let all: &'a [NodeSlot<'a>] = &self.slots;
        for slot in all {
            slot.siblings.set(Some(all));
        }
    }

    /// A handle to node `id`, or `None` if `id` is not a node of this arena's document.
    pub(crate) fn node(&'a self, id: NodeId) -> Option<NodeHandle<'a>> {
        self.link();
        self.slots.get(id.index()).map(NodeHandle)
    }

    /// A handle to node `id` presented as an element, or `None` if `id` is unknown or is
    /// not an element.
    pub(crate) fn element(&'a self, id: NodeId) -> Option<ElementHandle<'a>> {
        self.node(id).and_then(NodeHandle::as_element)
    }
}

/// A borrow-based handle to one arena node, for the duration of one style pass.
///
/// `Copy` (stylo requires `TNode: Copy`) and exactly one machine word — see the module docs
/// for why the width is not negotiable. Derefs to its [`NodeSlot`], so `handle.doc`,
/// `handle.store` and `handle.id` read the triple straight through.
#[derive(Clone, Copy)]
pub(crate) struct NodeHandle<'a>(&'a NodeSlot<'a>);

impl<'a> Deref for NodeHandle<'a> {
    type Target = NodeSlot<'a>;

    fn deref(&self) -> &Self::Target {
        self.0
    }
}

impl<'a> NodeHandle<'a> {
    /// A handle to a *different* node of the same document and store, or `None` if `id` is
    /// not a node of that document.
    pub(crate) fn with(self, id: NodeId) -> Option<Self> {
        self.0.siblings.get()?.get(id.index()).map(NodeHandle)
    }

    /// The arena node, or `None` if the id is unknown (never in practice — see
    /// [`crate::store::StyleStore`]'s scratch slot for the same argument).
    pub(crate) fn node(self) -> Option<&'a Node> {
        self.doc.get(self.id)
    }

    /// The node's kind-specific data, if the node exists.
    pub(crate) fn kind(self) -> Option<&'a NodeKind> {
        self.node().map(|node| &node.kind)
    }

    /// Whether this node is an element.
    pub(crate) fn is_element(self) -> bool {
        matches!(self.kind(), Some(NodeKind::Element(_)))
    }

    /// Whether this node is a text node.
    pub(crate) fn is_text(self) -> bool {
        matches!(self.kind(), Some(NodeKind::Text(_)))
    }

    /// Whether this node is the document root (arena index 0).
    pub(crate) fn is_document(self) -> bool {
        matches!(self.kind(), Some(NodeKind::Document))
    }

    /// This node as an element handle, if it is an element.
    pub(crate) fn as_element(self) -> Option<ElementHandle<'a>> {
        self.is_element().then_some(ElementHandle(self))
    }

    /// This node as a document handle, if it is the document node.
    pub(crate) fn as_document(self) -> Option<DocumentHandle<'a>> {
        self.is_document().then_some(DocumentHandle(self))
    }

    /// This node's parent.
    pub(crate) fn parent(self) -> Option<Self> {
        self.node()
            .and_then(Node::parent)
            .and_then(|id| self.with(id))
    }

    /// This node's first child.
    pub(crate) fn first_child(self) -> Option<Self> {
        self.node()
            .and_then(Node::first_child)
            .and_then(|id| self.with(id))
    }

    /// This node's last child.
    pub(crate) fn last_child(self) -> Option<Self> {
        self.node()
            .and_then(Node::last_child)
            .and_then(|id| self.with(id))
    }

    /// The sibling immediately before this node.
    pub(crate) fn prev_sibling(self) -> Option<Self> {
        self.node()
            .and_then(Node::prev_sibling)
            .and_then(|id| self.with(id))
    }

    /// The sibling immediately after this node.
    pub(crate) fn next_sibling(self) -> Option<Self> {
        self.node()
            .and_then(Node::next_sibling)
            .and_then(|id| self.with(id))
    }

    /// This node's direct children, in document order.
    pub(crate) fn children(self) -> ChildHandles<'a> {
        ChildHandles {
            parent: self,
            ids: self.doc.children(self.id),
        }
    }
}

impl PartialEq for NodeHandle<'_> {
    fn eq(&self, other: &Self) -> bool {
        // Ids are only comparable within one document (`cl_dom::NodeId` docs), so compare
        // the arena identity too. Two handles for the same node always share the `doc`
        // reference, since handles are only ever derived from one another.
        self.id == other.id && std::ptr::eq(self.doc, other.doc)
    }
}

impl Eq for NodeHandle<'_> {}

impl Hash for NodeHandle<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.index().hash(state);
    }
}

impl std::fmt::Debug for NodeHandle<'_> {
    /// Prints the node's identity only. Deliberately *not* derived: a derive would recurse
    /// into `Document` and dump the whole arena every time stylo logs a node.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.kind() {
            Some(NodeKind::Element(element)) => {
                write!(f, "<{}#{}>", element.name.local, self.id.index())
            }
            Some(NodeKind::Text(_)) => write!(f, "#text#{}", self.id.index()),
            Some(NodeKind::Document) => write!(f, "#document#{}", self.id.index()),
            Some(_) => write!(f, "#node#{}", self.id.index()),
            None => write!(f, "#missing#{}", self.id.index()),
        }
    }
}

/// Iterator over a node's direct children as handles.
///
/// Wraps [`cl_dom::Children`], which is itself iterative and bounded by `Document::len()`,
/// so it terminates even on a corrupted arena.
pub(crate) struct ChildHandles<'a> {
    /// The parent handle, reused to carry `doc`/`store` to each child.
    parent: NodeHandle<'a>,
    /// The underlying id iterator.
    ids: cl_dom::Children<'a>,
}

impl<'a> Iterator for ChildHandles<'a> {
    type Item = NodeHandle<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        self.ids.next().and_then(|id| self.parent.with(id))
    }
}

/// A [`NodeHandle`] presented to stylo as an element (`TElement`,
/// `selectors::Element`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct ElementHandle<'a>(pub NodeHandle<'a>);

impl<'a> ElementHandle<'a> {
    /// The underlying node handle.
    pub(crate) fn node(self) -> NodeHandle<'a> {
        self.0
    }

    /// This element's arena data, or `None` if the handle does not name an element.
    pub(crate) fn element(self) -> Option<&'a Element> {
        self.0.doc.element(self.0.id)
    }

    /// This element's qualified name, or the [`placeholder_name`] if the handle does not
    /// name an element.
    pub(crate) fn name(self) -> &'a QualName {
        match self.element() {
            Some(element) => &element.name,
            None => placeholder_name(),
        }
    }

    /// The value of the attribute `local` in any namespace, or `None`.
    pub(crate) fn attr(self, local: &LocalName) -> Option<&'a str> {
        self.0.doc.attr(self.0.id, local)
    }

    /// The nearest preceding sibling that is an element.
    pub(crate) fn prev_sibling_element(self) -> Option<Self> {
        let mut cursor = self.0.prev_sibling();
        while let Some(node) = cursor {
            if let Some(element) = node.as_element() {
                return Some(element);
            }
            cursor = node.prev_sibling();
        }
        None
    }

    /// The nearest following sibling that is an element.
    pub(crate) fn next_sibling_element(self) -> Option<Self> {
        let mut cursor = self.0.next_sibling();
        while let Some(node) = cursor {
            if let Some(element) = node.as_element() {
                return Some(element);
            }
            cursor = node.next_sibling();
        }
        None
    }

    /// The first child that is an element.
    pub(crate) fn first_element_child(self) -> Option<Self> {
        self.0.children().find_map(NodeHandle::as_element)
    }
}

/// A [`NodeHandle`] presented to stylo as the document node (`TDocument`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(crate) struct DocumentHandle<'a>(pub NodeHandle<'a>);

impl<'a> DocumentHandle<'a> {
    /// The underlying node handle.
    pub(crate) fn node(self) -> NodeHandle<'a> {
        self.0
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::{ElementHandle, NodeArena, NodeHandle};
    use crate::store::StyleStore;
    use cl_dom::{Attr, Document, Element, LocalName, NodeId, NodeKind, QualName, StrTendril, ns};
    use style::shared_lock::SharedRwLock;

    /// `<html><body><p id="a" class="x y">hi</p><span></span></body></html>`.
    pub(crate) struct Fixture {
        pub doc: Document,
        pub html: NodeId,
        pub body: NodeId,
        pub p: NodeId,
        pub text: NodeId,
        pub span: NodeId,
    }

    /// An HTML-namespace element node with `attrs` in no namespace.
    ///
    /// Shared with the `stylo_dom`/`stylo_selectors` test modules, which build their own
    /// small documents (an `<a href>`, an attribute zoo, a comment-only element) rather than
    /// bend [`fixture`] — every existing assertion about its shape would have to move.
    pub(crate) fn element(name: &str, attrs: &[(&str, &str)]) -> NodeKind {
        NodeKind::Element(Element {
            name: QualName::new(None, ns!(html), LocalName::from(name)),
            attrs: attrs
                .iter()
                .map(|(k, v)| Attr {
                    name: QualName::new(None, ns!(), LocalName::from(*k)),
                    value: StrTendril::from(*v),
                })
                .collect(),
            template_contents: None,
        })
    }

    #[allow(clippy::expect_used)]
    pub(crate) fn fixture() -> Fixture {
        let mut doc = Document::new("file:///test.html");
        let root = doc.root();
        let html = doc.create(element("html", &[]));
        let body = doc.create(element("body", &[]));
        let p = doc.create(element("p", &[("id", "a"), ("class", "x y")]));
        let text = doc.create(NodeKind::Text(StrTendril::from("hi")));
        let span = doc.create(element("span", &[]));
        doc.append_child(root, html).expect("append html");
        doc.append_child(html, body).expect("append body");
        doc.append_child(body, p).expect("append p");
        doc.append_child(p, text).expect("append text");
        doc.append_child(body, span).expect("append span");

        Fixture {
            doc,
            html,
            body,
            p,
            text,
            span,
        }
    }

    pub(crate) fn store(doc: &Document) -> StyleStore {
        StyleStore::new(doc.len(), SharedRwLock::new())
    }

    /// A node handle for `id` out of `arena`.
    ///
    /// Every test id comes from a fixture built against the same document the arena was
    /// sized for, so the lookup cannot miss; `expect` documents that rather than forcing
    /// every call site to unwrap.
    #[allow(clippy::expect_used)]
    pub(crate) fn node_handle<'a>(arena: &'a NodeArena<'a>, id: NodeId) -> NodeHandle<'a> {
        arena
            .node(id)
            .expect("node id belongs to the arena's document")
    }

    /// The same node presented as an element — without checking that it *is* one, so tests
    /// can also assert what a mis-constructed handle does.
    pub(crate) fn element_handle<'a>(arena: &'a NodeArena<'a>, id: NodeId) -> ElementHandle<'a> {
        ElementHandle(node_handle(arena, id))
    }

    #[test]
    fn as_element_should_reject_non_elements() {
        let f = fixture();
        let store = store(&f.doc);
        let arena = NodeArena::new(&f.doc, &store);
        assert!(node_handle(&arena, f.p).as_element().is_some());
        assert!(node_handle(&arena, f.text).as_element().is_none());
        assert!(node_handle(&arena, f.doc.root()).as_document().is_some());
    }

    #[test]
    fn element_sibling_walks_should_skip_text_nodes() {
        let f = fixture();
        let store = store(&f.doc);
        let arena = NodeArena::new(&f.doc, &store);
        let p = element_handle(&arena, f.p);
        let span = element_handle(&arena, f.span);

        assert_eq!(p.next_sibling_element(), Some(span));
        assert_eq!(span.prev_sibling_element(), Some(p));
        assert_eq!(p.prev_sibling_element(), None);
        assert_eq!(span.next_sibling_element(), None);
        // <p>'s only child is a text node, so it has no element children.
        assert_eq!(p.first_element_child(), None);
        let body = element_handle(&arena, f.body);
        assert_eq!(body.first_element_child(), Some(p));
        let _ = f.html;
    }

    #[test]
    fn handles_for_the_same_node_should_compare_equal() {
        let f = fixture();
        let store = store(&f.doc);
        let arena = NodeArena::new(&f.doc, &store);
        let a = node_handle(&arena, f.p);
        let b = node_handle(&arena, f.p);
        let c = node_handle(&arena, f.span);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
