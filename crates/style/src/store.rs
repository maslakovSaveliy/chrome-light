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
//! The single-thread invariant ADR-0015 asks for is still enforced: [`StyleStore::new`]
//! records the creating thread and every mutating entry point below carries
//! `debug_assert_eq!(std::thread::current().id(), self.owner)`, so a future switch to
//! `traverse_dom(.., Some(pool))` trips in debug rather than silently racing on the
//! `Cell`s.
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
use style::shared_lock::SharedRwLock;

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
            selector_flags: Cell::new(ElementSelectorFlags::empty()),
            dirty_descendants: Cell::new(false),
            pending_children: Cell::new(0),
        }
    }
}

/// Per-pass storage for stylo's element state, indexed by [`NodeId`].
///
/// Created by `resolve()` with `Document::len()` slots and dropped when the pass ends.
/// Not `Sync` (it is full of `Cell`s) and deliberately so: the style traversal is
/// sequential, and `!Sync` makes that a compile-time fact for anything that tries to share
/// a `&StyleStore` across threads.
pub(crate) struct StyleStore {
    /// One slot per arena node, `slots[id.index()]`.
    slots: Box<[Slot]>,
    /// Slot handed out for a [`NodeId`] that is not in range.
    ///
    /// `TElement::ensure_data` must return an `ElementDataMut`, so slot lookup has to be
    /// total; there is no `Option` to give back and panicking is not allowed. A store is
    /// always built from the same `Document` the handles borrow, and that document cannot
    /// grow while the (shared) borrow is alive, so no real node ever lands here.
    scratch: Slot,
    /// The lock protecting every stylesheet reachable from the stylist, handed to stylo
    /// through `TDocument::shared_lock`.
    ///
    /// It lives here rather than in the handle so the handle stays the `Copy` triple
    /// `(&Document, &StyleStore, NodeId)` that ADR-0015 §1 specifies.
    lock: SharedRwLock,
    /// The thread that created this store; every mutating entry point debug-asserts
    /// against it (ADR-0015 §1, "sequential traversal only").
    owner: ThreadId,
}

impl StyleStore {
    /// Creates a store with `len` slots (use `Document::len()`), guarded by `lock`.
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

    /// The slot for `id`, or the scratch slot if `id` is out of range (see [`Self::scratch`]).
    fn slot(&self, id: NodeId) -> &Slot {
        self.slots.get(id.index()).unwrap_or(&self.scratch)
    }

    /// Marks `id` as having style data and hands out an exclusive borrow of it, creating
    /// the data if this is the first call (`TElement::ensure_data`).
    pub(crate) fn ensure_data(&self, id: NodeId) -> ElementDataMut<'_> {
        debug_assert_eq!(
            std::thread::current().id(),
            self.owner,
            "style traversal must stay on the thread that built the StyleStore"
        );
        let slot = self.slot(id);
        slot.present.set(true);
        slot.data.borrow_mut()
    }

    /// Drops `id`'s style data (`TElement::clear_data`).
    pub(crate) fn clear_data(&self, id: NodeId) {
        debug_assert_eq!(
            std::thread::current().id(),
            self.owner,
            "style traversal must stay on the thread that built the StyleStore"
        );
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
        debug_assert_eq!(
            std::thread::current().id(),
            self.owner,
            "style traversal must stay on the thread that built the StyleStore"
        );
        let slot = self.slot(id);
        slot.present.get().then(|| slot.data.borrow_mut())
    }

    /// `id`'s interned `id` attribute, computing it with `intern` on first call
    /// (`TElement::id`). `None` means the element has no `id` attribute.
    pub(crate) fn id_atom<F>(&self, id: NodeId, intern: F) -> Option<&Atom>
    where
        F: FnOnce() -> Option<Atom>,
    {
        self.slot(id).id_atom.get_or_init(intern).as_ref()
    }

    /// The selector flags accumulated on `id` so far.
    pub(crate) fn selector_flags(&self, id: NodeId) -> ElementSelectorFlags {
        self.slot(id).selector_flags.get()
    }

    /// Adds `flags` to `id`'s selector flags.
    pub(crate) fn insert_selector_flags(&self, id: NodeId, flags: ElementSelectorFlags) {
        debug_assert_eq!(
            std::thread::current().id(),
            self.owner,
            "style traversal must stay on the thread that built the StyleStore"
        );
        let slot = self.slot(id);
        slot.selector_flags.set(slot.selector_flags.get() | flags);
    }

    /// Whether some descendant of `id` still needs styling.
    pub(crate) fn has_dirty_descendants(&self, id: NodeId) -> bool {
        self.slot(id).dirty_descendants.get()
    }

    /// Sets or clears `id`'s "has dirty descendants" bit.
    pub(crate) fn set_dirty_descendants(&self, id: NodeId, dirty: bool) {
        debug_assert_eq!(
            std::thread::current().id(),
            self.owner,
            "style traversal must stay on the thread that built the StyleStore"
        );
        self.slot(id).dirty_descendants.set(dirty);
    }

    /// Records how many children of `id` the bottom-up traversal still has to process.
    pub(crate) fn store_children_to_process(&self, id: NodeId, n: isize) {
        debug_assert_eq!(
            std::thread::current().id(),
            self.owner,
            "style traversal must stay on the thread that built the StyleStore"
        );
        self.slot(id).pending_children.set(n);
    }

    /// Notes that one child of `id` finished, returning how many are left.
    pub(crate) fn did_process_child(&self, id: NodeId) -> isize {
        debug_assert_eq!(
            std::thread::current().id(),
            self.owner,
            "style traversal must stay on the thread that built the StyleStore"
        );
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

    #[test]
    fn out_of_range_ids_should_use_the_scratch_slot_instead_of_panicking() {
        let store = store(1);
        let outside = NodeId::from_index(9);

        assert!(!store.has_data(outside));
        drop(store.ensure_data(outside));
        assert!(store.has_data(outside));
        // A real node's slot is untouched by the out-of-range write.
        assert!(!store.has_data(NodeId::from_index(0)));
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
