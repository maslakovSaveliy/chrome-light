//! The M1b transport across REAL processes (ADR-0016): this test binary relaunches itself as a
//! "gpu" and a "renderer" child, so three processes take part — the test plays the browser.
//! Checks the bootstrap exception (the gpu sends its display-list sender up), the browser relaying
//! it down to the renderer, shared memory renderer → gpu and gpu → browser with copy-then-
//! validate, peer exit, and that repeated shm sends do not leak in either process.
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::print_stderr,
    reason = "test assertions and child-process diagnostics"
)]

use std::{
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

use cl_ipc::{
    ChildProtocol, ReceiverCtx, Validate,
    bootstrap::{BootstrapServer, BrowserEndpoint, connect_child},
    handshake,
    pump::{PumpEvent, spawn_reader},
    shm::copy_bounded,
    transfer::{DisplayListMsg, FrameMsg},
    validate::IpcViolation,
};
use cl_platform::ProcessType;
use ipc_channel::ipc::{IpcSender, IpcSharedMemory, channel};
use serde::{Deserialize, Serialize};

const TIMEOUT: Duration = Duration::from_secs(20);
/// Marker argument that turns a helper test from a no-op into a child process.
const CHILD_MARKER: &str = "cl-ipc-xproc-child";
const MAX_SHM: usize = 64 * 1024 * 1024;

// ---- test protocols -------------------------------------------------------------------------

#[derive(Debug)]
struct GpuP;
#[derive(Debug)]
struct RendererP;

#[derive(Debug, Serialize, Deserialize)]
enum GpuDown {
    HelloAck { accepted: bool },
    FrameSender(IpcSender<FrameMsg>),
    SendFrames { count: u32, width: u32, height: u32 },
    Exit,
}

#[derive(Debug, Serialize, Deserialize)]
enum GpuUp {
    /// The checksum of a display list the gpu received from the renderer.
    Checksum { seq: u64, len: u64, sum: u64 },
    /// All requested frames were sent.
    FramesSent,
}

#[derive(Debug, Serialize, Deserialize)]
enum RendererDown {
    HelloAck { accepted: bool },
    DisplayListSender(IpcSender<DisplayListMsg>),
    SendDisplayList { seq: u64, len: u32 },
    Exit,
}

#[derive(Debug, Serialize, Deserialize)]
enum RendererUp {
    Sent { seq: u64 },
}

impl Validate for GpuUp {
    fn validate(&self, _ctx: &ReceiverCtx) -> Result<(), IpcViolation> {
        Ok(())
    }
}

impl Validate for RendererUp {
    fn validate(&self, _ctx: &ReceiverCtx) -> Result<(), IpcViolation> {
        Ok(())
    }
}

impl ChildProtocol for GpuP {
    const ROLE: ProcessType = ProcessType::Gpu;
    type Down = GpuDown;
    type Up = GpuUp;
    fn hello_ack(accepted: bool) -> GpuDown {
        GpuDown::HelloAck { accepted }
    }
    fn as_hello_ack(msg: &GpuDown) -> Option<bool> {
        match msg {
            GpuDown::HelloAck { accepted } => Some(*accepted),
            _ => None,
        }
    }
}

impl ChildProtocol for RendererP {
    const ROLE: ProcessType = ProcessType::Renderer;
    type Down = RendererDown;
    type Up = RendererUp;
    fn hello_ack(accepted: bool) -> RendererDown {
        RendererDown::HelloAck { accepted }
    }
    fn as_hello_ack(msg: &RendererDown) -> Option<bool> {
        match msg {
            RendererDown::HelloAck { accepted } => Some(*accepted),
            _ => None,
        }
    }
}

// ---- helpers ----------------------------------------------------------------------------------

/// The bootstrap server name if this process was relaunched as a child, else `None`.
fn child_server_name() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    let at = args.iter().position(|a| a == CHILD_MARKER)?;
    args.get(at + 1).cloned()
}

/// Kills the child when the test ends, so a failing assertion leaves no process behind.
struct KillOnDrop(Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn pattern(len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| u8::try_from((i * 31) % 251).expect("< 251"))
        .collect()
}

fn checksum(bytes: &[u8]) -> u64 {
    // FNV-1a: order-sensitive, so a shuffled or truncated copy does not match.
    bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Spawns this test binary as `helper` and completes the bootstrap + handshake as the browser.
fn spawn_child<P: ChildProtocol>(helper: &str) -> (KillOnDrop, BrowserEndpoint<P>) {
    let server = BootstrapServer::<P>::new().expect("bootstrap server");
    let name = server.name().to_owned();
    let exe = std::env::current_exe().expect("current exe");
    let child = Command::new(exe)
        .args(["--exact", helper, CHILD_MARKER, &name, "--nocapture"])
        .stdin(Stdio::null())
        .spawn()
        .expect("spawn child");
    let pid = child.id();
    let guard = KillOnDrop(child);
    let ep = server.accept_with_timeout(TIMEOUT).expect("child connects");
    let ctx = ReceiverCtx {
        expected_process_type: P::ROLE,
        expected_pid: Some(pid),
    };
    handshake::browser_side(&ep, &ctx, TIMEOUT).expect("handshake");
    (guard, ep)
}

/// Resident set size of `pid` in KiB, from `ps` (macOS/Linux) or `tasklist` (Windows).
fn rss_kib(pid: u32) -> u64 {
    if let Ok(out) = Command::new("ps")
        .args(["-o", "rss=", "-p", &pid.to_string()])
        .output()
        && out.status.success()
        && let Ok(kib) = String::from_utf8_lossy(&out.stdout).trim().parse::<u64>()
    {
        return kib;
    }
    let out = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"])
        .output()
        .expect("neither ps nor tasklist could report RSS");
    // "name","pid","session","#","12,345 K" — the last quoted field is the working set in KiB.
    let text = String::from_utf8_lossy(&out.stdout);
    let digits: String = text
        .trim()
        .rsplit("\",\"")
        .next()
        .unwrap_or_default()
        .chars()
        .filter(char::is_ascii_digit)
        .collect();
    digits.parse().expect("tasklist working set")
}

/// Open handles of `pid`: fds from `/proc/<pid>/fd` (Linux), `lsof` entries (macOS), or the
/// kernel handle count (Windows, via PowerShell). RSS cannot see a leaked handle whose mapping
/// is gone, so the leak test counts these too. On macOS `lsof` does not list Mach port rights,
/// so a leaked port is invisible here; Linux and Windows CI carry that check.
fn handle_count(pid: u32) -> u64 {
    if let Ok(entries) = std::fs::read_dir(format!("/proc/{pid}/fd")) {
        return u64::try_from(entries.count()).expect("fits");
    }
    if let Ok(out) = Command::new("lsof")
        .args(["-n", "-P", "-p", &pid.to_string()])
        .output()
        && out.status.success()
    {
        let lines = String::from_utf8_lossy(&out.stdout).lines().count();
        return u64::try_from(lines.saturating_sub(1)).expect("fits"); // minus the header
    }
    let out = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            &format!("(Get-Process -Id {pid}).HandleCount"),
        ])
        .output()
        .expect("no way to count handles: /proc, lsof and powershell all missing");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .expect("powershell HandleCount")
}

// ---- child roles (no-ops in a normal test run) ------------------------------------------------

/// The gpu child: creates the display-list channel (the receiver creates it) and sends its
/// sender up in the bootstrap; checksums every display list it receives; sends frames on the
/// frame channel the browser sends down.
#[test]
fn gpu_helper() {
    let Some(name) = child_server_name() else {
        return;
    };
    let (dl_tx, dl_rx) = channel::<DisplayListMsg>().expect("display-list channel");
    let ep = connect_child::<GpuP>(&name, Some(dl_tx)).expect("connect");
    handshake::child_side(&ep, ProcessType::Gpu, std::process::id()).expect("handshake");
    let (up, down) = ep.split();

    let dl_up = up.clone();
    std::thread::spawn(move || {
        while let Ok(msg) = dl_rx.recv() {
            let copy = copy_bounded(&msg.bytes, MAX_SHM).expect("display list within limit");
            let reply = GpuUp::Checksum {
                seq: msg.seq,
                len: u64::try_from(copy.len()).expect("fits"),
                sum: checksum(&copy),
            };
            if dl_up.send(&reply).is_err() {
                return;
            }
        }
    });

    let mut frames: Option<IpcSender<FrameMsg>> = None;
    while let Ok(msg) = down.recv() {
        match msg {
            GpuDown::FrameSender(tx) => frames = Some(tx),
            GpuDown::SendFrames {
                count,
                width,
                height,
            } => {
                let tx = frames.as_ref().expect("frame sender first");
                let len = usize::try_from(width * height * 4).expect("fits");
                let pixels = pattern(len);
                for seq in 0..count {
                    tx.send(FrameMsg {
                        seq: u64::from(seq),
                        width,
                        height,
                        pixels: IpcSharedMemory::from_bytes(&pixels),
                    })
                    .expect("send frame");
                }
                up.send(&GpuUp::FramesSent).expect("report frames sent");
            }
            GpuDown::Exit => return,
            GpuDown::HelloAck { .. } => panic!("duplicate HelloAck"),
        }
    }
}

/// The renderer child: receives the display-list sender from the browser (never from the gpu)
/// and sends display lists on it.
#[test]
fn renderer_helper() {
    let Some(name) = child_server_name() else {
        return;
    };
    let ep = connect_child::<RendererP>(&name, None).expect("connect");
    handshake::child_side(&ep, ProcessType::Renderer, std::process::id()).expect("handshake");
    let mut dl: Option<IpcSender<DisplayListMsg>> = None;
    loop {
        match ep.recv().expect("recv") {
            RendererDown::DisplayListSender(tx) => dl = Some(tx),
            RendererDown::SendDisplayList { seq, len } => {
                let bytes = pattern(usize::try_from(len).expect("fits"));
                dl.as_ref()
                    .expect("display-list sender first")
                    .send(DisplayListMsg {
                        seq,
                        bytes: IpcSharedMemory::from_bytes(&bytes),
                    })
                    .expect("send display list");
                ep.send(&RendererUp::Sent { seq }).expect("report sent");
            }
            RendererDown::Exit => return,
            RendererDown::HelloAck { .. } => panic!("duplicate HelloAck"),
        }
    }
}

// ---- tests ------------------------------------------------------------------------------------

#[test]
fn dl_shm_should_cross_brokered_channel_3proc() {
    let (_gpu_guard, mut gpu) = spawn_child::<GpuP>("gpu_helper");
    let dl_sender = gpu
        .take_display_lists()
        .expect("the gpu sends its display-list sender in the bootstrap");
    let (_renderer_guard, renderer) = spawn_child::<RendererP>("renderer_helper");

    // The browser relays the gpu's sender DOWN to the renderer.
    renderer
        .send(RendererDown::DisplayListSender(dl_sender))
        .expect("relay sender");
    let len: u32 = 1024 * 1024;
    renderer
        .send(RendererDown::SendDisplayList { seq: 7, len })
        .expect("ask for a display list");
    assert!(matches!(
        renderer.recv_timeout(TIMEOUT).expect("renderer reply"),
        RendererUp::Sent { seq: 7 }
    ));

    let expected = pattern(usize::try_from(len).expect("fits"));
    match gpu.recv_timeout(TIMEOUT).expect("gpu reply") {
        GpuUp::Checksum {
            seq,
            len: got_len,
            sum,
        } => {
            assert_eq!(seq, 7);
            assert_eq!(got_len, u64::from(len));
            assert_eq!(sum, checksum(&expected), "the gpu saw different bytes");
        }
        other @ GpuUp::FramesSent => panic!("unexpected {other:?}"),
    }
    renderer.send(RendererDown::Exit).expect("renderer exit");
    gpu.send(GpuDown::Exit).expect("gpu exit");
}

#[test]
fn frame_shm_should_reach_browser() {
    let (_guard, gpu) = spawn_child::<GpuP>("gpu_helper");
    // The browser creates the frame channel (it receives the shm) and sends the sender down.
    let (frame_tx, frame_rx) = channel::<FrameMsg>().expect("frame channel");
    gpu.send(GpuDown::FrameSender(frame_tx))
        .expect("send frame sender");
    gpu.send(GpuDown::SendFrames {
        count: 1,
        width: 800,
        height: 600,
    })
    .expect("ask for a frame");

    let frame = frame_rx.try_recv_timeout(TIMEOUT).expect("frame arrives");
    assert_eq!((frame.seq, frame.width, frame.height), (0, 800, 600));
    let pixels = copy_bounded(&frame.pixels, 800 * 600 * 4).expect("exact size passes");
    assert_eq!(pixels, pattern(800 * 600 * 4));
    gpu.send(GpuDown::Exit).expect("gpu exit");
}

#[test]
fn empty_shm_should_roundtrip() {
    let (_guard, gpu) = spawn_child::<GpuP>("gpu_helper");
    let (frame_tx, frame_rx) = channel::<FrameMsg>().expect("frame channel");
    gpu.send(GpuDown::FrameSender(frame_tx))
        .expect("send frame sender");
    gpu.send(GpuDown::SendFrames {
        count: 1,
        width: 0,
        height: 0,
    })
    .expect("ask for an empty frame");

    let frame = frame_rx.try_recv_timeout(TIMEOUT).expect("frame arrives");
    assert_eq!((frame.width, frame.height), (0, 0));
    assert_eq!(
        copy_bounded(&frame.pixels, 0).expect("empty passes"),
        Vec::<u8>::new()
    );
    gpu.send(GpuDown::Exit).expect("gpu exit");
}

#[test]
fn peer_exit_should_surface_closed() {
    let (_guard, gpu) = spawn_child::<GpuP>("gpu_helper");
    let (down, up) = gpu.split();
    let (events_tx, events_rx) = mpsc::channel::<String>();
    spawn_reader("test-gpu-up", up, move |event: PumpEvent<GpuUp>| {
        let name = match event {
            PumpEvent::Message(_) => "message",
            PumpEvent::Closed => "closed",
            PumpEvent::Error(_) => "error",
            PumpEvent::Failed(_) => "failed",
        };
        let _ = events_tx.send(name.to_owned());
        true
    })
    .expect("reader thread");

    down.send(GpuDown::Exit).expect("ask the gpu to exit");
    let event = events_rx.recv_timeout(TIMEOUT).expect("an event");
    assert_eq!(event, "closed", "a peer that exits must surface as Closed");
}

#[test]
fn foreign_payload_on_typed_channel_should_err() {
    // A sender reinterpreted as another type sends bytes the receiver cannot decode as its own
    // type: that must be an error, never a panic (in-process: the decoding is the same code).
    let (tx, rx) = channel::<FrameMsg>().expect("channel");
    let foreign: IpcSender<String> = tx.to_opaque().to();
    foreign
        .send("not a frame".to_owned())
        .expect("send foreign payload");
    let result = rx.try_recv_timeout(TIMEOUT);
    assert!(result.is_err(), "a foreign payload decoded as {result:?}");
}

#[test]
fn repeated_shm_sends_should_not_leak() {
    let (guard, gpu) = spawn_child::<GpuP>("gpu_helper");
    let gpu_pid = guard.0.id();
    let (frame_tx, frame_rx) = channel::<FrameMsg>().expect("frame channel");
    gpu.send(GpuDown::FrameSender(frame_tx))
        .expect("send frame sender");

    let side = 1024; // 1024 × 1024 × 4 = 4 MiB per frame
    let max = side * side * 4;
    let receive = |count: u32| {
        gpu.send(GpuDown::SendFrames {
            count,
            width: side,
            height: side,
        })
        .expect("ask for frames");
        for _ in 0..count {
            let frame = frame_rx.try_recv_timeout(TIMEOUT).expect("frame arrives");
            let copy = copy_bounded(&frame.pixels, usize::try_from(max).expect("fits"))
                .expect("within limit");
            assert_eq!(copy.len(), usize::try_from(max).expect("fits"));
        }
        assert!(matches!(
            gpu.recv_timeout(TIMEOUT).expect("gpu reply"),
            GpuUp::FramesSent
        ));
    };

    let me = std::process::id();
    receive(20); // warm up allocators and channel buffers before measuring
    let (browser_before, gpu_before) = (rss_kib(me), rss_kib(gpu_pid));
    let (browser_handles_before, gpu_handles_before) = (handle_count(me), handle_count(gpu_pid));
    receive(500);
    let (browser_after, gpu_after) = (rss_kib(me), rss_kib(gpu_pid));
    let (browser_handles_after, gpu_handles_after) = (handle_count(me), handle_count(gpu_pid));
    eprintln!(
        "RSS KiB: browser {browser_before} -> {browser_after}, gpu {gpu_before} -> {gpu_after}; \
         handles: browser {browser_handles_before} -> {browser_handles_after}, \
         gpu {gpu_handles_before} -> {gpu_handles_after}"
    );
    // 500 frames each carrying one shm region: a per-frame handle leak would add ~500.
    let handle_limit = 64;
    assert!(
        browser_handles_after.saturating_sub(browser_handles_before) < handle_limit,
        "browser handles {browser_handles_before} -> {browser_handles_after} over 500 frames"
    );
    assert!(
        gpu_handles_after.saturating_sub(gpu_handles_before) < handle_limit,
        "gpu handles {gpu_handles_before} -> {gpu_handles_after} over 500 frames"
    );

    let limit = 64 * 1024; // 64 MiB in KiB, against 2 000 MiB sent
    assert!(
        browser_after.saturating_sub(browser_before) < limit,
        "browser grew {browser_before} -> {browser_after} KiB over 500 × 4 MiB frames"
    );
    assert!(
        gpu_after.saturating_sub(gpu_before) < limit,
        "gpu grew {gpu_before} -> {gpu_after} KiB over 500 × 4 MiB frames"
    );
    gpu.send(GpuDown::Exit).expect("gpu exit");
}
