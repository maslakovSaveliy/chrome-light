//! Per-role protocols. A child role fixes what the browser sends it (`Down`, a typed
//! `ipc-channel` message from the trusted browser) and what it sends back (`Up`, our postcard
//! encoding, validated by the browser). See [`crate::bootstrap`] for why the two directions use
//! different encodings. The roles are in [`crate::proto`].

use cl_platform::ProcessType;
use serde::{Serialize, de::DeserializeOwned};

use crate::{bad_message::BadMessageReason, validate::Validate};

/// The message types of one child role.
pub trait ChildProtocol: 'static {
    /// The role the browser spawns a child of this protocol as.
    const ROLE: ProcessType;
    /// The longest encoded `Up` message the browser decodes; anything longer is refused unread.
    const MAX_UP_BYTES: usize;
    /// The bad-message reason for an `Up` message that does not decode.
    const UNDECODABLE: BadMessageReason;
    /// Browser → child: a typed `ipc-channel` message from a trusted sender.
    type Down: Serialize + DeserializeOwned + std::fmt::Debug + Send + 'static;
    /// Child → browser: postcard over a byte channel, validated on receipt.
    type Up: Serialize + DeserializeOwned + Validate + std::fmt::Debug + Send + 'static;
    /// The browser's answer to the child's `Hello`.
    fn hello_ack(accepted: bool) -> Self::Down;
    /// `Some(accepted)` if `msg` is that answer.
    fn as_hello_ack(msg: &Self::Down) -> Option<bool>;
}
