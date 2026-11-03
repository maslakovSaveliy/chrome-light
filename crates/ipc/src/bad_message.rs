//! Why the browser killed a child (or a gpu restarted), as a stable number — modelled on Chromium
//! `content/browser/bad_message.h` (ADR-0016 §9). Codes are logged and asserted on by tests;
//! a code is never renumbered or reused, a retired reason keeps its number.

/// The reason a message from a less trusted process was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[repr(u16)]
pub enum BadMessageReason {
    /// A renderer message to the browser did not decode.
    RendererUndecodable = 1,
    /// A renderer message decoded but a field broke a limit or an invariant.
    RendererInvalidField = 2,
    /// A renderer message that is not legal in the current state.
    RendererUnexpectedMessage = 3,
    /// A renderer asked for more subresources than one document may have.
    RendererTooManySubresources = 4,
    /// A display list's shared memory is larger than the gpu accepts.
    DisplayListTooLarge = 20,
    /// A display list did not decode.
    DisplayListUndecodable = 21,
    /// A display list decoded but failed validation.
    DisplayListInvalid = 22,
    /// A display list named a font the gpu does not have.
    DisplayListUnknownFont = 23,
    /// A display list exceeded an item, glyph or nesting limit while decoding.
    DisplayListLimitsExceeded = 24,
    /// A frame's size is not the size the browser requested.
    FrameWrongSize = 40,
    /// A frame's pixel buffer is shorter than its size needs.
    FrameShortPixels = 41,
    /// A frame answers a `seq` the browser did not ask for.
    FrameUnexpectedSeq = 42,
    /// A gpu message to the browser did not decode.
    GpuUndecodable = 43,
    /// A gpu message that is not legal in the current state.
    GpuUnexpectedMessage = 44,
}

impl BadMessageReason {
    /// The stable numeric code.
    #[must_use]
    pub fn code(self) -> u16 {
        self as u16
    }
}

impl std::fmt::Display for BadMessageReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?} ({})", self.code())
    }
}

#[cfg(test)]
mod tests {
    use super::BadMessageReason as R;

    #[test]
    fn bad_message_reason_codes_are_stable() {
        let expected: [(R, u16); 14] = [
            (R::RendererUndecodable, 1),
            (R::RendererInvalidField, 2),
            (R::RendererUnexpectedMessage, 3),
            (R::RendererTooManySubresources, 4),
            (R::DisplayListTooLarge, 20),
            (R::DisplayListUndecodable, 21),
            (R::DisplayListInvalid, 22),
            (R::DisplayListUnknownFont, 23),
            (R::DisplayListLimitsExceeded, 24),
            (R::FrameWrongSize, 40),
            (R::FrameShortPixels, 41),
            (R::FrameUnexpectedSeq, 42),
            (R::GpuUndecodable, 43),
            (R::GpuUnexpectedMessage, 44),
        ];
        for (reason, code) in expected {
            assert_eq!(reason.code(), code, "{reason:?} was renumbered");
        }
    }

    #[test]
    fn display_should_name_the_reason_and_its_code() {
        assert_eq!(R::DisplayListInvalid.to_string(), "DisplayListInvalid (22)");
    }
}
