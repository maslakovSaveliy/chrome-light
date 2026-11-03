#![no_main]

use cl_ipc::{
    ChildProtocol, ReceiverCtx, Validate,
    codec::decode_bounded,
    message::Hello,
    proto::{
        gpu::{FromGpu, GpuProtocol},
        renderer::{FromRenderer, RendererProtocol},
    },
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Everything the browser decodes from a child: the handshake opener and both roles' upward
    // messages, bounded and validated exactly as the receiver does. Any panic here is a security
    // bug (docs/CODING_STANDARDS.md §2).
    let ctx = |role| ReceiverCtx {
        expected_process_type: role,
        expected_pid: None,
    };
    if let Ok(hello) = decode_bounded::<Hello>(data, 64) {
        let _ = hello.validate(&ctx(RendererProtocol::ROLE));
    }
    if let Ok(msg) = decode_bounded::<FromRenderer>(data, RendererProtocol::MAX_UP_BYTES) {
        let _ = msg.validate(&ctx(RendererProtocol::ROLE));
    }
    if let Ok(msg) = decode_bounded::<FromGpu>(data, GpuProtocol::MAX_UP_BYTES) {
        let _ = msg.validate(&ctx(GpuProtocol::ROLE));
    }
});
