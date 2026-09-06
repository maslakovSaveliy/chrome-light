//! Headless shell used by reftests and (from M1) the WPT product adapter. Deterministic by
//! construction: fixed viewport, no system fonts, CPU raster.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

use std::path::Path;

use tiny_skia::{Color, Pixmap};

/// Testshell failure.
#[derive(Debug)]
pub enum ShellError {
    /// `--viewport` was not `WIDTHxHEIGHT` with both > 0.
    InvalidViewport(String),
    /// Pixmap allocation failed (zero size or too large).
    Alloc {
        /// Width.
        width: u32,
        /// Height.
        height: u32,
    },
    /// PNG decode failed.
    Png {
        /// File.
        path: String,
        /// Cause.
        source: String,
    },
    /// Two images differ in size.
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
    /// I/O.
    Io(std::io::Error),
}

impl std::fmt::Display for ShellError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShellError::InvalidViewport(s) => {
                write!(f, "invalid viewport {s:?}, expected WIDTHxHEIGHT")
            }
            ShellError::Alloc { width, height } => {
                write!(f, "cannot allocate {width}x{height} pixmap")
            }
            ShellError::Png { path, source } => write!(f, "png {path}: {source}"),
            ShellError::SizeMismatch { a_w, a_h, b_w, b_h } => {
                write!(f, "size mismatch: {a_w}x{a_h} vs {b_w}x{b_h}")
            }
            ShellError::Io(e) => write!(f, "io: {e}"),
        }
    }
}

impl std::error::Error for ShellError {}

impl From<std::io::Error> for ShellError {
    fn from(e: std::io::Error) -> Self {
        ShellError::Io(e)
    }
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

/// An opaque white canvas. M0-ONLY: the real pipeline (cl-html → … → cl-gfx) replaces this in M1;
/// the CLI and reftest harness stay the same.
pub fn render_blank(width: u32, height: u32) -> Result<Pixmap, ShellError> {
    let mut pm = Pixmap::new(width, height).ok_or(ShellError::Alloc { width, height })?;
    pm.fill(Color::WHITE);
    Ok(pm)
}

/// Count differing pixels between two PNGs of equal size.
pub fn compare_png(a: &Path, b: &Path) -> Result<Diff, ShellError> {
    let load = |p: &Path| {
        Pixmap::load_png(p).map_err(|e| ShellError::Png {
            path: p.display().to_string(),
            source: e.to_string(),
        })
    };
    let pa = load(a)?;
    let pb = load(b)?;
    if (pa.width(), pa.height()) != (pb.width(), pb.height()) {
        return Err(ShellError::SizeMismatch {
            a_w: pa.width(),
            a_h: pa.height(),
            b_w: pb.width(),
            b_h: pb.height(),
        });
    }
    let differing_pixels = pa
        .pixels()
        .iter()
        .zip(pb.pixels())
        .filter(|(x, y)| x != y)
        .count() as u64;
    Ok(Diff {
        differing_pixels,
        width: pa.width(),
        height: pa.height(),
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

    #[test]
    fn render_blank_should_be_opaque_white_of_requested_size() {
        let pm = render_blank(4, 3).expect("alloc");
        assert_eq!((pm.width(), pm.height()), (4, 3));
        assert!(pm.data().chunks(4).all(|px| px == [255, 255, 255, 255]));
    }

    #[test]
    fn compare_png_should_report_zero_for_identical_files() {
        let dir = std::env::temp_dir().join(format!("cl-ts-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let a = dir.join("a.png");
        let b = dir.join("b.png");
        render_blank(8, 8)
            .expect("alloc")
            .save_png(&a)
            .expect("save");
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
        render_blank(8, 8)
            .expect("alloc")
            .save_png(&a)
            .expect("save");
        let mut pm = render_blank(8, 8).expect("alloc");
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
        render_blank(8, 8)
            .expect("alloc")
            .save_png(&a)
            .expect("save");
        render_blank(9, 8)
            .expect("alloc")
            .save_png(&b)
            .expect("save");
        assert!(matches!(
            compare_png(&a, &b),
            Err(ShellError::SizeMismatch { .. })
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
