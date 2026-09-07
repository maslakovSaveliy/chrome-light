//! Wire codec: `postcard` with a hard size limit. This is the exact decoder that runs on
//! hostile bytes in every process, so it is fuzzed (`tools/fuzz/fuzz_targets/ipc_decode.rs`).

use serde::{Serialize, de::DeserializeOwned};

/// Largest message we will encode or decode. Bulk data (frames, resources) goes through shared
/// memory, never through messages.
pub const MAX_MESSAGE_BYTES: usize = 16 * 1024 * 1024;

/// Codec failure. Never panics; hostile input yields `Err`.
#[derive(Debug, thiserror::Error)]
pub enum CodecError {
    /// Encoded size exceeds [`MAX_MESSAGE_BYTES`].
    #[error("message too large: {size} bytes > {max}")]
    TooLarge {
        /// Offending size.
        size: usize,
        /// The limit.
        max: usize,
    },
    /// `postcard` could not (de)serialize.
    #[error("postcard: {0}")]
    Postcard(#[from] postcard::Error),
    /// Bytes remained after a complete value — a framing violation.
    #[error("trailing bytes after message: {0}")]
    Trailing(usize),
}

/// Serialize a message. Fails if the result would exceed [`MAX_MESSAGE_BYTES`].
pub fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, CodecError> {
    let bytes = postcard::to_allocvec(value)?;
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(CodecError::TooLarge {
            size: bytes.len(),
            max: MAX_MESSAGE_BYTES,
        });
    }
    Ok(bytes)
}

/// Deserialize exactly one message from `bytes`. Rejects oversized input before touching it and
/// rejects trailing bytes. Never panics on any input.
pub fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, CodecError> {
    if bytes.len() > MAX_MESSAGE_BYTES {
        return Err(CodecError::TooLarge {
            size: bytes.len(),
            max: MAX_MESSAGE_BYTES,
        });
    }
    let (value, rest) = postcard::take_from_bytes::<T>(bytes)?;
    if !rest.is_empty() {
        return Err(CodecError::Trailing(rest.len()));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use cl_platform::ProcessType;
    use proptest::prelude::*;

    use super::*;
    use crate::message::{Hello, ToBrowser};

    #[test]
    #[allow(clippy::expect_used)]
    fn encode_then_decode_should_round_trip_hello() {
        let msg = ToBrowser::Hello(Hello {
            protocol_version: 1,
            process_type: ProcessType::Gpu,
            pid: 7,
        });
        let bytes = encode(&msg).expect("encode");
        let back: ToBrowser = decode(&bytes).expect("decode");
        assert_eq!(back, msg);
    }

    #[test]
    #[allow(clippy::expect_used)]
    fn decode_should_reject_oversized_input_before_parsing() {
        let big = vec![0u8; MAX_MESSAGE_BYTES + 1];
        let err = decode::<ToBrowser>(&big).expect_err("must reject");
        assert!(matches!(err, CodecError::TooLarge { .. }));
    }

    #[test]
    #[allow(clippy::expect_used)]
    fn decode_should_reject_trailing_bytes() {
        let msg = ToBrowser::Pong(9);
        let mut bytes = encode(&msg).expect("encode");
        bytes.push(0xFF);
        let err = decode::<ToBrowser>(&bytes).expect_err("must reject");
        assert!(matches!(err, CodecError::Trailing(1)));
    }

    #[test]
    fn decode_should_reject_garbage_without_panicking() {
        for bytes in [&[][..], &[0xFF][..], &[0x05, 0x00, 0x00][..]] {
            let _ = decode::<ToBrowser>(bytes);
        }
    }

    proptest! {
        #[test]
        #[allow(clippy::expect_used)]
        fn pong_round_trips_for_any_nonce(n in any::<u64>()) {
            let bytes = encode(&ToBrowser::Pong(n)).expect("encode");
            prop_assert_eq!(decode::<ToBrowser>(&bytes).expect("decode"), ToBrowser::Pong(n));
        }

        #[test]
        fn arbitrary_bytes_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..256)) {
            let _ = decode::<ToBrowser>(&bytes);
        }
    }
}
