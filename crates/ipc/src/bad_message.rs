//! Why the browser killed a child (or a gpu restarted), as a stable number — modelled on Chromium
//! `content/browser/bad_message.h` (ADR-0016 §9). Codes are logged and asserted on by tests;
//! a code is never renumbered or reused, a retired reason keeps its number. On the wire
//! (`FromGpu::BadDisplayList`) a reason travels as its code, not as serde's variant index, so
//! adding a variant anywhere in the list never shifts the encoding; an unknown code is refused.

/// The reason a message from a less trusted process was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(into = "u16", try_from = "u16")]
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

/// Every reason, for decoding a code back. The test-only `in_all` stops compiling when a variant
/// is added, as a reminder to list it here.
const ALL: [BadMessageReason; 14] = [
    BadMessageReason::RendererUndecodable,
    BadMessageReason::RendererInvalidField,
    BadMessageReason::RendererUnexpectedMessage,
    BadMessageReason::RendererTooManySubresources,
    BadMessageReason::DisplayListTooLarge,
    BadMessageReason::DisplayListUndecodable,
    BadMessageReason::DisplayListInvalid,
    BadMessageReason::DisplayListUnknownFont,
    BadMessageReason::DisplayListLimitsExceeded,
    BadMessageReason::FrameWrongSize,
    BadMessageReason::FrameShortPixels,
    BadMessageReason::FrameUnexpectedSeq,
    BadMessageReason::GpuUndecodable,
    BadMessageReason::GpuUnexpectedMessage,
];

/// Exhaustive over the variants: a new variant breaks this match until it is added here — and,
/// as the comment in each arm demands, to [`ALL`] (checked by `every_code_should_decode_back…`).
#[cfg(test)]
const fn in_all(reason: BadMessageReason) -> bool {
    match reason {
        // Each of these is in `ALL`.
        BadMessageReason::RendererUndecodable
        | BadMessageReason::RendererInvalidField
        | BadMessageReason::RendererUnexpectedMessage
        | BadMessageReason::RendererTooManySubresources
        | BadMessageReason::DisplayListTooLarge
        | BadMessageReason::DisplayListUndecodable
        | BadMessageReason::DisplayListInvalid
        | BadMessageReason::DisplayListUnknownFont
        | BadMessageReason::DisplayListLimitsExceeded
        | BadMessageReason::FrameWrongSize
        | BadMessageReason::FrameShortPixels
        | BadMessageReason::FrameUnexpectedSeq
        | BadMessageReason::GpuUndecodable
        | BadMessageReason::GpuUnexpectedMessage => true,
    }
}

impl BadMessageReason {
    /// The stable numeric code.
    #[must_use]
    pub fn code(self) -> u16 {
        self as u16
    }

    /// `true` for the reasons a gpu may report about a renderer's display list (codes 20–24).
    #[must_use]
    pub fn is_display_list(self) -> bool {
        matches!(
            self,
            Self::DisplayListTooLarge
                | Self::DisplayListUndecodable
                | Self::DisplayListInvalid
                | Self::DisplayListUnknownFont
                | Self::DisplayListLimitsExceeded
        )
    }
}

/// A number that is not a [`BadMessageReason`] code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("unknown bad-message reason code {0}")]
pub struct UnknownReasonCode(pub u16);

impl TryFrom<u16> for BadMessageReason {
    type Error = UnknownReasonCode;
    fn try_from(code: u16) -> Result<Self, UnknownReasonCode> {
        ALL.into_iter()
            .find(|r| r.code() == code)
            .ok_or(UnknownReasonCode(code))
    }
}

impl From<BadMessageReason> for u16 {
    fn from(reason: BadMessageReason) -> u16 {
        reason.code()
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
    fn every_code_should_decode_back_and_unknown_codes_should_not() {
        for reason in super::ALL {
            assert!(super::in_all(reason));
            assert_eq!(R::try_from(reason.code()), Ok(reason));
        }
        // `ALL` lists each variant once: as many distinct codes as entries.
        let mut codes: Vec<u16> = super::ALL.iter().map(|r| r.code()).collect();
        codes.sort_unstable();
        codes.dedup();
        assert_eq!(codes.len(), super::ALL.len());
        assert!(R::try_from(5).is_err());
        assert!(R::try_from(0).is_err());
    }

    #[test]
    fn display_should_name_the_reason_and_its_code() {
        assert_eq!(R::DisplayListInvalid.to_string(), "DisplayListInvalid (22)");
    }
}
