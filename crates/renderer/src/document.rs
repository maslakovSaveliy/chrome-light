//! [`ParsedDocument`] → [`LoadedDocument`] → [`Rendered`]: one document's way through the
//! pipeline, in the order the renderer process lives it — parse, ask for subresources,
//! receive them, render (and render again on every resize).

use std::collections::{HashMap, HashSet};

use cl_dom::Document;
use cl_dom::serialize::dom_dump;
use cl_fonts::FontDb;
use cl_layout::dump::{box_tree_dump, fragment_tree_dump};
use cl_layout::{Viewport, box_tree};
use cl_net::Url;
use cl_paint::DisplayList;
use cl_paint::dump::display_list_dump;
use cl_style::dump::computed_style_dump;
use cl_style::{SheetWarning, StyleEngine, StyledDocument, stylesheet_requests};

use crate::RenderError;

/// Whether [`LoadedDocument::render`] builds the textual stage dumps ([`Stages`]) next to
/// the display list.
///
/// Opt-in because the dumps are large on a large document: on the 1 MB smoke page
/// (`crates/testshell/tests/smoke.rs`, ~55 000 DOM nodes) building them roughly triples the
/// render's peak memory (measurements: `docs/history/perf-m1a-2026-09.md`, debt D1). Only
/// `cl-testshell dump --stage` and tests that inspect a stage ask for them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StageDumps {
    /// Build none: [`Rendered::stages`] is `None`. The default.
    #[default]
    None,
    /// Build all five: [`Rendered::stages`] is `Some`.
    All,
}

/// One textual dump per pipeline stage, in pipeline order — exactly what
/// `cl-testshell dump --stage` prints.
///
/// Built by [`LoadedDocument::render`] only for [`StageDumps::All`]. [`Default`] is five
/// empty strings, the state a render fills in stage by stage.
#[derive(Debug, Clone, Default)]
pub struct Stages {
    /// [`cl_dom::serialize::dom_dump`] of the parsed document, before styling.
    pub dom: String,
    /// [`cl_style::dump::computed_style_dump`] of the resolved cascade.
    pub style: String,
    /// [`cl_layout::dump::box_tree_dump`] of the box tree built from the styled document.
    pub box_tree: String,
    /// [`cl_layout::dump::fragment_tree_dump`] of the positioned fragment tree.
    pub fragments: String,
    /// [`cl_paint::dump::display_list_dump`] of the paint-order display list.
    pub display_list: String,
}

/// A parsed document that has not been styled yet: the DOM plus the subresources it needs.
pub struct ParsedDocument {
    document: Document,
    requests: Vec<Url>,
}

impl ParsedDocument {
    /// Parses `bytes` as an HTML document whose base URL is `base`, and lists the
    /// subresources it references (see [`ParsedDocument::subresource_requests`]). Nothing is
    /// loaded.
    ///
    /// # Errors
    /// [`RenderError::Html`] if parsing fails (uninhabited today). Never panics on malformed
    /// `bytes`.
    pub fn parse(bytes: &[u8], base: &Url) -> Result<ParsedDocument, RenderError> {
        let document = cl_html::parse_document(bytes, base, None)?.document;
        // An unresolvable `href` is dropped here: the style pass reports the same warning for
        // it on every render (`collect_document_sheets`), so it is not stored twice.
        let (urls, _unresolvable) = stylesheet_requests(&document);
        let mut seen = HashSet::new();
        let requests = urls
            .into_iter()
            .filter(|url| seen.insert(url.clone()))
            .collect();
        Ok(ParsedDocument { document, requests })
    }

    /// The subresources the document references — in M1b its `<link rel~=stylesheet>`
    /// sheets — resolved against its base URL, in document order, each URL once. These are
    /// the keys [`LoadedDocument::render`] looks up in the [`SubresourceMap`].
    pub fn subresource_requests(&self) -> &[Url] {
        &self.requests
    }

    /// Combines the document with the loaded (or refused) subresources. A request with no
    /// entry in `subresources` renders as a warning, like a refused one.
    pub fn attach(self, subresources: SubresourceMap) -> LoadedDocument {
        LoadedDocument {
            document: Some(self.document),
            subresources,
        }
    }
}

/// The outcome of loading each requested subresource: its bytes, or why it is missing.
#[derive(Debug, Default)]
pub struct SubresourceMap {
    entries: HashMap<Url, Result<Vec<u8>, String>>,
}

impl SubresourceMap {
    /// An empty map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the loaded bytes of `url`.
    pub fn insert_ok(&mut self, url: Url, bytes: Vec<u8>) {
        self.entries.insert(url, Ok(bytes));
    }

    /// Records that `url` could not be loaded, and why (a policy refusal, an I/O error); the
    /// reason becomes the cause of the warning the render reports for it.
    pub fn insert_err(&mut self, url: Url, reason: String) {
        self.entries.insert(url, Err(reason));
    }

    /// What the style pass gets for `url`.
    ///
    /// The bytes are cloned on every render: `StyleEngine::collect_document_sheets` takes
    /// owned bytes and the engine is rebuilt per render. Bounded by what the browser sends
    /// (≤ 64 MiB per document, ADR-0016 §6) and paid once per resize.
    fn lookup(&self, url: &Url) -> Result<Vec<u8>, String> {
        match self.entries.get(url) {
            Some(Ok(bytes)) => Ok(bytes.clone()),
            Some(Err(reason)) => Err(reason.clone()),
            None => Err("not provided to the renderer".to_owned()),
        }
    }
}

/// A parsed document with its subresources, ready to render at any viewport, any number of
/// times.
///
/// `// M1-ONLY: full re-style on resize`: every [`LoadedDocument::render`] builds a new
/// [`StyleEngine`] for the viewport, re-adds the sheets and runs style, layout and paint
/// from the stored DOM. Nothing but the DOM and the subresource bytes survives between
/// renders, so a render at a size equals a fresh document's render at that size.
pub struct LoadedDocument {
    /// `None` only after a style pass failed and consumed the document (see
    /// [`RenderError::DocumentLost`]).
    document: Option<Document>,
    subresources: SubresourceMap,
}

/// One render's output.
#[derive(Debug, Clone)]
pub struct Rendered {
    /// The paint-order display list for the viewport.
    pub display_list: DisplayList,
    /// One line per stylesheet that could not be added (`<url or href>: <cause>`): an
    /// unresolvable `href`, a refused or missing subresource. Never a render failure.
    pub warnings: Vec<String>,
    /// The stage dumps: `Some` exactly when [`StageDumps::All`] was asked for.
    pub stages: Option<Stages>,
}

impl LoadedDocument {
    /// Styles, lays out and paints the document for a viewport of `viewport` CSS pixels at
    /// device pixel ratio 1, shaping text with `fonts`.
    ///
    /// # Errors
    /// [`RenderError::Style`] for a zero-sized viewport (the document stays usable);
    /// [`RenderError::Layout`] on a font failure; [`RenderError::DocumentLost`] if an earlier
    /// render lost the document. Never panics on any document.
    pub fn render(
        &mut self,
        viewport: (u32, u32),
        fonts: &mut FontDb,
        dumps: StageDumps,
    ) -> Result<Rendered, RenderError> {
        let document = self.document.take().ok_or(RenderError::DocumentLost)?;
        let mut stages = (dumps == StageDumps::All).then(Stages::default);
        if let Some(stages) = stages.as_mut() {
            stages.dom = dom_dump(&document);
        }

        #[allow(
            clippy::cast_precision_loss,
            reason = "viewport dimensions are well under f32's 24-bit exact integer range"
        )]
        let size = (viewport.0 as f32, viewport.1 as f32);
        let (styled, warnings) = match self.style(document, size) {
            Ok(styled) => styled,
            Err((error, document)) => {
                self.document = document;
                return Err(error);
            }
        };

        let painted = paint(&styled, size, fonts, stages.as_mut());
        self.document = Some(styled.into_document());
        Ok(Rendered {
            display_list: painted?,
            warnings,
            stages,
        })
    }

    /// The style pass: a fresh engine for `size`, the UA sheet, the document's sheets (from
    /// the subresource map), the cascade. On failure the document comes back with the error
    /// unless the cascade itself consumed it.
    #[allow(
        clippy::type_complexity,
        reason = "a private helper's (error, document) pair; naming it would add nothing"
    )]
    fn style(
        &self,
        document: Document,
        size: (f32, f32),
    ) -> Result<(StyledDocument, Vec<String>), (RenderError, Option<Document>)> {
        let mut engine = match StyleEngine::new(size, 1.0) {
            Ok(engine) => engine,
            Err(e) => return Err((e.into(), Some(document))),
        };
        if let Err(e) = engine.add_ua_sheet() {
            return Err((e.into(), Some(document)));
        }
        let sheet_warnings =
            match engine.collect_document_sheets(&document, &|url| self.subresources.lookup(url)) {
                Ok(warnings) => warnings,
                Err(e) => return Err((e.into(), Some(document))),
            };
        let styled = engine.resolve(document).map_err(|e| (e.into(), None))?;
        Ok((styled, sheet_warnings.iter().map(format_warning).collect()))
    }
}

/// Layout and paint of a styled document, filling the remaining stage dumps.
fn paint(
    styled: &StyledDocument,
    size: (f32, f32),
    fonts: &mut FontDb,
    mut stages: Option<&mut Stages>,
) -> Result<DisplayList, RenderError> {
    if let Some(stages) = stages.as_mut() {
        stages.style = computed_style_dump(styled);
        // Built only for its dump: `cl_layout::layout` below builds (and drops) its own.
        stages.box_tree = box_tree_dump(&box_tree::build(styled));
    }
    let fragment_tree = cl_layout::layout(styled, Viewport::new(size.0, size.1), fonts)?;
    if let Some(stages) = stages.as_mut() {
        stages.fragments = fragment_tree_dump(&fragment_tree, styled.document(), fonts);
    }
    let display_list = cl_paint::build(&fragment_tree, styled.document());
    if let Some(stages) = stages {
        stages.display_list = display_list_dump(&display_list);
    }
    Ok(display_list)
}

/// One [`SheetWarning`] as a single human-readable line.
fn format_warning(warning: &SheetWarning) -> String {
    format!("{}: {}", warning.url_or_href, warning.cause)
}
