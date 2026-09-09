//! The bundled font bytes and the family names they are expected to register under.
//!
//! `ChromeLight` never reads fonts from the host system (see the crate-level docs on
//! [`crate::FontDb::bundled`]): the only two fonts that exist to the engine are
//! embedded into the binary at compile time from `assets/`. Provenance, licenses,
//! and checksums for both files live in `assets/SHA256SUMS`, `assets/LICENSE-Ahem`,
//! and `assets/LICENSE-OFL`; `assets/fetch.sh` re-downloads them from the pinned
//! upstream commits.

/// Raw bytes of `assets/Ahem.ttf`, embedded at compile time.
///
/// Ahem is a CC0/public-domain test font (Todd Fahrner, distributed by the Web
/// Platform Tests project) whose defining property is that every glyph is a solid
/// black square exactly one em on a side -- see
/// [`ahem_glyph_should_be_square_em`](../tests) for the test that proves these
/// bytes really are Ahem.
pub static AHEM_BYTES: &[u8] = include_bytes!("../assets/Ahem.ttf");

/// The family name embedded in [`AHEM_BYTES`]'s `name` table, and the name
/// `FontDb::key_for("Ahem")` matches against.
pub const AHEM_FAMILY: &str = "Ahem";

/// Raw bytes of `assets/NotoSans-Regular.ttf`, embedded at compile time.
///
/// Noto Sans Regular (hinted build), licensed under OFL-1.1, shipped unmodified.
pub static NOTO_SANS_BYTES: &[u8] = include_bytes!("../assets/NotoSans-Regular.ttf");

/// The family name embedded in [`NOTO_SANS_BYTES`]'s `name` table, and the name
/// `FontDb::key_for` matches against for `"Noto Sans"` as well as for the
/// `sans-serif`/`monospace` generic CSS families in M1a (see
/// [`crate::FontDb::key_for`]).
pub const NOTO_SANS_FAMILY: &str = "Noto Sans";
