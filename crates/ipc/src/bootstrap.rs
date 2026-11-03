//! Process bootstrap. The browser creates a one-shot named server; the child receives the name
//! in argv, connects, and hands over its channel ends in a single [`BootstrapMsg`].
//!
//! Directions differ on purpose (ADR-0016 §3, §5):
//! - **Upward** (child → browser, untrusted sender): a raw byte channel carrying our own
//!   [`crate::codec`] (postcard) encoding of [`ChildProtocol::Up`], validated by the receiver.
//!   `ipc-channel`'s own deserializer never runs on upward control traffic, so a hostile child
//!   cannot make it deserialize channel ends (which panics on a bad index).
//! - **Downward** (browser → child, trusted sender): one typed `ipc-channel` channel of
//!   [`ChildProtocol::Down`], which may carry strings, vectors, shared memory and channel ends.
//!
//! The bootstrap message is the only upward transfer of channel ends, and it is sent by a
//! freshly spawned child before it has loaded any untrusted content. The child creates its
//! downward channel (the process that receives on a channel creates it); a gpu child also
//! creates the display-list channel and sends its sender up here, for the browser to relay to
//! the renderer.

use std::{
    marker::PhantomData,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use ipc_channel::{
    TryRecvError,
    ipc::{
        IpcBytesReceiver, IpcBytesSender, IpcOneShotServer, IpcReceiver, IpcSender, bytes_channel,
        channel,
    },
};
use serde::{Deserialize, Serialize};

use crate::{
    codec,
    error::IpcError,
    message::ToBrowser,
    protocol::{ChildProtocol, M0Protocol},
    transfer::DisplayListMsg,
};

/// The single message that crosses the one-shot server: the child's channel ends.
#[derive(Serialize, Deserialize)]
#[serde(bound = "")]
struct BootstrapMsg<D> {
    /// Receiving end of the child's upward byte channel.
    to_browser: IpcBytesReceiver,
    /// Sending end of the child's typed downward channel.
    to_child: IpcSender<D>,
    /// A gpu child's display-list channel sender, for the browser to relay to the renderer.
    display_lists: Option<IpcSender<DisplayListMsg>>,
}

/// Browser-side endpoint talking to one child of protocol `P`.
#[derive(Debug)]
pub struct BrowserEndpoint<P: ChildProtocol = M0Protocol> {
    tx: IpcSender<P::Down>,
    rx: IpcBytesReceiver,
    display_lists: Option<IpcSender<DisplayListMsg>>,
}

/// Child-side endpoint talking to the browser.
#[derive(Debug)]
pub struct ChildEndpoint<P: ChildProtocol = M0Protocol> {
    tx: ChildSender<P>,
    rx: IpcReceiver<P::Down>,
}

/// The child's upward sender, cloneable so several threads of a child can report.
#[derive(Debug)]
pub struct ChildSender<P: ChildProtocol = M0Protocol> {
    tx: IpcBytesSender,
    _protocol: PhantomData<fn() -> P>,
}

impl<P: ChildProtocol> Clone for ChildSender<P> {
    fn clone(&self) -> Self {
        Self {
            tx: self.tx.clone(),
            _protocol: PhantomData,
        }
    }
}

/// The browser's upward receiver for one child: our codec over a byte channel (see
/// [`crate::pump::PumpSource`]). Decoding failures are errors, never panics; validation is the
/// consumer's job.
#[derive(Debug)]
pub struct UpReceiver<P: ChildProtocol = M0Protocol> {
    rx: IpcBytesReceiver,
    _protocol: PhantomData<fn() -> P>,
}

impl<P: ChildProtocol> UpReceiver<P> {
    /// Block for the next raw upward message.
    pub(crate) fn recv_bytes(&self) -> Result<Vec<u8>, ipc_channel::IpcError> {
        self.rx.recv()
    }
}

fn timed_out(what: &'static str) -> IpcError {
    IpcError::Transport(std::io::Error::new(std::io::ErrorKind::TimedOut, what))
}

impl<P: ChildProtocol> BrowserEndpoint<P> {
    /// Send one message to the child on its typed downward channel. Taken by value: downward
    /// messages may move shared memory and channel ends to the child.
    pub fn send(&self, msg: P::Down) -> Result<(), IpcError> {
        self.tx.send(msg)?;
        Ok(())
    }

    /// Block for the next upward message. Decoding failure is an error, never a panic.
    pub fn recv(&self) -> Result<P::Up, IpcError> {
        let bytes = self.rx.recv()?;
        Ok(codec::decode(&bytes)?)
    }

    /// Receive the next upward message with a deadline. The browser must never block
    /// unboundedly on a child (ADR-0005).
    pub fn recv_timeout(&self, timeout: Duration) -> Result<P::Up, IpcError> {
        let bytes = self.recv_bytes_timeout(timeout, "child did not answer in time")?;
        Ok(codec::decode(&bytes)?)
    }

    /// The first upward message is always the M0 `ToBrowser::Hello`, whatever `P::Up` is.
    pub(crate) fn recv_hello_timeout(&self, timeout: Duration) -> Result<ToBrowser, IpcError> {
        let bytes = self.recv_bytes_timeout(timeout, "child did not say hello in time")?;
        Ok(codec::decode(&bytes)?)
    }

    /// M0-ONLY: a 1 ms `park_timeout` poll over `try_recv`. Long-lived children are read by a
    /// [`crate::pump`] reader thread instead (see [`BrowserEndpoint::split`]).
    fn recv_bytes_timeout(
        &self,
        timeout: Duration,
        what: &'static str,
    ) -> Result<Vec<u8>, IpcError> {
        let deadline = Instant::now().checked_add(timeout);
        loop {
            match self.rx.try_recv() {
                Ok(bytes) => return Ok(bytes),
                Err(TryRecvError::Empty) => {
                    if deadline.is_some_and(|d| Instant::now() >= d) {
                        return Err(timed_out(what));
                    }
                    std::thread::park_timeout(Duration::from_millis(1));
                }
                Err(TryRecvError::IpcError(e)) => return Err(e.into()),
            }
        }
    }

    /// Takes the display-list sender a gpu child sent up in its bootstrap (once).
    pub fn take_display_lists(&mut self) -> Option<IpcSender<DisplayListMsg>> {
        self.display_lists.take()
    }

    /// Splits into the downward sender and the upward receiver, so the receiver can move to a
    /// reader thread ([`crate::pump::spawn_reader`]).
    #[must_use]
    pub fn split(self) -> (IpcSender<P::Down>, UpReceiver<P>) {
        (
            self.tx,
            UpReceiver {
                rx: self.rx,
                _protocol: PhantomData,
            },
        )
    }
}

impl<P: ChildProtocol> ChildSender<P> {
    /// Send one message to the browser.
    pub fn send(&self, msg: &P::Up) -> Result<(), IpcError> {
        let bytes = codec::encode(msg)?;
        self.tx.send(&bytes)?;
        Ok(())
    }

    /// The handshake opener, encoded as the M0 `ToBrowser` the browser expects first.
    pub(crate) fn send_hello(&self, msg: &ToBrowser) -> Result<(), IpcError> {
        let bytes = codec::encode(msg)?;
        self.tx.send(&bytes)?;
        Ok(())
    }
}

impl<P: ChildProtocol> ChildEndpoint<P> {
    /// Send one message to the browser.
    pub fn send(&self, msg: &P::Up) -> Result<(), IpcError> {
        self.tx.send(msg)
    }

    pub(crate) fn send_hello(&self, msg: &ToBrowser) -> Result<(), IpcError> {
        self.tx.send_hello(msg)
    }

    /// Block for the next message from the browser.
    pub fn recv(&self) -> Result<P::Down, IpcError> {
        Ok(self.rx.recv()?)
    }

    /// Splits into a cloneable upward sender and the downward receiver.
    #[must_use]
    pub fn split(self) -> (ChildSender<P>, IpcReceiver<P::Down>) {
        (self.tx, self.rx)
    }
}

/// One-shot named server created by the browser before spawning a child.
pub struct BootstrapServer<P: ChildProtocol = M0Protocol> {
    server: IpcOneShotServer<BootstrapMsg<P::Down>>,
    name: String,
}

impl<P: ChildProtocol> std::fmt::Debug for BootstrapServer<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `IpcOneShotServer` itself has no `Debug` impl; the name is the only useful field.
        f.debug_struct("BootstrapServer")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl<P: ChildProtocol> BootstrapServer<P> {
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
    pub fn accept_with_timeout(self, timeout: Duration) -> Result<BrowserEndpoint<P>, IpcError> {
        match self.start_accept()?.poll(timeout)? {
            Some(endpoint) => Ok(endpoint),
            None => Err(timed_out(
                "child did not connect to bootstrap server in time",
            )),
        }
    }

    /// Start waiting for the child to connect, without blocking the caller. The accept runs on a
    /// helper thread (`ipc-channel`'s accept has no timeout); the caller polls the returned
    /// [`PendingAccept`] in short slices and can check on the child in between.
    ///
    /// If the child never connects, the helper thread stays blocked in `accept` until the
    /// process exits — one thread per abandoned bootstrap (known M0 debt).
    pub fn start_accept(self) -> Result<PendingAccept<P>, IpcError> {
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
pub struct PendingAccept<P: ChildProtocol = M0Protocol> {
    rx: mpsc::Receiver<Result<BootstrapMsg<P::Down>, ipc_channel::IpcError>>,
}

impl<P: ChildProtocol> std::fmt::Debug for PendingAccept<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingAccept").finish_non_exhaustive()
    }
}

impl<P: ChildProtocol> PendingAccept<P> {
    /// Wait up to `wait` for the child's connection. `Ok(None)` means it has not connected yet;
    /// call again or give up. An error means the accept itself failed and will not succeed later.
    pub fn poll(&self, wait: Duration) -> Result<Option<BrowserEndpoint<P>>, IpcError> {
        match self.rx.recv_timeout(wait) {
            Ok(Ok(msg)) => Ok(Some(BrowserEndpoint {
                tx: msg.to_child,
                rx: msg.to_browser,
                display_lists: msg.display_lists,
            })),
            Ok(Err(e)) => Err(IpcError::Channel(e.to_string())),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(IpcError::Channel("bootstrap accept thread died".into()))
            }
        }
    }
}

/// Called by a child with the name from `--ipc-bootstrap=`. Creates the child's upward byte
/// channel and typed downward channel and sends the browser its ends; a gpu child passes the
/// sender of the display-list channel it created as `display_lists`.
pub fn connect_child<P: ChildProtocol>(
    server_name: &str,
    display_lists: Option<IpcSender<DisplayListMsg>>,
) -> Result<ChildEndpoint<P>, IpcError> {
    let (to_browser_tx, to_browser_rx) = bytes_channel()?;
    let (to_child_tx, to_child_rx) = channel::<P::Down>()?;
    let boot: IpcSender<BootstrapMsg<P::Down>> = IpcSender::connect(server_name.to_owned())?;
    boot.send(BootstrapMsg {
        to_browser: to_browser_rx,
        to_child: to_child_tx,
        display_lists,
    })?;
    Ok(ChildEndpoint {
        tx: ChildSender {
            tx: to_browser_tx,
            _protocol: PhantomData,
        },
        rx: to_child_rx,
    })
}
