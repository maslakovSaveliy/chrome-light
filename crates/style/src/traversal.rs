//! The style traversal itself: the `style::traversal::DomTraversal` implementation
//! `style::driver::traverse_dom` drives over [`crate::handle::ElementHandle`].
//!
//! The shape is Gecko's `RecalcStyleOnly` (registry
//! `stylo-0.20.0/gecko/traversal.rs:14..53`) rather than Servo's `RecalcStyle`: we want
//! *only* the top-down cascade, with no post-order pass, because M1a builds its box tree
//! separately in `cl-layout` (Task 16) instead of during the style walk.
//!
//! Trait surface, verified against `stylo-0.20.0/traversal.rs`:
//!
//! * `DomTraversal<E: TElement>: Sync` (line 57) — `process_preorder` (62),
//!   `process_postorder` (74), `needs_postorder_traversal` (80, defaults to `true`),
//!   `pre_traverse` (140) and `shared_context` (234).
//! * `recalc_style_at(traversal, traversal_data, context, element, data, note_child)`
//!   (line 352) is what actually matches and cascades one element.
//!
//! # Sequential only
//!
//! `style::driver::traverse_dom(&traversal, token, None)` — the `None` is the rayon thread
//! pool, and it is mandatory here, not an optimisation choice. `with_pool_in_place_scope`
//! (`stylo-0.20.0/driver.rs:50`) runs the whole traversal inline on the calling thread when
//! the pool is `None`, and `parallel::style_trees` is then handed `scope: None` so it can
//! never distribute work. That is what makes [`crate::store::StyleStore`]'s `Cell`s sound;
//! see that module's docs for why the type system cannot enforce it and the store's
//! owning-thread `assert` has to.
//!
//! # No `unsafe` here
//!
//! Gecko's version calls `unsafe { el.ensure_data() }`, the `TElement` method. This module
//! is outside ADR-0015 §1's `unsafe` carve-out (`tools/check-unsafe-scope.sh` allows only
//! `store.rs`, `handle.rs` and `stylo_dom.rs`), so it goes straight to the safe
//! [`crate::store::StyleStore::ensure_data`] that `TElement::ensure_data` itself forwards
//! to. Same effect, no `unsafe` outside the carve-out.

use style::context::{SharedStyleContext, StyleContext};
use style::traversal::{DomTraversal, PerLevelTraversalData, recalc_style_at};

use crate::handle::{ElementHandle, NodeHandle};

/// A top-down-only style traversal over one document.
///
/// Owns the [`SharedStyleContext`] for the pass; `style::driver::traverse_dom` borrows it
/// back out through [`DomTraversal::shared_context`] for every element it visits.
pub(crate) struct RecalcStyle<'a> {
    /// The stylist, guards, snapshot map and traversal flags shared by the whole pass.
    shared: SharedStyleContext<'a>,
}

impl<'a> RecalcStyle<'a> {
    /// Wraps `shared` in a traversal ready to hand to `style::driver::traverse_dom`.
    pub(crate) fn new(shared: SharedStyleContext<'a>) -> Self {
        Self { shared }
    }
}

impl<'a, 'dom> DomTraversal<ElementHandle<'dom>> for RecalcStyle<'a> {
    fn process_preorder<F>(
        &self,
        traversal_data: &PerLevelTraversalData,
        context: &mut StyleContext<'_, ElementHandle<'dom>>,
        node: NodeHandle<'dom>,
        note_child: F,
    ) where
        F: FnMut(NodeHandle<'dom>),
    {
        // Text nodes reach us because `TElement::traversal_children` yields them (stylo
        // needs them for `:empty` and text-node damage); they have no style of their own.
        let Some(element) = node.as_element() else {
            return;
        };
        // `recalc_style_at` takes `&mut ElementData`, so the slot has to exist before the
        // call. This is `TElement::ensure_data`'s own body, minus the `unsafe fn` wrapper —
        // see the module docs.
        let mut data = node.store.ensure_data(node.id);
        recalc_style_at(
            self,
            traversal_data,
            context,
            element,
            &mut data,
            note_child,
        );
    }

    fn process_postorder(
        &self,
        _context: &mut StyleContext<'_, ElementHandle<'dom>>,
        _node: NodeHandle<'dom>,
    ) {
        // Unreachable: `needs_postorder_traversal` is false, so
        // `DomTraversal::handle_postorder_traversal` (`stylo-0.20.0/traversal.rs:105`)
        // returns before it can call this. Gecko writes `unreachable!()` here; a no-op
        // keeps the crate's "no reachable panic" rule true by construction instead of by
        // argument.
    }

    fn needs_postorder_traversal() -> bool {
        // M1a builds no box tree during styling (that is `cl-layout`, Task 16), so there is
        // nothing to bubble up.
        false
    }

    fn shared_context(&self) -> &SharedStyleContext<'a> {
        &self.shared
    }
}
