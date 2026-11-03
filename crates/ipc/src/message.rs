//! The handshake message shared by every role. Role messages live in [`crate::proto`]. No enum
//! here is `#[non_exhaustive]`: the codec must reject unknown variants, so adding one is a
//! protocol version bump.

use cl_platform::ProcessType;
use serde::{Deserialize, Serialize};

/// Bump on any wire-incompatible change. Checked in [`Hello`]. Version 2: M1b role protocols
/// (renderer, gpu); `Hello` travels bare as the first upward message.
pub const PROTOCOL_VERSION: u32 = 2;

/// First message from a child after bootstrap, on its upward byte channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    /// Must equal [`PROTOCOL_VERSION`].
    pub protocol_version: u32,
    /// What the child claims to be. Cross-checked against what the browser spawned.
    pub process_type: ProcessType,
    /// Child's own pid. Cross-checked against the spawned pid.
    pub pid: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_version_should_be_two_in_m1b() {
        assert_eq!(PROTOCOL_VERSION, 2);
    }
}
