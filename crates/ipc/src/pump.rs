//! Reader threads: one per incoming channel, forwarding what arrives into the browser's single
//! event queue, so the browser never blocks on a child (ADR-0005). Replaces the M0
//! `recv_timeout` poll for long-lived children.

use std::thread::JoinHandle;

use ipc_channel::ipc::IpcReceiver;
use serde::{Serialize, de::DeserializeOwned};

use crate::{
    bad_message::BadMessageReason,
    bootstrap::{UpReceiver, decode_up, up_failure_reason},
    protocol::ChildProtocol,
};

/// What a reader thread reports.
#[derive(Debug)]
pub enum PumpEvent<T> {
    /// A message arrived and decoded.
    Message(T),
    /// Every sender is gone: the peer exited or closed the channel. The reader stops.
    Closed,
    /// A message arrived but did not decode or validate — from a less trusted peer this is a bad
    /// message (the browser kills the sender, ADR-0016 §9). The reader keeps going unless the
    /// sink stops it.
    Error(BadInput),
    /// The transport reported an OS-level receive error; the channel is unusable and the reader
    /// stops. The cause may be local, or a malformed message from the peer (on macOS
    /// `ipc-channel` reports a malformed Mach message this way), so the consumer must tear the
    /// child down — kill it (and restart a gpu) — without necessarily logging a bad-message
    /// reason. Never leave the child running with a dead channel.
    Failed(String),
}

/// A message that was not accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadInput {
    /// The bad-message reason, when the receiver knows the role (upward control channels).
    /// `None` on typed channels, whose consumer maps the failure itself.
    pub reason: Option<BadMessageReason>,
    /// What went wrong, for the log.
    pub detail: String,
}

/// Why a blocking receive returned no message.
#[derive(Debug)]
pub enum PumpRecvError {
    /// Every sender is gone.
    Closed,
    /// A message arrived but was not acceptable.
    Bad(BadInput),
    /// The transport reported an OS-level receive error (see [`PumpEvent::Failed`]).
    Failed(String),
}

/// A channel a reader thread can block on.
pub trait PumpSource<T> {
    /// Block for the next message.
    fn recv_blocking(&self) -> Result<T, PumpRecvError>;
}

fn classify(e: ipc_channel::IpcError) -> PumpRecvError {
    match e {
        ipc_channel::IpcError::Disconnected => PumpRecvError::Closed,
        ipc_channel::IpcError::SerializationError(e) => PumpRecvError::Bad(BadInput {
            reason: None,
            detail: e.to_string(),
        }),
        ipc_channel::IpcError::Io(e) => PumpRecvError::Failed(e.to_string()),
    }
}

impl<T: Serialize + DeserializeOwned> PumpSource<T> for IpcReceiver<T> {
    fn recv_blocking(&self) -> Result<T, PumpRecvError> {
        self.recv().map_err(classify)
    }
}

impl<P: ChildProtocol> PumpSource<P::Up> for UpReceiver<P> {
    fn recv_blocking(&self) -> Result<P::Up, PumpRecvError> {
        let bytes = self.recv_bytes().map_err(classify)?;
        decode_up::<P>(&bytes, self.ctx()).map_err(|e| {
            PumpRecvError::Bad(BadInput {
                reason: Some(up_failure_reason::<P>(&e)),
                detail: e.to_string(),
            })
        })
    }
}

/// Starts a named reader thread that feeds `sink` from `rx` until the channel closes or `sink`
/// returns `false`.
pub fn spawn_reader<T, R, F>(name: &str, rx: R, mut sink: F) -> std::io::Result<JoinHandle<()>>
where
    T: Send + 'static,
    R: PumpSource<T> + Send + 'static,
    F: FnMut(PumpEvent<T>) -> bool + Send + 'static,
{
    std::thread::Builder::new()
        .name(name.to_owned())
        .spawn(move || {
            loop {
                let event = match rx.recv_blocking() {
                    Ok(msg) => PumpEvent::Message(msg),
                    Err(PumpRecvError::Closed) => {
                        let _ = sink(PumpEvent::Closed);
                        return;
                    }
                    Err(PumpRecvError::Failed(e)) => {
                        let _ = sink(PumpEvent::Failed(e));
                        return;
                    }
                    Err(PumpRecvError::Bad(e)) => PumpEvent::Error(e),
                };
                if !sink(event) {
                    return;
                }
            }
        })
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use cl_platform::ProcessType;
    use ipc_channel::ipc::bytes_channel;

    use super::*;
    use crate::{
        codec,
        proto::{
            MAX_SUBRESOURCES_PER_DOCUMENT,
            renderer::{FromRenderer, RendererProtocol},
        },
        validate::ReceiverCtx,
    };

    fn ctx() -> ReceiverCtx {
        ReceiverCtx {
            expected_process_type: ProcessType::Renderer,
            expected_pid: None,
        }
    }

    #[test]
    #[allow(clippy::expect_used)]
    fn bad_input_should_carry_its_reason_and_the_reader_should_keep_going() {
        let (tx, rx) = bytes_channel().expect("channel");
        let up = UpReceiver::<RendererProtocol>::from_bytes_receiver(rx, ctx());
        let (events_tx, events_rx) = mpsc::channel::<String>();
        let reader = spawn_reader("test-up", up, move |event: PumpEvent<FromRenderer>| {
            let name = match event {
                PumpEvent::Message(FromRenderer::Pong(n)) => format!("pong {n}"),
                PumpEvent::Message(other) => format!("message {other:?}"),
                PumpEvent::Closed => "closed".to_owned(),
                PumpEvent::Error(bad) => format!("error {:?}", bad.reason),
                PumpEvent::Failed(e) => format!("failed {e}"),
            };
            let _ = events_tx.send(name);
            true
        })
        .expect("reader");

        // Undecodable, then longer than MAX_UP_BYTES, then decodable but over a limit, then valid.
        tx.send(&[0xff, 0xff, 0xff, 0xff, 0xff])
            .expect("send garbage");
        tx.send(&vec![0; RendererProtocol::MAX_UP_BYTES + 1])
            .expect("send oversize");
        let too_many = FromRenderer::NeedSubresources {
            doc_id: 1,
            urls: vec!["u".to_owned(); MAX_SUBRESOURCES_PER_DOCUMENT + 1],
        };
        tx.send(&codec::encode(&too_many).expect("encode"))
            .expect("send too many");
        tx.send(&codec::encode(&FromRenderer::Pong(5)).expect("encode"))
            .expect("send pong");
        drop(tx);
        reader.join().expect("reader thread");

        let events: Vec<String> = events_rx.iter().collect();
        assert_eq!(
            events,
            [
                "error Some(RendererUndecodable)",
                "error Some(RendererUndecodable)",
                "error Some(RendererTooManySubresources)",
                "pong 5",
                "closed"
            ]
        );
    }
}
