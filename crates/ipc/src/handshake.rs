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

#[cfg(test)]
mod tests {
    use std::thread;

    use super::*;
    use crate::{
        bootstrap::{BootstrapServer, connect_child},
        proto::renderer::RendererProtocol,
    };

    #[test]
    #[allow(clippy::expect_used)]
    fn a_garbage_first_message_should_be_refused_with_a_codec_error() {
        let server = BootstrapServer::<RendererProtocol>::new().expect("server");
        let name = server.name().to_owned();
        let child = thread::spawn(move || {
            let ep = connect_child::<RendererProtocol>(&name, None).expect("connect");
            let (up, down) = ep.split();
            // Longer than any Hello: refused unread.
            up.send_raw(&[0xff; 200]).expect("send garbage");
            down.recv().expect("an answer")
        });
        let ep = server
            .accept_with_timeout(Duration::from_secs(10))
            .expect("accept");
        let ctx = ReceiverCtx {
            expected_process_type: ProcessType::Renderer,
            expected_pid: None,
        };
        let err = browser_side(&ep, &ctx, Duration::from_secs(10)).expect_err("refused");
        assert!(matches!(err, IpcError::Codec(_)), "got {err:?}");
        let answer = child.join().expect("child thread");
        assert_eq!(RendererProtocol::as_hello_ack(&answer), Some(false));
    }
}
