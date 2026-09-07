//! Receiver-side validation. The receiver decides what it expects; the payload never gets to
//! assert its own identity (docs/SECURITY.md §3).

use cl_platform::ProcessType;

use crate::message::{Hello, PROTOCOL_VERSION, ToBrowser, ToChild};

/// What the receiving side knows independently of the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReceiverCtx {
    /// The process type the browser actually spawned.
    pub expected_process_type: ProcessType,
    /// The pid the browser actually spawned (None when unknown, e.g. in-process tests).
    pub expected_pid: Option<u32>,
}

/// A message that fails validation. Treated as hostile: log and drop the connection.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[allow(clippy::enum_variant_names)]
pub enum IpcViolation {
    /// Wire protocol mismatch.
    #[error("protocol version {got} != expected {expected}")]
    VersionMismatch {
        /// Version claimed by peer.
        got: u32,
        /// Our version.
        expected: u32,
    },
    /// Peer claims to be a different process type than we spawned.
    #[error("process type mismatch: claimed {claimed}, expected {expected}")]
    ProcessTypeMismatch {
        /// Claimed.
        claimed: ProcessType,
        /// Spawned.
        expected: ProcessType,
    },
    /// Peer claims a pid we did not spawn.
    #[error("pid mismatch: claimed {claimed}, expected {expected}")]
    PidMismatch {
        /// Claimed.
        claimed: u32,
        /// Spawned.
        expected: u32,
    },
}

/// Implemented by every message type crossing a process boundary.
pub trait Validate {
    /// Check the message against what the receiver independently knows.
    fn validate(&self, ctx: &ReceiverCtx) -> Result<(), IpcViolation>;
}

impl Validate for Hello {
    fn validate(&self, ctx: &ReceiverCtx) -> Result<(), IpcViolation> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(IpcViolation::VersionMismatch {
                got: self.protocol_version,
                expected: PROTOCOL_VERSION,
            });
        }
        if self.process_type != ctx.expected_process_type {
            return Err(IpcViolation::ProcessTypeMismatch {
                claimed: self.process_type,
                expected: ctx.expected_process_type,
            });
        }
        if let Some(expected) = ctx.expected_pid
            && self.pid != expected
        {
            return Err(IpcViolation::PidMismatch {
                claimed: self.pid,
                expected,
            });
        }
        Ok(())
    }
}

impl Validate for ToBrowser {
    fn validate(&self, ctx: &ReceiverCtx) -> Result<(), IpcViolation> {
        match self {
            Self::Hello(h) => h.validate(ctx),
            Self::Pong(_) => Ok(()),
        }
    }
}

impl Validate for ToChild {
    fn validate(&self, _ctx: &ReceiverCtx) -> Result<(), IpcViolation> {
        match self {
            Self::HelloAck { .. } | Self::Ping(_) | Self::Shutdown => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> ReceiverCtx {
        ReceiverCtx {
            expected_process_type: ProcessType::Renderer,
            expected_pid: Some(100),
        }
    }

    fn hello() -> Hello {
        Hello {
            protocol_version: PROTOCOL_VERSION,
            process_type: ProcessType::Renderer,
            pid: 100,
        }
    }

    #[test]
    fn valid_hello_should_pass() {
        assert_eq!(hello().validate(&ctx()), Ok(()));
    }

    #[test]
    fn hello_with_wrong_version_should_fail() {
        let h = Hello {
            protocol_version: PROTOCOL_VERSION + 1,
            ..hello()
        };
        assert_eq!(
            h.validate(&ctx()),
            Err(IpcViolation::VersionMismatch {
                got: PROTOCOL_VERSION + 1,
                expected: PROTOCOL_VERSION
            })
        );
    }

    #[test]
    fn hello_claiming_browser_type_should_fail() {
        let h = Hello {
            process_type: ProcessType::Browser,
            ..hello()
        };
        assert_eq!(
            h.validate(&ctx()),
            Err(IpcViolation::ProcessTypeMismatch {
                claimed: ProcessType::Browser,
                expected: ProcessType::Renderer
            })
        );
    }

    #[test]
    fn hello_with_foreign_pid_should_fail() {
        let h = Hello {
            pid: 999,
            ..hello()
        };
        assert_eq!(
            h.validate(&ctx()),
            Err(IpcViolation::PidMismatch {
                claimed: 999,
                expected: 100
            })
        );
    }

    #[test]
    fn hello_pid_should_be_skipped_when_receiver_does_not_know_pid() {
        let c = ReceiverCtx {
            expected_pid: None,
            ..ctx()
        };
        let h = Hello {
            pid: 999,
            ..hello()
        };
        assert_eq!(h.validate(&c), Ok(()));
    }

    #[test]
    #[allow(clippy::expect_used)]
    fn pong_and_to_child_messages_should_always_pass() {
        assert_eq!(ToBrowser::Pong(1).validate(&ctx()), Ok(()));
        assert_eq!(ToChild::Ping(1).validate(&ctx()), Ok(()));
        assert_eq!(ToChild::Shutdown.validate(&ctx()), Ok(()));
        assert_eq!(
            ToChild::HelloAck { accepted: true }.validate(&ctx()),
            Ok(())
        );
    }
}
