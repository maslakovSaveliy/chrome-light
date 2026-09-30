//! [`GfxError`]: everything [`crate::cpu::rasterize`] can refuse to do.
//!
//! The list is deliberately short. A display list reaching the raster backend has already
//! passed `cl_paint::validate` (see [`GfxError::Invalid`]), so by the time any pixel is
//! touched the geometry is known to be representable; what is left is the *local* state the
//! validator explicitly refuses to judge (whether a [`cl_fonts::FontKey`] names a face in
//! *this* process's database — `cl_paint::validate`'s module docs say why that is a raster-
//! time condition, not a validation failure), plus the caller's own canvas size.

use cl_fonts::FontKey;
use cl_paint::DisplayListError;

/// A reason [`crate::cpu::rasterize`] produced no pixmap.
///
/// `#[non_exhaustive]`: the GPU backends of M1b+ will add variants (device lost, surface
/// creation, shader compilation) and a downstream `match` must not break when they do.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum GfxError {
    /// The display list failed `cl_paint::validate` against the canvas bounds.
    ///
    /// This is the hostile-renderer case (`docs/SECURITY.md` §3, the "renderer → gpu" trust
    /// boundary row): the raster backend runs the schema check *before* allocating the
    /// pixmap, so a malformed list costs the gpu process one linear pass over the items and
    /// nothing else.
    #[error("display list failed validation: {0}")]
    Invalid(#[from] DisplayListError),

    /// The requested canvas is not one this backend will allocate: zero in either dimension,
    /// or larger than [`crate::MAX_DIMENSION`] on either axis.
    ///
    /// Reported for the *caller's* `width`/`height` arguments, never for anything inside the
    /// display list — see [`crate::cpu::rasterize`]'s docs for why this check runs before
    /// validation rather than being folded into it.
    #[error("cannot allocate a {width}x{height} pixmap")]
    PixmapTooLarge {
        /// The width the caller asked for.
        width: u32,
        /// The height the caller asked for.
        height: u32,
    },

    /// A [`cl_paint::DisplayItem::Text`] run named a font key this process's
    /// [`cl_fonts::FontDb`] does not know.
    ///
    /// Expected in normal operation from M1b on: the key travels over IPC from a renderer
    /// that may be holding a stale font database. Rasterising the rest of the list while
    /// silently dropping the text would hide the mismatch, so it is an error.
    #[error("glyph run names font key {key:?}, which is not in the bundled font database")]
    FontNotBundled {
        /// The unrecognised key, exactly as the display list carried it.
        key: FontKey,
    },

    /// `swash` could not parse the bytes of a *bundled* face.
    ///
    /// Unreachable in practice: the only bytes this crate ever hands `swash` come from
    /// [`cl_fonts::FontFace::data`], which is `include_bytes!`-embedded at compile time and
    /// checksummed in `crates/fonts/assets/SHA256SUMS` — and `cl-fonts` already parsed the
    /// same bytes through `fontique` when it built the database. It exists so that a
    /// corrupt build is a `Result::Err` rather than an `unwrap` in the gpu process; the
    /// message carries the family name and face index, which is all a bug report needs.
    #[error("bundled font bytes failed to parse: {0}")]
    Font(String),
}
