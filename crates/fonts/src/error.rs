//! Error type for building the bundled [`crate::FontDb`].
//!
//! Every variant here indicates that the bundled font bytes embedded via
//! `include_bytes!` did not parse or register the way this crate expects. None of
//! these are expected to occur in a correctly-built binary (the bundled bytes are
//! fixed at compile time and checked in CI), but `bundled()` returns `Result`
//! rather than panicking so a corrupted build still fails loudly instead of
//! silently falling back to system fonts or an empty database.

/// Failure modes for [`crate::FontDb::bundled`].
#[derive(Debug, thiserror::Error)]
pub enum FontError {
    /// Registering the bundled font data for `name` with `fontique` produced no
    /// font faces at all. This would mean the embedded bytes are not a font
    /// `fontique` can parse (e.g. the asset file was truncated or replaced with
    /// something else).
    #[error("bundled font `{name}` registered zero font faces")]
    NoFacesRegistered {
        /// The asset's expected family name (e.g. `"Ahem"`), used only for
        /// diagnostics.
        name: &'static str,
    },
    /// Registering the bundled font data for `name` produced faces split across
    /// more than one family. Each bundled asset is a single-family TTF, so this
    /// would indicate the file's `name` table is inconsistent or the wrong file
    /// was bundled under this name.
    #[error("bundled font `{name}` registered {count} distinct families, expected exactly 1")]
    UnexpectedFamilyCount {
        /// The asset's expected family name, used only for diagnostics.
        name: &'static str,
        /// How many distinct families `fontique` actually reported.
        count: usize,
    },
    /// The family name `fontique` read out of the bundled font's `name` table
    /// does not match the family name this crate expects for that asset. Catches
    /// the case where the wrong bytes were bundled under the wrong constant.
    #[error("bundled font `{expected}` registered under unexpected family name {actual:?}")]
    UnexpectedFamilyName {
        /// The family name this crate expected (e.g. `"Noto Sans"`).
        expected: &'static str,
        /// The family name `fontique` actually reported.
        actual: String,
    },
}
