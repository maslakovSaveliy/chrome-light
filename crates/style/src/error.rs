//! Style-engine error types.

/// Failure constructing a [`crate::StyleEngine`] or attaching a stylesheet to it.
#[derive(Debug, thiserror::Error)]
pub enum StyleError {
    /// The requested viewport size or device-pixel-ratio was not finite and positive.
    #[error("invalid device parameters")]
    Device,
    /// A stylesheet could not be built or attached to the stylist.
    #[error("stylesheet: {0}")]
    Sheet(String),
    /// The stylesheet's base URL could not be parsed.
    #[error("base url: {0}")]
    Url(String),
}
