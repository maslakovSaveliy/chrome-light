//! The gpu process (M1b Task 9): in M1–M3 a CPU raster service (ADR-0016 §1). [`GpuState`] is
//! the whole policy as a state machine; `child::serve_gpu` is its IO shell.
//!
//! Every display list comes from the renderer, so it is hostile input (ADR-0016 §4, §7):
//!
//! 1. The shared memory's length is checked against [`MAX_DISPLAY_LIST_BYTES`] before a byte
//!    of it is copied.
//! 2. The browser sets the surface and its `seq`: a list for an older `seq`, or for one
//!    already rastered, is `Stale`; a list for a newer one is held in a single slot (the
//!    highest `seq` wins) until its `SetSurface` arrives.
//! 3. The list is copied out of shared memory, decoded with the in-decode limits of
//!    [`cl_paint::wire`], validated and rasterised by [`cl_gfx::cpu::rasterize`] at the
//!    browser's size — never the size the list claims.
//! 4. A list that fails any of this is reported as `BadDisplayList` with its reason (the
//!    browser kills the renderer); the gpu keeps serving. A raster failure that is not the
//!    list's fault is the gpu's own: it exits and the browser restarts it.

use cl_fonts::{FontDb, FontError};
use cl_gfx::GfxError;
use cl_ipc::bad_message::BadMessageReason;
use cl_ipc::proto::Viewport;
use cl_ipc::proto::gpu::FromGpu;
use cl_ipc::shm::copy_bounded;
use cl_ipc::transfer::DisplayListMsg;
use cl_paint::wire::{MAX_DISPLAY_LIST_BYTES, WireError, decode_display_list};

/// What happened.
#[derive(Debug)]
pub enum GpuInput {
    /// `ToGpu::SetSurface`: raster at this size, expect this `seq` next.
    SetSurface {
        /// The surface size.
        viewport: Viewport,
        /// The frame number.
        seq: u64,
    },
    /// `ToGpu::FrameSender`: the shell now holds the browser's frame sender.
    FrameSender,
    /// A display list from the renderer, its shared memory not yet copied.
    DisplayList(DisplayListMsg),
    /// `ToGpu::Ping`.
    Ping(u64),
    /// `ToGpu::Shutdown`.
    Shutdown,
}

/// What the IO shell must do, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GpuOutput {
    /// Send to the browser.
    Up(FromGpu),
    /// Send this frame on the browser's frame channel.
    Frame {
        /// The frame number it answers.
        seq: u64,
        /// Width in pixels.
        width: u32,
        /// Height in pixels.
        height: u32,
        /// Premultiplied RGBA8, `width × height × 4` bytes.
        pixels: Vec<u8>,
    },
    /// Exit the process.
    Exit,
    /// Exit with an error: the gpu failed for a reason that is not the display list's.
    Fail(String),
}

/// The gpu's state between inputs. See the module docs for the pipeline.
pub struct GpuState {
    fonts: FontDb,
    /// The browser's current surface and its `seq`.
    surface: Option<(Viewport, u64)>,
    /// One owned, size-checked copy of a list for a `seq` not yet surfaced.
    pending: Option<(u64, Vec<u8>)>,
    /// The highest `seq` rastered; each `seq` is rastered once.
    last_rastered: u64,
    frame_sender: bool,
    /// A frame made before the browser's frame sender arrived.
    held_frame: Option<GpuOutput>,
}

impl GpuState {
    /// A gpu with the bundled fonts.
    ///
    /// # Errors
    /// [`FontError`] if the bundled fonts do not load (a corrupted build).
    pub fn new() -> Result<Self, FontError> {
        Ok(GpuState {
            fonts: FontDb::bundled()?,
            surface: None,
            pending: None,
            last_rastered: 0,
            frame_sender: false,
            held_frame: None,
        })
    }

    /// Handles one input and returns what to do, in order.
    pub fn on(&mut self, input: GpuInput) -> Vec<GpuOutput> {
        let mut out = Vec::new();
        match input {
            GpuInput::SetSurface { viewport, seq } => {
                self.surface = Some((viewport, seq));
                match self.pending.take() {
                    Some((held, bytes)) if held == seq => self.raster(seq, &bytes, &mut out),
                    Some((held, _)) if held < seq => out.push(stale(held)),
                    other => self.pending = other,
                }
            }
            GpuInput::FrameSender => {
                self.frame_sender = true;
                out.extend(self.held_frame.take());
            }
            GpuInput::DisplayList(msg) => self.on_list(&msg, &mut out),
            GpuInput::Ping(n) => out.push(GpuOutput::Up(FromGpu::Pong(n))),
            GpuInput::Shutdown => out.push(GpuOutput::Exit),
        }
        out
    }

    fn on_list(&mut self, msg: &DisplayListMsg, out: &mut Vec<GpuOutput>) {
        let seq = msg.seq;
        // Nothing is copied before these checks: the renderer chose the length and the seq.
        if msg.bytes.len() > MAX_DISPLAY_LIST_BYTES {
            out.push(bad(seq, BadMessageReason::DisplayListTooLarge));
            return;
        }
        let surface_seq = self.surface.map(|(_, s)| s);
        let held_seq = self.pending.as_ref().map(|(s, _)| *s);
        let behind_surface = surface_seq.is_some_and(|s| seq < s);
        let future = surface_seq.is_none_or(|s| seq > s);
        let below_held = future && held_seq.is_some_and(|held| held >= seq);
        if seq <= self.last_rastered || behind_surface || below_held {
            out.push(stale(seq));
            return;
        }
        let Ok(bytes) = copy_bounded(&msg.bytes, MAX_DISPLAY_LIST_BYTES) else {
            out.push(bad(seq, BadMessageReason::DisplayListTooLarge));
            return;
        };
        if future {
            // One slot: this list is newer than any held one.
            if let Some((held, _)) = self.pending.replace((seq, bytes)) {
                out.push(stale(held));
            }
        } else {
            self.raster(seq, &bytes, out);
        }
    }

    /// Decodes, validates and rasterises an owned copy at the current surface.
    fn raster(&mut self, seq: u64, bytes: &[u8], out: &mut Vec<GpuOutput>) {
        let Some((viewport, _)) = self.surface else {
            return;
        };
        let dl = match decode_display_list(bytes, MAX_DISPLAY_LIST_BYTES) {
            Ok(dl) => dl,
            Err(e) => {
                out.push(bad(seq, wire_reason(&e)));
                return;
            }
        };
        self.last_rastered = self.last_rastered.max(seq);
        let (width, height) = (viewport.width(), viewport.height());
        let pixmap = match cl_gfx::cpu::rasterize(&dl, width, height, &self.fonts) {
            Ok(pixmap) => pixmap,
            Err(GfxError::Invalid(_)) => {
                out.push(bad(seq, BadMessageReason::DisplayListInvalid));
                return;
            }
            Err(GfxError::FontNotBundled { .. }) => {
                out.push(bad(seq, BadMessageReason::DisplayListUnknownFont));
                return;
            }
            Err(e) => {
                out.push(GpuOutput::Fail(format!("raster failed: {e}")));
                return;
            }
        };
        let frame = GpuOutput::Frame {
            seq,
            width,
            height,
            pixels: pixmap.data().to_vec(),
        };
        if self.frame_sender {
            out.push(frame);
        } else {
            self.held_frame = Some(frame);
        }
    }
}

fn stale(seq: u64) -> GpuOutput {
    GpuOutput::Up(FromGpu::Stale { seq })
}

fn bad(seq: u64, reason: BadMessageReason) -> GpuOutput {
    GpuOutput::Up(FromGpu::BadDisplayList { seq, reason })
}

/// The bad-message reason for a display list that did not decode.
fn wire_reason(error: &WireError) -> BadMessageReason {
    match error {
        WireError::TooLarge { .. } => BadMessageReason::DisplayListTooLarge,
        WireError::Undecodable(_) | WireError::Trailing(_) => {
            BadMessageReason::DisplayListUndecodable
        }
        WireError::LimitsExceeded => BadMessageReason::DisplayListLimitsExceeded,
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "a failed setup step should abort the test, loudly"
)]
mod tests {
    use cl_fonts::FontKey;
    use cl_ipc::bad_message::BadMessageReason;
    use cl_ipc::proto::Viewport;
    use cl_ipc::proto::gpu::FromGpu;
    use cl_ipc::transfer::DisplayListMsg;
    use cl_layout::{Au, Glyph, GlyphRun, Point, Rect, Rgba8};
    use cl_paint::wire::{MAX_DISPLAY_LIST_BYTES, encode_display_list};
    use cl_paint::{DisplayItem, DisplayList, MAX_ITEMS};
    use ipc_channel::ipc::IpcSharedMemory;

    use super::{GpuInput, GpuOutput, GpuState};

    const RED: Rgba8 = Rgba8 {
        r: 255,
        g: 0,
        b: 0,
        a: 255,
    };

    fn vp(width: u32, height: u32) -> Viewport {
        Viewport::new(width, height).expect("viewport")
    }

    /// A gpu with a frame sender.
    fn gpu() -> GpuState {
        let mut g = GpuState::new().expect("fonts");
        assert_eq!(g.on(GpuInput::FrameSender), []);
        g
    }

    /// A red 10×10 square at the origin, painted against `bounds` (what a renderer laid out
    /// for — the gpu must not care).
    fn square(bounds: (f32, f32)) -> DisplayList {
        DisplayList {
            items: vec![DisplayItem::Rect {
                rect: Rect::from_px(0.0, 0.0, 10.0, 10.0),
                color: RED,
            }],
            bounds: Rect::from_px(0.0, 0.0, bounds.0, bounds.1),
        }
    }

    fn list(seq: u64, dl: &DisplayList) -> GpuInput {
        let bytes = encode_display_list(dl).expect("encode");
        raw(seq, &bytes)
    }

    fn raw(seq: u64, bytes: &[u8]) -> GpuInput {
        GpuInput::DisplayList(DisplayListMsg {
            seq,
            bytes: IpcSharedMemory::from_bytes(bytes),
        })
    }

    fn surface(width: u32, height: u32, seq: u64) -> GpuInput {
        GpuInput::SetSurface {
            viewport: vp(width, height),
            seq,
        }
    }

    fn stale(seq: u64) -> GpuOutput {
        GpuOutput::Up(FromGpu::Stale { seq })
    }

    fn bad(seq: u64, reason: BadMessageReason) -> GpuOutput {
        GpuOutput::Up(FromGpu::BadDisplayList { seq, reason })
    }

    /// `(seq, width, height)` of each frame, and its pixel length checked.
    fn frames(outputs: &[GpuOutput]) -> Vec<(u64, u32, u32)> {
        outputs
            .iter()
            .filter_map(|o| match o {
                GpuOutput::Frame {
                    seq,
                    width,
                    height,
                    pixels,
                } => {
                    let len = usize::try_from(width * height * 4).expect("small");
                    assert_eq!(pixels.len(), len);
                    Some((*seq, *width, *height))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_list_for_the_surface_seq_is_rastered() {
        let mut g = gpu();
        assert_eq!(g.on(surface(40, 30, 1)), []);
        let out = g.on(list(1, &square((40.0, 30.0))));
        assert_eq!(frames(&out), [(1, 40, 30)]);
        let GpuOutput::Frame { pixels, .. } = out.first().expect("frame") else {
            unreachable!("checked above")
        };
        // Premultiplied RGBA: the square's first pixel is red, the canvas outside it white.
        assert_eq!(pixels.get(..4), Some(&[255, 0, 0, 255][..]));
        let outside = (5 * 40 + 20) * 4;
        assert_eq!(
            pixels.get(outside..outside + 4),
            Some(&[255, 255, 255, 255][..])
        );
    }

    #[test]
    fn dl_before_set_surface_held_then_rastered() {
        let mut g = gpu();
        assert_eq!(g.on(list(1, &square((40.0, 30.0)))), []);
        assert_eq!(frames(&g.on(surface(40, 30, 1))), [(1, 40, 30)]);
    }

    #[test]
    fn held_list_below_the_new_surface_seq_is_stale() {
        let mut g = gpu();
        assert_eq!(g.on(list(1, &square((40.0, 30.0)))), []);
        assert_eq!(g.on(surface(40, 30, 2)), [stale(1)]);
        assert_eq!(frames(&g.on(list(2, &square((40.0, 30.0))))), [(2, 40, 30)]);
    }

    #[test]
    fn a_future_list_is_held_and_the_highest_seq_wins() {
        let mut g = gpu();
        let _ = g.on(surface(40, 30, 1));
        assert_eq!(g.on(list(3, &square((40.0, 30.0)))), []);
        // A lower future seq does not displace the held one.
        assert_eq!(g.on(list(2, &square((40.0, 30.0)))), [stale(2)]);
        // A higher one does.
        assert_eq!(g.on(list(4, &square((40.0, 30.0)))), [stale(3)]);
        assert_eq!(g.on(surface(40, 30, 3)), []);
        assert_eq!(frames(&g.on(surface(40, 30, 4))), [(4, 40, 30)]);
    }

    #[test]
    fn stale_seq_rejected_as_stale() {
        let mut g = gpu();
        let _ = g.on(surface(40, 30, 2));
        assert_eq!(g.on(list(1, &square((40.0, 30.0)))), [stale(1)]);
    }

    #[test]
    fn a_seq_is_rastered_once() {
        let mut g = gpu();
        let _ = g.on(surface(40, 30, 1));
        assert_eq!(frames(&g.on(list(1, &square((40.0, 30.0))))), [(1, 40, 30)]);
        assert_eq!(g.on(list(1, &square((40.0, 30.0)))), [stale(1)]);
    }

    #[test]
    fn raster_size_comes_from_surface_not_display_list() {
        let mut g = gpu();
        let _ = g.on(surface(200, 150, 1));
        let out = g.on(list(1, &square((100.0, 100.0))));
        assert_eq!(frames(&out), [(1, 200, 150)]);
    }

    #[test]
    fn oversize_shm_rejected_before_copy() {
        let mut g = gpu();
        let _ = g.on(surface(40, 30, 1));
        let huge = vec![0_u8; MAX_DISPLAY_LIST_BYTES + 1];
        assert_eq!(
            g.on(raw(1, &huge)),
            [bad(1, BadMessageReason::DisplayListTooLarge)]
        );
    }

    #[test]
    fn undecodable_bytes_reported_as_bad() {
        let mut g = gpu();
        let _ = g.on(surface(40, 30, 1));
        assert_eq!(
            g.on(raw(1, &[0xff, 0xff, 0xff])),
            [bad(1, BadMessageReason::DisplayListUndecodable)]
        );
        // Still serving: the next good list is rastered.
        let _ = g.on(surface(40, 30, 2));
        assert_eq!(frames(&g.on(list(2, &square((40.0, 30.0))))), [(2, 40, 30)]);
    }

    #[test]
    fn a_list_over_the_item_limit_is_reported() {
        let mut g = gpu();
        let _ = g.on(surface(40, 30, 1));
        // A count of MAX_ITEMS + 1 PopClips (one byte each), as postcard writes it.
        let mut bytes = Vec::new();
        let mut n = u64::try_from(MAX_ITEMS + 1).expect("fits");
        loop {
            let byte = u8::try_from(n & 0x7f).expect("seven bits");
            n >>= 7;
            if n == 0 {
                bytes.push(byte);
                break;
            }
            bytes.push(byte | 0x80);
        }
        bytes.extend(std::iter::repeat_n(4_u8, MAX_ITEMS + 1));
        assert_eq!(
            g.on(raw(1, &bytes)),
            [bad(1, BadMessageReason::DisplayListLimitsExceeded)]
        );
    }

    #[test]
    fn invalid_geometry_is_reported() {
        let mut g = gpu();
        let _ = g.on(surface(40, 30, 1));
        let far = DisplayList {
            items: vec![DisplayItem::Rect {
                rect: Rect::from_px(100_000.0, 0.0, 10.0, 10.0),
                color: RED,
            }],
            bounds: Rect::from_px(0.0, 0.0, 40.0, 30.0),
        };
        assert_eq!(
            g.on(list(1, &far)),
            [bad(1, BadMessageReason::DisplayListInvalid)]
        );
    }

    #[test]
    fn unknown_font_rejected_gpu_alive() {
        let mut g = gpu();
        let _ = g.on(surface(40, 30, 1));
        let unknown: FontKey =
            postcard::from_bytes(&postcard::to_allocvec(&9_999_u32).expect("encode"))
                .expect("a FontKey is a u32 on the wire");
        let text = DisplayList {
            items: vec![DisplayItem::Text {
                run: GlyphRun {
                    font: unknown,
                    size: Au::from_px(16.0),
                    origin: Point {
                        x: Au::from_px(1.0),
                        y: Au::from_px(12.0),
                    },
                    glyphs: vec![Glyph {
                        id: 1,
                        x: Au::ZERO,
                        y: Au::ZERO,
                        advance: Au::from_px(8.0),
                    }],
                    color: RED,
                },
            }],
            bounds: Rect::from_px(0.0, 0.0, 40.0, 30.0),
        };
        assert_eq!(
            g.on(list(1, &text)),
            [bad(1, BadMessageReason::DisplayListUnknownFont)]
        );
        let _ = g.on(surface(40, 30, 2));
        assert_eq!(frames(&g.on(list(2, &square((40.0, 30.0))))), [(2, 40, 30)]);
    }

    #[test]
    fn a_frame_made_before_the_frame_sender_waits_for_it() {
        let mut g = GpuState::new().expect("fonts");
        let _ = g.on(surface(40, 30, 1));
        assert_eq!(g.on(list(1, &square((40.0, 30.0)))), []);
        assert_eq!(frames(&g.on(GpuInput::FrameSender)), [(1, 40, 30)]);
        assert_eq!(g.on(GpuInput::FrameSender), []);
    }

    #[test]
    fn ping_and_shutdown() {
        let mut g = gpu();
        assert_eq!(g.on(GpuInput::Ping(5)), [GpuOutput::Up(FromGpu::Pong(5))]);
        assert_eq!(g.on(GpuInput::Shutdown), [GpuOutput::Exit]);
    }
}
