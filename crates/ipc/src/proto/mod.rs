//! Protocol v2: the renderer and gpu roles (ADR-0016, spec §3). Limits are normative names from
//! the M1b plan's Global Constraints; every upward limit is enforced by `Validate` at the
//! receiver, and a violation names a [`BadMessageReason`].

use serde::{Deserialize, Serialize};

use crate::{bad_message::BadMessageReason, validate::IpcViolation};

pub mod gpu;
pub mod renderer;

/// Largest surface edge, in pixels — the same bound as `cl_gfx`'s raster.
pub const MAX_SURFACE_DIMENSION: u32 = 16_384;
/// Most subresource URLs one document may request.
pub const MAX_SUBRESOURCES_PER_DOCUMENT: usize = 64;
/// Longest subresource URL, in bytes.
pub const MAX_URL_BYTES: usize = 8 * 1024;
/// Most warnings one `DocumentReady` may carry.
pub const MAX_WARNINGS: usize = 64;
/// Longest warning, in bytes.
pub const MAX_WARNING_BYTES: usize = 1024;

/// A surface size in CSS pixels at DPR 1, each edge `1..=MAX_SURFACE_DIMENSION`. The invariant
/// holds for every value, including one deserialized from the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "(u32, u32)", into = "(u32, u32)")]
pub struct Viewport {
    width: u32,
    height: u32,
}

impl Viewport {
    /// A viewport of `width × height`; zero or larger than [`MAX_SURFACE_DIMENSION`] is refused.
    pub fn new(width: u32, height: u32) -> Result<Self, IpcViolation> {
        let edge = 1..=MAX_SURFACE_DIMENSION;
        if edge.contains(&width) && edge.contains(&height) {
            Ok(Self { width, height })
        } else {
            Err(IpcViolation::Bad(BadMessageReason::RendererInvalidField))
        }
    }

    /// Width in pixels.
    #[must_use]
    pub fn width(self) -> u32 {
        self.width
    }

    /// Height in pixels.
    #[must_use]
    pub fn height(self) -> u32 {
        self.height
    }
}

impl TryFrom<(u32, u32)> for Viewport {
    type Error = IpcViolation;
    fn try_from((width, height): (u32, u32)) -> Result<Self, IpcViolation> {
        Self::new(width, height)
    }
}

impl From<Viewport> for (u32, u32) {
    fn from(v: Viewport) -> Self {
        (v.width, v.height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viewport_zero_or_over_16384_is_invalid() {
        assert!(Viewport::new(0, 600).is_err());
        assert!(Viewport::new(800, 0).is_err());
        assert!(Viewport::new(16_385, 600).is_err());
        assert!(Viewport::new(800, 16_385).is_err());
        assert!(Viewport::new(1, 1).is_ok());
        assert!(Viewport::new(16_384, 16_384).is_ok());
    }

    #[test]
    #[allow(clippy::expect_used)]
    fn viewport_should_refuse_an_invalid_size_on_the_wire() {
        let bytes = crate::codec::encode(&(0_u32, 600_u32)).expect("encode");
        assert!(crate::codec::decode::<Viewport>(&bytes).is_err());
    }
}
