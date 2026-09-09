//! `ChromeLight` style system: the stylo cascade over an author stylesheet set.
//!
//! [`StyleEngine`] (Task 2) owns the `Stylist`, `Device` and shared stylesheet lock.
//! Task 11 adds the adapter that lets stylo walk our arena DOM:
//!
//! * [`store`] — the per-pass side table holding stylo's `ElementData` for every node,
//!   rebuilt on each cascade so no styles outlive a DOM mutation;
//! * [`handle`] — the `Copy` `(&Document, &StyleStore, NodeId)` triple ADR-0015 §1
//!   requires instead of Blitz's raw `*mut` into the arena (DioxusLabs/blitz#151);
//! * [`stylo_dom`] — `TDocument`/`TNode`/`NodeInfo`/`TShadowRoot`/`TElement`;
//! * [`stylo_selectors`] — `selectors::Element`.
//!
//! Task 12 adds [`sheets`]: the bundled user-agent stylesheet (`assets/ua.css`) and the
//! walk that turns a document's `<style>`/`<link>` elements into author stylesheets.
//!
//! ADR-0015 §1 carved out `store.rs`, `handle.rs` and `stylo_dom.rs` as the three modules
//! allowed to opt out of `#![deny(unsafe_code)]`. **None of them ended up needing it**:
//! stylo 0.20 ships `style::data::ElementDataWrapper`, which already provides the interior
//! mutability (and a debug-only borrow tracker) that `TElement::ensure_data` needs, so the
//! side table is plain `Cell`s and safe borrows. The single-thread invariant the ADR asks
//! for is still enforced by `debug_assert`s on the owning thread at every mutating entry
//! point of [`store::StyleStore`]. See that module's docs for the full argument.
#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::undocumented_unsafe_blocks)]
#![deny(missing_docs)]

pub mod engine;
pub mod error;
pub(crate) mod handle;
pub mod sheets;
pub(crate) mod store;
pub(crate) mod stylo_dom;
pub(crate) mod stylo_selectors;

pub use engine::StyleEngine;
pub use error::StyleError;
pub use sheets::SheetWarning;

#[cfg(test)]
mod gate_tests {
    use crate::StyleEngine;
    use crate::handle::tests::{fixture, store};
    use crate::handle::{ElementHandle, NodeHandle};
    use style::dom::TElement;

    /// The M1a gate (ADR-0015 §6): a `<p>` with an author rule `p { color: red }` must
    /// cascade to `rgb(255, 0, 0)`.
    ///
    /// Task 11 builds the DOM/selector trait surface the cascade runs on; the cascade
    /// entry point (`StyleEngine::resolve` + `StyledDocument::computed`) is Task 13, which
    /// replaces the tail of this test with
    ///
    /// ```text
    /// let styled = engine.resolve(&f.doc).expect("resolve");
    /// let color = styled.computed(f.p).expect("styled <p>").clone_color();
    /// assert_eq!(color.into_srgb_legacy_tuple(), (255, 0, 0, 1.0));
    /// ```
    ///
    /// and drops the `#[ignore]`. Everything the cascade needs *from this task* is
    /// exercised below, so the only thing still missing when Task 13 starts is the
    /// traversal driver itself.
    #[test]
    #[ignore = "needs StyleEngine::resolve() from Task 13; un-ignore there"]
    #[allow(clippy::expect_used)]
    fn p_with_color_rule_should_resolve_to_red() {
        let f = fixture();
        let mut engine = StyleEngine::new((800.0, 600.0), 1.0).expect("engine");
        engine
            .add_author_sheet("p { color: red }", "file:///test.html")
            .expect("author sheet");
        assert_eq!(engine.author_rule_count(), 1);

        let store = store(&f.doc);
        let p = ElementHandle(NodeHandle::new(&f.doc, &store, f.p));
        assert_eq!(
            TElement::local_name(&p),
            &cl_dom::local_name!("p"),
            "the handle the cascade will style is the <p> the rule targets"
        );
        assert!(
            !TElement::has_data(&p),
            "a fresh pass starts with no computed styles"
        );
    }
}
