//! Process bootstrap. The browser creates a one-shot named server; the child receives the name
//! in argv, connects, and hands over two raw byte channels. All later traffic uses those channels
//! with our [`crate::codec`], so `ipc-channel`'s own serializer only ever sees the bootstrap
//! message.

use std::{
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use ipc_channel::{
    TryRecvError,
    ipc::{IpcBytesReceiver, IpcBytesSender, IpcOneShotServer, IpcSender, bytes_channel},
};
use serde::{Deserialize, Serialize};

use crate::{
    codec,
    error::IpcError,
    message::{ToBrowser, ToChild},
};

/// The single message that crosses the one-shot server: the child's ends of two byte channels.
#[derive(Serialize, Deserialize)]
struct BootstrapMsg {
    to_browser: IpcBytesReceiver,
    to_child: IpcBytesSender,
}

/// Browser-side endpoint talking to one child.
#[derive(Debug)]
pub struct BrowserEndpoint {
    tx: IpcBytesSender,
    rx: IpcBytesReceiver,
}

/// Child-side endpoint talking to the browser.
#[derive(Debug)]
pub struct ChildEndpoint {
    tx: IpcBytesSender,
    rx: IpcBytesReceiver,
}

impl BrowserEndpoint {
    /// Send one message to the child.
    pub fn send(&self, msg: &ToChild) -> Result<(), IpcError> {
        let bytes = codec::encode(msg)?;
        self.tx.send(&bytes)?;
        Ok(())
    }

    /// Block for the next message from the child. Decoding failure is an error, never a panic.
    pub fn recv(&self) -> Result<ToBrowser, IpcError> {
        let bytes = self.rx.recv()?;
        Ok(codec::decode(&bytes)?)
    }

    /// Receive with a deadline. The browser must never block unboundedly on a child
    /// (ADR-0005). M0-ONLY: implemented as a 1 ms `park_timeout` poll over `try_recv`; M1 replaces
    /// this with a proper multiplexed wait (`IpcReceiverSet`) when the browser owns many children.
    pub fn recv_timeout(&self, timeout: Duration) -> Result<ToBrowser, IpcError> {
        let deadline = Instant::now() + timeout;
        loop {
            match self.rx.try_recv() {
                Ok(bytes) => return Ok(codec::decode(&bytes)?),
                Err(TryRecvError::Empty) => {
                    if Instant::now() >= deadline {
                        return Err(IpcError::Transport(std::io::Error::new(
                            std::io::ErrorKind::TimedOut,
                            "child did not answer in time",
                        )));
                    }
                    std::thread::park_timeout(Duration::from_millis(1));
                }
                Err(TryRecvError::IpcError(e)) => return Err(IpcError::Channel(e.to_string())),
            }
        }
    }
}

impl ChildEndpoint {
    /// Send one message to the browser.
    pub fn send(&self, msg: &ToBrowser) -> Result<(), IpcError> {
        let bytes = codec::encode(msg)?;
        self.tx.send(&bytes)?;
        Ok(())
    }

    /// Block for the next message from the browser.
    pub fn recv(&self) -> Result<ToChild, IpcError> {
        let bytes = self.rx.recv()?;
        Ok(codec::decode(&bytes)?)
    }
}

/// One-shot named server created by the browser before spawning a child.
pub struct BootstrapServer {
    server: IpcOneShotServer<BootstrapMsg>,
    name: String,
}

impl std::fmt::Debug for BootstrapServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `IpcOneShotServer` itself has no `Debug` impl; the name is the only useful field.
        f.debug_struct("BootstrapServer")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl BootstrapServer {
    /// Create a server with a fresh OS-level name to pass to the child as `--ipc-bootstrap=`.
    pub fn new() -> Result<Self, IpcError> {
        let (server, name) = IpcOneShotServer::new()?;
        Ok(Self { server, name })
    }

    /// The name the child must connect to.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Wait for the child to connect. `ipc-channel`'s accept has no timeout, so it runs on a
    /// helper thread; a hung or crashed child yields `IpcError::Transport(TimedOut)` and the
    /// caller kills it. A caller that can also watch the child process should use
    /// [`BootstrapServer::start_accept`] instead, to stop waiting as soon as the child exits.
    pub fn accept_with_timeout(self, timeout: Duration) -> Result<BrowserEndpoint, IpcError> {
        match self.start_accept()?.poll(timeout)? {
            Some(endpoint) => Ok(endpoint),
            None => Err(IpcError::Transport(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "child did not connect to bootstrap server in time",
            ))),
        }
    }

    /// Start waiting for the child to connect, without blocking the caller. The accept runs on a
    /// helper thread (`ipc-channel`'s accept has no timeout); the caller polls the returned
    /// [`PendingAccept`] in short slices and can check on the child in between.
    ///
    /// If the child never connects, the helper thread stays blocked in `accept` until the
    /// process exits — one thread per abandoned bootstrap (known M0 debt).
    pub fn start_accept(self) -> Result<PendingAccept, IpcError> {
        let (tx, rx) = mpsc::channel();
        let server = self.server;
        thread::Builder::new()
            .name("ipc-bootstrap-accept".into())
            .spawn(move || {
                let result = server.accept().map(|(_, msg)| msg);
                // Receiver may have stopped waiting and gone away; nothing to do then.
                let _ = tx.send(result);
            })?;
        Ok(PendingAccept { rx })
    }
}

/// A bootstrap accept in progress on its helper thread (see [`BootstrapServer::start_accept`]).
#[derive(Debug)]
pub struct PendingAccept {
    rx: mpsc::Receiver<Result<BootstrapMsg, ipc_channel::IpcError>>,
}

impl PendingAccept {
    /// Wait up to `wait` for the child's connection. `Ok(None)` means it has not connected yet;
    /// call again or give up. An error means the accept itself failed and will not succeed later.
    pub fn poll(&self, wait: Duration) -> Result<Option<BrowserEndpoint>, IpcError> {
        match self.rx.recv_timeout(wait) {
            Ok(Ok(msg)) => Ok(Some(BrowserEndpoint {
                tx: msg.to_child,
                rx: msg.to_browser,
            })),
            Ok(Err(e)) => Err(IpcError::Channel(e.to_string())),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(IpcError::Channel("bootstrap accept thread died".into()))
            }
        }
    }
}

/// Called by a child with the name from `--ipc-bootstrap=`.
pub fn connect_child(server_name: &str) -> Result<ChildEndpoint, IpcError> {
    let (to_browser_tx, to_browser_rx) = bytes_channel()?;
    let (to_child_tx, to_child_rx) = bytes_channel()?;
    let boot: IpcSender<BootstrapMsg> = IpcSender::connect(server_name.to_owned())?;
    boot.send(BootstrapMsg {
        to_browser: to_browser_rx,
        to_child: to_child_tx,
    })?;
    Ok(ChildEndpoint {
        tx: to_browser_tx,
        rx: to_child_rx,
    })
}
