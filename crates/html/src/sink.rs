//! The `html5ever` `TreeSink` that turns tree-construction callbacks into mutations on a
//! [`cl_dom::Document`].
//!
//! [`DomSink`] wraps its state in two `RefCell`s — the arena `Document` (`doc`) and the
//! collected parse-error messages (`errors`) — the *only* `RefCell`s anywhere in this
//! pipeline (`docs/CODING_STANDARDS.md` §1 forbids `Rc`/`RefCell` everywhere else): every
//! `TreeSink` method takes `&self` (html5ever's own trait shape, not a choice we get to make —
//! see the Task 8 report for the registry source this was verified against), so interior
//! mutability is the only way to mutate either field from behind that `&self`. Neither escapes
//! this module: [`DomSink::finish`] takes `self` by value (html5ever's `TreeSink::finish`
//! signature, not `&self`) and unwraps both `RefCell`s with `into_inner`, handing the caller a
//! plain owned [`cl_dom::Document`] and `Vec<String>` of parse errors (wrapped together into
//! [`ParseOutputInner`]).
//!
//! Every method here is written to never panic, even though several of the trait's own doc
//! comments say "feel free to panic!" for cases html5ever promises never to trigger (e.g.
//! [`TreeSink::elem_name`] on a non-element handle, [`TreeSink::get_template_contents`] on a
//! non-`<template>` handle). This crate's no-panic-on-any-input rule does not carve out an
//! exception for "the parser's own internal call contract" — hostile bytes drive the tokenizer
//! and tree builder, not this sink directly, but a bug anywhere upstream should degrade to a
//! wrong (or error-reported) tree rather than a crash. Concretely: [`DomSink::elem_name`] falls
//! back to an empty [`QualName`] instead of unwrapping a missing lookup, and
//! [`TreeSink::get_template_contents`] falls back to returning the handle it was given instead
//! of unwrapping `Element::template_contents`.

use std::borrow::Cow;
use std::cell::{Ref, RefCell};
use std::sync::LazyLock;

use cl_dom::{Attr, Doctype, Document, Element, Node, NodeId, NodeKind, QuirksMode};
use html5ever::Attribute;
use html5ever::tendril::StrTendril;
use html5ever::tree_builder::{
    ElementFlags, NodeOrText, QuirksMode as Html5everQuirksMode, TreeSink,
};
use markup5ever::{LocalName, Namespace, QualName};

/// The name [`DomSink::elem_name`] falls back to if it is ever asked about a handle that is not
/// (or is no longer) a live element — html5ever's own contract says this never happens, but
/// this crate never unwraps that promise into a panic; see the module docs.
static UNKNOWN_ELEMENT_NAME: LazyLock<QualName> =
    LazyLock::new(|| QualName::new(None, Namespace::from(""), LocalName::from("")));

/// The owned result of [`DomSink::finish`]: the tree html5ever built, plus every parse error it
/// reported along the way. `pub(crate)` — [`crate::ParseOutput`] (the crate's real public
/// output type) wraps this with the encoding info the sink itself never sees, since encoding is
/// decided before html5ever's tokenizer ever runs (see [`crate::parse_document`]).
pub(crate) struct ParseOutputInner {
    /// The parsed document tree.
    pub(crate) document: Document,
    /// Every parse error html5ever reported while building the tree, in report order.
    pub(crate) parse_errors: Vec<String>,
}

/// Bridges html5ever's [`TreeSink`] callbacks onto a [`cl_dom::Document`].
///
/// `Handle = NodeId`: an arena index is already `Copy`/`Clone`/`Eq`, exactly what `TreeSink`
/// requires of a handle, so no separate handle type is needed. Construct with [`DomSink::new`],
/// drive it through html5ever's `driver::parse_document(..).one(..)` (see
/// [`crate::parse_document`]/[`crate::parse_document_str`]), then read back the tree via
/// [`TreeSink::finish`].
pub(crate) struct DomSink {
    doc: RefCell<Document>,
    errors: RefCell<Vec<String>>,
}

impl DomSink {
    /// Creates a sink over a fresh, empty [`Document`] rooted at `base_url`.
    pub(crate) fn new(base_url: &str) -> Self {
        Self {
            doc: RefCell::new(Document::new(base_url)),
            errors: RefCell::new(Vec::new()),
        }
    }
}

/// If `candidate` names an existing [`NodeKind::Text`] node, appends `text` onto it (tendril
/// concatenation, not a new node) and returns `Ok(())`. Otherwise returns `Err(text)`, handing
/// the tendril straight back so the caller can turn it into a fresh `Text` node instead —
/// avoiding a clone either way.
///
/// Shared by [`TreeSink::append`] and [`TreeSink::append_before_sibling`], which the html5ever
/// contract requires to apply the *same* "merge into the trailing/preceding text sibling" rule
/// (see both methods' doc comments in `markup5ever::interface::tree_builder`): a run of
/// character tokens can arrive as several `AppendText` calls (e.g. split around a character
/// reference), and the tree these calls build must never contain two adjacent `Text` siblings.
fn merge_into_trailing_text(
    doc: &mut Document,
    candidate: Option<NodeId>,
    text: StrTendril,
) -> Result<(), StrTendril> {
    if let Some(id) = candidate
        && let Some(NodeKind::Text(existing)) = doc.get_mut(id).map(|node| &mut node.kind)
    {
        existing.push_tendril(&text);
        return Ok(());
    }
    Err(text)
}

impl TreeSink for DomSink {
    type Handle = NodeId;
    type Output = ParseOutputInner;
    type ElemName<'a> = Ref<'a, QualName>;

    fn finish(self) -> Self::Output {
        ParseOutputInner {
            document: self.doc.into_inner(),
            parse_errors: self.errors.into_inner(),
        }
    }

    fn parse_error(&self, msg: Cow<'static, str>) {
        self.errors.borrow_mut().push(msg.into_owned());
    }

    fn get_document(&self) -> Self::Handle {
        self.doc.borrow().root()
    }

    fn elem_name<'a>(&'a self, target: &'a Self::Handle) -> Self::ElemName<'a> {
        let id = *target;
        Ref::map(self.doc.borrow(), move |doc| {
            doc.element(id)
                .map_or(&*UNKNOWN_ELEMENT_NAME, |element| &element.name)
        })
    }

    fn create_element(
        &self,
        name: QualName,
        attrs: Vec<Attribute>,
        flags: ElementFlags,
    ) -> Self::Handle {
        let mut doc = self.doc.borrow_mut();
        let attrs: Vec<Attr> = attrs
            .into_iter()
            .map(|attr| Attr {
                name: attr.name,
                value: attr.value,
            })
            .collect();
        let id = doc.create(NodeKind::Element(Element {
            name,
            attrs,
            template_contents: None,
        }));
        // A <template> element also gets a DocumentFragment holding its (inert) contents; see
        // `Element::template_contents`'s docs and `TreeSink::get_template_contents`' contract.
        if flags.template {
            let contents = doc.create(NodeKind::DocumentFragment);
            if let Some(NodeKind::Element(element)) = doc.get_mut(id).map(|node| &mut node.kind) {
                element.template_contents = Some(contents);
            }
        }
        id
    }

    fn create_comment(&self, text: StrTendril) -> Self::Handle {
        self.doc.borrow_mut().create(NodeKind::Comment(text))
    }

    fn create_pi(&self, target: StrTendril, data: StrTendril) -> Self::Handle {
        self.doc
            .borrow_mut()
            .create(NodeKind::ProcessingInstruction { target, data })
    }

    fn append(&self, parent: &Self::Handle, child: NodeOrText<Self::Handle>) {
        let mut doc = self.doc.borrow_mut();
        match child {
            NodeOrText::AppendNode(node) => {
                let _ = doc.append_child(*parent, node);
            }
            NodeOrText::AppendText(text) => {
                let last = doc.get(*parent).and_then(Node::last_child);
                if let Err(text) = merge_into_trailing_text(&mut doc, last, text) {
                    let id = doc.create(NodeKind::Text(text));
                    let _ = doc.append_child(*parent, id);
                }
            }
        }
    }

    fn append_based_on_parent_node(
        &self,
        element: &Self::Handle,
        prev_element: &Self::Handle,
        child: NodeOrText<Self::Handle>,
    ) {
        let has_parent = self
            .doc
            .borrow()
            .get(*element)
            .and_then(Node::parent)
            .is_some();
        if has_parent {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev_element, child);
        }
    }

    fn append_doctype_to_document(
        &self,
        name: StrTendril,
        public_id: StrTendril,
        system_id: StrTendril,
    ) {
        let mut doc = self.doc.borrow_mut();
        let root = doc.root();
        let id = doc.create(NodeKind::Doctype(Doctype {
            name,
            public_id,
            system_id,
        }));
        let _ = doc.append_child(root, id);
    }

    fn get_template_contents(&self, target: &Self::Handle) -> Self::Handle {
        self.doc
            .borrow()
            .element(*target)
            .and_then(|element| element.template_contents)
            .unwrap_or(*target)
    }

    fn same_node(&self, x: &Self::Handle, y: &Self::Handle) -> bool {
        x == y
    }

    fn set_quirks_mode(&self, mode: Html5everQuirksMode) {
        let mapped = match mode {
            Html5everQuirksMode::Quirks => QuirksMode::Quirks,
            Html5everQuirksMode::LimitedQuirks => QuirksMode::LimitedQuirks,
            Html5everQuirksMode::NoQuirks => QuirksMode::NoQuirks,
        };
        self.doc.borrow_mut().set_quirks_mode(mapped);
    }

    fn append_before_sibling(&self, sibling: &Self::Handle, new_node: NodeOrText<Self::Handle>) {
        let mut doc = self.doc.borrow_mut();
        match new_node {
            NodeOrText::AppendNode(node) => {
                // "new_node may have an old parent, from which it should be removed" — detach
                // is a no-op if it has none (see `Document::detach`'s docs), so this is safe to
                // call unconditionally rather than checking first.
                let _ = doc.detach(node);
                let _ = doc.insert_before(*sibling, node);
            }
            NodeOrText::AppendText(text) => {
                let prev = doc.get(*sibling).and_then(Node::prev_sibling);
                if let Err(text) = merge_into_trailing_text(&mut doc, prev, text) {
                    let id = doc.create(NodeKind::Text(text));
                    let _ = doc.insert_before(*sibling, id);
                }
            }
        }
    }

    fn add_attrs_if_missing(&self, target: &Self::Handle, attrs: Vec<Attribute>) {
        let mut doc = self.doc.borrow_mut();
        if let Some(NodeKind::Element(element)) = doc.get_mut(*target).map(|node| &mut node.kind) {
            for attr in attrs {
                let already_present = element
                    .attrs
                    .iter()
                    .any(|existing| existing.name == attr.name);
                if !already_present {
                    element.attrs.push(Attr {
                        name: attr.name,
                        value: attr.value,
                    });
                }
            }
        }
    }

    fn remove_from_parent(&self, target: &Self::Handle) {
        let _ = self.doc.borrow_mut().detach(*target);
    }

    fn reparent_children(&self, node: &Self::Handle, new_parent: &Self::Handle) {
        let _ = self.doc.borrow_mut().reparent_children(*node, *new_parent);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use cl_dom::QuirksMode;
    use cl_dom::serialize::html5lib_tree;
    use encoding_rs::WINDOWS_1251;
    use html5ever::Attribute;
    use html5ever::tendril::StrTendril;
    use html5ever::tree_builder::{ElementFlags, NodeOrText, TreeSink};
    use markup5ever::{LocalName, QualName, ns};
    use proptest::prelude::*;

    use crate::sink::DomSink;
    use crate::{EncodingSource, parse_document, parse_document_str};

    fn base() -> cl_net::Url {
        cl_net::Url::parse("http://example.test/").expect("static url")
    }

    #[test]
    fn parse_should_build_html_head_body_when_missing() {
        let output = parse_document_str("<p>hi", &base()).expect("parses");
        let expected = "#document\n\
            | <html>\n\
            |   <head>\n\
            |   <body>\n\
            |     <p>\n\
            |       \"hi\"";
        assert_eq!(html5lib_tree(&output.document), expected);
    }

    #[test]
    fn text_nodes_should_merge_adjacent_appends() {
        // Adjacent AppendText calls on the same parent must merge into one Text node...
        let merged_sink = DomSink::new("about:blank");
        let root = merged_sink.get_document();
        merged_sink.append(&root, NodeOrText::AppendText(StrTendril::from("a")));
        merged_sink.append(&root, NodeOrText::AppendText(StrTendril::from("b")));
        let merged = merged_sink.finish();
        assert_eq!(html5lib_tree(&merged.document), "#document\n| \"ab\"");

        // ...but a Comment node between two AppendText calls must NOT be merged across.
        let split_sink = DomSink::new("about:blank");
        let root = split_sink.get_document();
        split_sink.append(&root, NodeOrText::AppendText(StrTendril::from("a")));
        let comment = split_sink.create_comment(StrTendril::from("x"));
        split_sink.append(&root, NodeOrText::AppendNode(comment));
        split_sink.append(&root, NodeOrText::AppendText(StrTendril::from("b")));
        let split = split_sink.finish();
        assert_eq!(
            html5lib_tree(&split.document),
            "#document\n| \"a\"\n| <!-- x -->\n| \"b\""
        );
    }

    #[test]
    fn append_before_sibling_text_should_merge_into_prev_text_sibling() {
        // The sibling's previous sibling IS a text node: `append_before_sibling` must apply
        // the same merge rule as `append` (markup5ever's trait docs require this symmetry)
        // rather than creating a second, adjacent Text node.
        let sink = DomSink::new("about:blank");
        let root = sink.get_document();
        sink.append(&root, NodeOrText::AppendText(StrTendril::from("a")));
        let p = sink.create_element(
            QualName::new(None, ns!(html), LocalName::from("p")),
            Vec::new(),
            ElementFlags::default(),
        );
        sink.append(&root, NodeOrText::AppendNode(p));

        sink.append_before_sibling(&p, NodeOrText::AppendText(StrTendril::from("b")));

        let output = sink.finish();
        assert_eq!(
            html5lib_tree(&output.document),
            "#document\n| \"ab\"\n| <p>"
        );
    }

    #[test]
    fn append_before_sibling_text_should_create_new_node_when_prev_sibling_is_not_text() {
        // Case A: the sibling's previous sibling exists but is not a text node (a comment)
        // — no merge, a new Text node is inserted between the comment and the sibling.
        let comment_sink = DomSink::new("about:blank");
        let root = comment_sink.get_document();
        let comment = comment_sink.create_comment(StrTendril::from("x"));
        comment_sink.append(&root, NodeOrText::AppendNode(comment));
        let p = comment_sink.create_element(
            QualName::new(None, ns!(html), LocalName::from("p")),
            Vec::new(),
            ElementFlags::default(),
        );
        comment_sink.append(&root, NodeOrText::AppendNode(p));

        comment_sink.append_before_sibling(&p, NodeOrText::AppendText(StrTendril::from("b")));

        let with_comment = comment_sink.finish();
        assert_eq!(
            html5lib_tree(&with_comment.document),
            "#document\n| <!-- x -->\n| \"b\"\n| <p>"
        );

        // Case B: the sibling has no previous sibling at all (it is the first child) — no
        // merge, a new Text node is inserted as the new first child.
        let first_child_sink = DomSink::new("about:blank");
        let root = first_child_sink.get_document();
        let p = first_child_sink.create_element(
            QualName::new(None, ns!(html), LocalName::from("p")),
            Vec::new(),
            ElementFlags::default(),
        );
        first_child_sink.append(&root, NodeOrText::AppendNode(p));

        first_child_sink.append_before_sibling(&p, NodeOrText::AppendText(StrTendril::from("b")));

        let first_child = first_child_sink.finish();
        assert_eq!(
            html5lib_tree(&first_child.document),
            "#document\n| \"b\"\n| <p>"
        );
    }

    #[test]
    fn template_contents_should_be_stored_in_fragment() {
        let output = parse_document_str("<template><b>x</b></template>", &base()).expect("parses");
        let dump = html5lib_tree(&output.document);
        assert!(dump.contains("content"), "dump was:\n{dump}");
        assert!(dump.contains("<b>"), "dump was:\n{dump}");
    }

    #[test]
    fn duplicate_attributes_in_one_tag_should_keep_the_first() {
        // NOTE: this is an end-to-end regression test, not a unit test of
        // `DomSink::add_attrs_if_missing`. html5ever's tokenizer deduplicates attributes
        // within a single start tag before `create_element` is ever called
        // (`finish_attribute`'s duplicate check, `html5ever::tokenizer`), so `<p a=1 a=2>`
        // never reaches `add_attrs_if_missing` at all — it documents real tokenizer
        // behaviour. See `add_attrs_if_missing_should_keep_first_value_and_add_new_names`
        // below for a direct unit test of the sink method itself.
        let output = parse_document_str("<p a=1 a=2>", &base()).expect("parses");
        let dump = html5lib_tree(&output.document);
        assert!(dump.contains("a=\"1\""), "dump was:\n{dump}");
        assert!(!dump.contains("a=\"2\""), "dump was:\n{dump}");
    }

    #[test]
    fn add_attrs_if_missing_should_keep_first_value_and_add_new_names() {
        // Drives `DomSink::add_attrs_if_missing` directly, bypassing the tokenizer (which
        // would never call this method with an attribute name the element already has —
        // see the note on `duplicate_attributes_in_one_tag_should_keep_the_first` above).
        // html5ever itself calls this method from the tree builder's "re-encountered
        // <html>/<body> start tag" and similar attribute-copy-forward rules.
        let sink = DomSink::new("about:blank");
        let name_a = QualName::new(None, ns!(), LocalName::from("a"));
        let name_b = QualName::new(None, ns!(), LocalName::from("b"));
        let element = sink.create_element(
            QualName::new(None, ns!(html), LocalName::from("p")),
            vec![Attribute {
                name: name_a.clone(),
                value: StrTendril::from("1"),
            }],
            ElementFlags::default(),
        );

        // "a" already exists on the element: this call must NOT overwrite its value.
        sink.add_attrs_if_missing(
            &element,
            vec![Attribute {
                name: name_a,
                value: StrTendril::from("2"),
            }],
        );
        // "b" is a new name: this call must add it.
        sink.add_attrs_if_missing(
            &element,
            vec![Attribute {
                name: name_b,
                value: StrTendril::from("3"),
            }],
        );

        let output = sink.finish();
        assert_eq!(
            output.document.attr(element, &LocalName::from("a")),
            Some("1")
        );
        assert_eq!(
            output.document.attr(element, &LocalName::from("b")),
            Some("3")
        );
    }

    #[test]
    fn quirks_mode_should_be_set_without_doctype() {
        let output = parse_document_str("<p>hi", &base()).expect("parses");
        assert_eq!(output.document.quirks_mode(), QuirksMode::Quirks);
    }

    #[test]
    fn parse_errors_should_be_collected_not_fatal() {
        let output = parse_document_str("</p>", &base()).expect("still Ok, not Err");
        assert!(!output.parse_errors.is_empty());
    }

    #[test]
    fn parse_document_should_decode_windows_1251_via_meta() {
        let (encoded, _, _) = WINDOWS_1251.encode("Привет");
        let mut bytes = b"<html><head><meta charset=windows-1251></head><body><p>".to_vec();
        bytes.extend_from_slice(&encoded);
        bytes.extend_from_slice(b"</p></body></html>");

        let output = parse_document(&bytes, &base(), None).expect("parses");
        assert_eq!(output.encoding, WINDOWS_1251);
        assert_eq!(output.encoding_source, EncodingSource::MetaPrescan);
        let dump = html5lib_tree(&output.document);
        assert!(dump.contains("Привет"), "dump was:\n{dump}");
    }

    proptest! {
        #[test]
        fn malformed_bytes_should_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..=4096)) {
            let _ = parse_document(&bytes, &base(), None);
        }
    }
}
