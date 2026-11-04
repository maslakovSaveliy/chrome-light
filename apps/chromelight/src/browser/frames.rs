//! [`check_frame`]: whether a frame from the gpu is the one the browser asked for.
//!
//! The browser decides a frame's `seq` and size; the gpu only answers. A frame for any other
//! `seq`, of any other size, or whose pixel buffer does not match its size is a bad message
//! from the gpu (ADR-0016 §7, §9): the browser restarts the gpu.

use cl_ipc::bad_message::BadMessageReason;

/// The integer fields of a [`cl_ipc::transfer::FrameMsg`], once its pixels are copied out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameMeta {
    /// The frame number the gpu says it answers.
    pub seq: u64,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
}

/// Checks a frame of `pixels_len` bytes against the `seq` the browser is waiting for and the
/// size it requested for it.
///
/// # Errors
/// [`BadMessageReason::FrameUnexpectedSeq`] for another `seq`;
/// [`BadMessageReason::FrameWrongSize`] for another size, or more bytes than the size needs;
/// [`BadMessageReason::FrameShortPixels`] for fewer.
pub fn check_frame(
    meta: &FrameMeta,
    pixels_len: usize,
    expected_seq: u64,
    expected: (u32, u32),
) -> Result<(), BadMessageReason> {
    if meta.seq != expected_seq {
        return Err(BadMessageReason::FrameUnexpectedSeq);
    }
    if (meta.width, meta.height) != expected {
        return Err(BadMessageReason::FrameWrongSize);
    }
    // u32 × u32 × 4 fits in u128 (and in u64 for every size up to MAX_SURFACE_DIMENSION²).
    let needed = u128::from(meta.width) * u128::from(meta.height) * 4;
    match u128::try_from(pixels_len).map_or(std::cmp::Ordering::Greater, |len| len.cmp(&needed)) {
        std::cmp::Ordering::Equal => Ok(()),
        std::cmp::Ordering::Less => Err(BadMessageReason::FrameShortPixels),
        std::cmp::Ordering::Greater => Err(BadMessageReason::FrameWrongSize),
    }
}

#[cfg(test)]
mod tests {
    use cl_ipc::bad_message::BadMessageReason;

    use super::{FrameMeta, check_frame};

    fn meta(seq: u64, width: u32, height: u32) -> FrameMeta {
        FrameMeta { seq, width, height }
    }

    #[test]
    fn a_frame_of_the_requested_seq_size_and_length_is_good() {
        assert_eq!(check_frame(&meta(3, 4, 2), 4 * 2 * 4, 3, (4, 2)), Ok(()));
    }

    #[test]
    fn frame_with_wrong_size_or_short_pixels_is_bad() {
        assert_eq!(
            check_frame(&meta(3, 5, 2), 5 * 2 * 4, 3, (4, 2)),
            Err(BadMessageReason::FrameWrongSize)
        );
        assert_eq!(
            check_frame(&meta(3, 4, 2), 4 * 2 * 4 - 1, 3, (4, 2)),
            Err(BadMessageReason::FrameShortPixels)
        );
        // Longer than the size needs: the size the gpu claims is not the size it sent.
        assert_eq!(
            check_frame(&meta(3, 4, 2), 4 * 2 * 4 + 1, 3, (4, 2)),
            Err(BadMessageReason::FrameWrongSize)
        );
    }

    #[test]
    fn a_frame_for_another_seq_is_bad() {
        assert_eq!(
            check_frame(&meta(2, 4, 2), 4 * 2 * 4, 3, (4, 2)),
            Err(BadMessageReason::FrameUnexpectedSeq)
        );
    }

    #[test]
    fn a_huge_claimed_size_does_not_overflow() {
        assert_eq!(
            check_frame(&meta(1, u32::MAX, u32::MAX), 16, 1, (u32::MAX, u32::MAX)),
            Err(BadMessageReason::FrameShortPixels)
        );
    }
}
