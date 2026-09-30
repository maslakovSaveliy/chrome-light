//! `selectors::Element` for [`ElementHandle`], against stylo's
//! `style::selector_parser::SelectorImpl`.
//!
//! The trait is defined in the vendored registry copy at `selectors-0.40.0/tree.rs:46`.
//! `SelectorImpl`'s associated types (`stylo-0.20.0/servo/selector_parser.rs:568..578`)
//! are what makes this fit our DOM almost for free:
//! `BorrowedLocalName`/`BorrowedNamespaceUrl` are `web_atoms::{LocalName, Namespace}`,
//! i.e. exactly the `markup5ever` atoms `cl_dom::QualName` already stores, so tag and
//! namespace matching is a pointer-cheap atom comparison with no conversion. `Identifier`
//! and `AttrValue` are stylo's own atom set, so ids and classes are interned on the way in.
//!
//! Selector support in M1a: type/universal, `#id`, `.class`, `[attr]` with every operator
//! `selectors` implements, the combinators, and the tree-structural pseudo-classes
//! `:root`, `:first-child`, `:last-child`, `:only-child`, `:empty` and `:nth-*` — the
//! last group falls out of [`Element::prev_sibling_element`] /
//! [`Element::next_sibling_element`] and needs no code here. `:not()`, `:is()` and
//! `:where()` are implemented inside `selectors` itself. Of the non-tree-structural
//! pseudo-classes only `:link` and `:any-link` match (any HTML `<a>`/`<area>`/`<link>`
//! with an `href`); every other one returns `false` — see
//! [`Element::match_non_ts_pseudo_class`] for the list and why that is the right answer
//! for a static, non-interactive pipeline.

use std::ptr::NonNull;

use selectors::attr::{AttrSelectorOperation, CaseSensitivity, NamespaceConstraint};
use selectors::bloom::{BLOOM_HASH_MASK, BloomFilter};
use selectors::matching::{ElementSelectorFlags, MatchingContext};
use selectors::{Element, OpaqueElement};
use style::CaseSensitivityExt;
use style::bloom::each_relevant_element_hash;
use style::dom::TElement;
use style::selector_parser::{NonTSPseudoClass, PseudoElement, SelectorImpl};
use style::values::{AtomIdent, AtomString};
use style::{Atom, LocalName as StyloLocalName, Namespace as StyloNamespace};

use crate::handle::{ElementHandle, NodeHandle};

impl Element for ElementHandle<'_> {
    type Impl = SelectorImpl;

    fn opaque(&self) -> OpaqueElement {
        // `OpaqueElement` is a `NonNull<()>` used purely as an identity token — it is never
        // dereferenced (`selectors-0.40.0/tree.rs:17`, and recovering the pointer needs the
        // `unsafe fn as_const_ptr`, which we never call). Node ids are unique within the
        // document, so `index + 1` is a perfectly good token and, unlike Blitz's approach of
        // handing out a real `&Node` address, it cannot be confused for a live borrow.
        //
        // `+ 1` keeps it non-null for the root node; `NonNull::dangling()` is the
        // never-taken fallback that keeps this total instead of `unwrap`ping.
        let token = self.node().id.index().wrapping_add(1) as *mut ();
        OpaqueElement::from_non_null_ptr(NonNull::new(token).unwrap_or(NonNull::dangling()))
    }

    fn parent_element(&self) -> Option<Self> {
        self.node().parent().and_then(NodeHandle::as_element)
    }

    fn parent_node_is_shadow_root(&self) -> bool {
        false
    }

    fn containing_shadow_host(&self) -> Option<Self> {
        None
    }

    fn is_pseudo_element(&self) -> bool {
        // M1a generates no pseudo-element boxes, so no handle ever stands for one.
        false
    }

    fn prev_sibling_element(&self) -> Option<Self> {
        ElementHandle::prev_sibling_element(*self)
    }

    fn next_sibling_element(&self) -> Option<Self> {
        ElementHandle::next_sibling_element(*self)
    }

    fn first_element_child(&self) -> Option<Self> {
        ElementHandle::first_element_child(*self)
    }

    fn is_html_element_in_html_document(&self) -> bool {
        // Every document we style is an HTML document (`TDocument::is_html_document`), so
        // this reduces to "is the element in the HTML namespace" — which is what decides
        // whether tag names and attribute names match ASCII-case-insensitively.
        self.name().ns == cl_dom::ns!(html)
    }

    fn has_local_name(&self, local_name: &cl_dom::LocalName) -> bool {
        self.name().local == *local_name
    }

    fn has_namespace(&self, ns: &cl_dom::Namespace) -> bool {
        self.name().ns == *ns
    }

    fn is_same_type(&self, other: &Self) -> bool {
        let (this, that) = (self.name(), other.name());
        this.local == that.local && this.ns == that.ns
    }

    fn attr_matches(
        &self,
        ns: &NamespaceConstraint<&StyloNamespace>,
        local_name: &StyloLocalName,
        operation: &AttrSelectorOperation<&AtomString>,
    ) -> bool {
        let Some(element) = self.element() else {
            return false;
        };
        element.attrs.iter().any(|attr| {
            if attr.name.local != local_name.0 {
                return false;
            }
            let namespace_matches = match ns {
                NamespaceConstraint::Any => true,
                NamespaceConstraint::Specific(url) => attr.name.ns == url.0,
            };
            namespace_matches && operation.eval_str(&attr.value)
        })
    }

    fn match_non_ts_pseudo_class(
        &self,
        pc: &NonTSPseudoClass,
        _context: &mut MatchingContext<'_, Self::Impl>,
    ) -> bool {
        match *pc {
            // The only element state M1a tracks. A link is always unvisited (we keep no
            // history), so `:link` and `:any-link` agree and `:visited` never matches —
            // which is also the privacy-safe answer.
            NonTSPseudoClass::Link | NonTSPseudoClass::AnyLink => self.is_link_element(),
            // Everything else needs state this pipeline does not have: user interaction
            // (`:hover`, `:active`, `:focus*`), form state (`:checked`, `:disabled`,
            // `:enabled`, `:required`, `:valid`, `:in-range`, ...), navigation
            // (`:visited`, `:target`), custom elements (`:defined`, `:state()`) or
            // dialog/popover state (`:modal`, `:open`, `:fullscreen`, `:popover-open`).
            // Reporting `false` makes those selectors simply not apply, which is the
            // correct rendering of a static document with no interaction.
            _ => false,
        }
    }

    fn match_pseudo_element(
        &self,
        _pe: &PseudoElement,
        _context: &mut MatchingContext<'_, Self::Impl>,
    ) -> bool {
        // No pseudo-element boxes are generated in M1a, so nothing can match `::before`,
        // `::after`, `::selection`, ... `is_pseudo_element` is `false` for the same reason.
        false
    }

    fn apply_selector_flags(&self, flags: ElementSelectorFlags) {
        let node = self.node();
        let self_flags = flags.for_self();
        if !self_flags.is_empty() {
            node.store.insert_selector_flags(node.id, self_flags);
        }

        let parent_flags = flags.for_parent();
        if !parent_flags.is_empty()
            && let Some(parent) = node.parent()
        {
            parent.store.insert_selector_flags(parent.id, parent_flags);
        }
    }

    fn is_link(&self) -> bool {
        self.is_link_element()
    }

    fn is_html_slot_element(&self) -> bool {
        false
    }

    fn has_id(&self, id: &AtomIdent, case_sensitivity: CaseSensitivity) -> bool {
        // Goes through `TElement::id` so the element's own id is interned once per pass
        // rather than on every candidate rule.
        TElement::id(self).is_some_and(|own| case_sensitivity.eq_atom(own, &id.0))
    }

    fn has_class(&self, name: &AtomIdent, case_sensitivity: CaseSensitivity) -> bool {
        let Some(class) = self.attr(&cl_dom::local_name!("class")) else {
            return false;
        };
        class
            .split_ascii_whitespace()
            .any(|candidate| case_sensitivity.eq_atom(&Atom::from(candidate), &name.0))
    }

    fn has_custom_state(&self, _name: &AtomIdent) -> bool {
        false
    }

    fn imported_part(&self, _name: &AtomIdent) -> Option<AtomIdent> {
        None
    }

    fn is_part(&self, _name: &AtomIdent) -> bool {
        false
    }

    fn is_empty(&self) -> bool {
        self.is_empty_element()
    }

    fn is_root(&self) -> bool {
        self.is_root_element()
    }

    fn add_element_unique_hashes(&self, filter: &mut BloomFilter) -> bool {
        // stylo already knows which of an element's atoms are worth hashing (tag,
        // namespace, id, classes, non-excluded attribute names) —
        // `stylo-0.20.0/bloom.rs:115`.
        each_relevant_element_hash(*self, |hash| filter.insert_hash(hash & BLOOM_HASH_MASK));
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{AtomIdent, Element, ElementHandle};
    use crate::handle::NodeArena;
    use crate::handle::tests::{element as element_kind, element_handle, fixture, store};
    use cl_dom::{Document, NodeId, local_name};
    use selectors::attr::CaseSensitivity::{AsciiCaseInsensitive, CaseSensitive};
    use selectors::context::{
        MatchingForInvalidation, MatchingMode, NeedsSelectorFlags, SelectorCaches,
    };
    use selectors::matching::{MatchingContext, QuirksMode, matches_selector_list};
    use style::dom::TElement;
    use style::selector_parser::SelectorParser;
    use style::stylesheets::UrlExtraData;

    /// Runs `selector` against `handle` through the real stylo parser and the real
    /// `selectors` matching engine, in `quirks` mode.
    #[allow(clippy::expect_used)]
    fn matches_in(selector: &str, handle: ElementHandle<'_>, quirks: QuirksMode) -> bool {
        let url_data = UrlExtraData::from(url::Url::parse("file:///test.html").expect("base url"));
        let list = SelectorParser::parse_author_origin_no_namespace(selector, &url_data)
            .expect("selector parses");
        let mut caches = SelectorCaches::default();
        let mut context = MatchingContext::new(
            MatchingMode::Normal,
            None,
            &mut caches,
            quirks,
            NeedsSelectorFlags::No,
            MatchingForInvalidation::No,
        );
        matches_selector_list(&list, &handle, &mut context)
    }

    /// `<html><a href="#x" id="Top" class="Nav Bar" title="Hello" lang="en-GB"
    /// data-flag=""></a><a></a></html>` — everything the attribute and `:link` cases need,
    /// kept out of [`fixture`] so its own shape assertions stay valid.
    #[allow(clippy::expect_used)]
    fn attr_doc() -> (Document, NodeId, NodeId) {
        let mut doc = Document::new("file:///test.html");
        let root = doc.root();
        let html = doc.create(element_kind("html", &[]));
        let link = doc.create(element_kind(
            "a",
            &[
                ("href", "#x"),
                ("id", "Top"),
                ("class", "Nav Bar"),
                ("title", "Hello"),
                ("lang", "en-GB"),
                ("data-flag", ""),
            ],
        ));
        let anchor = doc.create(element_kind("a", &[]));
        doc.append_child(root, html).expect("append html");
        doc.append_child(html, link).expect("append link");
        doc.append_child(html, anchor).expect("append anchor");
        (doc, link, anchor)
    }

    #[test]
    fn element_handle_should_expose_tag_id_and_classes() {
        let f = fixture();
        let store = store(&f.doc);
        let arena = NodeArena::new(&f.doc, &store);
        let p = element_handle(&arena, f.p);

        assert_eq!(TElement::local_name(&p), &local_name!("p"));
        assert!(p.has_local_name(&local_name!("p")));
        assert!(!p.has_local_name(&local_name!("div")));

        assert!(p.has_id(&AtomIdent::from("a"), CaseSensitive));
        assert!(!p.has_id(&AtomIdent::from("b"), CaseSensitive));

        assert!(p.has_class(&AtomIdent::from("x"), CaseSensitive));
        assert!(p.has_class(&AtomIdent::from("y"), CaseSensitive));
        assert!(!p.has_class(&AtomIdent::from("z"), CaseSensitive));
    }

    #[test]
    fn structural_pseudo_classes_should_follow_the_dom() {
        let f = fixture();
        let store = store(&f.doc);
        let arena = NodeArena::new(&f.doc, &store);
        let element = |id| element_handle(&arena, id);

        assert!(element(f.html).is_root());
        assert!(!element(f.body).is_root());
        assert!(element(f.span).is_empty());
        assert!(!element(f.p).is_empty());
        assert_eq!(element(f.p).next_sibling_element(), Some(element(f.span)));
        assert_eq!(element(f.span).prev_sibling_element(), Some(element(f.p)));
        assert!(!element(f.p).is_link());
    }

    /// Drives a real stylo-parsed selector list through the `selectors::Element` impl.
    /// This is the piece Task 13's cascade sits directly on top of, so proving it here
    /// de-risks the gate before `resolve()` exists.
    #[test]
    #[allow(clippy::expect_used)]
    fn stylo_parsed_selectors_should_match_through_the_handle() {
        let f = fixture();
        let store = store(&f.doc);
        let arena = NodeArena::new(&f.doc, &store);
        let element = |id| element_handle(&arena, id);
        let url_data = UrlExtraData::from(url::Url::parse("file:///test.html").expect("base url"));

        let mut caches = SelectorCaches::default();
        let mut matches = |selector: &str, handle: ElementHandle<'_>| {
            let list = SelectorParser::parse_author_origin_no_namespace(selector, &url_data)
                .expect("selector parses");
            let mut context = MatchingContext::new(
                MatchingMode::Normal,
                None,
                &mut caches,
                QuirksMode::NoQuirks,
                NeedsSelectorFlags::No,
                MatchingForInvalidation::No,
            );
            matches_selector_list(&list, &handle, &mut context)
        };

        assert!(matches("p", element(f.p)));
        assert!(!matches("div", element(f.p)));
        assert!(matches("#a", element(f.p)));
        assert!(matches(".x", element(f.p)));
        assert!(matches(".x.y", element(f.p)));
        assert!(!matches(".z", element(f.p)));
        assert!(matches("p[id=a]", element(f.p)));
        assert!(matches("body > p", element(f.p)));
        assert!(matches("html p", element(f.p)));
        assert!(matches("p + span", element(f.span)));
        assert!(matches(":root", element(f.html)));
        assert!(matches("span:empty", element(f.span)));
        assert!(matches("p:first-child", element(f.p)));
        assert!(matches("span:last-child", element(f.span)));
        assert!(!matches("p:only-child", element(f.p)));
        assert!(matches("p:not(.z)", element(f.p)));
        assert!(matches(":is(p, div)", element(f.p)));
        assert!(matches(":where(.x)", element(f.p)));
        assert!(!matches("a:any-link", element(f.p)));
    }

    /// The one non-tree-structural pseudo-class M1a implements, from the matching side:
    /// an `<a href>` must match `:link` *and* `:any-link` (it is always unvisited), and a
    /// bare `<a>` must match neither.
    #[test]
    fn link_pseudo_classes_should_match_an_anchor_with_href() {
        let (doc, link, anchor) = attr_doc();
        let store = store(&doc);
        let arena = NodeArena::new(&doc, &store);
        let handle = |id| element_handle(&arena, id);

        assert!(handle(link).is_link(), "<a href> is a link");
        assert!(!handle(anchor).is_link(), "<a> without href is not");

        for selector in ["a:link", "a:any-link", ":any-link"] {
            assert!(
                matches_in(selector, handle(link), QuirksMode::NoQuirks),
                "{selector} should match <a href>"
            );
            assert!(
                !matches_in(selector, handle(anchor), QuirksMode::NoQuirks),
                "{selector} should not match a bare <a>"
            );
        }
        // No history is kept, so a link is never visited — also the privacy-safe answer.
        assert!(!matches_in("a:visited", handle(link), QuirksMode::NoQuirks));
    }

    /// `attr_matches` beyond `[id=a]`: the existence form and every operator `selectors`
    /// can hand us, each with a matching and a non-matching case, plus the `i` flag.
    #[test]
    fn attr_matches_should_implement_every_operator() {
        let (doc, link, _) = attr_doc();
        let store = store(&doc);
        let arena = NodeArena::new(&doc, &store);
        let element = element_handle(&arena, link);
        let hits = |selector: &str| matches_in(selector, element, QuirksMode::NoQuirks);

        // Existence.
        assert!(hits("[title]"));
        assert!(hits("[data-flag]"), "an empty value still exists");
        assert!(!hits("[data-missing]"));
        // Exact.
        assert!(hits("[title=Hello]"));
        assert!(!hits("[title=Hell]"));
        // ~= (whitespace-separated word).
        assert!(hits("[class~=Nav]"));
        assert!(hits("[class~=Bar]"));
        assert!(!hits("[class~=Na]"), "~= matches whole words only");
        // |= (exact, or followed by a hyphen).
        assert!(hits("[lang|=en]"));
        assert!(!hits("[lang|=e]"));
        // ^= $= *=
        assert!(hits("[title^=Hel]"));
        assert!(!hits("[title^=ello]"));
        assert!(hits("[title$=llo]"));
        assert!(!hits("[title$=Hel]"));
        assert!(hits("[title*=ell]"));
        assert!(!hits("[title*=xyz]"));
        // The `i` flag, against the same value that fails case-sensitively.
        assert!(!hits("[title=hello]"));
        assert!(hits("[title=hello i]"));
    }

    /// Quirks mode makes id and class matching ASCII-case-insensitive. Both the trait
    /// methods and the whole selector path have to agree about that.
    #[test]
    fn has_id_and_has_class_should_follow_quirks_mode() {
        let (doc, link, _) = attr_doc();
        let store = store(&doc);
        let arena = NodeArena::new(&doc, &store);
        let element = element_handle(&arena, link);

        assert!(element.has_id(&AtomIdent::from("Top"), CaseSensitive));
        assert!(!element.has_id(&AtomIdent::from("top"), CaseSensitive));
        assert!(element.has_id(&AtomIdent::from("top"), AsciiCaseInsensitive));

        assert!(element.has_class(&AtomIdent::from("Nav"), CaseSensitive));
        assert!(!element.has_class(&AtomIdent::from("nav"), CaseSensitive));
        assert!(element.has_class(&AtomIdent::from("nav"), AsciiCaseInsensitive));

        for selector in ["#top", ".nav"] {
            assert!(
                !matches_in(selector, element, QuirksMode::NoQuirks),
                "{selector} is case-sensitive without quirks"
            );
            assert!(
                matches_in(selector, element, QuirksMode::Quirks),
                "{selector} is ASCII-case-insensitive in quirks mode"
            );
        }
        // The exact-case selectors keep matching either way.
        assert!(matches_in("#Top.Nav", element, QuirksMode::NoQuirks));
        assert!(matches_in("#Top.Nav", element, QuirksMode::Quirks));
    }
}
