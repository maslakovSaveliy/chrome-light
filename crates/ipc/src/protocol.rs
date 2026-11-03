//! Per-role protocols. A child role fixes what the browser sends it (`Down`, a typed
//! `ipc-channel` message from the trusted browser) and what it sends back (`Up`, our postcard
//! encoding, validated by the browser). See [`crate::bootstrap`] for why the two directions use
//! different encodings.

use cl_platform::ProcessType;
use serde::{Serialize, de::DeserializeOwned};

use crate::{
    message::{ToBrowser, ToChild},
    validate::Validate,
};

/// The message types of one child role.
pub trait ChildProtocol: 'static {
    /// The role the browser spawns a child of this protocol as.
    const ROLE: ProcessType;
    /// Browser → child: a typed `ipc-channel` message from a trusted sender.
    type Down: Serialize + DeserializeOwned + std::fmt::Debug + Send + 'static;
    /// Child → browser: postcard over a byte channel, validated on receipt.
    type Up: Serialize + DeserializeOwned + Validate + std::fmt::Debug + Send + 'static;
    /// The browser's answer to the child's `Hello`.
    fn hello_ack(accepted: bool) -> Self::Down;
    /// `Some(accepted)` if `msg` is that answer.
    fn as_hello_ack(msg: &Self::Down) -> Option<bool>;
}

/// Transitional protocol for the M0 renderer (ping/pong/shutdown) until M1b's renderer and gpu
/// protocols replace it.
#[derive(Debug)]
pub struct M0Protocol;

impl ChildProtocol for M0Protocol {
    const ROLE: ProcessType = ProcessType::Renderer;
    type Down = ToChild;
    type Up = ToBrowser;

    fn hello_ack(accepted: bool) -> ToChild {
        ToChild::HelloAck { accepted }
    }

    fn as_hello_ack(msg: &ToChild) -> Option<bool> {
        match msg {
            ToChild::HelloAck { accepted } => Some(*accepted),
            ToChild::Ping(_) | ToChild::Shutdown => None,
        }
    }
}
