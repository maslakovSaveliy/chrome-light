//! Hello / `HelloAck` exchange. The browser validates the child's claims against what it spawned.

use std::time::Duration;

use cl_platform::ProcessType;
use tracing::warn;

use crate::{
    bootstrap::{BrowserEndpoint, ChildEndpoint},
    error::IpcError,
    message::{Hello, PROTOCOL_VERSION, ToBrowser, ToChild},
    validate::{ReceiverCtx, Validate},
};

/// Browser side: wait for `Hello`, validate, ack. On violation the child is told `accepted: false`
/// and the error is returned so the caller kills the process. The browser must never block
/// unboundedly on a hostile or hung child (ADR-0005); `timeout` bounds the wait.
pub fn browser_side(
    ep: &BrowserEndpoint,
    ctx: &ReceiverCtx,
    timeout: Duration,
) -> Result<Hello, IpcError> {
    let msg = ep.recv_timeout(timeout)?;
    let ToBrowser::Hello(hello) = msg else {
        ep.send(&ToChild::HelloAck { accepted: false })?;
        return Err(IpcError::UnexpectedMessage("expected Hello"));
    };
    if let Err(violation) = hello.validate(ctx) {
        warn!(%violation, "rejecting child during handshake");
        ep.send(&ToChild::HelloAck { accepted: false })?;
        return Err(violation.into());
    }
    ep.send(&ToChild::HelloAck { accepted: true })?;
    Ok(hello)
}

/// Child side: send `Hello`, wait for ack.
pub fn child_side(ep: &ChildEndpoint, process_type: ProcessType, pid: u32) -> Result<(), IpcError> {
    ep.send(&ToBrowser::Hello(Hello {
        protocol_version: PROTOCOL_VERSION,
        process_type,
        pid,
    }))?;
    match ep.recv()? {
        ToChild::HelloAck { accepted: true } => Ok(()),
        ToChild::HelloAck { accepted: false } => Err(IpcError::Rejected),
        ToChild::Ping(_) | ToChild::Shutdown => {
            Err(IpcError::UnexpectedMessage("expected HelloAck"))
        }
    }
}
