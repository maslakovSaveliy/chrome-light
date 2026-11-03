//! `ChromeLight` graphics (M1a Task 21): the CPU raster backend that turns a
//! [`cl_paint::DisplayList`] — the flat, paint-order draw commands `cl-paint` produced from
//! a laid-out page — into actual pixels.
//!
//! This is the last stage of the M1a static pipeline (`cl-html` → `cl-dom` → `cl-style` →
//! `cl-layout` → `cl-paint` → **`cl-gfx`** → `cl-testshell`'s PNG). One module:
//!
//! * [`cpu`] — [`cpu::rasterize`], `tiny-skia` for rects and borders, `swash` for glyph alpha
//!   masks, a whole-pixel clip stack and a per-`(face, glyph, size)` glyph cache.
//!
//! # Trust posture
//!
//! From M1b the display list arrives over shared memory from a sandboxed, potentially
//! compromised renderer (`docs/SECURITY.md` §3, the "renderer → gpu" row). Two consequences
//! shape this crate:
//!
//! * [`cpu::rasterize`] runs `cl_paint::validate` before it allocates anything, so a
//!   malformed list never reaches a draw call.
//! * `cl_paint::validate` deliberately does **not** bounds-check individual glyph positions
//!   against the canvas (its module docs say so), so every glyph blit here computes its
//!   destination with total arithmetic and writes through `get_mut`, never through an index.
//!   A glyph that lands wholly or partly off the canvas is clipped, never a panic and never
//!   an out-of-bounds write.
//!
//! The *font bytes*, by contrast, are trusted: `cl_fonts::FontDb` only ever serves the two
//! `include_bytes!`-embedded, checksummed bundled faces, so the only attacker-controlled
//! input to the glyph rasteriser is the requested size — which is why that is the thing
//! [`MAX_GLYPH_PX`] caps.
//!
//! # `unsafe`
//!
//! `#![forbid(unsafe_code)]`. `tools/check-unsafe-scope.sh` lists `crates/gfx/` among the
//! crates allowed to contain `unsafe`, but that allowance is for the `wgpu`/`vello` GPU
//! backends of M1b+, which talk to platform graphics APIs. The CPU backend has no FFI of its
//! own — `tiny-skia`, `swash` and `zeno` are pure Rust — so it forbids `unsafe` outright
//! rather than merely denying it.
//!
//! # Where `f32` is allowed
//!
//! Everything upstream of this crate works in [`cl_layout::Au`] (1/60 CSS pixel integers) and
//! `cl_layout::au`'s module docs make that a hard rule. This crate is the one legitimate
//! exception, and only at the raster boundary: `tiny_skia::Rect` and `swash`'s glyph scaler
//! both take `f32`, so a rect's four edges and a run's font size are converted exactly once,
//! with [`cl_layout::Au::to_px`], at the call that hands them to those libraries. No `f32`
//! is stored, accumulated, or compared anywhere in this crate.
//!
//! # Scope (M1a)
//!
//! Rasterised: solid rect fills, four-sided solid borders (no mitred corners), axis-aligned
//! rect clipping, and alpha-mask glyph runs at whole-pixel positions. Not implemented, all
//! deferred: GPU backends, `border-radius`, gradients, images, sub-pixel glyph positioning,
//! sub-pixel (LCD) anti-aliasing, colour fonts, transforms, opacity groups and layers.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod error;

pub mod cpu;

pub use error::GfxError;

/// Re-exported so a caller can name the return type of [`cpu::rasterize`] without depending
/// on `tiny-skia` itself (`cl-testshell`, Task 22, is the first such caller).
pub use tiny_skia::Pixmap;

/// The largest canvas edge, in device pixels, [`cpu::rasterize`] will allocate.
///
/// 16384 is the conventional maximum texture dimension across the GPU backends M1b will add,
/// so the CPU backend refusing anything larger keeps the two paths agreeing about which
/// canvas sizes exist. A `u32` pair past this would also be a multi-gigabyte allocation:
/// `tiny_skia::Pixmap::new` itself only rejects sizes past `i32::MAX / 4`, which is far too
/// permissive for a process that must survive a hostile display list.
pub const MAX_DIMENSION: u32 = 16_384;

/// The largest font size, in CSS pixels, a glyph will be rasterised at.
///
/// A [`cl_paint::DisplayItem::Text`] run's `size` is attacker-controlled and
/// `cl_paint::validate` does not bound it (nothing about a large size makes the *geometry*
/// invalid). Rasterising a glyph scales its outline by that size, so an unbounded size is an
/// unbounded alpha-mask allocation in the gpu process. Runs above this cap are skipped
/// entirely rather than clamped: clamping would paint text at a size nothing asked for, which
/// is worse than painting none. 1024 px is far past any legitimate CSS `font-size` a page
/// renders at, and bounds a single glyph mask to a few megabytes.
pub const MAX_GLYPH_PX: f32 = 1024.0;
