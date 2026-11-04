//! IPC error types.

use crate::validate::IpcViolation;

/// Any failure on an IPC endpoint.
#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    /// OS-level transport failure (pipe closed, mach port dead, ...).
    #[error("ipc transport: {0}")]
    Transport(#[from] std::io::Error),
    /// `ipc-channel` reported an error (deserialization of bootstrap, OS failure).
    #[error("ipc channel: {0}")]
    Channel(String),
    /// Every sender of the channel is gone: the peer exited or closed it.
    #[error("ipc channel closed by peer")]
    Disconnected,
    /// Our own wire codec failed.
    #[error("ipc codec: {0}")]
    Codec(#[from] crate::codec::CodecError),
    /// Message failed validation at the receiver — treated as hostile.
    #[error("ipc violation: {0}")]
    Violation(#[from] IpcViolation),
    /// Peer sent a message not legal in the current handshake state.
    #[error("unexpected message during handshake: {0}")]
    UnexpectedMessage(&'static str),
    /// The peer rejected our hello.
    #[error("handshake rejected by peer")]
    Rejected,
}

impl From<ipc_channel::IpcError> for IpcError {
    fn from(e: ipc_channel::IpcError) -> Self {
        match e {
            ipc_channel::IpcError::Disconnected => Self::Disconnected,
            other => Self::Channel(other.to_string()),
        }
    }
}
