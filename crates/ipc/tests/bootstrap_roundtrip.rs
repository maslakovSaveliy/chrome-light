//! Two threads in one process play browser and child over real ipc-channel transport.

use std::{thread, time::Duration};

use cl_ipc::{
    ReceiverCtx,
    bootstrap::{BootstrapServer, connect_child},
    handshake,
    message::{ToBrowser, ToChild},
};
use cl_platform::ProcessType;

#[test]
#[allow(clippy::expect_used, clippy::panic)]
fn child_should_connect_handshake_and_answer_ping() {
    let server = BootstrapServer::new().expect("server");
    let name = server.name().to_owned();

    let child = thread::spawn(move || {
        let ep = connect_child(&name).expect("connect");
        handshake::child_side(&ep, ProcessType::Renderer, 4242).expect("child handshake");
        loop {
            match ep.recv().expect("recv") {
                ToChild::Ping(n) => ep.send(&ToBrowser::Pong(n)).expect("send pong"),
                ToChild::Shutdown => break,
                ToChild::HelloAck { .. } => panic!("duplicate ack"),
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

    ep.send(&ToChild::Ping(77)).expect("send ping");
    assert_eq!(ep.recv().expect("recv"), ToBrowser::Pong(77));
    ep.send(&ToChild::Shutdown).expect("send shutdown");
    child.join().expect("child thread");
}

#[test]
#[allow(clippy::expect_used)]
fn browser_should_reject_child_claiming_wrong_process_type() {
    let server = BootstrapServer::new().expect("server");
    let name = server.name().to_owned();

    let child = thread::spawn(move || {
        let ep = connect_child(&name).expect("connect");
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
    let server = BootstrapServer::new().expect("server");
    let err = server
        .accept_with_timeout(Duration::from_millis(200))
        .expect_err("must time out");
    assert!(matches!(err, cl_ipc::IpcError::Transport(_)), "got {err:?}");
}

#[test]
#[allow(clippy::expect_used)]
fn recv_timeout_should_time_out_when_child_stays_silent() {
    let server = BootstrapServer::new().expect("server");
    let name = server.name().to_owned();
    let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
    let child = thread::spawn(move || {
        let _ep = connect_child(&name).expect("connect");
        // Connected but never sends Hello; wait until told to exit.
        let _ = stop_rx.recv();
    });
    let ep = server
        .accept_with_timeout(Duration::from_secs(10))
        .expect("accept");
    let err = ep
        .recv_timeout(Duration::from_millis(200))
        .expect_err("must time out");
    assert!(
        matches!(err, cl_ipc::IpcError::Transport(ref e) if e.kind() == std::io::ErrorKind::TimedOut),
        "got {err:?}"
    );
    let _ = stop_tx.send(());
    child.join().expect("child thread");
}
