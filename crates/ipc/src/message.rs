//! Wire messages for M0. Every enum is `#[non_exhaustive]`-free on purpose: the codec must
//! reject unknown variants, so adding a variant is a protocol version bump.

use cl_platform::ProcessType;
use serde::{Deserialize, Serialize};

/// Bump on any wire-incompatible change. Checked in `Hello`.
pub const PROTOCOL_VERSION: u32 = 1;

/// First message from a child after bootstrap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    /// Must equal [`PROTOCOL_VERSION`].
    pub protocol_version: u32,
    /// What the child claims to be. Cross-checked against what the browser spawned.
    pub process_type: ProcessType,
    /// Child's own pid. Cross-checked against the spawned pid.
    pub pid: u32,
}

/// Child → browser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToBrowser {
    /// Handshake opener.
    Hello(Hello),
    /// Reply to [`ToChild::Ping`] carrying the same nonce.
    Pong(u64),
}

/// Browser → child.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToChild {
    /// Handshake answer.
    HelloAck {
        /// `false` means the child must exit immediately.
        accepted: bool,
    },
    /// Liveness probe with a nonce.
    Ping(u64),
    /// Orderly exit request.
    Shutdown,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_version_should_be_one_in_m0() {
        assert_eq!(PROTOCOL_VERSION, 1);
    }

    #[test]
    fn hello_should_be_clone_and_eq() {
        let h = Hello {
            protocol_version: 1,
            process_type: ProcessType::Renderer,
            pid: 42,
        };
        assert_eq!(h.clone(), h);
    }
}
