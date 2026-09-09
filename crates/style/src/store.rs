//! The per-pass side table that holds stylo's `ElementData` (and the handful of other
//! per-element bits stylo expects to be able to set through a shared reference) for every
//! node of an arena [`cl_dom::Document`].
//!
//! # Why a side table
//!
//! `cl-dom` is `#![forbid(unsafe_code)]` and knows nothing about stylo (ADR-0015 §2), so
//! the cascade cannot hang its state off `cl_dom::Node`. Instead each `resolve()` builds
//! one `StyleStore` sized to `Document::len()`, indexed by the same [`NodeId`], and throws
//! it away afterwards — so no stale styles can survive a DOM mutation.
//!
//! # Why there is no `unsafe` here
//!
//! ADR-0015 §1 budgeted `Box<[UnsafeCell<Option<ElementData>>]>` for this table, because
//! `TElement::{ensure_data, clear_data, mutate_data}` hand out `&mut ElementData` through
//! a `&self`. stylo 0.20 no longer requires the embedder to build that interior
//! mutability itself: `style::data::ElementDataWrapper` (registry
//! `stylo-0.20.0/data.rs:306`) *is* the `UnsafeCell<ElementData>` plus, under
//! `debug_assertions`, an `AtomicRefCell<()>` borrow tracker, and `ElementDataMut` /
//! `ElementDataRef` have private fields so they can only be produced by
//! `ElementDataWrapper::{borrow, borrow_mut}`. Storing a wrapper per node and a separate
//! `Cell<bool>` "has data" flag therefore covers the whole trait surface in safe Rust,
//! with stylo's own debug borrow tracker catching an aliasing mistake instead of us
//! re-implementing (and having to re-audit) the same thing.
//!
//! # Why the single-thread rule is a runtime check, not a type
//!
//! ADR-0015 §1 requires sequential traversal only. It is tempting to read `StyleStore`'s
//! `!Sync` (it is full of `Cell`s) as a compile-time guarantee of that. **It is not, and
//! nothing in the type system enforces it here.** stylo declares
//!
//! ```text
//! unsafe impl<N: TNode> Send for SendNode<N> {}       // stylo-0.20.0/dom.rs:1132
//! unsafe impl<E: TElement> Send for SendElement<E> {} // stylo-0.20.0/dom.rs:1150
//! ```
//!
//! *unconditionally* — there is no `N: Send`/`E: Send` bound to fail — and
//! `driver::traverse_dom` wraps the root in `SendNode` on every call
//! (`stylo-0.20.0/driver.rs:122`). So `traverse_dom(.., Some(pool))` would compile against
//! our `!Sync` handles and then race on these `Cell`s, with no diagnostic at all in a
//! release build.
//!
//! The rule is therefore enforced where it can be: [`StyleStore::new`] records the creating
//! thread and every mutating entry point below calls [`StyleStore::assert_owner_thread`],
//! a **hard** `assert_eq!`. A mis-threaded style pass aborts instead of quietly corrupting
//! the table. It is not a `debug_assert!` precisely because the failure it guards is
//! invisible in release: `ElementDataWrapper`'s borrow tracker is itself
//! `#[cfg(debug_assertions)]` (ADR-0015 amendment, point 1). The assert can only fire on a
//! programming error inside this crate — passing a pool to `traverse_dom`, or handing a
//! store to another thread — never on document content, so it costs no
//! "panic on parser-reachable input".
//!
//! As in `handle.rs`, the store has no non-test constructor until Task 13's
//! `StyleEngine::resolve()`, hence the scoped `dead_code` allow.
#![cfg_attr(not(test), allow(dead_code))]

use std::cell::{Cell, OnceCell};
use std::thread::ThreadId;

use cl_dom::NodeId;
use selectors::matching::ElementSelectorFlags;
use style::Atom;
use style::data::{ElementData, ElementDataMut, ElementDataRef, ElementDataWrapper};
use style::properties::PropertyDeclarationBlock;
use style::servo_arc::Arc as StyloArc;
use style::shared_lock::{Locked, SharedRwLock};

/// Everything one style pass keeps for a single arena node.
///
/// Allocated for *every* node, not just elements: the table is indexed by raw
/// [`NodeId`], and one 40-ish-byte slot per text node is far cheaper than the indirection
/// a sparse map would cost on the hot matching path.
struct Slot {
    /// Whether `data` is logically present, i.e. whether `TElement::has_data` is true.
    ///
    /// `data` itself is always allocated (an empty `ElementData` is 24 bytes and cannot
    /// fail to construct); this flag is what distinguishes "never styled" from "styled".
    present: Cell<bool>,
    /// stylo's own container for `ElementData`, providing the interior mutability that
    /// `TElement::ensure_data` needs plus a debug-only borrow tracker.
    data: ElementDataWrapper,
    /// The node's `id` attribute, interned into a stylo atom on first use.
    ///
    /// `TElement::id` must return a `&WeakAtom` that outlives the call, and the DOM stores
    /// ids as `StrTendril`, so the interned atom needs somewhere to live. The document is
    /// immutable for the whole pass (handles hold `&Document`), so caching is sound and
    /// the `OnceCell` is never reset.
    id_atom: OnceCell<Option<Atom>>,
    /// The node's parsed `style="..."` attribute, on first use.
    ///
    /// Same reason as [`Slot::id_atom`]: `TElement::style_attribute` returns a borrow
    /// (`ArcBorrow<'_, Locked<PropertyDeclarationBlock>>`) that must outlive the call, so
    /// the parsed block needs an owner that lives for the whole pass. `None` means the
    /// element has no `style` attribute (or its base URL could not be resolved); an
    /// attribute whose value fails to parse yields an empty block, which is what the CSSOM
    /// says an invalid style attribute produces.
    style_attr: OnceCell<Option<StyloArc<Locked<PropertyDeclarationBlock>>>>,
    /// Selector flags accumulated by `selectors::Element::apply_selector_flags`.
    selector_flags: Cell<ElementSelectorFlags>,
    /// stylo's "some descendant needs restyling" bit.
    dirty_descendants: Cell<bool>,
    /// Counter used by stylo's *parallel* bottom-up traversal
    /// (`TElement::{store_children_to_process, did_process_child}`). Unused while we call
    /// `traverse_dom(.., None)`, but implemented rather than left to panic.
    pending_children: Cell<isize>,
}

impl Slot {
    /// A fresh, unstyled slot.
    fn new() -> Self {
        Self {
            present: Cell::new(false),
            data: ElementDataWrapper::default(),
            id_atom: OnceCell::new(),
            style_attr: OnceCell::new(),
            selector_flags: Cell::new(ElementSelectorFlags::empty()),
            dirty_descendants: Cell::new(false),
            pending_children: Cell::new(0),
        }
    }
}

/// Per-pass storage for stylo's element state, indexed by [`NodeId`].
///
/// Created by `resolve()` with `Document::len()` slots and dropped when the pass ends.
/// Not `Sync`, because it is full of `Cell`s — but see the module docs: `!Sync` buys no
/// guarantee against stylo's parallel traversal, so the sequential-only rule is enforced by
/// [`StyleStore::assert_owner_thread`] instead.
pub(crate) struct StyleStore {
    /// One slot per arena node, `slots[id.index()]`.
    slots: Box<[Slot]>,
    /// Slot handed out for a [`NodeId`] that is not in range.
    ///
    /// `TElement::ensure_data` must return an `ElementDataMut`, so slot lookup has to be
    /// total; there is no `Option` to give back and panicking is not allowed. A store is
    /// always built from the same `Document` the handles borrow, and that document cannot
    /// grow while the (shared) borrow is alive, so no real node ever lands here.
    ///
    /// It is a *fallback*, not a valid destination: if several out-of-range ids reached it
    /// at once they would alias one `ElementDataWrapper`, which is UB in release (the
    /// borrow tracker that catches it is `#[cfg(debug_assertions)]`). Two checks keep that
    /// unreachable — [`StyleStore::slot`] debug-asserts the index is in range, and
    /// [`crate::handle::NodeHandle::new`] asserts the store was sized for the very document
    /// the handle borrows.
    scratch: Slot,
    /// The lock protecting every stylesheet reachable from the stylist, handed to stylo
    /// through `TDocument::shared_lock`.
    ///
    /// It lives here rather than in the handle so the handle stays the `Copy` triple
    /// `(&Document, &StyleStore, NodeId)` that ADR-0015 §1 specifies.
    lock: SharedRwLock,
    /// The thread that created this store; every mutating entry point asserts against it
    /// (ADR-0015 §1, "sequential traversal only" — see the module docs for why this has to
    /// be a runtime check).
    owner: ThreadId,
}

impl StyleStore {
    /// Creates a store with `len` slots (use `Document::len()`), guarded by `lock`.
    ///
    /// `len` **must** be the `Document::len()` of the very document the pass will walk:
    /// [`crate::handle::NodeHandle::new`] asserts it, because a store sized from a stale
    /// length collapses distinct elements onto [`StyleStore::scratch`].
    ///
    /// `lock` **must** be the engine's `SharedRwLock` — the one whose guards the cascade
    /// reads stylesheets through. [`StyleStore::style_attr`] wraps each parsed inline style
    /// in it, and `Locked::read_with` panics when handed a guard from an unrelated lock
    /// (`stylo-0.20.0/shared_lock.rs:139`), so Task 13's `resolve()` has to pass
    /// `engine.lock.clone()` rather than a fresh `SharedRwLock::new()`.
    pub(crate) fn new(len: usize, lock: SharedRwLock) -> Self {
        Self {
            slots: (0..len).map(|_| Slot::new()).collect(),
            scratch: Slot::new(),
            lock,
            owner: std::thread::current().id(),
        }
    }

    /// The shared stylesheet lock, for `TDocument::shared_lock`.
    pub(crate) fn lock(&self) -> &SharedRwLock {
        &self.lock
    }

    /// How many slots this store was built for, i.e. the `Document::len()` passed to
    /// [`StyleStore::new`].
    ///
    /// Exists so [`crate::handle::NodeHandle::new`] can check that a store and a document
    /// really belong together before any id is used to index the table.
    pub(crate) fn slot_count(&self) -> usize {
        self.slots.len()
    }

    /// Aborts unless the caller is running on the thread that built this store.
    ///
    /// A hard `assert_eq!` rather than a `debug_assert_eq!`: the race it guards against
    /// (see the module docs — stylo's `SendNode`/`SendElement` are `Send` unconditionally,
    /// so a parallel `traverse_dom` compiles) is undetectable in a release build, because
    /// `ElementDataWrapper`'s own borrow tracker only exists under `debug_assertions`. It
    /// can only fire on a programming error in this crate, never on document content, so it
    /// does not make any parser-reachable input panic.
    #[track_caller]
    fn assert_owner_thread(&self) {
        assert_eq!(
            std::thread::current().id(),
            self.owner,
            "style traversal must stay on the thread that built the StyleStore"
        );
    }

    /// The slot for `id`, or the scratch slot if `id` is out of range (see [`Self::scratch`]).
    #[track_caller]
    fn slot(&self, id: NodeId) -> &Slot {
        // The scratch slot keeps lookup total, but reaching it always means a bug: a store
        // built for a stale `Document::len()` would funnel several *real* elements onto one
        // `ElementDataWrapper` and alias it. Debug builds say so loudly; release builds
        // degrade to one shared slot rather than an out-of-bounds panic.
        debug_assert!(
            id.index() < self.slots.len(),
            "NodeId {} is outside a StyleStore built for {} nodes — the store and the \
             document have drifted apart",
            id.index(),
            self.slots.len()
        );
        self.slots.get(id.index()).unwrap_or(&self.scratch)
    }

    /// Marks `id` as having style data and hands out an exclusive borrow of it, creating
    /// the data if this is the first call (`TElement::ensure_data`).
    pub(crate) fn ensure_data(&self, id: NodeId) -> ElementDataMut<'_> {
        self.assert_owner_thread();
        let slot = self.slot(id);
        slot.present.set(true);
        slot.data.borrow_mut()
    }

    /// Drops `id`'s style data (`TElement::clear_data`).
    pub(crate) fn clear_data(&self, id: NodeId) {
        self.assert_owner_thread();
        let slot = self.slot(id);
        slot.present.set(false);
        *slot.data.borrow_mut() = ElementData::default();
    }

    /// Whether `id` currently has style data (`TElement::has_data`).
    pub(crate) fn has_data(&self, id: NodeId) -> bool {
        self.slot(id).present.get()
    }

    /// Shared borrow of `id`'s style data, or `None` if it has none.
    ///
    /// Also the accessor a finished pass reads computed values through: the returned
    /// guard keeps stylo's debug borrow tracker honest, so reading while some
    /// `ElementDataMut` is still outstanding fails loudly in debug builds instead of
    /// aliasing silently.
    pub(crate) fn get(&self, id: NodeId) -> Option<ElementDataRef<'_>> {
        let slot = self.slot(id);
        slot.present.get().then(|| slot.data.borrow())
    }

    /// Exclusive borrow of `id`'s style data, or `None` if it has none
    /// (`TElement::mutate_data`).
    pub(crate) fn get_mut(&self, id: NodeId) -> Option<ElementDataMut<'_>> {
        self.assert_owner_thread();
        let slot = self.slot(id);
        slot.present.get().then(|| slot.data.borrow_mut())
    }

    /// `id`'s interned `id` attribute, computing it with `intern` on first call
    /// (`TElement::id`). `None` means the element has no `id` attribute.
    pub(crate) fn id_atom<F>(&self, id: NodeId, intern: F) -> Option<&Atom>
    where
        F: FnOnce() -> Option<Atom>,
    {
        // `OnceCell::get_or_init` *writes* through `&self` on the first call, so this is a
        // mutating entry point like the rest — and a hot one, reached from `TElement::id`
        // on every selector match.
        self.assert_owner_thread();
        self.slot(id).id_atom.get_or_init(intern).as_ref()
    }

    /// `id`'s parsed `style="..."` attribute (`TElement::style_attribute`), produced by
    /// `parse` on first call and wrapped in this store's [`SharedRwLock`] so the cascade can
    /// read it through the same guard as every stylesheet.
    ///
    /// `parse` returning `None` means "this element has no usable style attribute" and is
    /// cached as such: it runs at most once per node per pass.
    pub(crate) fn style_attr<F>(
        &self,
        id: NodeId,
        parse: F,
    ) -> Option<&StyloArc<Locked<PropertyDeclarationBlock>>>
    where
        F: FnOnce() -> Option<PropertyDeclarationBlock>,
    {
        // As in `id_atom`: the first call writes through `&self`.
        self.assert_owner_thread();
        self.slot(id)
            .style_attr
            .get_or_init(|| parse().map(|block| StyloArc::new(self.lock.wrap(block))))
            .as_ref()
    }

    /// The selector flags accumulated on `id` so far.
    pub(crate) fn selector_flags(&self, id: NodeId) -> ElementSelectorFlags {
        self.slot(id).selector_flags.get()
    }

    /// Adds `flags` to `id`'s selector flags.
    pub(crate) fn insert_selector_flags(&self, id: NodeId, flags: ElementSelectorFlags) {
        self.assert_owner_thread();
        let slot = self.slot(id);
        slot.selector_flags.set(slot.selector_flags.get() | flags);
    }

    /// Whether some descendant of `id` still needs styling.
    pub(crate) fn has_dirty_descendants(&self, id: NodeId) -> bool {
        self.slot(id).dirty_descendants.get()
    }

    /// Sets or clears `id`'s "has dirty descendants" bit.
    pub(crate) fn set_dirty_descendants(&self, id: NodeId, dirty: bool) {
        self.assert_owner_thread();
        self.slot(id).dirty_descendants.set(dirty);
    }

    /// Records how many children of `id` the bottom-up traversal still has to process.
    pub(crate) fn store_children_to_process(&self, id: NodeId, n: isize) {
        self.assert_owner_thread();
        self.slot(id).pending_children.set(n);
    }

    /// Notes that one child of `id` finished, returning how many are left.
    pub(crate) fn did_process_child(&self, id: NodeId) -> isize {
        self.assert_owner_thread();
        let slot = self.slot(id);
        // Clamped at zero: `isize::saturating_sub` alone would bottom out at `isize::MIN`,
        // and "fewer than zero children left" is not a state stylo can act on.
        let left = slot.pending_children.get().saturating_sub(1).max(0);
        slot.pending_children.set(left);
        left
    }
}

#[cfg(test)]
mod tests {
    use super::StyleStore;
    use cl_dom::NodeId;
    use style::shared_lock::SharedRwLock;

    fn store(len: usize) -> StyleStore {
        StyleStore::new(len, SharedRwLock::new())
    }

    #[test]
    fn store_slot_should_be_none_before_ensure_data() {
        let store = store(4);
        let id = NodeId::from_index(2);

        assert!(!store.has_data(id));
        assert!(store.get(id).is_none());

        drop(store.ensure_data(id));

        assert!(store.has_data(id));
        assert!(store.get(id).is_some());
    }

    #[test]
    fn clear_data_should_forget_the_slot_again() {
        let store = store(4);
        let id = NodeId::from_index(0);
        drop(store.ensure_data(id));
        store.clear_data(id);

        assert!(!store.has_data(id));
        assert!(store.get(id).is_none());
    }

    /// An out-of-range id means the store and the document have drifted apart, which would
    /// funnel unrelated elements onto the one shared scratch slot. Debug builds must say so
    /// rather than degrade quietly; release builds still cannot panic or index out of
    /// bounds, which is what the scratch slot is for.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "outside a StyleStore built for")]
    fn out_of_range_ids_should_trip_the_bounds_assert() {
        let store = store(1);
        let _ = store.has_data(NodeId::from_index(9));
    }

    #[test]
    fn slot_count_should_report_the_length_the_store_was_built_for() {
        assert_eq!(store(7).slot_count(), 7);
    }

    #[test]
    fn style_attr_should_be_parsed_once_and_cached() {
        let store = store(2);
        let id = NodeId::from_index(1);
        let calls = std::cell::Cell::new(0);
        let count = || {
            calls.set(calls.get() + 1);
            None
        };
        assert!(store.style_attr(id, count).is_none());
        assert!(store.style_attr(id, count).is_none());
        assert_eq!(
            calls.get(),
            1,
            "the parse closure runs at most once per node"
        );
    }

    #[test]
    fn pending_children_should_count_down_without_underflow() {
        let store = store(2);
        let id = NodeId::from_index(1);
        store.store_children_to_process(id, 2);
        assert_eq!(store.did_process_child(id), 1);
        assert_eq!(store.did_process_child(id), 0);
        assert_eq!(store.did_process_child(id), 0);
    }
}
