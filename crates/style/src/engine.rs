//! Owns the stylo `Stylist`, `Device` and the shared lock. Sheet bookkeeping is manual
//! (`stylist.append_stylesheet`) like Blitz, not `DocumentStylesheetSet`.

use cl_dom::{Document, NodeId};
use style::Atom;
use style::animation::DocumentAnimationSet;
use style::context::{
    QuirksMode, RegisteredSpeculativePainter, RegisteredSpeculativePainters, SharedStyleContext,
    StyleSystemOptions,
};
use style::device::Device;
use style::device::servo::FontMetricsProvider;
use style::font_metrics::FontMetrics;
use style::invalidation::element::restyle_hints::RestyleHint;
use style::media_queries::{MediaList, MediaType};
use style::properties::ComputedValues;
use style::properties::style_structs::Font;
use style::queries::values::PrefersColorScheme;
use style::selector_parser::SnapshotMap;
use style::servo::media_features::PointerCapabilities;
use style::servo_arc::Arc as StyloArc;
use style::shared_lock::{SharedRwLock, StylesheetGuards};
use style::stylesheets::{AllowImportRules, DocumentStyleSheet, Origin, Stylesheet, UrlExtraData};
use style::stylist::Stylist;
use style::thread_state::{self, ThreadState};
use style::traversal::DomTraversal;
use style::traversal_flags::TraversalFlags;
use style::values::computed::font::{GenericFontFamily, QueryFontMetricsFlags};
use style::values::computed::{CSSPixelLength, Length};

use crate::error::StyleError;
use crate::handle::{ElementHandle, NodeArena};
use crate::store::StyleStore;
use crate::traversal::RecalcStyle;

/// A [`FontMetricsProvider`] that always reports zeroed metrics with a fixed x-height
/// (`0.5em`, per the brief). M1a has no font shaping yet — a real provider backed by
/// `cl-fonts` lands with the DOM/layout integration in Task 11 and beyond.
#[derive(Debug)]
struct NullFontMetricsProvider;

impl FontMetricsProvider for NullFontMetricsProvider {
    fn query_font_metrics(
        &self,
        _vertical: bool,
        _font: &Font,
        base_size: CSSPixelLength,
        _flags: QueryFontMetricsFlags,
    ) -> FontMetrics {
        FontMetrics {
            x_height: Some(base_size.scale_by(0.5)),
            ..FontMetrics::default()
        }
    }

    fn base_size_for_generic(&self, _generic: GenericFontFamily) -> Length {
        Length::new(16.0)
    }
}

/// Owns one document's stylo `Stylist`: the `Device` (viewport, media type, font metrics),
/// the `SharedRwLock` protecting every stylesheet reachable from it, and the user-agent
/// and author stylesheets appended so far (tracked manually, mirroring Blitz rather than
/// stylo's `DocumentStylesheetSet`).
pub struct StyleEngine {
    lock: SharedRwLock,
    stylist: Stylist,
    /// `Origin::UserAgent` sheets, in the order they were appended. Populated by
    /// `add_ua_sheet` (`sheets.rs`).
    ua: Vec<DocumentStyleSheet>,
    /// `Origin::Author` sheets, in the order they were appended: author stylesheets from
    /// [`StyleEngine::add_author_sheet`] plus, once collected, every `<style>`/`<link>`
    /// sheet `collect_document_sheets` (`sheets.rs`) finds in a document, in document
    /// order.
    author: Vec<DocumentStyleSheet>,
}

impl StyleEngine {
    /// Construct a new engine for a `viewport` of `(width, height)` CSS pixels at device
    /// pixel ratio `dpr`.
    ///
    /// # Errors
    /// Returns [`StyleError::Device`] if `viewport` or `dpr` are not finite, positive values.
    pub fn new(viewport: (f32, f32), dpr: f32) -> Result<Self, StyleError> {
        let (width, height) = viewport;
        let dimensions_valid =
            width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0;
        let dpr_valid = dpr.is_finite() && dpr > 0.0;
        if !dimensions_valid || !dpr_valid {
            return Err(StyleError::Device);
        }

        let viewport_size = euclid::Size2D::new(width, height);
        let device_size = euclid::Size2D::new(width, height) * dpr;
        let device_pixel_ratio = euclid::Scale::new(dpr);

        let device = Device::new(
            MediaType::screen(),
            QuirksMode::NoQuirks,
            viewport_size,
            device_size,
            device_pixel_ratio,
            Box::new(NullFontMetricsProvider),
            ComputedValues::initial_values_with_font_override(Font::initial_values()),
            PrefersColorScheme::Light,
            PointerCapabilities::default(),
            PointerCapabilities::default(),
        );

        let stylist = Stylist::new(device, QuirksMode::NoQuirks);

        Ok(Self {
            lock: SharedRwLock::new(),
            stylist,
            ua: Vec::new(),
            author: Vec::new(),
        })
    }

    /// Parse `css` as an author-origin stylesheet resolved against base URL `base`, and
    /// append it to the stylist.
    ///
    /// # Errors
    /// Returns [`StyleError::Url`] if `base` cannot be parsed as an absolute URL.
    pub fn add_author_sheet(&mut self, css: &str, base: &str) -> Result<(), StyleError> {
        let sheet = self.build_and_append(css, base, Origin::Author)?;
        self.author.push(sheet);
        Ok(())
    }

    /// Parse `css` as a user-agent-origin stylesheet resolved against base URL `base`, and
    /// append it to the stylist. `pub(crate)`: `sheets.rs`'s public `add_ua_sheet` is the
    /// real entry point (it always passes the bundled `assets/ua.css`); this exists so that
    /// code lives in `sheets.rs` per the task brief while still reaching the private
    /// `stylist`/`lock`/`ua` fields declared here.
    ///
    /// # Errors
    /// Returns [`StyleError::Url`] if `base` cannot be parsed as an absolute URL.
    pub(crate) fn add_ua_sheet_from(&mut self, css: &str, base: &str) -> Result<(), StyleError> {
        let sheet = self.build_and_append(css, base, Origin::UserAgent)?;
        self.ua.push(sheet);
        Ok(())
    }

    /// Parses `css` as a stylesheet of the given `origin` resolved against base URL `base`,
    /// and appends it to the stylist. Shared by [`StyleEngine::add_author_sheet`] and
    /// [`StyleEngine::add_ua_sheet_from`], which differ only in `origin` and which
    /// per-origin list they push the returned handle onto.
    ///
    /// # Errors
    /// Returns [`StyleError::Url`] if `base` cannot be parsed as an absolute URL.
    fn build_and_append(
        &mut self,
        css: &str,
        base: &str,
        origin: Origin,
    ) -> Result<DocumentStyleSheet, StyleError> {
        let base_url = url::Url::parse(base).map_err(|e| StyleError::Url(e.to_string()))?;
        let url_data = UrlExtraData::from(base_url);

        let sheet = Stylesheet::from_str(
            css,
            url_data,
            origin,
            StyloArc::new(self.lock.wrap(MediaList::empty())),
            self.lock.clone(),
            /* stylesheet_loader = */ None,
            /* error_reporter = */ None,
            QuirksMode::NoQuirks,
            AllowImportRules::No,
        );
        let sheet = DocumentStyleSheet(StyloArc::new(sheet));

        self.stylist
            .append_stylesheet(sheet.clone(), &self.lock.read());

        Ok(sheet)
    }

    /// Total number of top-level CSS rules across every author stylesheet appended so far
    /// (via [`StyleEngine::add_author_sheet`] or `sheets.rs`'s `collect_document_sheets`).
    pub fn author_rule_count(&self) -> usize {
        self.rule_counts(&self.author).iter().sum()
    }

    /// Per-sheet top-level CSS rule counts of every author stylesheet, in the order the
    /// sheets were appended.
    ///
    /// Exists so callers (mainly tests) can confirm sheets were appended in a particular
    /// order — e.g. document order, for `collect_document_sheets` — without needing the
    /// cascade itself to observe the effect.
    pub fn author_rule_counts(&self) -> Vec<usize> {
        self.rule_counts(&self.author)
    }

    /// Total number of top-level CSS rules across every user-agent stylesheet appended so
    /// far (in practice, just the one bundled `assets/ua.css`).
    pub fn ua_rule_count(&self) -> usize {
        self.rule_counts(&self.ua).iter().sum()
    }

    /// Runs one full style pass over `doc` and returns it together with the computed
    /// values the cascade produced for every element.
    ///
    /// This is the entry point the whole crate exists for. It:
    ///
    /// 1. flushes the `Stylist` so every sheet appended since the last pass is in the
    ///    cascade data (`Stylist::flush`, `stylo-0.20.0/stylist.rs:1055`);
    /// 2. builds a fresh [`StyleStore`] sized to `doc.len()` and locked with **this
    ///    engine's** [`SharedRwLock`] — never a new one: `Locked::read_with` panics when
    ///    handed a guard from a different lock, and the inline `style` attributes the store
    ///    caches are read through the guard built here;
    /// 3. seeds the root element with `RestyleHint::restyle_subtree()`, which is what makes
    ///    stylo walk past the root at all (`recalc_style_at` only visits children when the
    ///    propagated hint is non-empty or the dirty-descendants bit is set —
    ///    `stylo-0.20.0/traversal.rs:456`);
    /// 4. drives `style::driver::traverse_dom(&traversal, token, None)` — `None` thread
    ///    pool, sequential only, see [`crate::traversal`];
    /// 5. lifts each element's primary style out of the store and drops the store, so no
    ///    stylo `ElementData` outlives the pass.
    ///
    /// A document with no root element (nothing `cl-html` can produce, but a directly
    /// built [`Document`] can be empty) is returned unstyled rather than rejected.
    ///
    /// # Errors
    /// Currently never: every step is infallible over an already-parsed document, and
    /// malformed CSS or a malformed `style` attribute is dropped by the parser rather than
    /// reported. `Result` is the signature so the resource limits M1b will need (a cap on
    /// element count, say) can be added without breaking `cl-layout`.
    #[allow(
        clippy::unnecessary_wraps,
        reason = "infallible today by design; Result kept for the resource limits M1b adds"
    )]
    pub fn resolve(&mut self, doc: Document) -> Result<StyledDocument, StyleError> {
        ensure_layout_thread_state();

        // The guard must come from a *clone* of the lock rather than `self.lock`, or it
        // would borrow `self` for as long as it lives and `self.stylist.flush` below could
        // not take `&mut`. A clone shares the same underlying lock, so the guard is still
        // valid for every `Locked` value this engine ever wrapped.
        let lock = self.lock.clone();
        let guard = lock.read();
        let guards = StylesheetGuards::same(&guard);
        // Discarded: an initial pass has nothing to invalidate — every element is styled
        // from scratch — so the returned invalidation set has no one to apply to.
        let _invalidations = self.stylist.flush(&guards);

        let store = StyleStore::new(doc.len(), self.lock.clone());
        let styles = cascade_document(&self.stylist, guards, &doc, &store);
        drop(store);

        Ok(StyledDocument {
            doc,
            styles: styles.into_boxed_slice(),
        })
    }

    /// Per-sheet top-level CSS rule counts of `sheets`, in order.
    fn rule_counts(&self, sheets: &[DocumentStyleSheet]) -> Vec<usize> {
        let guard = self.lock.read();
        sheets
            .iter()
            .map(|sheet| {
                sheet
                    .0
                    .contents
                    .read_with(&guard)
                    .rules
                    .read_with(&guard)
                    .0
                    .len()
            })
            .collect()
    }
}

/// A [`Document`] plus the primary [`ComputedValues`] the cascade produced for each of its
/// elements, indexed by [`NodeId`].
///
/// Produced by [`StyleEngine::resolve`] and consumed by `cl-layout` (Task 16). The stylo
/// `ElementData` the traversal worked in lives only for the duration of that call; what
/// survives here is one reference-counted `ComputedValues` per element, so a styled
/// document holds no borrow of the engine and no interior-mutable per-pass state.
pub struct StyledDocument {
    /// The document that was styled, owned so `cl-layout` can walk it alongside the styles.
    doc: Document,
    /// `styles[id.index()]` is the primary style of node `id`, or `None` for a node that is
    /// not an element or that the traversal never reached (the descendants of a
    /// `display: none` element, which stylo deliberately skips).
    styles: Box<[Option<StyloArc<ComputedValues>>]>,
}

impl StyledDocument {
    /// The document these styles belong to.
    pub fn document(&self) -> &Document {
        &self.doc
    }

    /// The primary computed style of `id`, or `None` if `id` is not a styled element.
    ///
    /// `None` covers three cases, all legitimate: the node is not an element (text,
    /// comment, doctype, the document node itself); the node is a descendant of a
    /// `display: none` element, whose subtree stylo does not cascade
    /// (`stylo-0.20.0/traversal.rs:459`); or the id does not belong to this document.
    pub fn computed(&self, id: NodeId) -> Option<&ComputedValues> {
        self.styles.get(id.index())?.as_deref()
    }

    /// Gives the document back, dropping the computed styles.
    pub fn into_document(self) -> Document {
        self.doc
    }
}

/// An empty `RegisteredSpeculativePainters`: `SharedStyleContext` requires one, and the CSS
/// Painting API (`paint()` images evaluated speculatively during styling) is not part of
/// `ChromeLight`. Reporting "no painter registered" makes
/// `traversal::notify_paint_worklet` (`stylo-0.20.0/traversal.rs:640`) skip every image.
struct NoSpeculativePainters;

impl RegisteredSpeculativePainters for NoSpeculativePainters {
    fn get(&self, _name: &Atom) -> Option<&dyn RegisteredSpeculativePainter> {
        None
    }
}

/// Marks the calling thread as a layout thread if it has no state yet.
///
/// stylo's `SequentialTaskList::drop` and `SequentialTask::execute`
/// (`stylo-0.20.0/context.rs:466` and `:527`) `debug_assert!` that they run on a thread
/// whose `thread_state` contains `LAYOUT`. The list is part of `ThreadLocalStyleContext`,
/// which `traverse_dom` creates and drops around every pass, so without this a debug build
/// (every `cargo test` run) would abort inside the traversal.
///
/// `thread_state::initialize` panics if the thread was already initialized to something
/// *else*, so it is only called on a thread with no state at all — re-entering `resolve()`
/// on the same thread, or on a thread stylo already marked, is a no-op.
fn ensure_layout_thread_state() {
    if thread_state::get().is_empty() {
        thread_state::initialize(ThreadState::LAYOUT);
    }
}

/// The document element: the first element child of the document node (`<html>` for
/// anything `cl-html` parses). `None` for a document that contains no element at all.
fn root_element(doc: &Document) -> Option<NodeId> {
    doc.children(doc.root())
        .find(|id| doc.element(*id).is_some())
}

/// Cascades `doc` in place into `store` and lifts the result out as one
/// `Option<Arc<ComputedValues>>` per arena node, indexed by `NodeId::index`.
///
/// Split out of [`StyleEngine::resolve`] because the `SharedStyleContext` borrows several
/// stack locals (the snapshot map, the painter registry) that must not outlive the
/// traversal, and a function body is the clearest way to bound them.
fn cascade_document(
    stylist: &Stylist,
    guards: StylesheetGuards<'_>,
    doc: &Document,
    store: &StyleStore,
) -> Vec<Option<StyloArc<ComputedValues>>> {
    let mut styles: Vec<Option<StyloArc<ComputedValues>>> = vec![None; doc.len()];

    let Some(root_id) = root_element(doc) else {
        return styles;
    };

    // Every field is required by `SharedStyleContext` (`stylo-0.20.0/context.rs:126..159`).
    // `animations` and `registered_speculative_painters` exist only under
    // `#[cfg(feature = "servo")]`, which is the feature set this workspace pins stylo to.
    let snapshots = SnapshotMap::new();
    let painters = NoSpeculativePainters;
    let shared = SharedStyleContext {
        stylist,
        // No `:visited` support (Task 11 implements `:link`/`:any-link` only), and no
        // history to drive it from.
        visited_styles_enabled: false,
        options: StyleSystemOptions::default(),
        guards,
        // Fixed rather than `Instant::now()`: M1a runs no animations, and a wall-clock read
        // here would make the pass non-deterministic for no benefit.
        current_time_for_animations: 0.0,
        traversal_flags: TraversalFlags::empty(),
        // Snapshots record pre-mutation element state so a *re*style can invalidate
        // correctly. Each document is styled exactly once, so the map stays empty and
        // `TElement::has_snapshot` is always false.
        snapshot_map: &snapshots,
        animations: DocumentAnimationSet::default(),
        registered_speculative_painters: &painters,
    };

    // One slot per node, holding the `(&Document, &StyleStore, NodeId)` triple; a handle is
    // a reference into it. See `crate::handle`'s docs for why the handle itself has to be a
    // single word.
    let arena = NodeArena::new(doc, store);
    let Some(root) = arena.element(root_id) else {
        return styles;
    };
    // Without this the traversal styles the root element and stops: `recalc_style_at`
    // only descends when the hint it propagates is non-empty or the element already has
    // the dirty-descendants bit, and a fresh `ElementData` has neither. `restyle_subtree()`
    // is `RESTYLE_SELF | RESTYLE_DESCENDANTS`, and `RestyleHint::propagate` turns
    // `RESTYLE_DESCENDANTS` back into a full `restyle_subtree()` for each child
    // (`stylo-0.20.0/invalidation/element/restyle_hints.rs:123`), so it carries all the way
    // down. This is the same "the whole document is dirty" seed a browser uses on first
    // layout.
    store
        .ensure_data(root_id)
        .hint
        .insert(RestyleHint::restyle_subtree());

    let traversal = RecalcStyle::new(shared);
    let token = <RecalcStyle<'_> as DomTraversal<ElementHandle<'_>>>::pre_traverse(
        root,
        traversal.shared_context(),
    );
    if token.should_traverse() {
        // `traverse_dom` returns the traversal root, which we already have. `None` is the
        // rayon pool: sequential only — see `crate::traversal`.
        let _root = style::driver::traverse_dom(&traversal, token, None);
    }

    for id in doc.descendants(doc.root()) {
        let Some(data) = store.get(id) else { continue };
        let Some(primary) = data.styles.get_primary() else {
            continue;
        };
        if let Some(slot) = styles.get_mut(id.index()) {
            *slot = Some(primary.clone());
        }
    }

    styles
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::expect_used)]
    fn stylist_should_accept_one_author_rule() {
        let mut engine = StyleEngine::new((800.0, 600.0), 1.0).expect("engine");
        engine
            .add_author_sheet("p { color: red }", "file:///test.html")
            .expect("sheet");
        assert_eq!(engine.author_rule_count(), 1);
    }

    #[test]
    #[allow(clippy::expect_used)]
    fn add_author_sheet_should_reject_invalid_base_url() {
        let mut engine = StyleEngine::new((800.0, 600.0), 1.0).expect("engine");
        let result = engine.add_author_sheet("p { color: red }", "not a url");
        assert!(matches!(result, Err(StyleError::Url(_))));
    }
}
