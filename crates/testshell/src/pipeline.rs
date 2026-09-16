//! The M1a single-process render pipeline: `cl-html` → `cl-dom` → `cl-style` → `cl-layout` →
//! `cl-paint` → `cl-gfx`, wired together the same way `crates/gfx/tests/pipeline.rs` chains
//! them, now behind [`render_file`]/[`render_bytes`] for `cl-testshell`'s `render`/`dump`
//! commands and (Task 23) the reftest harness.
//!
//! `// M1a-ONLY`: every stage below runs synchronously in this one process and thread. From
//! M1b each stage moves into its own sandboxed process talking typed, validated IPC
//! (`docs/ARCHITECTURE.md`); [`render_file`]/[`render_bytes`]'s signatures are not expected to
//! change when that happens, only what runs behind them.
//!
//! Deterministic by construction, matching every other guarantee `docs/TESTING.md` §2 lists
//! for this shell: device pixel ratio is always `1.0` (not configurable — [`RenderOptions`]
//! only exposes the viewport size), no font hinting (`cl-layout`'s shaper and `cl-gfx`'s
//! rasteriser never enable it), and the only two fonts that can ever be selected are the
//! bundled ones from [`cl_fonts::FontDb::bundled`] — no host system font is ever read, so the
//! same document renders identically on every machine this crate builds on.

use std::path::Path;

use cl_dom::serialize::dom_dump;
use cl_fonts::FontDb;
use cl_layout::dump::{box_tree_dump, fragment_tree_dump};
use cl_layout::{Viewport, box_tree};
use cl_net::{NetError, Url};
use cl_paint::dump::display_list_dump;
use cl_style::dump::computed_style_dump;
use cl_style::{SheetWarning, StyleEngine};
use tiny_skia::Pixmap;

use crate::ShellError;

/// Render inputs that are not part of the document itself.
///
/// The viewport is the only thing a caller can vary: device pixel ratio, hinting and the
/// font set are all fixed (see the module docs) so a render is reproducible from nothing
/// more than the document bytes, the base URL, and this struct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderOptions {
    /// Viewport size in CSS pixels, `(width, height)`.
    pub viewport: (u32, u32),
}

impl Default for RenderOptions {
    /// `800×600`, the same default the CLI's `--viewport` flag has always used.
    fn default() -> Self {
        RenderOptions {
            viewport: (800, 600),
        }
    }
}

/// One textual dump per pipeline stage, in pipeline order — exactly what `dump --stage`
/// prints (see [`crate::dump::Stage`]).
///
/// Computed eagerly by [`render_bytes`] alongside the pixels: producing all five dumps is
/// cheap relative to rasterising a page (they are string renderings of trees the pipeline
/// builds regardless), so keeping [`RenderOutput`] a plain, fully-populated struct is
/// simpler than making each field lazy. Revisit if a benchmark ever shows this dominating
/// render time on a large document.
#[derive(Debug, Clone)]
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

/// The result of rendering one document: the rasterised pixels, every intermediate stage's
/// dump, and any non-fatal stylesheet warnings collected along the way.
#[derive(Debug, Clone)]
pub struct RenderOutput {
    /// The rasterised canvas: `opts.viewport` pixels, DPR 1.
    pub pixmap: Pixmap,
    /// Every intermediate stage's textual dump (see [`Stages`]).
    pub stages: Stages,
    /// One formatted line per [`cl_style::SheetWarning`] collected while loading
    /// `<link rel=stylesheet>` sheets referenced by the document.
    ///
    /// Not part of the brief's original `RenderOutput` sketch: added because
    /// [`cl_style::StyleEngine::collect_document_sheets`] reports a missing or unloadable
    /// external stylesheet as a warning rather than a hard failure (the module docs on
    /// [`render_bytes`] explain why a render must not fail over it), and that information
    /// has to surface *somewhere* rather than being silently swallowed. The CLI prints these
    /// to stderr with `render`/`dump`; tests can assert on them directly instead of
    /// scraping stderr.
    pub warnings: Vec<String>,
}

/// Renders the document at `path` (a local file) with `opts`.
///
/// `path` is canonicalised (so a relative CLI argument resolves the same way an absolute one
/// would, and the document's base URL — used to resolve every relative `href`/`src`/`@import`
/// in it — is always an absolute `file:` URL) and converted with [`cl_net::Url::from_file_path`].
/// Its bytes are then loaded through [`cl_net::load_file`] (the same size-capped loader a
/// `<link>` sheet is loaded through — see [`render_bytes`]) and handed to [`render_bytes`].
///
/// # Errors
/// [`ShellError::Io`] if `path` does not exist or cannot be canonicalised. [`ShellError::Net`]
/// if the canonical path cannot be expressed as a `file:` URL, or [`cl_net::load_file`] fails
/// (oversized file, I/O error). Otherwise see [`render_bytes`].
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "Task 22's brief fixes this exact signature (`opts: &RenderOptions`) verbatim; \
              `RenderOptions` happening to be two `u32`s today does not change the public API"
)]
pub fn render_file(path: &Path, opts: &RenderOptions) -> Result<RenderOutput, ShellError> {
    let canonical = path.canonicalize()?;
    let base = Url::from_file_path(&canonical)?;
    let bytes = cl_net::load_file(&base)?;
    render_bytes(&bytes, &base, opts)
}

/// Renders `bytes` — a document's raw bytes, resolved against `base` — with `opts`.
///
/// Runs the whole M1a static pipeline in this process, in order:
///
/// 1. [`cl_html::parse_document`] decodes and parses `bytes` into a [`cl_dom::Document`].
///    `transport_label` is always `None`: `cl-testshell` never speaks HTTP, so there is no
///    `Content-Type` charset to hand the sniffer.
/// 2. [`cl_style::StyleEngine`]: the bundled UA sheet
///    ([`cl_style::StyleEngine::add_ua_sheet`]), then every `<style>` and
///    `<link rel~=stylesheet href>` the document itself carries
///    ([`cl_style::StyleEngine::collect_document_sheets`]), loaded through a closure over
///    [`cl_net::load_file`] — so a relative `href` resolves against `base` and a `file:` link
///    naming a remote host is refused exactly like the top-level document would be. A sheet
///    that fails to resolve or load becomes a [`cl_style::SheetWarning`], collected into
///    [`RenderOutput::warnings`] instead of aborting the render (see that field's docs).
/// 3. [`cl_layout::layout`] over the resolved [`cl_style::StyledDocument`] and a fresh
///    [`cl_fonts::FontDb::bundled`].
/// 4. [`cl_paint::build()`] into a [`cl_paint::DisplayList`].
/// 5. [`cl_gfx::cpu::rasterize`] into [`RenderOutput::pixmap`].
///
/// [`RenderOutput::stages`] captures a dump of the tree at each step (see [`Stages`]).
///
/// # Errors
/// [`ShellError::Html`] if parsing fails (see [`cl_html::HtmlError`] — uninhabited today).
/// [`ShellError::Style`] if the style engine cannot be constructed, a stylesheet cannot be
/// built, or `base` is not a valid document base URL. [`ShellError::Font`] if the bundled font
/// database fails to load (see [`cl_fonts::FontError`] — expected only from a corrupted
/// build). [`ShellError::Layout`] if layout fails (font plumbing only — see
/// [`cl_layout::LayoutError`]). [`ShellError::Gfx`] if rasterising fails (oversized viewport,
/// or a font key the bundled database does not recognise). Never panics on malformed `bytes`:
/// every stage through `cl-layout`/`cl-paint` is documented total over its input, and
/// `cl-gfx` reports invalid geometry as `Err` rather than indexing out of bounds.
#[allow(
    clippy::trivially_copy_pass_by_ref,
    reason = "Task 22's brief fixes this exact signature (`opts: &RenderOptions`) verbatim; \
              `RenderOptions` happening to be two `u32`s today does not change the public API"
)]
pub fn render_bytes(
    bytes: &[u8],
    base: &Url,
    opts: &RenderOptions,
) -> Result<RenderOutput, ShellError> {
    let (width, height) = opts.viewport;
    #[allow(
        clippy::cast_precision_loss,
        reason = "viewport dimensions are well under f32's 24-bit exact integer range"
    )]
    let (width_px, height_px) = (width as f32, height as f32);

    let parsed = cl_html::parse_document(bytes, base, None)?;
    let dom = dom_dump(&parsed.document);

    let mut engine = StyleEngine::new((width_px, height_px), 1.0)?;
    engine.add_ua_sheet()?;
    let sheet_warnings = engine.collect_document_sheets(&parsed.document, &load_sheet)?;
    let warnings = sheet_warnings.iter().map(format_warning).collect();

    let styled = engine.resolve(parsed.document)?;
    let style = computed_style_dump(&styled);

    let tree = box_tree::build(&styled);
    let box_tree_text = box_tree_dump(&tree);

    let mut fonts = FontDb::bundled()?;
    let viewport = Viewport::new(width_px, height_px);
    let fragment_tree = cl_layout::layout(&styled, viewport, &mut fonts)?;
    let fragments = fragment_tree_dump(&fragment_tree, styled.document(), &fonts);

    let display_list = cl_paint::build(&fragment_tree, styled.document());
    let display_list_text = display_list_dump(&display_list);

    let pixmap = cl_gfx::cpu::rasterize(&display_list, width, height, &fonts)?;

    Ok(RenderOutput {
        pixmap,
        stages: Stages {
            dom,
            style,
            box_tree: box_tree_text,
            fragments,
            display_list: display_list_text,
        },
        warnings,
    })
}

/// The `load` callback [`cl_style::StyleEngine::collect_document_sheets`] uses to fetch a
/// `<link rel=stylesheet>`'s resolved URL: exactly [`cl_net::load_file`], so an external
/// sheet is size-capped the same way the top-level document is, and a `file:` URL naming a
/// remote host is refused rather than silently treated as "no such file".
fn load_sheet(url: &Url) -> Result<Vec<u8>, NetError> {
    cl_net::load_file(url)
}

/// Renders one [`SheetWarning`] as a single human-readable line for
/// [`RenderOutput::warnings`].
fn format_warning(warning: &SheetWarning) -> String {
    format!("{}: {}", warning.url_or_href, warning.cause)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn render_bytes_should_produce_a_pixmap_of_the_requested_viewport() {
        let base = Url::parse("file:///pipeline/test.html").expect("base url");
        let opts = RenderOptions { viewport: (16, 8) };
        let output = render_bytes(b"<!doctype html><p>hi</p>", &base, &opts).expect("render");
        assert_eq!(output.pixmap.width(), 16);
        assert_eq!(output.pixmap.height(), 8);
        assert!(output.warnings.is_empty());
    }

    #[test]
    fn render_bytes_should_populate_every_stage_dump() {
        let base = Url::parse("file:///pipeline/test.html").expect("base url");
        let output = render_bytes(
            b"<!doctype html><p>hi</p>",
            &base,
            &RenderOptions::default(),
        )
        .expect("render");
        assert!(output.stages.dom.contains("#document"));
        assert!(output.stages.style.contains("display"));
        assert!(!output.stages.box_tree.is_empty());
        assert!(output.stages.fragments.contains("Block"));
        assert!(!output.stages.display_list.is_empty());
    }

    #[test]
    fn render_bytes_should_warn_not_fail_on_a_missing_link() {
        let base = Url::parse("file:///pipeline/test.html").expect("base url");
        let html = br#"<!doctype html><link rel="stylesheet" href="missing.css"><p>hi</p>"#;
        let output =
            render_bytes(html, &base, &RenderOptions::default()).expect("render still succeeds");
        assert_eq!(output.warnings.len(), 1);
        assert!(
            output
                .warnings
                .first()
                .expect("just asserted len() == 1")
                .contains("missing.css")
        );
    }

    #[test]
    fn render_file_should_reject_a_missing_path() {
        let missing = std::env::temp_dir().join("cl-testshell-pipeline-does-not-exist.html");
        let err = render_file(&missing, &RenderOptions::default())
            .expect_err("missing file must be rejected");
        assert!(matches!(err, ShellError::Io(_)));
    }
}
