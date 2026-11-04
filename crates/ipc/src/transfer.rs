//! Typed messages that carry bulk data upward, from a less trusted process (renderer → gpu,
//! gpu → browser). They hold only [`IpcSharedMemory`] and fixed-size integers — no `Vec`,
//! `String` or channel ends — so `ipc-channel` never deserializes a channel end from them, and
//! the hostile-index panic of `src/ipc.rs` cannot run (a bad shm index is an `Err`). The receiver
//! must treat the shared memory as hostile: check its length, copy it
//! ([`crate::shm::copy_bounded`]), then validate the copy.
//!
//! Not covered by the type: a sender can still attach extra channel ends, which `ipc-channel`
//! drops behind a `debug_assert!` (a panic in debug, a port/fd leak in release), and on Linux a
//! non-socket fd that cannot be mapped aborts the receiver. Those are transport gaps of
//! `ipc-channel` itself, tracked in `docs/SECURITY.md` "Known gaps" and due before Gate S0.

use ipc_channel::ipc::IpcSharedMemory;
use serde::{Deserialize, Serialize};

/// An encoded display list, renderer → gpu.
#[derive(Serialize, Deserialize)]
pub struct DisplayListMsg {
    /// The frame number the browser asked for; the gpu checks it against its own surface.
    pub seq: u64,
    /// The encoded display list. Writable by the sender after sending — copy before use.
    pub bytes: IpcSharedMemory,
}

/// A rasterised frame, gpu → browser.
#[derive(Serialize, Deserialize)]
pub struct FrameMsg {
    /// The frame number this frame answers.
    pub seq: u64,
    /// Width in pixels; the browser checks it against the size it requested.
    pub width: u32,
    /// Height in pixels; the browser checks it against the size it requested.
    pub height: u32,
    /// Premultiplied RGBA8, `width * height * 4` bytes. Copy before use.
    pub pixels: IpcSharedMemory,
}

impl std::fmt::Debug for DisplayListMsg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DisplayListMsg")
            .field("seq", &self.seq)
            .field("bytes_len", &self.bytes.len())
            .finish()
    }
}

impl std::fmt::Debug for FrameMsg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrameMsg")
            .field("seq", &self.seq)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("pixels_len", &self.pixels.len())
            .finish()
    }
}
