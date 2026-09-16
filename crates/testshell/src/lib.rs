//! Headless shell used by reftests and (from M1) the WPT product adapter. Deterministic by
//! construction: fixed viewport, no system fonts, CPU raster — see [`pipeline`]'s module docs
//! for the full list of what is fixed and why.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod dump;
pub mod pipeline;
pub mod reftest;

use std::path::Path;

use tiny_skia::Pixmap;

pub use dump::{Stage, select};
pub use pipeline::{RenderOptions, RenderOutput, Stages, render_bytes, render_file};

/// Testshell failure.
#[derive(Debug, thiserror::Error)]
pub enum ShellError {
    /// `--viewport` was not `WIDTHxHEIGHT` with both > 0.
    #[error("invalid viewport {0:?}, expected WIDTHxHEIGHT")]
    InvalidViewport(String),
    /// `--stage` was not one of `dom`, `style`, `box-tree`, `fragments`, `display-list`.
    #[error("invalid stage {0:?}, expected dom|style|box-tree|fragments|display-list")]
    InvalidStage(String),
    /// PNG decode (or, for the CLI's own writes, encode) failed.
    #[error("png {path}: {cause}")]
    Png {
        /// File.
        path: String,
        /// Cause. Named `cause` rather than `source`: thiserror treats a field literally
        /// named `source` as `std::error::Error::source()` and requires it to implement
        /// `Error`, which a plain formatted `String` (from `Pixmap::load_png`'s or
        /// `Pixmap::save_png`'s own error type) does not.
        cause: String,
    },
    /// Two images differ in size.
    #[error("size mismatch: {a_w}x{a_h} vs {b_w}x{b_h}")]
    SizeMismatch {
        /// A width.
        a_w: u32,
        /// A height.
        a_h: u32,
        /// B width.
        b_w: u32,
        /// B height.
        b_h: u32,
    },
    /// I/O (reading a PNG to compare, or resolving/canonicalising an input path).
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    /// `cl-net` failed to resolve a URL or load a resource (the document itself, or a
    /// `<link rel=stylesheet>` — see [`pipeline::render_bytes`]).
    #[error("net: {0}")]
    Net(#[from] cl_net::NetError),
    /// `cl-html` failed to parse the document. Uninhabited today — see
    /// [`cl_html::HtmlError`]'s docs — kept so a future fallible parse path needs no API
    /// break here.
    #[error("html: {0}")]
    Html(#[from] cl_html::HtmlError),
    /// `cl-style` failed to construct the style engine, build a stylesheet, or resolve the
    /// cascade.
    #[error("style: {0}")]
    Style(#[from] cl_style::StyleError),
    /// The bundled font database failed to load. Expected only from a corrupted build — see
    /// [`cl_fonts::FontError`]'s docs — kept so that failure is a reported `Err` rather than
    /// a panic.
    #[error("fonts: {0}")]
    Font(#[from] cl_fonts::FontError),
    /// `cl-layout` failed to lay out the styled document (font plumbing only — see
    /// [`cl_layout::LayoutError`]).
    #[error("layout: {0}")]
    Layout(#[from] cl_layout::LayoutError),
    /// `cl-gfx` failed to rasterise the display list.
    #[error("gfx: {0}")]
    Gfx(#[from] cl_gfx::GfxError),
}

/// Result of comparing two PNGs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Diff {
    /// Pixels whose RGBA differs.
    pub differing_pixels: u64,
    /// Common width.
    pub width: u32,
    /// Common height.
    pub height: u32,
}

/// Parse `WIDTHxHEIGHT`.
pub fn parse_viewport(s: &str) -> Result<(u32, u32), ShellError> {
    let bad = || ShellError::InvalidViewport(s.to_owned());
    let (w, h) = s.split_once('x').ok_or_else(bad)?;
    let w: u32 = w.parse().map_err(|_| bad())?;
    let h: u32 = h.parse().map_err(|_| bad())?;
    if w == 0 || h == 0 {
        return Err(bad());
    }
    Ok((w, h))
}

/// Count differing pixels between two PNGs of equal size.
pub fn compare_png(a: &Path, b: &Path) -> Result<Diff, ShellError> {
    let load = |p: &Path| {
        Pixmap::load_png(p).map_err(|e| ShellError::Png {
            path: p.display().to_string(),
            cause: e.to_string(),
        })
    };
    compare_pixmaps(&load(a)?, &load(b)?)
}

/// Count differing pixels between two already-decoded pixmaps of equal size — the in-memory
/// core [`compare_png`] loads two PNGs into before delegating here, and what
/// [`reftest::run_pair`] uses directly (a reftest pair is rendered straight to a [`Pixmap`],
/// so round-tripping it through a PNG file just to diff it would be pure overhead).
pub fn compare_pixmaps(a: &Pixmap, b: &Pixmap) -> Result<Diff, ShellError> {
    if (a.width(), a.height()) != (b.width(), b.height()) {
        return Err(ShellError::SizeMismatch {
            a_w: a.width(),
            a_h: a.height(),
            b_w: b.width(),
            b_h: b.height(),
        });
    }
    let differing_pixels = a
        .pixels()
        .iter()
        .zip(b.pixels())
        .filter(|(x, y)| x != y)
        .count() as u64;
    Ok(Diff {
        differing_pixels,
        width: a.width(),
        height: a.height(),
    })
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn parse_viewport_should_accept_width_x_height() {
        assert_eq!(parse_viewport("800x600").expect("ok"), (800, 600));
    }

    #[test]
    fn parse_viewport_should_reject_zero_and_garbage() {
        assert!(parse_viewport("0x600").is_err());
        assert!(parse_viewport("800").is_err());
        assert!(parse_viewport("axb").is_err());
        assert!(parse_viewport("800x600x1").is_err());
    }

    fn write_png(width: u32, height: u32, path: &Path) {
        let opts = RenderOptions {
            viewport: (width, height),
        };
        let base = cl_net::Url::parse("file:///lib-test/x.html").expect("base url");
        render_bytes(b"<!doctype html>", &base, &opts)
            .expect("render")
            .pixmap
            .save_png(path)
            .expect("save");
    }

    #[test]
    fn compare_png_should_report_zero_for_identical_files() {
        let dir = std::env::temp_dir().join(format!("cl-ts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let a = dir.join("a.png");
        let b = dir.join("b.png");
        write_png(8, 8, &a);
        std::fs::copy(&a, &b).expect("copy");
        let d = compare_png(&a, &b).expect("compare");
        assert_eq!(
            d,
            Diff {
                differing_pixels: 0,
                width: 8,
                height: 8
            }
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn compare_png_should_count_one_changed_pixel() {
        let dir = std::env::temp_dir().join(format!("cl-ts2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let a = dir.join("a.png");
        let b = dir.join("b.png");
        write_png(8, 8, &a);
        let mut pm = Pixmap::load_png(&a).expect("reload");
        #[allow(clippy::expect_used)]
        if let Some(px) = pm.pixels_mut().get_mut(10) {
            *px = tiny_skia::PremultipliedColorU8::from_rgba(0, 0, 0, 255).expect("color");
        }
        pm.save_png(&b).expect("save");
        let d = compare_png(&a, &b).expect("compare");
        assert_eq!(d.differing_pixels, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn compare_png_should_fail_on_size_mismatch() {
        let dir = std::env::temp_dir().join(format!("cl-ts3-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let a = dir.join("a.png");
        let b = dir.join("b.png");
        write_png(8, 8, &a);
        write_png(9, 8, &b);
        assert!(matches!(
            compare_png(&a, &b),
            Err(ShellError::SizeMismatch { .. })
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
