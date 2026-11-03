//! The gpu role: what the browser sends the gpu (typed, trusted) and what the gpu sends back
//! (postcard, validated here). Display lists and frames travel on their own channels
//! ([`crate::transfer`]).

use cl_platform::ProcessType;
use ipc_channel::ipc::IpcSender;
use serde::{Deserialize, Serialize};

use super::Viewport;
use crate::{
    bad_message::BadMessageReason,
    protocol::ChildProtocol,
    transfer::FrameMsg,
    validate::{IpcViolation, ReceiverCtx, Validate},
};

/// Browser → gpu.
#[derive(Debug, Serialize, Deserialize)]
pub enum ToGpu {
    /// Handshake answer.
    HelloAck {
        /// `false` means the gpu must exit.
        accepted: bool,
    },
    /// The size to raster at and the frame number to expect next.
    SetSurface {
        /// The surface size.
        viewport: Viewport,
        /// The frame number.
        seq: u64,
    },
    /// Where to send frames: the sender of the frame channel the browser created.
    FrameSender {
        /// The channel's sender.
        sender: IpcSender<FrameMsg>,
    },
    /// Liveness probe with a nonce.
    Ping(u64),
    /// Orderly exit request.
    Shutdown,
}

/// Gpu → browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FromGpu {
    /// Reply to [`ToGpu::Ping`] with the same nonce.
    Pong(u64),
    /// A display list for an older frame number arrived and was dropped.
    Stale {
        /// Its frame number.
        seq: u64,
    },
    /// A display list from the renderer was rejected; the browser kills the renderer.
    BadDisplayList {
        /// Its frame number.
        seq: u64,
        /// Why.
        reason: BadMessageReason,
    },
}

impl Validate for FromGpu {
    fn validate(&self, _ctx: &ReceiverCtx) -> Result<(), IpcViolation> {
        match self {
            // The browser checks `seq` against its own state.
            Self::Pong(_) | Self::Stale { .. } => Ok(()),
            // The gpu may only report display-list reasons; anything else would make the browser
            // kill the renderer for a reason the renderer never caused — a bad gpu message.
            Self::BadDisplayList { reason, .. } if reason.is_display_list() => Ok(()),
            Self::BadDisplayList { .. } => {
                Err(IpcViolation::Bad(BadMessageReason::GpuUnexpectedMessage))
            }
        }
    }
}

/// The gpu's protocol.
#[derive(Debug)]
pub struct GpuProtocol;

impl ChildProtocol for GpuProtocol {
    const ROLE: ProcessType = ProcessType::Gpu;
    /// `FromGpu` is a handful of integers.
    const MAX_UP_BYTES: usize = 1024;
    const UNDECODABLE: BadMessageReason = BadMessageReason::GpuUndecodable;
    type Down = ToGpu;
    type Up = FromGpu;

    fn hello_ack(accepted: bool) -> ToGpu {
        ToGpu::HelloAck { accepted }
    }

    fn as_hello_ack(msg: &ToGpu) -> Option<bool> {
        match msg {
            ToGpu::HelloAck { accepted } => Some(*accepted),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::codec;

    fn from_gpu() -> impl Strategy<Value = FromGpu> {
        prop_oneof![
            any::<u64>().prop_map(FromGpu::Pong),
            any::<u64>().prop_map(|seq| FromGpu::Stale { seq }),
            any::<u64>().prop_map(|seq| FromGpu::BadDisplayList {
                seq,
                reason: BadMessageReason::DisplayListInvalid,
            }),
        ]
    }

    fn ctx() -> ReceiverCtx {
        ReceiverCtx {
            expected_process_type: ProcessType::Gpu,
            expected_pid: None,
        }
    }

    #[test]
    fn bad_display_list_should_accept_only_display_list_reasons() {
        let with = |reason| FromGpu::BadDisplayList { seq: 1, reason };
        assert_eq!(
            with(BadMessageReason::DisplayListInvalid).validate(&ctx()),
            Ok(())
        );
        assert_eq!(
            with(BadMessageReason::RendererUndecodable).validate(&ctx()),
            Err(IpcViolation::Bad(BadMessageReason::GpuUnexpectedMessage))
        );
        assert_eq!(
            with(BadMessageReason::FrameWrongSize).validate(&ctx()),
            Err(IpcViolation::Bad(BadMessageReason::GpuUnexpectedMessage))
        );
    }

    #[test]
    #[allow(clippy::expect_used)]
    fn a_reason_should_travel_as_its_code() {
        let msg = FromGpu::BadDisplayList {
            seq: 1,
            reason: BadMessageReason::DisplayListInvalid,
        };
        // Variant index 2, seq 1, then the code 22 — not serde's variant index of the reason.
        assert_eq!(codec::encode(&msg).expect("encode"), [2, 1, 22]);
        assert!(
            codec::decode::<FromGpu>(&[2, 1, 5]).is_err(),
            "unknown code 5 must not decode"
        );
    }

    proptest! {
        #[test]
        fn from_gpu_should_round_trip_through_decode_bounded(msg in from_gpu()) {
            let bytes = codec::encode(&msg).map_err(|e| TestCaseError::fail(e.to_string()))?;
            let back: FromGpu = codec::decode_bounded(&bytes, GpuProtocol::MAX_UP_BYTES)
                .map_err(|e| TestCaseError::fail(e.to_string()))?;
            prop_assert_eq!(back, msg);
        }
    }
}
