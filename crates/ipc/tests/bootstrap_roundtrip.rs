//! Two threads in one process play browser and child over real ipc-channel transport.

use std::{thread, time::Duration};

use cl_ipc::{
    ReceiverCtx,
    bootstrap::{BootstrapServer, connect_child},
    handshake,
    proto::renderer::{FromRenderer, RendererProtocol, ToRenderer},
};
use cl_platform::ProcessType;

#[test]
#[allow(clippy::expect_used, clippy::panic)]
fn child_should_connect_handshake_and_answer_ping() {
    let server = BootstrapServer::<RendererProtocol>::new().expect("server");
    let name = server.name().to_owned();

    let child = thread::spawn(move || {
        let ep = connect_child::<RendererProtocol>(&name, None).expect("connect");
        handshake::child_side(&ep, ProcessType::Renderer, 4242).expect("child handshake");
        loop {
            match ep.recv().expect("recv") {
                ToRenderer::Ping(n) => ep.send(&FromRenderer::Pong(n)).expect("send pong"),
                ToRenderer::Shutdown => break,
                other => panic!("unexpected {other:?}"),
            }
        }
    });

    let ep = server
        .accept_with_timeout(Duration::from_secs(10))
        .expect("accept");
    let ctx = ReceiverCtx {
        expected_process_type: ProcessType::Renderer,
        expected_pid: Some(4242),
    };
    let hello =
        handshake::browser_side(&ep, &ctx, Duration::from_secs(10)).expect("browser handshake");
    assert_eq!(hello.pid, 4242);

    ep.send(ToRenderer::Ping(77)).expect("send ping");
    assert_eq!(ep.recv(&ctx).expect("recv"), FromRenderer::Pong(77));
    ep.send(ToRenderer::Shutdown).expect("send shutdown");
    child.join().expect("child thread");
}

#[test]
#[allow(clippy::expect_used)]
fn browser_should_reject_child_claiming_wrong_process_type() {
    let server = BootstrapServer::<RendererProtocol>::new().expect("server");
    let name = server.name().to_owned();

    let child = thread::spawn(move || {
        let ep = connect_child::<RendererProtocol>(&name, None).expect("connect");
        // Claims Gpu although the browser expects Renderer.
        let result = handshake::child_side(&ep, ProcessType::Gpu, 1);
        assert!(result.is_err(), "child must see rejection");
    });

    let ep = server
        .accept_with_timeout(Duration::from_secs(10))
        .expect("accept");
    let ctx = ReceiverCtx {
        expected_process_type: ProcessType::Renderer,
        expected_pid: Some(1),
    };
    let err = handshake::browser_side(&ep, &ctx, Duration::from_secs(10)).expect_err("must reject");
    assert!(matches!(err, cl_ipc::IpcError::Violation(_)), "got {err:?}");
    child.join().expect("child thread");
}

#[test]
#[allow(clippy::expect_used)]
fn accept_should_time_out_when_no_child_connects() {
    let server = BootstrapServer::<RendererProtocol>::new().expect("server");
    let err = server
        .accept_with_timeout(Duration::from_millis(200))
        .expect_err("must time out");
    assert!(matches!(err, cl_ipc::IpcError::Transport(_)), "got {err:?}");
}

#[test]
#[allow(clippy::expect_used)]
fn recv_timeout_should_time_out_when_child_stays_silent() {
    let server = BootstrapServer::<RendererProtocol>::new().expect("server");
    let name = server.name().to_owned();
    let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
    let child = thread::spawn(move || {
        let _ep = connect_child::<RendererProtocol>(&name, None).expect("connect");
        // Connected but never sends Hello; wait until told to exit.
        let _ = stop_rx.recv();
    });
    let ep = server
        .accept_with_timeout(Duration::from_secs(10))
        .expect("accept");
    let ctx = ReceiverCtx {
        expected_process_type: ProcessType::Renderer,
        expected_pid: None,
    };
    let err = ep
        .recv_timeout(Duration::from_millis(200), &ctx)
        .expect_err("must time out");
    assert!(
        matches!(err, cl_ipc::IpcError::Transport(ref e) if e.kind() == std::io::ErrorKind::TimedOut),
        "got {err:?}"
    );
    let _ = stop_tx.send(());
    child.join().expect("child thread");
}

#[test]
#[allow(clippy::expect_used)]
fn browser_should_refuse_a_display_list_sender_from_a_non_gpu_child() {
    let server = BootstrapServer::<RendererProtocol>::new().expect("server");
    let name = server.name().to_owned();
    let child = thread::spawn(move || {
        let (dl_tx, _dl_rx) =
            ipc_channel::ipc::channel::<cl_ipc::transfer::DisplayListMsg>().expect("channel");
        // A renderer has no business offering a display-list channel.
        let _ep = connect_child::<RendererProtocol>(&name, Some(dl_tx)).expect("connect");
    });
    let err = server
        .accept_with_timeout(Duration::from_secs(10))
        .expect_err("a renderer's display-list sender must be refused");
    assert!(
        matches!(err, cl_ipc::IpcError::UnexpectedMessage(_)),
        "got {err:?}"
    );
    child.join().expect("child thread");
}
