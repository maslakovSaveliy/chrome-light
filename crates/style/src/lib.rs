//! `ChromeLight` style system: the stylo cascade over an author stylesheet set.
//!
//! [`StyleEngine`] (Task 2) owns the `Stylist`, `Device` and shared stylesheet lock.
//! Task 11 adds the adapter that lets stylo walk our arena DOM:
//!
//! * [`store`] — the per-pass side table holding stylo's `ElementData` for every node,
//!   rebuilt on each cascade so no styles outlive a DOM mutation;
//! * [`handle`] — the `Copy` borrow-based handle ADR-0015 §1 requires instead of Blitz's raw
//!   `*mut` into the arena (DioxusLabs/blitz#151). Task 13 had to shrink it from the ADR's
//!   three-word `(&Document, &StyleStore, NodeId)` triple to a one-word reference into a
//!   per-pass slot arena, because stylo's style-sharing cache hard-asserts that an element
//!   handle is exactly `usize`-sized; that module's docs carry the evidence;
//! * [`stylo_dom`] — `TDocument`/`TNode`/`NodeInfo`/`TShadowRoot`/`TElement`;
//! * [`stylo_selectors`] — `selectors::Element`.
//!
//! Task 12 adds [`sheets`]: the bundled user-agent stylesheet (`assets/ua.css`) and the
//! walk that turns a document's `<style>`/`<link>` elements into author stylesheets.
//!
//! Task 13 adds [`traversal`] (the `DomTraversal` stylo's driver walks) and [`dump`], and
//! turns all of it on: `StyleEngine::resolve` builds a [`StyledDocument`] holding one
//! `ComputedValues` per element.
//!
//! ADR-0015 §1 carved out `store.rs`, `handle.rs` and `stylo_dom.rs` as the three modules
//! allowed to opt out of `#![deny(unsafe_code)]`. **None of them ended up needing it**:
//! stylo 0.20 ships `style::data::ElementDataWrapper`, which already provides the interior
//! mutability (and a debug-only borrow tracker) that `TElement::ensure_data` needs, so the
//! side table is plain `Cell`s and safe borrows. The single-thread invariant the ADR asks
//! for is enforced by a **hard** owning-thread `assert_eq!` at every mutating entry point of
//! [`store::StyleStore`] — `!Sync` proves nothing here, because stylo's `SendNode`/
//! `SendElement` are `Send` unconditionally. See that module's docs for the full argument.
#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::undocumented_unsafe_blocks)]
#![deny(missing_docs)]

pub mod dump;
pub mod engine;
pub mod error;
pub(crate) mod handle;
pub mod sheets;
pub(crate) mod store;
pub(crate) mod stylo_dom;
pub(crate) mod stylo_selectors;
pub(crate) mod traversal;

pub use engine::{StyleEngine, StyledDocument};
pub use error::StyleError;
pub use sheets::SheetWarning;

/// stylo's computed-value bundle for one element, re-exported so consumers can name the
/// type [`StyledDocument::computed`] hands back without depending on `stylo` themselves.
///
/// This is the *only* stylo type in this crate's public API (ADR-0015 §2, enforced by
/// `tools/check-stylo-scope.sh`): everything else — `servo_arc::Arc`, `ElementData`, the
/// `Stylist` — stays inside. `cl-layout` reads it through its single `style_adapt` module.
pub use style::properties::ComputedValues;

#[cfg(test)]
pub(crate) mod test_dom {
    //! Small DOM helpers shared by this crate's unit tests: parse a fragment of HTML with
    //! the real Task 8 pipeline (`cl-html`), then find a node in the result by tag name.
    //! Both are needed by tests in several modules (`lib.rs`'s gate test, `sheets.rs`'s UA
    //! test), which is why they live here rather than in either one.

    use cl_dom::{Document, LocalName, NodeId};
    use cl_net::Url;

    /// The base URL every test document is parsed and resolved against.
    pub(crate) const BASE_URL: &str = "file:///test.html";

    /// Parses `html` into a [`Document`] with `cl-html`, exactly the way the browser does.
    #[allow(clippy::expect_used)]
    pub(crate) fn parse(html: &str) -> Document {
        let base = Url::parse(BASE_URL).expect("base url");
        cl_html::parse_document_str(html, &base)
            .expect("parse never fails today")
            .document
    }

    /// The [`NodeId`] of the first element named `tag`, in document order.
    pub(crate) fn find(doc: &Document, tag: &str) -> Option<NodeId> {
        let local = LocalName::from(tag);
        doc.descendants(doc.root())
            .find(|id| doc.element(*id).is_some_and(|el| el.name.local == local))
    }
}

#[cfg(test)]
mod gate_tests {
    use crate::StyleEngine;
    use crate::test_dom::{BASE_URL, find, parse};
    use style::color::AbsoluteColor;
    use style::values::specified::Display;

    /// The M1a gate (ADR-0015 §6): a `<p>` with an author rule `p { color: red }` must
    /// cascade to `rgb(255, 0, 0)`.
    ///
    /// This is the whole static style pipeline end to end — `cl-html` parses the document,
    /// the UA sheet and one author sheet go into the `Stylist`, `resolve()` drives stylo's
    /// sequential traversal over the arena handles, and `computed()` reads the primary
    /// style back out.
    #[test]
    #[allow(clippy::expect_used)]
    fn p_with_color_rule_should_resolve_to_red() {
        let doc = parse("<p>hi</p>");
        let mut engine = StyleEngine::new((800.0, 600.0), 1.0).expect("engine");
        engine.add_ua_sheet().expect("ua sheet");
        engine
            .add_author_sheet("p { color: red }", BASE_URL)
            .expect("author sheet");
        assert_eq!(engine.author_rule_count(), 1);

        let styled = engine.resolve(doc).expect("resolve");
        let p = find(styled.document(), "p").expect("<p> in the parsed document");
        let color = styled.computed(p).expect("styled <p>").clone_color();
        assert_eq!(color, AbsoluteColor::srgb_legacy(255, 0, 0, 1.0));
    }

    /// A `<span>` with no rule of its own inherits `color` from its styled parent — the
    /// half of the cascade that a single-element document cannot exercise.
    #[test]
    #[allow(clippy::expect_used)]
    fn span_should_inherit_color_from_parent() {
        let doc = parse("<div><span>hi</span></div>");
        let mut engine = StyleEngine::new((800.0, 600.0), 1.0).expect("engine");
        engine.add_ua_sheet().expect("ua sheet");
        engine
            .add_author_sheet("div { color: rgb(0, 128, 0) }", BASE_URL)
            .expect("author sheet");

        let styled = engine.resolve(doc).expect("resolve");
        let div = find(styled.document(), "div").expect("<div>");
        let span = find(styled.document(), "span").expect("<span>");
        let green = AbsoluteColor::srgb_legacy(0, 128, 0, 1.0);
        assert_eq!(
            styled.computed(div).expect("styled <div>").clone_color(),
            green
        );
        assert_eq!(
            styled.computed(span).expect("styled <span>").clone_color(),
            green,
            "color is an inherited property, so the span takes the div's"
        );
    }

    /// `display: none` removes an element from layout, not from styling: stylo still
    /// cascades the element itself (it is only its *descendants* that are skipped), so
    /// `computed()` must hand back real values for it.
    #[test]
    #[allow(clippy::expect_used)]
    fn display_none_should_still_have_computed_values() {
        let doc = parse("<div id=hidden>x</div>");
        let mut engine = StyleEngine::new((800.0, 600.0), 1.0).expect("engine");
        engine.add_ua_sheet().expect("ua sheet");
        engine
            .add_author_sheet("#hidden { display: none; color: blue }", BASE_URL)
            .expect("author sheet");

        let styled = engine.resolve(doc).expect("resolve");
        let div = find(styled.document(), "div").expect("<div>");
        let values = styled.computed(div).expect("styled <div>");
        assert_eq!(values.get_box().clone_display(), Display::None);
        assert_eq!(
            values.clone_color(),
            AbsoluteColor::srgb_legacy(0, 0, 255, 1.0),
            "the rest of the cascade still runs for a display:none element"
        );
    }

    /// An inline `style` attribute beats an author rule of the same specificity, whatever
    /// the source order — that is the cascade's origin ordering, not just the fact that
    /// Task 11's `TElement::style_attribute` parses the attribute.
    #[test]
    #[allow(clippy::expect_used)]
    fn inline_style_should_win_over_an_author_rule_of_equal_specificity() {
        let doc = parse(r#"<p style="color: rgb(0, 0, 255)">hi</p>"#);
        let mut engine = StyleEngine::new((800.0, 600.0), 1.0).expect("engine");
        engine.add_ua_sheet().expect("ua sheet");
        engine
            .add_author_sheet("p { color: red }", BASE_URL)
            .expect("author sheet");

        let styled = engine.resolve(doc).expect("resolve");
        let p = find(styled.document(), "p").expect("<p>");
        assert_eq!(
            styled.computed(p).expect("styled <p>").clone_color(),
            AbsoluteColor::srgb_legacy(0, 0, 255, 1.0),
            "the style attribute cascades above a same-specificity author rule"
        );
    }

    /// Non-elements have no primary style, so `computed()` reports `None` for them rather
    /// than inventing one.
    #[test]
    #[allow(clippy::expect_used)]
    fn computed_should_be_none_for_non_elements() {
        let doc = parse("<p>hi</p>");
        let mut engine = StyleEngine::new((800.0, 600.0), 1.0).expect("engine");
        engine.add_ua_sheet().expect("ua sheet");
        let styled = engine.resolve(doc).expect("resolve");

        let root = styled.document().root();
        assert!(
            styled.computed(root).is_none(),
            "the document node is not an element"
        );
        let text = styled
            .document()
            .descendants(root)
            .find(|id| styled.document().element(*id).is_none() && *id != root)
            .expect("a non-element node");
        assert!(styled.computed(text).is_none());
    }

    /// `resolve()` must survive a document with no element at all (an empty fragment
    /// cannot happen through `cl-html`, but a directly-built `Document` can), and give
    /// back the document unchanged.
    #[test]
    #[allow(clippy::expect_used)]
    fn resolve_should_accept_a_document_with_no_root_element() {
        let doc = cl_dom::Document::new(BASE_URL);
        let len = doc.len();
        let mut engine = StyleEngine::new((800.0, 600.0), 1.0).expect("engine");
        engine.add_ua_sheet().expect("ua sheet");
        let styled = engine.resolve(doc).expect("resolve");
        assert_eq!(styled.into_document().len(), len);
    }
}
