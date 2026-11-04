//! End-to-end: the real binary as a gpu child. The test plays the browser and the renderer:
//! it takes the display-list sender the gpu created and sent up in its bootstrap, hands the gpu
//! a frame channel and a surface, sends display lists, and checks the frame and the bad-list
//! report that come back. The timeouts below only keep a hung child from hanging the run.
#![allow(clippy::expect_used, reason = "test assertions on a child process")]

use std::path::Path;
use std::time::Duration;

use cl_ipc::BadMessageReason;
use cl_ipc::proto::Viewport;
use cl_ipc::proto::gpu::{FromGpu, GpuProtocol, ToGpu};
use cl_ipc::transfer::{DisplayListMsg, FrameMsg};
use cl_layout::{Rect, Rgba8};
use cl_paint::wire::encode_display_list;
use cl_paint::{DisplayItem, DisplayList};
use cl_process::{SpawnOptions, spawn};
use ipc_channel::ipc::{self, IpcSharedMemory};

const WAIT: Duration = Duration::from_secs(20);

#[test]
fn a_gpu_child_rasters_a_list_and_reports_a_bad_one() {
    let opts = SpawnOptions {
        extra_args: vec!["--no-sandbox".to_owned()],
        timeout: WAIT,
        trace_path: None,
    };
    let exe = Path::new(env!("CARGO_BIN_EXE_chromelight"));
    let mut gpu = spawn::<GpuProtocol>(exe, &opts).expect("spawn gpu");
    let ctx = gpu.receiver_ctx();
    let lists = gpu
        .endpoint
        .take_display_lists()
        .expect("the gpu sends its display-list sender in the bootstrap");

    let (frames_tx, frames) = ipc::channel::<FrameMsg>().expect("frame channel");
    gpu.endpoint
        .send(ToGpu::FrameSender { sender: frames_tx })
        .expect("send frame sender");
    let viewport = Viewport::new(40, 30).expect("viewport");
    gpu.endpoint
        .send(ToGpu::SetSurface { viewport, seq: 1 })
        .expect("send surface");

    let square = DisplayList {
        items: vec![DisplayItem::Rect {
            rect: Rect::from_px(0.0, 0.0, 10.0, 10.0),
            color: Rgba8 {
                r: 255,
                g: 0,
                b: 0,
                a: 255,
            },
        }],
        bounds: Rect::from_px(0.0, 0.0, 40.0, 30.0),
    };
    let bytes = encode_display_list(&square).expect("encode");
    lists
        .send(DisplayListMsg {
            seq: 1,
            bytes: IpcSharedMemory::from_bytes(&bytes),
        })
        .expect("send list");

    let frame = frames.try_recv_timeout(WAIT).expect("a frame");
    assert_eq!((frame.seq, frame.width, frame.height), (1, 40, 30));
    assert_eq!(frame.pixels.len(), 40 * 30 * 4);
    assert_eq!(frame.pixels.get(..4), Some(&[255, 0, 0, 255][..]));

    // A list that does not decode is reported, and the gpu stays up.
    gpu.endpoint
        .send(ToGpu::SetSurface { viewport, seq: 2 })
        .expect("send surface");
    lists
        .send(DisplayListMsg {
            seq: 2,
            bytes: IpcSharedMemory::from_bytes(&[0xff, 0xff, 0xff]),
        })
        .expect("send garbage");
    assert_eq!(
        gpu.endpoint.recv_timeout(WAIT, &ctx).expect("a report"),
        FromGpu::BadDisplayList {
            seq: 2,
            reason: BadMessageReason::DisplayListUndecodable
        }
    );
    // A message of another type on the display-list channel: reported with seq 0 (no seq to
    // trust), the gpu stays up.
    let foreign = lists.to_opaque().to::<u64>();
    foreign.send(42).expect("send a foreign message");
    assert_eq!(
        gpu.endpoint.recv_timeout(WAIT, &ctx).expect("a report"),
        FromGpu::BadDisplayList {
            seq: 0,
            reason: BadMessageReason::DisplayListUndecodable
        }
    );
    gpu.endpoint.send(ToGpu::Ping(9)).expect("send ping");
    assert_eq!(
        gpu.endpoint.recv_timeout(WAIT, &ctx).expect("a pong"),
        FromGpu::Pong(9)
    );

    gpu.endpoint.send(ToGpu::Shutdown).expect("send shutdown");
    let outcome = gpu.wait_timeout(WAIT).expect("wait");
    assert!(outcome.status.success(), "{:?}", outcome.status);
}
