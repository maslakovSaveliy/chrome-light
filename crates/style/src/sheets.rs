//! UA and document (`<style>`/`<link>`) stylesheet collection.
//!
//! [`StyleEngine::add_ua_sheet`] appends the bundled `assets/ua.css` at `Origin::UserAgent`
//! (Task 12's "without it, `<div>` is not a block, `<head>` is visible" bundled sheet).
//! [`StyleEngine::collect_document_sheets`] walks a parsed [`cl_dom::Document`] in tree
//! (i.e. document) order and adds every `<style>` element's text and every
//! `<link rel~=stylesheet href>`'s resolved, loaded resource as an author sheet, exactly
//! the way a real browser builds up a document's `DocumentOrShadowRoot.styleSheets`.
//!
//! A missing or unloadable external stylesheet must never blank the rest of the page: a
//! bad `href`, a `load` failure, or non-UTF-8 content all become a [`SheetWarning`] pushed
//! onto the returned list rather than aborting collection. The only way
//! [`StyleEngine::collect_document_sheets`] itself returns `Err` is a document whose own
//! [`cl_dom::Document::base_url`] is not a valid absolute URL — without that, no relative
//! `href` in the document could be resolved at all, so there is nothing sound left to do.

use cl_dom::{Document, NodeId, NodeKind, local_name};
use cl_net::{NetError, Url};

use crate::StyleEngine;
use crate::error::StyleError;

/// The bundled user-agent stylesheet (`assets/ua.css`), embedded at compile time.
const UA_CSS: &str = include_str!("../assets/ua.css");

/// Base URL the bundled UA sheet is parsed against. It contains no `url()`/`@import`
/// references, so this only needs to be *some* valid absolute URL — a distinct, clearly
/// synthetic scheme so it can never collide with a real document or resource URL.
const UA_BASE_URL: &str = "chrome-light-ua://ua.css";

/// One stylesheet [`StyleEngine::collect_document_sheets`] could not add while walking a
/// document: either the `<link>`'s `href` could not be resolved against the document's
/// base URL, or loading/decoding the resolved resource failed.
///
/// Collection continues past a warning; see the module docs for why a missing stylesheet
/// must never stop the rest of the page from rendering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetWarning {
    /// The stylesheet that failed: the resolved absolute URL if resolution against the
    /// document's base URL succeeded, otherwise the raw (unresolved) `href` attribute
    /// value.
    pub url_or_href: String,
    /// A human-readable description of why it failed (the underlying [`StyleError`] or
    /// [`NetError`] message).
    pub cause: String,
}

impl StyleEngine {
    /// Parses and appends the bundled user-agent stylesheet (`assets/ua.css`) at
    /// `Origin::UserAgent`.
    ///
    /// UA-origin rules always lose to author rules in the cascade regardless of
    /// specificity or source order, so this only needs to be called once per engine —
    /// before or after any `add_author_sheet`/`collect_document_sheets` calls, the
    /// result is the same.
    ///
    /// # Errors
    /// Returns [`StyleError`] if the bundled stylesheet's fixed base URL fails to parse.
    /// That base URL is a compile-time constant, so in practice this never happens; the
    /// `Result` return exists so a corrupted build (or a future edit to `UA_BASE_URL`)
    /// fails loudly instead of silently skipping the UA sheet.
    pub fn add_ua_sheet(&mut self) -> Result<(), StyleError> {
        self.add_ua_sheet_from(UA_CSS, UA_BASE_URL)
    }

    /// Walks `doc` in document order and adds every `<style>` element's text content and
    /// every `<link rel~=stylesheet href>`'s resolved, loaded stylesheet as an author
    /// sheet (`Origin::Author`, appended in the order they are found).
    ///
    /// `rel~=stylesheet` means: split `rel` on ASCII whitespace and match `stylesheet`
    /// case-insensitively against any one token — see [`rel_has_stylesheet_token`] for the
    /// full reasoning, including why `rel="alternate stylesheet"` matches.
    ///
    /// `load` fetches the bytes for a resolved stylesheet URL (the test shell passes
    /// `cl_net::load_file`; a future network process would pass something HTTP-aware). A
    /// `<link>` with no `href`, or whose `rel` does not carry the `stylesheet` token, is
    /// silently skipped — it names no stylesheet, so there is nothing to warn about.
    ///
    /// # Errors
    /// Returns [`StyleError::Url`] if `doc.base_url()` itself is not a valid absolute URL
    /// (nothing relative in the document could be resolved without it). Every other
    /// failure — a bad `href`, or `load` returning `Err` — is collected into the returned
    /// `Vec<SheetWarning>` instead; see the module docs.
    pub fn collect_document_sheets(
        &mut self,
        doc: &Document,
        load: &dyn Fn(&Url) -> Result<Vec<u8>, NetError>,
    ) -> Result<Vec<SheetWarning>, StyleError> {
        let base = Url::parse(doc.base_url()).map_err(|e| StyleError::Url(e.to_string()))?;
        let mut warnings = Vec::new();

        for id in doc.descendants(doc.root()) {
            let Some(element) = doc.element(id) else {
                continue;
            };
            if element.name.local == local_name!("style") {
                let css = text_content(doc, id);
                self.add_author_sheet(&css, doc.base_url())?;
            } else if element.name.local == local_name!("link") {
                self.collect_link(doc, id, &base, load, &mut warnings);
            }
        }

        Ok(warnings)
    }

    /// Handles one `<link>` element for [`StyleEngine::collect_document_sheets`]: if its
    /// `rel` carries the `stylesheet` token and it has an `href`, resolves the `href`
    /// against `base`, loads it via `load`, and appends it as an author sheet. Any failure
    /// pushes a [`SheetWarning`] onto `warnings` instead of propagating — see the module
    /// docs.
    fn collect_link(
        &mut self,
        doc: &Document,
        id: NodeId,
        base: &Url,
        load: &dyn Fn(&Url) -> Result<Vec<u8>, NetError>,
        warnings: &mut Vec<SheetWarning>,
    ) {
        let rel = doc.attr(id, &local_name!("rel")).unwrap_or_default();
        if !rel_has_stylesheet_token(rel) {
            return;
        }
        let Some(href) = doc.attr(id, &local_name!("href")) else {
            return;
        };

        let url = match base.join(href) {
            Ok(url) => url,
            Err(e) => {
                warnings.push(SheetWarning {
                    url_or_href: href.to_owned(),
                    cause: e.to_string(),
                });
                return;
            }
        };

        let bytes = match load(&url) {
            Ok(bytes) => bytes,
            Err(e) => {
                warnings.push(SheetWarning {
                    url_or_href: url.as_str().to_owned(),
                    cause: e.to_string(),
                });
                return;
            }
        };

        // CSS's default encoding is UTF-8 absent an `@charset`/BOM override; M1a does not
        // implement charset detection, so bytes that are not valid UTF-8 are decoded
        // lossily rather than treated as a hard failure — good enough for the ASCII/UTF-8
        // fixtures this task covers, and still never a crash on hostile input.
        let css = String::from_utf8_lossy(&bytes);
        if let Err(e) = self.add_author_sheet(&css, url.as_str()) {
            warnings.push(SheetWarning {
                url_or_href: url.as_str().to_owned(),
                cause: e.to_string(),
            });
        }
    }
}

/// Concatenates the text of every descendant text node of `id`, in document order.
///
/// A `<style>` element is an HTML "raw text" element, so in a well-formed parse tree it
/// has exactly one `Text` child; walking descendants rather than assuming that exact shape
/// costs nothing here and stays correct if a caller hands us a hand-built or unusual DOM
/// with the text split across multiple nodes (or none at all, yielding an empty sheet).
fn text_content(doc: &Document, id: NodeId) -> String {
    let mut text = String::new();
    for child in doc.descendants(id) {
        if let Some(NodeKind::Text(t)) = doc.get(child).map(|node| &node.kind) {
            text.push_str(t);
        }
    }
    text
}

/// Whether `rel`'s whitespace-separated token list contains `stylesheet`, matched
/// ASCII-case-insensitively.
///
/// Spec reading (HTML Standard, the `link` element's `rel` attribute:
/// <https://html.spec.whatwg.org/multipage/semantics.html#the-link-element>): `rel` is a
/// "set of space-separated tokens", each matched against a registered keyword
/// case-insensitively — the same `~=` (attribute-value-contains-token) semantics the task
/// brief's own `<link rel~=stylesheet href>` notation names. So:
/// - `rel="stylesheet"` and `rel="STYLESHEET"` both match (single token, case-insensitive);
/// - `rel="alternate stylesheet"` also matches: its token set is `{alternate, stylesheet}`,
///   which contains the `stylesheet` token. A full browser treats an *alternate*
///   stylesheet as inactive unless the user selects it via a preferred-stylesheet-set UI;
///   M1a has no such UI (or any other stylesheet-selection mechanism), and "load every
///   sheet whose token set contains `stylesheet`" is the closest sound approximation —
///   and exactly what `rel~=stylesheet` denotes;
/// - `rel="stylesheetish"` does **not** match: it is one token, `stylesheetish`, distinct
///   from `stylesheet` — token matching is exact-per-token, never substring.
fn rel_has_stylesheet_token(rel: &str) -> bool {
    rel.split_ascii_whitespace()
        .any(|token| token.eq_ignore_ascii_case("stylesheet"))
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use cl_dom::{Attr, Element, LocalName, QualName, StrTendril, ns};

    fn el(doc: &mut Document, name: &str, attrs: &[(&str, &str)]) -> NodeId {
        doc.create(NodeKind::Element(Element {
            name: QualName::new(None, ns!(html), LocalName::from(name)),
            attrs: attrs
                .iter()
                .map(|(k, v)| Attr {
                    name: QualName::new(None, ns!(), LocalName::from(*k)),
                    value: StrTendril::from(*v),
                })
                .collect(),
            template_contents: None,
        }))
    }

    fn text(doc: &mut Document, s: &str) -> NodeId {
        doc.create(NodeKind::Text(StrTendril::from(s)))
    }

    /// A `load` callback for tests that must never actually be called (no `<link>` in the
    /// fixture). Returns `Err` rather than panicking, so an accidental call is a normal
    /// assertion failure (via the returned warning) instead of a test-harness panic.
    fn unreachable_load(_url: &Url) -> Result<Vec<u8>, NetError> {
        Err(NetError::UnsupportedScheme(
            "no <link> in this fixture".to_owned(),
        ))
    }

    #[test]
    fn ua_sheet_should_parse_without_errors() {
        let mut engine = StyleEngine::new((800.0, 600.0), 1.0).expect("engine");
        engine.add_ua_sheet().expect("ua sheet parses");
        // A lower bound, not an exact count: `ua.css` covers ~19 selector groups (display
        // resets, the block-level list, heading sizes, etc.) as of this task, and editing
        // the CSS file later should not need editing this assertion in lockstep.
        assert!(
            engine.ua_rule_count() >= 10,
            "expected at least 10 top-level rules in the UA sheet, got {}",
            engine.ua_rule_count()
        );
    }

    #[test]
    fn collect_should_parse_style_elements_in_order() {
        let mut doc = Document::new("file:///test.html");
        let root = doc.root();
        let html = el(&mut doc, "html", &[]);
        let head = el(&mut doc, "head", &[]);
        let style1 = el(&mut doc, "style", &[]);
        let style1_text = text(&mut doc, "p { color: red }");
        let body = el(&mut doc, "body", &[]);
        let style2 = el(&mut doc, "style", &[]);
        let style2_text = text(&mut doc, "div { color: blue } span { color: green }");
        doc.append_child(root, html).expect("html");
        doc.append_child(html, head).expect("head");
        doc.append_child(head, style1).expect("style1");
        doc.append_child(style1, style1_text).expect("style1 text");
        doc.append_child(html, body).expect("body");
        doc.append_child(body, style2).expect("style2");
        doc.append_child(style2, style2_text).expect("style2 text");

        let mut engine = StyleEngine::new((800.0, 600.0), 1.0).expect("engine");
        let warnings = engine
            .collect_document_sheets(&doc, &unreachable_load)
            .expect("collects");

        assert!(warnings.is_empty());
        // Two sheets, in document order: the first (`head`'s) has 1 rule, the second
        // (`body`'s) has 2 — so the per-sheet counts alone prove the order, without
        // needing `resolve()` (Task 13) to observe a cascade effect.
        assert_eq!(engine.author_rule_counts(), vec![1, 2]);
    }

    #[test]
    fn collect_should_warn_not_fail_on_missing_link() {
        let mut doc = Document::new("file:///test.html");
        let root = doc.root();
        let link = el(
            &mut doc,
            "link",
            &[("rel", "stylesheet"), ("href", "missing.css")],
        );
        doc.append_child(root, link).expect("link");

        let mut engine = StyleEngine::new((800.0, 600.0), 1.0).expect("engine");
        let result = engine.collect_document_sheets(&doc, &|_url| {
            Err(NetError::Io(std::io::Error::other(
                "missing.css does not exist",
            )))
        });

        let warnings = result.expect("a missing link is a warning, not an Err");
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings
                .first()
                .expect("just asserted len() == 1")
                .url_or_href
                .ends_with("missing.css")
        );
        assert_eq!(engine.author_rule_count(), 0);
    }

    #[test]
    fn collect_should_load_and_add_a_resolved_link_stylesheet() {
        let mut doc = Document::new("file:///test.html");
        let root = doc.root();
        let link = el(
            &mut doc,
            "link",
            &[("rel", "stylesheet"), ("href", "style.css")],
        );
        doc.append_child(root, link).expect("link");

        let mut engine = StyleEngine::new((800.0, 600.0), 1.0).expect("engine");
        let warnings = engine
            .collect_document_sheets(&doc, &|url| {
                assert_eq!(url.as_str(), "file:///style.css");
                Ok(b"p { color: red }".to_vec())
            })
            .expect("collects");

        assert!(warnings.is_empty());
        assert_eq!(engine.author_rule_count(), 1);
    }

    #[test]
    fn collect_should_fail_on_invalid_document_base_url() {
        let doc = Document::new("not a url");
        let mut engine = StyleEngine::new((800.0, 600.0), 1.0).expect("engine");
        let result = engine.collect_document_sheets(&doc, &unreachable_load);
        assert!(matches!(result, Err(StyleError::Url(_))));
    }

    #[test]
    fn link_rel_matching_should_be_token_based() {
        assert!(rel_has_stylesheet_token("stylesheet"));
        assert!(rel_has_stylesheet_token("STYLESHEET"));
        assert!(rel_has_stylesheet_token("alternate stylesheet"));
        assert!(rel_has_stylesheet_token("stylesheet alternate"));
        assert!(!rel_has_stylesheet_token("stylesheetish"));
        assert!(!rel_has_stylesheet_token("icon"));
        assert!(!rel_has_stylesheet_token(""));
    }

    /// Without the bundled UA sheet a `<div>` has no `display` rule anywhere and falls back
    /// to `inline`; with it, the div is a block. This is the one test that proves
    /// `add_ua_sheet` actually reaches the cascade rather than just parsing.
    #[test]
    #[allow(clippy::expect_used)]
    fn div_should_be_display_block_without_author_css() {
        use crate::test_dom::{find, parse};
        use style::values::specified::Display;

        let doc = parse("<div>hi</div>");
        let mut engine = StyleEngine::new((800.0, 600.0), 1.0).expect("engine");
        engine.add_ua_sheet().expect("ua sheet");
        assert_eq!(engine.author_rule_count(), 0, "no author CSS at all");

        let styled = engine.resolve(doc).expect("resolve");
        let div = find(styled.document(), "div").expect("<div>");
        assert_eq!(
            styled
                .computed(div)
                .expect("styled <div>")
                .get_box()
                .clone_display(),
            Display::Block
        );
    }
}
