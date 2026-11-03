//! The renderer role: what the browser sends a renderer (typed, trusted) and what a renderer
//! sends back (postcard, validated here).

use cl_platform::ProcessType;
use ipc_channel::ipc::{IpcSender, IpcSharedMemory};
use serde::{Deserialize, Serialize};

use super::{
    MAX_SUBRESOURCES_PER_DOCUMENT, MAX_URL_BYTES, MAX_WARNING_BYTES, MAX_WARNINGS, Viewport,
};
use crate::{
    bad_message::BadMessageReason,
    protocol::ChildProtocol,
    transfer::DisplayListMsg,
    validate::{IpcViolation, ReceiverCtx, Validate},
};

/// Browser → renderer.
#[derive(Debug, Serialize, Deserialize)]
pub enum ToRenderer {
    /// Handshake answer.
    HelloAck {
        /// `false` means the renderer must exit.
        accepted: bool,
    },
    /// A document to parse and render. `bytes` are the document's bytes as the browser read them.
    LoadDocument {
        /// Identifies this document in later subresource traffic.
        doc_id: u64,
        /// The document's URL, for resolving relative references.
        base_url: String,
        /// The surface size to lay out for.
        viewport: Viewport,
        /// The frame number the display list must carry.
        seq: u64,
        /// The document bytes.
        bytes: IpcSharedMemory,
    },
    /// The answer to [`FromRenderer::NeedSubresources`], one reply per requested URL.
    Subresources {
        /// The document the request was for.
        doc_id: u64,
        /// Replies, by index into the request's URL list.
        replies: Vec<SubresourceReply>,
    },
    /// Re-lay out the document for a new size. Like [`ToRenderer::Redraw`], answered with a
    /// display list for `seq` laid out for this size, or with
    /// [`FromRenderer::RenderFailed`] for `seq`; received while the document is still loading,
    /// it sets the size and `seq` of the first list.
    Resize {
        /// The new surface size.
        viewport: Viewport,
        /// The frame number the display list must carry.
        seq: u64,
    },
    /// Where to send display lists: the sender of the current gpu's display-list channel.
    DisplayListSender {
        /// The gpu generation it belongs to; bumps on every gpu restart.
        generation: u64,
        /// The channel's sender.
        sender: IpcSender<DisplayListMsg>,
    },
    /// Produce the display list again with a new frame number (after a gpu restart, or a
    /// scale change that keeps the size). Every `Redraw` and [`ToRenderer::Resize`] is
    /// answered with a display list for its `seq`, laid out for the latest viewport the
    /// browser sent, or with [`FromRenderer::RenderFailed`] for that `seq` — the browser waits
    /// for exactly one of them. Received while the document is still loading, it sets the
    /// `seq` of the first list; if the last layout failed (or was for an older viewport), the
    /// renderer lays out again rather than resending an old list.
    Redraw {
        /// The frame number the display list must carry.
        seq: u64,
    },
    /// Liveness probe with a nonce.
    Ping(u64),
    /// Orderly exit request.
    Shutdown,
}

/// One subresource answer.
#[derive(Debug, Serialize, Deserialize)]
pub struct SubresourceReply {
    /// Index into the request's URL list.
    pub index: u32,
    /// The bytes, or why the browser refused.
    pub result: Result<IpcSharedMemory, SubresourceRefusal>,
}

/// Why the browser did not serve a subresource (ADR-0016 §6). Shown as a warning, never fatal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubresourceRefusal {
    /// Not a `file:` URL, or a `file:` URL with a remote host.
    NotFile,
    /// Its canonical path is outside the top-level document's directory.
    OutsideDocumentDirectory,
    /// Not a regular file (a directory, FIFO, device …).
    NotRegularFile,
    /// The per-document byte budget would be exceeded.
    TooLarge,
    /// The per-document URL count was exceeded.
    TooMany,
    /// Reading failed.
    Io,
}

/// Renderer → browser.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FromRenderer {
    /// Reply to [`ToRenderer::Ping`] with the same nonce.
    Pong(u64),
    /// The subresources a document needs, in document order.
    NeedSubresources {
        /// The document they are for.
        doc_id: u64,
        /// At most [`MAX_SUBRESOURCES_PER_DOCUMENT`], each at most [`MAX_URL_BYTES`].
        urls: Vec<String>,
    },
    /// The display list for `seq` was sent to the gpu.
    DocumentReady {
        /// The frame number.
        seq: u64,
        /// At most [`MAX_WARNINGS`], each at most [`MAX_WARNING_BYTES`].
        warnings: Vec<String>,
    },
    /// The renderer could not produce a display list for `seq`.
    RenderFailed {
        /// The frame number.
        seq: u64,
        /// Which stage failed.
        kind: RenderFailureKind,
    },
}

/// The pipeline stage a render failed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RenderFailureKind {
    /// Parsing the document.
    Parse,
    /// Building the style engine or a stylesheet.
    Style,
    /// Loading the bundled fonts.
    Fonts,
    /// Layout.
    Layout,
    /// Encoding the display list.
    Encode,
}

impl Validate for FromRenderer {
    fn validate(&self, _ctx: &ReceiverCtx) -> Result<(), IpcViolation> {
        let invalid = IpcViolation::Bad(BadMessageReason::RendererInvalidField);
        match self {
            Self::Pong(_) | Self::RenderFailed { .. } => Ok(()),
            Self::NeedSubresources { urls, .. } => {
                if urls.len() > MAX_SUBRESOURCES_PER_DOCUMENT {
                    return Err(IpcViolation::Bad(
                        BadMessageReason::RendererTooManySubresources,
                    ));
                }
                if urls.iter().any(|u| u.len() > MAX_URL_BYTES) {
                    return Err(invalid);
                }
                Ok(())
            }
            Self::DocumentReady { warnings, .. } => {
                if warnings.len() > MAX_WARNINGS
                    || warnings.iter().any(|w| w.len() > MAX_WARNING_BYTES)
                {
                    return Err(invalid);
                }
                Ok(())
            }
        }
    }
}

/// The renderer's protocol.
#[derive(Debug)]
pub struct RendererProtocol;

impl ChildProtocol for RendererProtocol {
    const ROLE: ProcessType = ProcessType::Renderer;
    /// The largest valid `FromRenderer` is `NeedSubresources` with 64 URLs of 8 KiB: each string
    /// costs its bytes plus a 2-byte length varint, plus a few bytes of framing. Anything longer
    /// is refused before decoding, which also bounds the transient memory a hostile message can
    /// make the browser allocate before `validate` runs.
    const MAX_UP_BYTES: usize = MAX_SUBRESOURCES_PER_DOCUMENT * (MAX_URL_BYTES + 2) + 64;
    const UNDECODABLE: BadMessageReason = BadMessageReason::RendererUndecodable;
    type Down = ToRenderer;
    type Up = FromRenderer;

    fn hello_ack(accepted: bool) -> ToRenderer {
        ToRenderer::HelloAck { accepted }
    }

    fn as_hello_ack(msg: &ToRenderer) -> Option<bool> {
        match msg {
            ToRenderer::HelloAck { accepted } => Some(*accepted),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;
    use crate::codec;

    fn ctx() -> ReceiverCtx {
        ReceiverCtx {
            expected_process_type: ProcessType::Renderer,
            expected_pid: None,
        }
    }

    fn reason(msg: &FromRenderer) -> Option<BadMessageReason> {
        msg.validate(&ctx())
            .err()
            .and_then(|v| v.bad_message_reason())
    }

    #[test]
    fn need_subresources_over_64_urls_is_invalid() {
        let at = |n: usize| FromRenderer::NeedSubresources {
            doc_id: 1,
            urls: vec!["file:///a.css".to_owned(); n],
        };
        assert_eq!(reason(&at(64)), None);
        assert_eq!(
            reason(&at(65)),
            Some(BadMessageReason::RendererTooManySubresources)
        );
    }

    #[test]
    fn url_over_8k_is_invalid() {
        let with = |len: usize| FromRenderer::NeedSubresources {
            doc_id: 1,
            urls: vec!["a".repeat(len)],
        };
        assert_eq!(reason(&with(MAX_URL_BYTES)), None);
        assert_eq!(
            reason(&with(MAX_URL_BYTES + 1)),
            Some(BadMessageReason::RendererInvalidField)
        );
    }

    #[test]
    fn warning_over_1k_is_invalid() {
        let with = |len: usize| FromRenderer::DocumentReady {
            seq: 1,
            warnings: vec!["w".repeat(len)],
        };
        assert_eq!(reason(&with(MAX_WARNING_BYTES)), None);
        assert_eq!(
            reason(&with(MAX_WARNING_BYTES + 1)),
            Some(BadMessageReason::RendererInvalidField)
        );
    }

    #[test]
    fn warnings_over_64_are_invalid() {
        let at = |n: usize| FromRenderer::DocumentReady {
            seq: 1,
            warnings: vec!["w".to_owned(); n],
        };
        assert_eq!(reason(&at(MAX_WARNINGS)), None);
        assert_eq!(
            reason(&at(MAX_WARNINGS + 1)),
            Some(BadMessageReason::RendererInvalidField)
        );
    }

    #[test]
    #[allow(clippy::expect_used)]
    fn the_largest_valid_message_should_fit_max_up_bytes() {
        let msg = FromRenderer::NeedSubresources {
            doc_id: u64::MAX,
            urls: vec!["u".repeat(MAX_URL_BYTES); MAX_SUBRESOURCES_PER_DOCUMENT],
        };
        let bytes = codec::encode(&msg).expect("encode");
        assert!(
            bytes.len() <= RendererProtocol::MAX_UP_BYTES,
            "{}",
            bytes.len()
        );
    }

    fn from_renderer() -> impl Strategy<Value = FromRenderer> {
        prop_oneof![
            any::<u64>().prop_map(FromRenderer::Pong),
            (any::<u64>(), prop::collection::vec(".{0,40}", 0..8))
                .prop_map(|(doc_id, urls)| FromRenderer::NeedSubresources { doc_id, urls }),
            (any::<u64>(), prop::collection::vec(".{0,40}", 0..8))
                .prop_map(|(seq, warnings)| FromRenderer::DocumentReady { seq, warnings }),
            (any::<u64>(), 0_u8..5).prop_map(|(seq, k)| FromRenderer::RenderFailed {
                seq,
                kind: match k {
                    0 => RenderFailureKind::Parse,
                    1 => RenderFailureKind::Style,
                    2 => RenderFailureKind::Fonts,
                    3 => RenderFailureKind::Layout,
                    _ => RenderFailureKind::Encode,
                },
            }),
        ]
    }

    proptest! {
        #[test]
        fn from_renderer_should_round_trip_through_decode_bounded(msg in from_renderer()) {
            let bytes = codec::encode(&msg).map_err(|e| TestCaseError::fail(e.to_string()))?;
            let back: FromRenderer = codec::decode_bounded(&bytes, RendererProtocol::MAX_UP_BYTES)
                .map_err(|e| TestCaseError::fail(e.to_string()))?;
            prop_assert_eq!(back, msg);
        }
    }
}
