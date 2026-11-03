//! Hello / `HelloAck` exchange. The browser validates the child's claims against what it spawned.
//! The first upward message is always a bare [`Hello`], whatever the protocol.

use std::time::Duration;

use cl_platform::ProcessType;
use tracing::warn;

use crate::{
    bootstrap::{BrowserEndpoint, ChildEndpoint},
    error::IpcError,
    message::{Hello, PROTOCOL_VERSION},
    protocol::ChildProtocol,
    validate::{ReceiverCtx, Validate},
};

/// Browser side: wait for `Hello`, validate, ack. On violation the child is told `accepted: false`
/// and the error is returned so the caller kills the process. The browser must never block
/// unboundedly on a hostile or hung child (ADR-0005); `timeout` bounds the wait.
pub fn browser_side<P: ChildProtocol>(
    ep: &BrowserEndpoint<P>,
    ctx: &ReceiverCtx,
    timeout: Duration,
) -> Result<Hello, IpcError> {
    let hello = match ep.recv_hello_timeout(timeout) {
        Ok(hello) => hello,
        Err(e @ IpcError::Codec(_)) => {
            ep.send(P::hello_ack(false))?;
            return Err(e);
        }
        Err(e) => return Err(e),
    };
    if let Err(violation) = hello.validate(ctx) {
        warn!(%violation, "rejecting child during handshake");
        ep.send(P::hello_ack(false))?;
        return Err(violation.into());
    }
    ep.send(P::hello_ack(true))?;
    Ok(hello)
}

/// Child side: send `Hello`, wait for ack.
pub fn child_side<P: ChildProtocol>(
    ep: &ChildEndpoint<P>,
    process_type: ProcessType,
    pid: u32,
) -> Result<(), IpcError> {
    ep.send_hello(&Hello {
        protocol_version: PROTOCOL_VERSION,
        process_type,
        pid,
    })?;
    match P::as_hello_ack(&ep.recv()?) {
        Some(true) => Ok(()),
        Some(false) => Err(IpcError::Rejected),
        None => Err(IpcError::UnexpectedMessage("expected HelloAck")),
    }
}
