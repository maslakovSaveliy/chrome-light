//! `style::dom::{TDocument, TNode, NodeInfo, TShadowRoot, TElement}` over the arena
//! handles.
//!
//! The trait definitions this file tracks live in the vendored registry copy at
//! `stylo-0.20.0/dom.rs`: `NodeInfo` (line 42), `TDocument` (119), `TNode` (153),
//! `TShadowRoot` (364) and `TElement` (409). Everything here is safe Rust; the per-element
//! state stylo mutates through `&self` lives in [`crate::store::StyleStore`], which
//! explains in its module docs why ADR-0015's `UnsafeCell` budget turned out not to be
//! needed against stylo 0.20.
//!
//! Shadow DOM is out of M1a's scope. Rather than `todo!()` in a reachable path, the
//! `TNode::ConcreteShadowRoot` slot is filled by [`NoShadowRoot`], an *uninhabited* type:
//! `as_shadow_root`, `shadow_root` and `containing_shadow` always return `None`, so no
//! value of it can ever exist and its trait methods are unreachable by construction rather
//! than by convention.
//!
//! # Why this module opts out of `#![deny(unsafe_code)]`
//!
//! Six `TElement` methods are declared `unsafe fn` by stylo
//! (`set_handled_snapshot`, `set_dirty_descendants`, `unset_dirty_descendants`,
//! `set_animation_only_dirty_descendants`, `ensure_data`, `clear_data`) because Gecko
//! implements them by mutating an element through a shared reference. *Implementing* an
//! `unsafe fn` is itself `unsafe_code`, so this module needs the ADR-0015 §1 carve-out
//! even though it contains no `unsafe` block and performs no unsafe operation: every one
//! of those bodies is an ordinary safe call into [`crate::store::StyleStore`], whose
//! `Cell`/`ElementDataWrapper` slots need no raw pointers. The obligation stylo places on
//! the caller — exclusive access, one thread — is checked at those entry points by the
//! store's owning-thread `debug_assert`s.
#![allow(unsafe_code)]
#![cfg_attr(not(test), allow(dead_code))]

use std::marker::PhantomData;

use cl_dom::{NodeKind, QuirksMode as DomQuirksMode, local_name, ns};
use selectors::matching::{ElementSelectorFlags, QuirksMode, VisitedHandlingMode};
use selectors::sink::Push;
use style::applicable_declarations::ApplicableDeclarationBlock;
use style::context::SharedStyleContext;
use style::data::{ElementDataMut, ElementDataRef};
use style::dom::{LayoutIterator, NodeInfo, OpaqueNode, TDocument, TElement, TNode, TShadowRoot};
use style::properties::PropertyDeclarationBlock;
use style::selector_parser::{AttrValue, Lang};
use style::servo_arc::{Arc as StyloArc, ArcBorrow};
use style::shared_lock::{Locked, SharedRwLock};
use style::stylist::CascadeData;
use style::values::AtomIdent;
use style::values::GenericAtomIdent;
use style::values::computed::Au;
use style::values::specified::Display;
use style::{Atom, LocalName as StyloLocalName, Namespace as StyloNamespace};
use stylo_dom::ElementState;

use crate::handle::{ChildHandles, DocumentHandle, ElementHandle, NodeHandle};

/// An uninhabited stand-in for `TNode::ConcreteShadowRoot`.
///
/// M1a has no shadow DOM. The `never` field makes the type impossible to construct, so
/// every `TShadowRoot` method below is discharged with `match self.never {}` — no
/// `todo!()`, no `unimplemented!()`, and no reachable panic. The `PhantomData` ties the
/// lifetime parameter that `TShadowRoot::ConcreteNode = NodeHandle<'a>` requires.
///
/// `dead_code` is allowed unconditionally here precisely *because* it is never
/// constructed — that is the point of the type, not an oversight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) struct NoShadowRoot<'a> {
    /// Uninhabited: the reason no `NoShadowRoot` value can exist.
    never: std::convert::Infallible,
    /// Carries `'a` so `ConcreteNode = NodeHandle<'a>` type-checks.
    _node: PhantomData<NodeHandle<'a>>,
}

impl<'a> TDocument for DocumentHandle<'a> {
    type ConcreteNode = NodeHandle<'a>;

    fn as_node(&self) -> Self::ConcreteNode {
        self.node()
    }

    fn is_html_document(&self) -> bool {
        // `cl-html` is the only producer of these documents (ADR-0004), so every document
        // reaching the cascade is an HTML document.
        true
    }

    fn quirks_mode(&self) -> QuirksMode {
        match self.node().doc.quirks_mode() {
            DomQuirksMode::NoQuirks => QuirksMode::NoQuirks,
            DomQuirksMode::LimitedQuirks => QuirksMode::LimitedQuirks,
            DomQuirksMode::Quirks => QuirksMode::Quirks,
        }
    }

    fn shared_lock(&self) -> &SharedRwLock {
        self.node().store.lock()
    }
}

impl NodeInfo for NodeHandle<'_> {
    fn is_element(&self) -> bool {
        NodeHandle::is_element(*self)
    }

    fn is_text_node(&self) -> bool {
        NodeHandle::is_text(*self)
    }
}

impl<'a> TNode for NodeHandle<'a> {
    type ConcreteElement = ElementHandle<'a>;
    type ConcreteDocument = DocumentHandle<'a>;
    type ConcreteShadowRoot = NoShadowRoot<'a>;

    fn parent_node(&self) -> Option<Self> {
        NodeHandle::parent(*self)
    }

    fn first_child(&self) -> Option<Self> {
        NodeHandle::first_child(*self)
    }

    fn last_child(&self) -> Option<Self> {
        NodeHandle::last_child(*self)
    }

    fn prev_sibling(&self) -> Option<Self> {
        NodeHandle::prev_sibling(*self)
    }

    fn next_sibling(&self) -> Option<Self> {
        NodeHandle::next_sibling(*self)
    }

    fn owner_doc(&self) -> Self::ConcreteDocument {
        DocumentHandle(self.with(self.doc.root()))
    }

    fn is_in_document(&self) -> bool {
        let root = self.doc.root();
        self.id == root || self.doc.ancestors(self.id).any(|id| id == root)
    }

    fn traversal_parent(&self) -> Option<Self::ConcreteElement> {
        self.parent_node().and_then(NodeHandle::as_element)
    }

    fn opaque(&self) -> OpaqueNode {
        OpaqueNode(self.id.index())
    }

    fn debug_id(self) -> usize {
        self.id.index()
    }

    fn as_element(&self) -> Option<Self::ConcreteElement> {
        NodeHandle::as_element(*self)
    }

    fn as_document(&self) -> Option<Self::ConcreteDocument> {
        NodeHandle::as_document(*self)
    }

    fn as_shadow_root(&self) -> Option<Self::ConcreteShadowRoot> {
        // No shadow DOM in M1a; see `NoShadowRoot`.
        None
    }
}

impl<'a> TShadowRoot for NoShadowRoot<'a> {
    type ConcreteNode = NodeHandle<'a>;

    fn as_node(&self) -> Self::ConcreteNode {
        match self.never {}
    }

    fn host(&self) -> <Self::ConcreteNode as TNode>::ConcreteElement {
        match self.never {}
    }

    fn style_data<'b>(&self) -> Option<&'b CascadeData>
    where
        Self: 'b,
    {
        match self.never {}
    }
}

impl<'a> TElement for ElementHandle<'a> {
    type ConcreteNode = NodeHandle<'a>;
    type TraversalChildrenIterator = ChildHandles<'a>;

    fn as_node(&self) -> Self::ConcreteNode {
        self.node()
    }

    fn traversal_children(&self) -> LayoutIterator<Self::TraversalChildrenIterator> {
        // `LayoutIterator` itself drops everything that is neither an element nor a text
        // node, so the raw child iterator is what stylo wants here.
        LayoutIterator(self.node().children())
    }

    fn is_html_element(&self) -> bool {
        self.name().ns == ns!(html)
    }

    fn is_mathml_element(&self) -> bool {
        self.name().ns == ns!(mathml)
    }

    fn is_svg_element(&self) -> bool {
        self.name().ns == ns!(svg)
    }

    fn style_attribute(&self) -> Option<ArcBorrow<'_, Locked<PropertyDeclarationBlock>>> {
        // The `style="..."` attribute is not parsed in M1a: `cl-dom` keeps attributes as
        // raw strings and there is nowhere to own the parsed block for the pass. Author
        // sheets are the only declaration source, which is all Task 12's UA sheet and the
        // M1a CSS subset need.
        None
    }

    fn animation_rule(
        &self,
        _context: &SharedStyleContext<'_>,
    ) -> Option<StyloArc<Locked<PropertyDeclarationBlock>>> {
        // No animations in M1a (static pipeline).
        None
    }

    fn transition_rule(
        &self,
        _context: &SharedStyleContext<'_>,
    ) -> Option<StyloArc<Locked<PropertyDeclarationBlock>>> {
        None
    }

    fn state(&self) -> ElementState {
        // The only element state M1a models is link-ness, which drives `:link`/`:any-link`
        // (see `stylo_selectors`). There is no interaction, history or form state yet, so
        // `:hover`, `:visited`, `:checked`, ... are all correctly "not set".
        if self.is_link_element() {
            ElementState::UNVISITED
        } else {
            ElementState::empty()
        }
    }

    fn has_part_attr(&self) -> bool {
        false
    }

    fn exports_any_part(&self) -> bool {
        false
    }

    fn id(&self) -> Option<&Atom> {
        let node = self.node();
        let element = *self;
        node.store.id_atom(node.id, move || {
            element.attr(&local_name!("id")).map(Atom::from)
        })
    }

    fn each_class<F>(&self, mut callback: F)
    where
        F: FnMut(&AtomIdent),
    {
        let Some(class) = self.attr(&local_name!("class")) else {
            return;
        };
        for name in class.split_ascii_whitespace() {
            callback(AtomIdent::cast(&Atom::from(name)));
        }
    }

    fn each_custom_state<F>(&self, _callback: F)
    where
        F: FnMut(&AtomIdent),
    {
        // `:state()` needs custom elements, which M1a does not have.
    }

    fn each_attr_name<F>(&self, mut callback: F)
    where
        F: FnMut(&StyloLocalName),
    {
        let Some(element) = self.element() else {
            return;
        };
        for attr in &element.attrs {
            callback(&GenericAtomIdent(attr.name.local.clone()));
        }
    }

    fn has_dirty_descendants(&self) -> bool {
        let node = self.node();
        node.store.has_dirty_descendants(node.id)
    }

    fn has_snapshot(&self) -> bool {
        // Snapshots exist to restyle after a DOM/state mutation; M1a styles each document
        // exactly once, so there is never a snapshot to reconcile.
        false
    }

    fn handled_snapshot(&self) -> bool {
        // Vacuously true: `has_snapshot` is always false.
        true
    }

    unsafe fn set_handled_snapshot(&self) {
        // Nothing to record while `has_snapshot` is always false.
    }

    unsafe fn set_dirty_descendants(&self) {
        let node = self.node();
        node.store.set_dirty_descendants(node.id, true);
    }

    unsafe fn unset_dirty_descendants(&self) {
        let node = self.node();
        node.store.set_dirty_descendants(node.id, false);
    }

    fn store_children_to_process(&self, n: isize) {
        let node = self.node();
        node.store.store_children_to_process(node.id, n);
    }

    fn did_process_child(&self) -> isize {
        let node = self.node();
        node.store.did_process_child(node.id)
    }

    unsafe fn ensure_data(&self) -> ElementDataMut<'_> {
        let node = self.node();
        node.store.ensure_data(node.id)
    }

    unsafe fn clear_data(&self) {
        let node = self.node();
        node.store.clear_data(node.id);
    }

    fn has_data(&self) -> bool {
        let node = self.node();
        node.store.has_data(node.id)
    }

    fn borrow_data(&self) -> Option<ElementDataRef<'_>> {
        let node = self.node();
        node.store.get(node.id)
    }

    fn mutate_data(&self) -> Option<ElementDataMut<'_>> {
        let node = self.node();
        node.store.get_mut(node.id)
    }

    fn skip_item_display_fixup(&self) -> bool {
        // Gecko only skips the fixup for native anonymous content, which we have none of.
        false
    }

    fn may_have_animations(&self) -> bool {
        false
    }

    fn has_animations(&self, _context: &SharedStyleContext<'_>) -> bool {
        false
    }

    fn has_css_animations(
        &self,
        _context: &SharedStyleContext<'_>,
        _pseudo_element: Option<style::selector_parser::PseudoElement>,
    ) -> bool {
        false
    }

    fn has_css_transitions(
        &self,
        _context: &SharedStyleContext<'_>,
        _pseudo_element: Option<style::selector_parser::PseudoElement>,
    ) -> bool {
        false
    }

    fn shadow_root(&self) -> Option<NoShadowRoot<'a>> {
        None
    }

    fn containing_shadow(&self) -> Option<NoShadowRoot<'a>> {
        None
    }

    fn lang_attr(&self) -> Option<AttrValue> {
        // `:lang()` is out of M1a's selector scope; reporting "no language" makes every
        // `:lang()` selector fail rather than match the wrong thing.
        None
    }

    fn match_element_lang(&self, _override_lang: Option<Option<AttrValue>>, _value: &Lang) -> bool {
        false
    }

    fn is_html_document_body_element(&self) -> bool {
        if self.name().local != local_name!("body") || self.name().ns != ns!(html) {
            return false;
        }
        // The body element is the first `<body>` child of the root element.
        self.node()
            .parent()
            .and_then(NodeHandle::as_element)
            .is_some_and(|parent| parent.node().parent().is_some_and(NodeHandle::is_document))
    }

    fn synthesize_presentational_hints_for_legacy_attributes<V>(
        &self,
        _visited_handling: VisitedHandlingMode,
        _hints: &mut V,
    ) where
        V: Push<ApplicableDeclarationBlock>,
    {
        // Presentational hints (`bgcolor`, `width`, `align`, ...) are legacy HTML
        // attribute mappings. M1a's CSS subset is driven from stylesheets only; the
        // attribute mappings are tracked as `partial` in `docs/SPEC_REGISTRY.md`.
    }

    fn local_name(&self) -> &cl_dom::LocalName {
        &self.name().local
    }

    fn namespace(&self) -> &cl_dom::Namespace {
        &self.name().ns
    }

    fn query_container_size(&self, _display: &Display) -> euclid::default::Size2D<Option<Au>> {
        // Container queries need layout to have run; reporting "no size" disables them
        // without panicking, which is what `@container`-free M1a content needs.
        euclid::default::Size2D::default()
    }

    fn has_selector_flags(&self, flags: ElementSelectorFlags) -> bool {
        let node = self.node();
        node.store.selector_flags(node.id).contains(flags)
    }

    fn relative_selector_search_direction(&self) -> ElementSelectorFlags {
        let node = self.node();
        let flags = node.store.selector_flags(node.id);
        for candidate in [
            ElementSelectorFlags::RELATIVE_SELECTOR_SEARCH_DIRECTION_ANCESTOR_SIBLING,
            ElementSelectorFlags::RELATIVE_SELECTOR_SEARCH_DIRECTION_ANCESTOR,
            ElementSelectorFlags::RELATIVE_SELECTOR_SEARCH_DIRECTION_SIBLING,
        ] {
            if flags.contains(candidate) {
                return candidate;
            }
        }
        ElementSelectorFlags::empty()
    }

    fn get_attr(&self, attr: &StyloLocalName, namespace: &StyloNamespace) -> Option<String> {
        let element = self.element()?;
        element
            .attrs
            .iter()
            .find(|a| a.name.local == attr.0 && a.name.ns == namespace.0)
            .map(|a| a.value.to_string())
    }
}

impl ElementHandle<'_> {
    /// Whether this element is a link for selector-matching purposes: an HTML `<a>`,
    /// `<area>` or `<link>` carrying an `href` attribute
    /// (<https://drafts.csswg.org/selectors-4/#the-any-link-pseudo>).
    ///
    /// M1a has no history, so a link is always *unvisited*: `:link` and `:any-link` match
    /// it, `:visited` never does.
    pub(crate) fn is_link_element(self) -> bool {
        let name = self.name();
        if name.ns != ns!(html) {
            return false;
        }
        let linkish = name.local == local_name!("a")
            || name.local == local_name!("area")
            || name.local == local_name!("link");
        linkish && self.attr(&local_name!("href")).is_some()
    }

    /// Whether this element is the root element of its document, i.e. its parent is the
    /// document node (`:root`).
    pub(crate) fn is_root_element(self) -> bool {
        self.node().parent().is_some_and(NodeHandle::is_document)
    }

    /// Whether this element matches `:empty`: no child elements and no non-empty text
    /// (<https://drafts.csswg.org/selectors-4/#the-empty-pseudo>). Comments and processing
    /// instructions do not count.
    pub(crate) fn is_empty_element(self) -> bool {
        !self.node().children().any(|child| match child.kind() {
            Some(NodeKind::Element(_)) => true,
            Some(NodeKind::Text(text)) => !text.is_empty(),
            _ => false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{NodeHandle, TElement, TNode};
    use crate::handle::ElementHandle;
    use crate::handle::tests::{fixture, store};
    use cl_dom::local_name;
    use style::context::ThreadLocalStyleContext;

    /// stylo's sequential `traverse_dom` still parks its per-thread context in a
    /// `ScopedTLS<T: Send>` (`stylo-0.20.0/driver.rs:119`, `scoped_tls.rs:21`), so the gate
    /// for Task 13 is that `ThreadLocalStyleContext<ElementHandle>` is `Send` *without* the
    /// handle itself being `Send`. It is: every element stylo stores in there is wrapped in
    /// `SendElement`/`SendNode` (`stylo-0.20.0/dom.rs:1131..1156`). Asserting it here means
    /// Task 13 cannot be surprised by a missing `unsafe impl Send`.
    const _: fn() = || {
        fn assert_send<T: Send>() {}
        assert_send::<ThreadLocalStyleContext<ElementHandle<'static>>>();
    };

    #[test]
    #[allow(clippy::expect_used)]
    fn node_handle_should_walk_tree_links() {
        let f = fixture();
        let store = store(&f.doc);
        let handle = |id| NodeHandle::new(&f.doc, &store, id);

        let body = handle(f.body);
        let p = handle(f.p);
        let span = handle(f.span);
        let html = handle(f.html);

        assert_eq!(TNode::parent_node(&body), Some(html));
        assert_eq!(TNode::first_child(&body), Some(p));
        assert_eq!(TNode::last_child(&body), Some(span));
        assert_eq!(TNode::next_sibling(&p), Some(span));
        assert_eq!(TNode::prev_sibling(&span), Some(p));
        assert_eq!(TNode::prev_sibling(&p), None);
        assert_eq!(TNode::next_sibling(&span), None);

        // ... and they agree with the `Document` the handles wrap.
        for (handle, id) in [(body, f.body), (p, f.p), (span, f.span)] {
            let node = f.doc.get(id).expect("node in fixture");
            assert_eq!(TNode::parent_node(&handle).map(|h| h.id), node.parent());
            assert_eq!(
                TNode::first_child(&handle).map(|h| h.id),
                node.first_child()
            );
            assert_eq!(TNode::last_child(&handle).map(|h| h.id), node.last_child());
            assert_eq!(
                TNode::prev_sibling(&handle).map(|h| h.id),
                node.prev_sibling()
            );
            assert_eq!(
                TNode::next_sibling(&handle).map(|h| h.id),
                node.next_sibling()
            );
        }

        assert!(TNode::is_in_document(&p));
        assert_eq!(TNode::owner_doc(&p).node().id, f.doc.root());
        assert!(TNode::as_shadow_root(&p).is_none());
    }

    #[test]
    fn traversal_children_should_yield_elements_and_text_only() {
        let f = fixture();
        let store = store(&f.doc);
        let body = ElementHandle(NodeHandle::new(&f.doc, &store, f.body));
        let ids: Vec<_> = TElement::traversal_children(&body).map(|n| n.id).collect();
        assert_eq!(ids, vec![f.p, f.span]);
    }

    #[test]
    fn element_data_should_start_absent_and_survive_ensure() {
        let f = fixture();
        let store = store(&f.doc);
        let p = ElementHandle(NodeHandle::new(&f.doc, &store, f.p));

        assert!(!TElement::has_data(&p));
        assert!(TElement::borrow_data(&p).is_none());
        assert!(TElement::mutate_data(&p).is_none());

        // SAFETY: `TElement::ensure_data` is `unsafe` because it hands out an exclusive
        // borrow of the element's `ElementData` through a shared `&self`, so the caller
        // owes it (a) no other borrow of that slot is alive — this is the only borrow in
        // scope, and it is dropped on the same line — and (b) single-threaded access: the
        // `StyleStore` was created on this thread, which its own `debug_assert_eq!` on the
        // owning `ThreadId` re-checks from inside the call.
        drop(unsafe { TElement::ensure_data(&p) });

        assert!(TElement::has_data(&p));
        assert!(TElement::borrow_data(&p).is_some());
    }

    #[test]
    fn structural_predicates_should_match_the_dom() {
        let f = fixture();
        let store = store(&f.doc);
        let element = |id| ElementHandle(NodeHandle::new(&f.doc, &store, id));

        assert!(element(f.html).is_root_element());
        assert!(!element(f.body).is_root_element());
        // <p> contains the text "hi", <span> contains nothing at all.
        assert!(!element(f.p).is_empty_element());
        assert!(element(f.span).is_empty_element());
        assert!(!element(f.p).is_link_element());
        assert!(TElement::is_html_document_body_element(&element(f.body)));
        assert!(!TElement::is_html_document_body_element(&element(f.p)));
        assert_eq!(TElement::local_name(&element(f.p)), &local_name!("p"));
        assert!(TElement::is_html_element(&element(f.p)));
        assert!(!TElement::is_svg_element(&element(f.p)));
    }
}
