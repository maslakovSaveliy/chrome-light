//! The `Copy` handles stylo's DOM traits are implemented on.
//!
//! ADR-0015 §1 requires a borrow-based handle rather than Blitz's `*mut NodeTree`
//! (DioxusLabs/blitz#151): a [`NodeHandle`] is the triple `(&Document, &StyleStore,
//! NodeId)`, so the arena and the side table are borrow-checked for the whole style pass
//! and the handle carries no pointer that could outlive them. Everything below is safe
//! Rust — the handles exist precisely so that the interesting invariants are expressed as
//! lifetimes instead of as `unsafe`.
//!
//! [`ElementHandle`] and [`DocumentHandle`] are newtypes over `NodeHandle` because stylo
//! wants three distinct types (`TElement`, `TDocument`, `TNode`) that can be converted
//! into one another. Neither newtype is a *proof* that the node really is an element or
//! the document — see [`NodeHandle::as_element`] / [`NodeHandle::as_document`], the only
//! places they are constructed from a fresh id, and [`ElementHandle::element`], which is
//! total and never panics.
//!
//! Until Task 13 adds `StyleEngine::resolve()`, nothing outside the tests constructs a
//! handle, so a non-test build of the library sees every item here as unreachable. The
//! `dead_code` allow below is scoped to that case and comes off with Task 13; test builds
//! stay strict.
#![cfg_attr(not(test), allow(dead_code))]

use std::hash::{Hash, Hasher};
use std::sync::OnceLock;

use cl_dom::{Document, Element, LocalName, Node, NodeId, NodeKind, QualName, ns};

use crate::store::StyleStore;

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

/// A borrow-based handle to one arena node, for the duration of one style pass.
///
/// `Copy` (stylo requires `TNode: Copy`) and three words wide.
#[derive(Clone, Copy)]
pub(crate) struct NodeHandle<'a> {
    /// The document this node lives in. Shared, hence immutable for the whole pass.
    pub doc: &'a Document,
    /// The side table holding this pass's `ElementData` and friends.
    pub store: &'a StyleStore,
    /// The node's arena index.
    pub id: NodeId,
}

impl<'a> NodeHandle<'a> {
    /// Builds a handle for `id` in `doc`, backed by `store`.
    pub(crate) fn new(doc: &'a Document, store: &'a StyleStore, id: NodeId) -> Self {
        Self { doc, store, id }
    }

    /// A handle to a *different* node of the same document and store.
    pub(crate) fn with(self, id: NodeId) -> Self {
        Self { id, ..self }
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
        self.node().and_then(Node::parent).map(|id| self.with(id))
    }

    /// This node's first child.
    pub(crate) fn first_child(self) -> Option<Self> {
        self.node()
            .and_then(Node::first_child)
            .map(|id| self.with(id))
    }

    /// This node's last child.
    pub(crate) fn last_child(self) -> Option<Self> {
        self.node()
            .and_then(Node::last_child)
            .map(|id| self.with(id))
    }

    /// The sibling immediately before this node.
    pub(crate) fn prev_sibling(self) -> Option<Self> {
        self.node()
            .and_then(Node::prev_sibling)
            .map(|id| self.with(id))
    }

    /// The sibling immediately after this node.
    pub(crate) fn next_sibling(self) -> Option<Self> {
        self.node()
            .and_then(Node::next_sibling)
            .map(|id| self.with(id))
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
        self.ids.next().map(|id| self.parent.with(id))
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
    use super::{ElementHandle, NodeHandle};
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

    #[allow(clippy::expect_used)]
    pub(crate) fn fixture() -> Fixture {
        fn element(name: &str, attrs: &[(&str, &str)]) -> NodeKind {
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

    #[test]
    fn as_element_should_reject_non_elements() {
        let f = fixture();
        let store = store(&f.doc);
        assert!(NodeHandle::new(&f.doc, &store, f.p).as_element().is_some());
        assert!(
            NodeHandle::new(&f.doc, &store, f.text)
                .as_element()
                .is_none()
        );
        assert!(
            NodeHandle::new(&f.doc, &store, f.doc.root())
                .as_document()
                .is_some()
        );
    }

    #[test]
    fn element_sibling_walks_should_skip_text_nodes() {
        let f = fixture();
        let store = store(&f.doc);
        let p = ElementHandle(NodeHandle::new(&f.doc, &store, f.p));
        let span = ElementHandle(NodeHandle::new(&f.doc, &store, f.span));

        assert_eq!(p.next_sibling_element(), Some(span));
        assert_eq!(span.prev_sibling_element(), Some(p));
        assert_eq!(p.prev_sibling_element(), None);
        assert_eq!(span.next_sibling_element(), None);
        // <p>'s only child is a text node, so it has no element children.
        assert_eq!(p.first_element_child(), None);
        let body = ElementHandle(NodeHandle::new(&f.doc, &store, f.body));
        assert_eq!(body.first_element_child(), Some(p));
        let _ = f.html;
    }

    #[test]
    fn handles_for_the_same_node_should_compare_equal() {
        let f = fixture();
        let store = store(&f.doc);
        let a = NodeHandle::new(&f.doc, &store, f.p);
        let b = NodeHandle::new(&f.doc, &store, f.p);
        let c = NodeHandle::new(&f.doc, &store, f.span);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
