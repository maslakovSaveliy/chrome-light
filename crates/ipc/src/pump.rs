//! Reader threads: one per incoming channel, forwarding what arrives into the browser's single
//! event queue, so the browser never blocks on a child (ADR-0005). Replaces the M0
//! `recv_timeout` poll for long-lived children.

use std::thread::JoinHandle;

use ipc_channel::ipc::IpcReceiver;
use serde::{Serialize, de::DeserializeOwned};

use crate::{bootstrap::UpReceiver, codec, protocol::ChildProtocol};

/// What a reader thread reports.
#[derive(Debug)]
pub enum PumpEvent<T> {
    /// A message arrived and decoded.
    Message(T),
    /// Every sender is gone: the peer exited or closed the channel. The reader stops.
    Closed,
    /// A message arrived but could not be decoded — from a less trusted peer this is a bad
    /// message (the browser kills the sender). The reader keeps going unless the sink stops it.
    Error(String),
}

/// Why a blocking receive returned no message.
#[derive(Debug)]
pub enum PumpRecvError {
    /// Every sender is gone.
    Closed,
    /// A message arrived but was not acceptable.
    Bad(String),
}

/// A channel a reader thread can block on.
pub trait PumpSource<T> {
    /// Block for the next message.
    fn recv_blocking(&self) -> Result<T, PumpRecvError>;
}

fn classify(e: ipc_channel::IpcError) -> PumpRecvError {
    match e {
        ipc_channel::IpcError::Disconnected => PumpRecvError::Closed,
        other => PumpRecvError::Bad(other.to_string()),
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
        codec::decode(&bytes).map_err(|e| PumpRecvError::Bad(e.to_string()))
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
                    Err(PumpRecvError::Bad(e)) => PumpEvent::Error(e),
                };
                if !sink(event) {
                    return;
                }
            }
        })
}
