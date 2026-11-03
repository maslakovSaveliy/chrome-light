//! Determining a document's character encoding and decoding it to text.
//!
//! Implements <https://html.spec.whatwg.org/multipage/parsing.html#determining-the-character-encoding>
//! (§13.2.3.2) with one deliberate deviation — see [`EncodingSource::Default`] — and skips the
//! steps that need state this crate does not have yet (a parent document's encoding for framed
//! content, a per-origin/per-user history of previously chosen encodings, frequency-analysis
//! autodetection): those are all lower-priority *tentative* fallbacks the spec itself only
//! reaches after BOM/transport/prescan fail, and `ChromeLight`'s UTF-8 default already covers
//! that case.

use std::borrow::Cow;

use encoding_rs::Encoding;

use crate::prescan::prescan_meta_charset;

/// Which stage of [`sniff_encoding`] produced its result, in the precedence order the sniffer
/// applies them: a source earlier in this list always wins over one later in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EncodingSource {
    /// A byte-order mark at the very start of the byte stream decided the encoding with
    /// certainty. See <https://html.spec.whatwg.org/multipage/parsing.html#encoding-sniffing-algorithm>
    /// step 1 ("BOM sniffing").
    Bom,
    /// The transport layer's own label decided it — e.g. the HTTP `Content-Type` header's
    /// `charset` parameter, passed in as `transport_label`. Spec step 4.
    TransportLabel,
    /// A `<meta charset>` or `<meta http-equiv="Content-Type" content="...charset=...">` found
    /// within the first 1024 bytes of the document (see [`crate::prescan`]) decided it.
    /// Spec step 5.
    MetaPrescan,
    /// None of the above determined an encoding, so `ChromeLight`'s own UTF-8 default was
    /// used — a deliberate deviation from the spec's locale-dependent default; see the
    /// `SPEC-DEVIATION` comment on [`sniff_encoding`]'s final fallback and the
    /// `encoding.spec.whatwg.org#decode` row in `SPEC_REGISTRY.md`.
    Default,
}

/// Determine which character encoding to decode `bytes` — an HTML document's raw bytes — with.
///
/// Applies, in order: BOM sniffing, then `transport_label` (e.g. the value of the HTTP
/// `Content-Type` header's `charset` parameter, when the caller has one) if it names a
/// supported encoding, then a bounded prescan of the document's own `<meta>` tags (at most the
/// first 1024 bytes), then `ChromeLight`'s UTF-8 default (see [`EncodingSource::Default`]).
///
/// Never panics and never performs unbounded work on `bytes`: only the BOM check and the
/// prescan look at `bytes` itself, and the prescan reads at most its first 1024 bytes.
#[must_use]
pub fn sniff_encoding(
    bytes: &[u8],
    transport_label: Option<&str>,
) -> (&'static Encoding, EncodingSource) {
    if let Some((encoding, _bom_len)) = Encoding::for_bom(bytes) {
        return (encoding, EncodingSource::Bom);
    }

    if let Some(encoding) = transport_label.and_then(|label| Encoding::for_label(label.as_bytes()))
    {
        return (encoding, EncodingSource::TransportLabel);
    }

    if let Some(encoding) = prescan_meta_charset(bytes) {
        return (encoding, EncodingSource::MetaPrescan);
    }

    // SPEC-DEVIATION(https://html.spec.whatwg.org/multipage/parsing.html#determining-the-character-encoding):
    // step 9 of the spec's algorithm falls back to a locale-/implementation-defined default for
    // a wholly unlabelled document (most browsers, including Chrome, use windows-1252 for most
    // locales) with confidence *tentative*. ChromeLight has no locale or per-user-override
    // plumbing yet in M1a, and UTF-8 is what the overwhelming majority of unlabelled documents
    // served today actually are, so we default to UTF-8 unconditionally rather than guessing a
    // locale; tracked in SPEC_REGISTRY (encoding.spec.whatwg.org#decode row).
    (encoding_rs::UTF_8, EncodingSource::Default)
}

/// Sniff `bytes`' encoding (see [`sniff_encoding`]) and decode it to text in one step.
///
/// A byte-order mark that matched the sniffed encoding is stripped from the output (it is not
/// part of the document's actual text); malformed byte sequences are replaced with U+FFFD
/// REPLACEMENT CHARACTER rather than causing an error, since `encoding_rs`'s decoder is total
/// over its input — this always succeeds, even on arbitrary/hostile bytes.
#[must_use]
pub fn decode<'a>(
    bytes: &'a [u8],
    transport_label: Option<&str>,
) -> (Cow<'a, str>, &'static Encoding, EncodingSource) {
    let (encoding, source) = sniff_encoding(bytes, transport_label);
    let (text, _had_errors) = encoding.decode_with_bom_removal(bytes);
    (text, encoding, source)
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use encoding_rs::{KOI8_R, UTF_8, UTF_16LE, WINDOWS_1251};

    use super::*;

    #[test]
    fn bom_should_win_over_meta() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(b"<meta charset=windows-1251>");
        let (encoding, source) = sniff_encoding(&bytes, None);
        assert_eq!(encoding, UTF_8);
        assert_eq!(source, EncodingSource::Bom);
    }

    #[test]
    fn utf16le_bom_should_be_detected() {
        let mut bytes = vec![0xFF, 0xFE];
        bytes.extend_from_slice(b"h\0i\0");
        let (encoding, source) = sniff_encoding(&bytes, None);
        assert_eq!(encoding, UTF_16LE);
        assert_eq!(source, EncodingSource::Bom);
    }

    #[test]
    fn transport_label_should_beat_prescan() {
        let bytes = b"<meta charset=windows-1251>";
        let (encoding, source) = sniff_encoding(bytes, Some("utf-8"));
        assert_eq!(encoding, UTF_8);
        assert_eq!(source, EncodingSource::TransportLabel);
    }

    #[test]
    fn meta_charset_should_be_found_in_first_1024_bytes() {
        let bytes = b"<html><head><meta charset=windows-1251></head></html>";
        let (encoding, source) = sniff_encoding(bytes, None);
        assert_eq!(encoding, WINDOWS_1251);
        assert_eq!(source, EncodingSource::MetaPrescan);
    }

    #[test]
    fn meta_charset_beyond_1024_bytes_should_be_ignored() {
        let mut bytes = vec![b' '; 1100];
        bytes.extend_from_slice(b"<meta charset=windows-1251>");
        let (encoding, source) = sniff_encoding(&bytes, None);
        assert_eq!(encoding, UTF_8);
        assert_eq!(source, EncodingSource::Default);
    }

    #[test]
    fn meta_http_equiv_content_type_should_be_parsed() {
        let bytes = br#"<meta http-equiv="Content-Type" content="text/html; charset=koi8-r">"#;
        let (encoding, source) = sniff_encoding(bytes, None);
        assert_eq!(encoding, KOI8_R);
        assert_eq!(source, EncodingSource::MetaPrescan);
    }

    #[test]
    fn meta_inside_comment_should_be_ignored() {
        let bytes = b"<!-- <meta charset=big5> -->";
        let (encoding, source) = sniff_encoding(bytes, None);
        assert_eq!(encoding, UTF_8);
        assert_eq!(source, EncodingSource::Default);
    }

    #[test]
    fn unknown_label_should_fall_back_to_default() {
        let bytes = b"<html></html>";
        let (encoding, source) = sniff_encoding(bytes, Some("not-a-real-encoding"));
        assert_eq!(encoding, UTF_8);
        assert_eq!(source, EncodingSource::Default);
    }

    #[test]
    fn utf16_meta_label_should_become_utf8() {
        let bytes = b"<meta charset=utf-16>";
        let (encoding, source) = sniff_encoding(bytes, None);
        assert_eq!(encoding, UTF_8);
        assert_eq!(source, EncodingSource::MetaPrescan);
    }

    #[test]
    fn x_user_defined_meta_label_should_become_windows_1252() {
        let bytes = b"<meta charset=x-user-defined>";
        let (encoding, source) = sniff_encoding(bytes, None);
        assert_eq!(encoding, encoding_rs::WINDOWS_1252);
        assert_eq!(source, EncodingSource::MetaPrescan);
    }

    #[test]
    fn decode_should_replace_invalid_sequences_not_fail() {
        let bytes = b"hello \x80\x81 world";
        let (text, encoding, source) = decode(bytes, None);
        assert_eq!(encoding, UTF_8);
        assert_eq!(source, EncodingSource::Default);
        assert!(text.contains('\u{FFFD}'));
        assert!(text.starts_with("hello "));
        assert!(text.ends_with(" world"));
    }

    #[test]
    fn decode_should_strip_a_bom_that_matches_the_sniffed_encoding() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("héllo".as_bytes());
        let (text, encoding, source) = decode(&bytes, None);
        assert_eq!(encoding, UTF_8);
        assert_eq!(source, EncodingSource::Bom);
        assert_eq!(text, "héllo");
    }
}
