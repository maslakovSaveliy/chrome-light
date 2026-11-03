//! `ChromeLight` IPC: typed messages, size-limited codec, receiver-side validation, bootstrap and
//! handshake between the browser process and its children (ADR-0005, docs/SECURITY.md §3).
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod bootstrap;
pub mod codec;
pub mod error;
pub mod handshake;
pub mod message;
pub mod protocol;
pub mod pump;
pub mod shm;
pub mod transfer;
pub mod validate;

pub use error::IpcError;
pub use message::PROTOCOL_VERSION;
pub use protocol::{ChildProtocol, M0Protocol};
pub use validate::{IpcViolation, ReceiverCtx, Validate};
