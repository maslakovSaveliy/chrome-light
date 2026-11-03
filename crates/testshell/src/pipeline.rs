//! The single-process render pipeline behind [`render_file`]/[`render_bytes`] for
//! `cl-testshell`'s `render`/`dump` commands and the reftest harness: the document pipeline
//! itself is [`cl_renderer`] (parse → subresources → render to a display list, M1b Task 5),
//! the same code the renderer process runs, followed by [`cl_gfx::cpu::rasterize`] — so the
//! three-process render and this one can be compared pixel for pixel.
//!
//! Where the renderer process asks the browser for its subresources over IPC, this shell
//! loads each one itself with [`cl_net::load_file`], synchronously, in this one process and
//! thread.
//!
//! Deterministic by construction, matching every other guarantee `docs/TESTING.md` §2 lists
//! for this shell: device pixel ratio is always `1.0` (not configurable — [`RenderOptions`]
//! only exposes the viewport size and the stage-dump switch, which never affects pixels), no
//! font hinting (`cl-layout`'s shaper and `cl-gfx`'s
//! rasteriser never enable it), and the only two fonts that can ever be selected are the
//! bundled ones from [`cl_fonts::FontDb::bundled`] — no host system font is ever read, so the
//! same document renders identically on every machine this crate builds on.

use std::path::Path;

use cl_fonts::FontDb;
use cl_net::Url;
use cl_renderer::{ParsedDocument, SubresourceMap};
use tiny_skia::Pixmap;

pub use cl_renderer::{StageDumps, Stages};

use crate::ShellError;

/// Render inputs that are not part of the document itself.
///
/// What a caller can vary is the viewport and whether the intermediate stage dumps are
/// built. Device pixel ratio, hinting and the font set are all fixed (see the module docs),
/// so the pixels are reproducible from nothing more than the document bytes, the base URL
/// and [`RenderOptions::viewport`]; [`RenderOptions::stage_dumps`] never changes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderOptions {
    /// Viewport size in CSS pixels, `(width, height)`.
    pub viewport: (u32, u32),
    /// Whether to build [`RenderOutput::stages`]. See [`StageDumps`].
    pub stage_dumps: StageDumps,
}

impl Default for RenderOptions {
    /// `800×600` (the same default the CLI's `--viewport` flag has always used), no stage
    /// dumps.
    fn default() -> Self {
        RenderOptions {
            viewport: (800, 600),
            stage_dumps: StageDumps::None,
        }
    }
}

/// The result of rendering one document: the rasterised pixels, the intermediate stages'
/// dumps if they were asked for, and any non-fatal stylesheet warnings collected along the
/// way.
#[derive(Debug, Clone)]
pub struct RenderOutput {
    /// The rasterised canvas: `opts.viewport` pixels, DPR 1.
    pub pixmap: Pixmap,
    /// Every intermediate stage's textual dump (see [`Stages`]): `Some` exactly when
    /// [`RenderOptions::stage_dumps`] was [`StageDumps::All`].
    pub stages: Option<Stages>,
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
pub fn render_file(path: &Path, opts: &RenderOptions) -> Result<RenderOutput, ShellError> {
    let canonical = path.canonicalize()?;
    let base = Url::from_file_path(&canonical)?;
    let bytes = cl_net::load_file(&base)?;
    render_bytes(&bytes, &base, opts)
}

/// Renders `bytes` — a document's raw bytes, resolved against `base` — with `opts`.
///
/// Runs, in this process and thread:
///
/// 1. [`cl_renderer::ParsedDocument::parse`] — `cl-html` decodes and parses `bytes` against
///    `base` (`transport_label` is always `None`: `cl-testshell` never speaks HTTP) and lists
///    the `<link rel~=stylesheet href>` sheets the document needs.
/// 2. Each of them is loaded with [`cl_net::load_file`], so a relative `href` resolves against
///    `base` and a `file:` link naming a remote host is refused exactly like the top-level
///    document would be. A sheet that fails to load becomes a warning in
///    [`RenderOutput::warnings`] instead of aborting the render (see that field's docs).
/// 3. [`cl_renderer::LoadedDocument::render`] — the bundled UA sheet, the document's own
///    `<style>`/`<link>` sheets, the cascade (`cl-style`), [`cl_layout::layout`] with a fresh
///    [`cl_fonts::FontDb::bundled`], and [`cl_paint::build()`] into a display list.
/// 4. [`cl_gfx::cpu::rasterize`] into [`RenderOutput::pixmap`].
///
/// With [`StageDumps::All`], [`RenderOutput::stages`] captures a dump of the tree at each
/// step (see [`Stages`]); with the default [`StageDumps::None`] no dump is built, and neither
/// is the extra box tree that only the box-tree dump needs ([`cl_layout::layout`] builds and
/// drops its own).
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
pub fn render_bytes(
    bytes: &[u8],
    base: &Url,
    opts: &RenderOptions,
) -> Result<RenderOutput, ShellError> {
    let (width, height) = opts.viewport;
    let painted = paint(bytes, base, opts)?;
    let pixmap = cl_gfx::cpu::rasterize(&painted.display_list, width, height, &painted.fonts)?;
    Ok(RenderOutput {
        pixmap,
        stages: painted.stages,
        warnings: painted.warnings,
    })
}

/// The display list [`render_bytes`] would rasterise for `bytes` — the pipeline up to and
/// including [`cl_paint::build()`], no raster. Stage dumps are never built here.
///
/// # Errors
/// As [`render_bytes`], minus the rasterisation errors.
pub fn display_list_for_bytes(
    bytes: &[u8],
    base: &Url,
    opts: &RenderOptions,
) -> Result<cl_paint::DisplayList, ShellError> {
    let opts = RenderOptions {
        stage_dumps: StageDumps::None,
        ..*opts
    };
    Ok(paint(bytes, base, &opts)?.display_list)
}

/// Everything [`render_bytes`] needs before rasterising.
struct Painted {
    display_list: cl_paint::DisplayList,
    fonts: FontDb,
    stages: Option<Stages>,
    warnings: Vec<String>,
}

/// [`cl_renderer`]'s pipeline with every subresource loaded from disk, with the stage dumps
/// when `opts` asks for them.
fn paint(bytes: &[u8], base: &Url, opts: &RenderOptions) -> Result<Painted, ShellError> {
    let parsed = ParsedDocument::parse(bytes, base)?;
    let mut subresources = SubresourceMap::new();
    for url in parsed.subresource_requests() {
        match cl_net::load_file(url) {
            Ok(bytes) => subresources.insert_ok(url.clone(), bytes),
            Err(e) => subresources.insert_err(url.clone(), e.to_string()),
        }
    }
    let mut fonts = FontDb::bundled()?;
    let rendered =
        parsed
            .attach(subresources)
            .render(opts.viewport, &mut fonts, opts.stage_dumps)?;
    Ok(Painted {
        display_list: rendered.display_list,
        fonts,
        stages: rendered.stages,
        warnings: rendered.warnings,
    })
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn render_bytes_should_produce_a_pixmap_of_the_requested_viewport() {
        let base = Url::parse("file:///pipeline/test.html").expect("base url");
        let opts = RenderOptions {
            viewport: (16, 8),
            ..RenderOptions::default()
        };
        let output = render_bytes(b"<!doctype html><p>hi</p>", &base, &opts).expect("render");
        assert_eq!(output.pixmap.width(), 16);
        assert_eq!(output.pixmap.height(), 8);
        assert!(output.warnings.is_empty());
    }

    #[test]
    fn render_bytes_should_populate_every_stage_dump_when_asked() {
        let base = Url::parse("file:///pipeline/test.html").expect("base url");
        let opts = RenderOptions {
            stage_dumps: StageDumps::All,
            ..RenderOptions::default()
        };
        let output = render_bytes(b"<!doctype html><p>hi</p>", &base, &opts).expect("render");
        let stages = output
            .stages
            .expect("StageDumps::All must populate RenderOutput::stages");
        assert!(stages.dom.contains("#document"));
        assert!(stages.style.contains("display"));
        assert!(!stages.box_tree.is_empty());
        assert!(stages.fragments.contains("Block"));
        assert!(!stages.display_list.is_empty());
    }

    #[test]
    fn render_bytes_should_skip_stage_dumps_by_default() {
        let base = Url::parse("file:///pipeline/test.html").expect("base url");
        let opts = RenderOptions::default();
        assert_eq!(opts.stage_dumps, StageDumps::None);
        let output = render_bytes(b"<!doctype html><p>hi</p>", &base, &opts).expect("render");
        assert!(
            output.stages.is_none(),
            "a render that did not ask for stage dumps must not build them"
        );
    }

    #[test]
    fn stage_dumps_should_not_change_the_pixels() {
        let base = Url::parse("file:///pipeline/test.html").expect("base url");
        let html = b"<!doctype html><p style=\"color:#c00\">hi <b>there</b></p>";
        let plain = render_bytes(html, &base, &RenderOptions::default()).expect("render");
        let dumped = render_bytes(
            html,
            &base,
            &RenderOptions {
                stage_dumps: StageDumps::All,
                ..RenderOptions::default()
            },
        )
        .expect("render");
        assert_eq!(plain.pixmap.data(), dumped.pixmap.data());
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
