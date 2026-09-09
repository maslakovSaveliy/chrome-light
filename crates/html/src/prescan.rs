//! The `<meta>` prescan: a bounded, byte-level scan of an HTML document's first 1024 bytes
//! that looks for `<meta charset>` / `<meta http-equiv="Content-Type" content="...">` before
//! any real tokenizing happens.
//!
//! Implements <https://html.spec.whatwg.org/multipage/parsing.html#prescan-a-byte-stream-to-determine-its-encoding>
//! (main loop, comment skipping, generic-tag skipping, the `meta`-specific attribute loop) and
//! its two referenced sub-algorithms: "get an attribute" (same page, §13.2.3.2) and
//! <https://html.spec.whatwg.org/multipage/urls-and-fetching.html#algorithm-for-extracting-a-character-encoding-from-a-meta-element>.
//!
//! This runs on wholly untrusted bytes before any other validation, so every accessor here
//! goes through `[T]::get` (never `[]`/`[a..b]`), every position advance is `saturating_add`,
//! and the position pointer is strictly increasing on every iteration of every loop — see the
//! comment on [`prescan_meta_charset`]'s main loop for why that bounds the total work.
//!
//! Deliberately not implemented: the spec's UTF-16-without-BOM sniff inside this same
//! algorithm (a document starting with the exact byte pattern for `<?x` in UTF-16LE/BE with no
//! BOM at all). Real UTF-16-labelled-only-by-byte-pattern `text/html` documents are not
//! something any deployed browser sees in practice — BOM sniffing already covers the case that
//! matters — so this is skipped for M1a; see the Task 7 report.

use encoding_rs::Encoding;

/// Per <https://html.spec.whatwg.org/multipage/parsing.html#prescan-a-byte-stream-to-determine-its-encoding>,
/// user agents are "encouraged to only prescan the first 1024 bytes" of the document. We treat
/// this as a hard cap rather than a suggestion: it bounds how much attacker-controlled input
/// this scan ever looks at, regardless of how large the real document is.
const PRESCAN_LIMIT_BYTES: usize = 1024;

/// HTML "ASCII whitespace": tab, LF, FF, CR, space. Used throughout the prescan and the
/// `get-an-attribute`/`extract-a-character-encoding` sub-algorithms.
fn is_ascii_ws(b: u8) -> bool {
    matches!(b, 0x09 | 0x0A | 0x0C | 0x0D | 0x20)
}

/// ASCII whitespace plus `/` — the "tag-space" byte set the spec uses both to recognize
/// `<meta` followed by an attribute and as the leading characters `get an attribute` skips.
fn is_ws_or_slash(b: u8) -> bool {
    is_ascii_ws(b) || b == b'/'
}

/// The spec's byte-level lowercasing rule, applied uniformly to attribute names/values and to
/// tag-name/keyword matching: `A`..=`Z` (0x41..=0x5A) becomes `b + 0x20`; anything else is
/// unchanged (this is *not* Unicode case folding — the prescan never decodes the bytes at all).
fn ascii_lower(b: u8) -> u8 {
    if b.is_ascii_uppercase() { b + 0x20 } else { b }
}

/// `bytes.get(pos)`, i.e. `None` past the end — the only way this module reads a byte.
fn byte(bytes: &[u8], pos: usize) -> Option<u8> {
    bytes.get(pos).copied()
}

/// `bytes.get(pos + offset)`, saturating rather than panicking if `pos + offset` overflows
/// (impossible in practice given the 1024-byte window, but this module never assumes it).
fn at(bytes: &[u8], pos: usize, offset: usize) -> Option<u8> {
    pos.checked_add(offset).and_then(|p| byte(bytes, p))
}

/// Advance `*pos` past every byte matching `pred`, stopping at the first non-matching byte or
/// at the end of `bytes`.
fn skip_while(bytes: &[u8], pos: &mut usize, pred: impl Fn(u8) -> bool) {
    while let Some(b) = byte(bytes, *pos) {
        if pred(b) {
            *pos = pos.saturating_add(1);
        } else {
            break;
        }
    }
}

fn eq_bytes(a: &[u8], b: &[u8]) -> bool {
    a == b
}

/// `true` if `bytes[pos..]` starts with the literal (case-sensitive) byte sequence `needle`.
fn starts_with_at(bytes: &[u8], pos: usize, needle: &[u8]) -> bool {
    for (i, &want) in needle.iter().enumerate() {
        if at(bytes, pos, i) != Some(want) {
            return false;
        }
    }
    true
}

/// First position at or after `from` where `haystack` contains `needle` (byte-exact, no case
/// folding — callers pass an already-lowercased haystack when they want case-insensitivity).
/// Bounded by `haystack.len()`, never recurses.
fn find_subslice(haystack: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(from.min(haystack.len()));
    }
    let rest = haystack.get(from..)?;
    if rest.len() < needle.len() {
        return None;
    }
    rest.windows(needle.len())
        .position(|w| w == needle)
        .map(|i| from.saturating_add(i))
}

/// The outcome of one call to the spec's "get an attribute" algorithm.
enum Attr {
    /// An attribute was sniffed (its value may be empty — e.g. `<meta charset>` with no `=`).
    Found(Vec<u8>, Vec<u8>),
    /// No attribute here — the position was already at `>` (or a stray one was skipped as
    /// whitespace-equivalent) with nothing between it and the previous terminator.
    None,
    /// Ran out of bytes (the 1024-byte window, or the real end of the document) while still
    /// partway through a name or value. Per the algorithm's own "if the user agent runs out of
    /// bytes... abort the prescan algorithm" clause, this must abort the *whole* prescan, not
    /// just this attribute or this tag — so callers propagate it rather than treating it like
    /// [`Attr::None`].
    Eof,
}

/// <https://html.spec.whatwg.org/multipage/parsing.html#concept-get-attributes-when-sniffing>
/// ("get an attribute"), byte-for-byte. `*pos` is advanced past whatever was consumed; on
/// [`Attr::Found`]/[`Attr::None`] it is left pointing at the byte that ended the attribute
/// (typically unconsumed — `/`, `>`, or the start of the next attribute), matching the spec's
/// "abort" steps, which describe stopping *at* that byte rather than past it. Name and value
/// bytes are ASCII-lowercased as they are read (per the spec, not a shortcut we're taking).
fn get_attribute(bytes: &[u8], pos: &mut usize) -> Attr {
    // Step 1: a leading run of whitespace-or-`/` is not part of the name.
    skip_while(bytes, pos, is_ws_or_slash);

    // Step 2: nothing left to sniff.
    match byte(bytes, *pos) {
        None => return Attr::Eof,
        Some(b'>') => return Attr::None,
        Some(_) => {}
    }

    // Steps 3-5: the name, byte by byte.
    let mut name = Vec::new();
    loop {
        let Some(b) = byte(bytes, *pos) else {
            return Attr::Eof;
        };
        if b == b'=' && !name.is_empty() {
            *pos = pos.saturating_add(1);
            return read_attribute_value(bytes, pos, name);
        }
        if is_ascii_ws(b) {
            return after_name_whitespace(bytes, pos, name);
        }
        if b == b'/' || b == b'>' {
            // Left unconsumed: the outer loop (meta attrs / generic tag skip) will see it next.
            return Attr::Found(name, Vec::new());
        }
        name.push(ascii_lower(b));
        *pos = pos.saturating_add(1);
    }
}

/// Steps 6-8 ("spaces"): whitespace was seen right after the name ended; look for `=`.
fn after_name_whitespace(bytes: &[u8], pos: &mut usize, name: Vec<u8>) -> Attr {
    skip_while(bytes, pos, is_ascii_ws);
    match byte(bytes, *pos) {
        None => Attr::Eof,
        Some(b'=') => {
            *pos = pos.saturating_add(1);
            read_attribute_value(bytes, pos, name)
        }
        Some(_) => Attr::Found(name, Vec::new()),
    }
}

/// Steps 9-12 ("value"): everything after a confirmed `=`.
fn read_attribute_value(bytes: &[u8], pos: &mut usize, name: Vec<u8>) -> Attr {
    skip_while(bytes, pos, is_ascii_ws);
    match byte(bytes, *pos) {
        None => Attr::Eof,
        Some(quote @ (b'"' | b'\'')) => {
            *pos = pos.saturating_add(1);
            let mut value = Vec::new();
            loop {
                match byte(bytes, *pos) {
                    None => return Attr::Eof,
                    Some(b) if b == quote => {
                        *pos = pos.saturating_add(1);
                        return Attr::Found(name, value);
                    }
                    Some(b) => {
                        value.push(ascii_lower(b));
                        *pos = pos.saturating_add(1);
                    }
                }
            }
        }
        Some(b'>') => Attr::Found(name, Vec::new()),
        Some(_) => {
            let mut value = Vec::new();
            loop {
                match byte(bytes, *pos) {
                    None => return Attr::Eof,
                    Some(b) if is_ascii_ws(b) || b == b'>' => return Attr::Found(name, value),
                    Some(b) => {
                        value.push(ascii_lower(b));
                        *pos = pos.saturating_add(1);
                    }
                }
            }
        }
    }
}

/// What scanning one `<meta ...>` tag's attributes decided.
enum TagOutcome {
    /// No encoding from this tag; resume the main scan from this position.
    Continue(usize),
    /// This tag's `charset` (directly, or via `http-equiv="Content-Type"` + `content`)
    /// resolved to a supported encoding.
    Found(&'static Encoding),
    /// Ran out of bytes partway through this tag's attributes — abort the whole prescan.
    Eof,
}

/// The `meta`-tag body of the main prescan loop: the "Let attribute list be empty... Let got
/// pragma be false... Attributes: Get an attribute..." block, through the final need-pragma /
/// got-pragma / charset decision. `pos` points just past the recognized `<meta` + tag-space.
fn scan_meta_attributes(bytes: &[u8], mut pos: usize) -> TagOutcome {
    let mut seen_names: Vec<Vec<u8>> = Vec::new();
    let mut got_pragma = false;
    let mut need_pragma: Option<bool> = None;
    // `None` = "null" (never set); `Some(None)` = a `charset`/`content` value that failed to
    // resolve to a supported encoding ("failure", distinct from null); `Some(Some(enc))` = a
    // resolved encoding. Mirrors the spec's own three-way `charset` variable exactly.
    let mut charset: Option<Option<&'static Encoding>> = None;

    loop {
        let (name, value) = match get_attribute(bytes, &mut pos) {
            Attr::Eof => return TagOutcome::Eof,
            Attr::None => break,
            Attr::Found(name, value) => (name, value),
        };

        // "If the attribute's name is already in attribute list, then return to the step
        // labeled attributes" — i.e. only the *first* occurrence of a given name counts.
        if seen_names.iter().any(|seen| eq_bytes(seen, &name)) {
            continue;
        }
        seen_names.push(name.clone());

        if eq_bytes(&name, b"http-equiv") {
            // `value` was already ASCII-lowercased byte-by-byte while it was read, so a plain
            // byte comparison against a lowercase literal is the case-insensitive match here.
            if eq_bytes(&value, b"content-type") {
                got_pragma = true;
            }
        } else if eq_bytes(&name, b"content") {
            if charset.is_none()
                && let Some(enc) = extract_encoding_from_meta_content(&value)
            {
                charset = Some(Some(enc));
                need_pragma = Some(true);
            }
        } else if eq_bytes(&name, b"charset") {
            // Unconditional, even if `content` already set `charset`: an explicit `charset`
            // attribute always wins, and always marks the pragma as not required.
            charset = Some(Encoding::for_label(&value));
            need_pragma = Some(false);
        }
    }

    match need_pragma {
        None => TagOutcome::Continue(pos),
        Some(true) if !got_pragma => TagOutcome::Continue(pos),
        _ => match charset {
            Some(Some(enc)) => TagOutcome::Found(normalize_prescanned_encoding(enc)),
            _ => TagOutcome::Continue(pos),
        },
    }
}

/// "If charset is a UTF-16 encoding, then set charset to UTF-8. If charset is x-user-defined,
/// then set charset to windows-1252." — applied once, uniformly, to whatever the `meta` tag's
/// attribute loop resolved (whether via `charset=` or `http-equiv`+`content`).
fn normalize_prescanned_encoding(encoding: &'static Encoding) -> &'static Encoding {
    if encoding == encoding_rs::UTF_16BE || encoding == encoding_rs::UTF_16LE {
        encoding_rs::UTF_8
    } else if encoding == encoding_rs::X_USER_DEFINED {
        encoding_rs::WINDOWS_1252
    } else {
        encoding
    }
}

/// <https://html.spec.whatwg.org/multipage/urls-and-fetching.html#algorithm-for-extracting-a-character-encoding-from-a-meta-element>,
/// run on an already-lowercased `content` attribute value: find `charset` followed by (optional
/// whitespace, then) `=`, then a quoted or unquoted value, and resolve that value as an
/// encoding label. Bounded by `content.len()` since `find_subslice`'s search start only ever
/// increases across retries.
fn extract_encoding_from_meta_content(content: &[u8]) -> Option<&'static Encoding> {
    const CHARSET: &[u8] = b"charset";

    let mut search_from = 0usize;
    let value_start_after_eq = loop {
        let found = find_subslice(content, search_from, CHARSET)?;
        let mut p = found.saturating_add(CHARSET.len());
        skip_while(content, &mut p, is_ascii_ws);
        if byte(content, p) == Some(b'=') {
            break p.saturating_add(1);
        }
        // "move position to point just before that next character, and jump back to loop"
        search_from = p;
    };

    let mut p = value_start_after_eq;
    skip_while(content, &mut p, is_ascii_ws);
    let label = match byte(content, p) {
        None => return None,
        Some(quote @ (b'"' | b'\'')) => {
            let start = p.saturating_add(1);
            let mut end = start;
            // The spec's "extract a character encoding from a meta element" requires a *matching*
            // closing quote: "if the next character is not present, return nothing". Running off the
            // end of `content` is therefore not "take everything up to EOF" — it is a hard failure,
            // and the whole meta element yields no encoding. Real browsers agree; treating an
            // unterminated quote as a valid label would let malformed markup pick our encoding.
            let mut closed = false;
            while let Some(b) = byte(content, end) {
                if b == quote {
                    closed = true;
                    break;
                }
                end = end.saturating_add(1);
            }
            if !closed {
                return None;
            }
            content.get(start..end)?
        }
        Some(_) => {
            let start = p;
            let mut end = start;
            while let Some(b) = byte(content, end) {
                if is_ascii_ws(b) || b == b';' {
                    break;
                }
                end = end.saturating_add(1);
            }
            content.get(start..end)?
        }
    };
    Encoding::for_label(label)
}

/// `true` if `bytes[pos]` is `<`, optionally followed by `/`, followed by an ASCII letter —
/// the spec's pattern for "some tag that isn't specifically recognized" (opening or closing).
/// Returns the position of that first name byte (where the tag-name skip should start).
fn generic_tag_name_start(bytes: &[u8], pos: usize) -> Option<usize> {
    if at(bytes, pos, 0) != Some(b'<') {
        return None;
    }
    let mut p = pos.saturating_add(1);
    if at(bytes, p, 0) == Some(b'/') {
        p = p.saturating_add(1);
    }
    match byte(bytes, p) {
        Some(b) if b.is_ascii_alphabetic() => Some(p),
        _ => None,
    }
}

/// "Advance to next whitespace/>, then repeatedly get attributes [discarding them]" — skips
/// an entire non-`meta` tag. Returns the position of the byte that ended the last
/// `get_attribute` call (normally `>`), for the caller's shared one-byte advance.
fn skip_generic_tag(bytes: &[u8], pos: usize) -> Option<usize> {
    let name_start = generic_tag_name_start(bytes, pos)?;
    let mut p = name_start;
    loop {
        match byte(bytes, p) {
            None => return None,
            Some(b) if is_ascii_ws(b) || b == b'>' => break,
            Some(_) => p = p.saturating_add(1),
        }
    }
    loop {
        match get_attribute(bytes, &mut p) {
            Attr::Eof => return None,
            Attr::None => return Some(p),
            Attr::Found(..) => {}
        }
    }
}

/// `<meta` (case-insensitive) followed by a tag-space byte (whitespace or `/` — *not* `>`, per
/// the spec's literal byte list, so `<meta>` with no space is *not* recognized here and falls
/// through to [`generic_tag_name_start`] instead, matching real browsers). Returns the position
/// just past that tag-space byte, ready for the attribute loop.
fn match_meta_start(bytes: &[u8], pos: usize) -> Option<usize> {
    if at(bytes, pos, 0) != Some(b'<') {
        return None;
    }
    if !matches!(at(bytes, pos, 1)?, b'M' | b'm') {
        return None;
    }
    if !matches!(at(bytes, pos, 2)?, b'E' | b'e') {
        return None;
    }
    if !matches!(at(bytes, pos, 3)?, b'T' | b't') {
        return None;
    }
    if !matches!(at(bytes, pos, 4)?, b'A' | b'a') {
        return None;
    }
    let tag_space = at(bytes, pos, 5)?;
    if is_ws_or_slash(tag_space) {
        Some(pos.saturating_add(6))
    } else {
        None
    }
}

/// Finds the end of an HTML comment (`<!--` ... `-->`) starting at `pos` (which points at the
/// leading `<`). Returns the position of the closing `>`, or `None` if the window ends first.
fn skip_comment(bytes: &[u8], pos: usize) -> Option<usize> {
    let mut p = pos.saturating_add(4); // past the opening "<!--"
    loop {
        match byte(bytes, p) {
            None => return None,
            Some(b'>') => {
                let prev1 = p.checked_sub(1).and_then(|q| byte(bytes, q));
                let prev2 = p.checked_sub(2).and_then(|q| byte(bytes, q));
                if prev1 == Some(b'-') && prev2 == Some(b'-') {
                    return Some(p);
                }
                p = p.saturating_add(1);
            }
            Some(_) => p = p.saturating_add(1),
        }
    }
}

/// `<!`, `</`, or `<?` that did *not* match the comment start above — skip to the next `>`.
fn starts_with_bang_slash_or_question(bytes: &[u8], pos: usize) -> bool {
    at(bytes, pos, 0) == Some(b'<') && matches!(at(bytes, pos, 1), Some(b'!' | b'/' | b'?'))
}

fn skip_to_gt(bytes: &[u8], pos: usize) -> Option<usize> {
    let mut p = pos.saturating_add(2);
    loop {
        match byte(bytes, p) {
            None => return None,
            Some(b'>') => return Some(p),
            Some(_) => p = p.saturating_add(1),
        }
    }
}

/// Scan (at most) the first 1024 bytes of `bytes` for a `<meta charset>` or
/// `<meta http-equiv="Content-Type" content="...charset=...">` declaration, per
/// <https://html.spec.whatwg.org/multipage/parsing.html#prescan-a-byte-stream-to-determine-its-encoding>.
///
/// Never panics on any input, and never reads past `min(bytes.len(), 1024)`: each of the main
/// loop's branches leaves the position pointer *at* whatever byte ended the structure it just
/// skipped (a comment's `>`, a tag's `>`, ...), and every iteration then advances by exactly
/// one more byte — so the position is strictly increasing on every iteration, which bounds the
/// whole scan to that window regardless of how adversarial the input is.
pub(crate) fn prescan_meta_charset(bytes: &[u8]) -> Option<&'static Encoding> {
    let limit = bytes.len().min(PRESCAN_LIMIT_BYTES);
    let window = bytes.get(..limit).unwrap_or(bytes);

    let mut pos = 0usize;
    while pos < window.len() {
        let next_pos = if starts_with_at(window, pos, b"<!--") {
            skip_comment(window, pos)?
        } else if let Some(after_tag_space) = match_meta_start(window, pos) {
            match scan_meta_attributes(window, after_tag_space) {
                TagOutcome::Found(enc) => return Some(enc),
                TagOutcome::Continue(p) => p,
                TagOutcome::Eof => return None,
            }
        } else if generic_tag_name_start(window, pos).is_some() {
            skip_generic_tag(window, pos)?
        } else if starts_with_bang_slash_or_question(window, pos) {
            skip_to_gt(window, pos)?
        } else {
            pos
        };
        pos = next_pos.saturating_add(1);
    }
    None
}

#[cfg(test)]
#[allow(clippy::expect_used, clippy::unwrap_used)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn scan(html: &[u8]) -> Option<&'static str> {
        prescan_meta_charset(html).map(Encoding::name)
    }

    #[test]
    fn charset_attribute_is_found() {
        assert_eq!(scan(b"<meta charset=windows-1251>"), Some("windows-1251"));
    }

    #[test]
    fn charset_attribute_is_case_insensitive_on_tag_and_value() {
        assert_eq!(scan(b"<META CHARSET=WINDOWS-1251>"), Some("windows-1251"));
    }

    #[test]
    fn quoted_charset_values_are_supported() {
        assert_eq!(
            scan(br#"<meta charset="windows-1251">"#),
            Some("windows-1251")
        );
        assert_eq!(scan(b"<meta charset='windows-1251'>"), Some("windows-1251"));
    }

    #[test]
    fn self_closing_slash_with_a_preceding_space_is_tolerated() {
        // A `/` that starts a *new* attribute scan is whitespace-equivalent (step 1 of "get an
        // attribute"), so as long as it's separated from the value by real whitespace it's
        // harmless.
        assert_eq!(scan(b"<meta charset=windows-1251 />"), Some("windows-1251"));
    }

    #[test]
    fn unmatched_double_quote_in_content_charset_should_yield_nothing() {
        // Spec: "extract a character encoding from a meta element" returns nothing when the quoted
        // value has no closing quote. Taking the rest of the attribute would let malformed markup
        // choose the document's encoding.
        let html = br#"<meta http-equiv='Content-Type' content='text/html; charset="windows-1251'>"#;
        assert_eq!(prescan_meta_charset(html), None);
    }

    #[test]
    fn unmatched_single_quote_in_content_charset_should_yield_nothing() {
        let html = br#"<meta http-equiv="content-type" content='charset="shift_jis'>"#;
        assert_eq!(prescan_meta_charset(html), None);
    }

    #[test]
    fn matched_quote_in_content_charset_should_still_be_extracted() {
        // Guards the fix above against over-correction: a properly closed inner quote still works.
        let html = br#"<meta http-equiv="content-type" content='text/html; charset="windows-1251"'>"#;
        assert_eq!(prescan_meta_charset(html), Some(encoding_rs::WINDOWS_1251));
    }

    #[test]
    fn self_closing_slash_glued_to_an_unquoted_value_becomes_part_of_it() {
        // Real gotcha, faithfully reproduced: unquoted attribute-value scanning only stops at
        // ASCII whitespace or `>` (spec step 11 has no `/` case), so a `/` glued directly onto
        // an unquoted value with no space is *not* special — it's swallowed into the value,
        // which then fails to resolve to any known encoding label.
        assert_eq!(scan(b"<meta charset=windows-1251/>"), None);
    }

    #[test]
    fn meta_without_tag_space_is_not_a_meta_tag() {
        // "<meta>" (no space/slash before '>') does not match the spec's `<meta` + tag-space
        // pattern, so it is treated as an ordinary (empty-attribute) tag rather than sniffed.
        assert_eq!(scan(b"<meta>"), None);
    }

    #[test]
    fn duplicate_attribute_name_keeps_the_first() {
        assert_eq!(
            scan(b"<meta charset=windows-1251 charset=big5>"),
            Some("windows-1251")
        );
    }

    #[test]
    fn explicit_charset_attribute_wins_over_content_regardless_of_order() {
        let html = br#"<meta http-equiv="Content-Type" content="text/html; charset=big5" charset="windows-1251">"#;
        assert_eq!(scan(html), Some("windows-1251"));
    }

    #[test]
    fn content_without_matching_http_equiv_is_ignored() {
        let html = br#"<meta content="text/html; charset=big5">"#;
        assert_eq!(scan(html), None);
    }

    #[test]
    fn http_equiv_without_charset_in_content_is_ignored() {
        let html = br#"<meta http-equiv="Content-Type" content="text/html">"#;
        assert_eq!(scan(html), None);
    }

    #[test]
    fn unrecognized_charset_label_does_not_stop_the_scan() {
        let html = b"<meta charset=not-a-real-encoding><meta charset=big5>";
        assert_eq!(scan(html), Some("Big5")); // encoding_rs's canonical name for the label "big5"
    }

    #[test]
    fn other_tags_and_attributes_are_skipped_without_confusing_the_scanner() {
        let html = br#"<html lang="en" data-x="a=b/c"><head><title>hi</title>
            <meta charset=windows-1251></head></html>"#;
        assert_eq!(scan(html), Some("windows-1251"));
    }

    #[test]
    fn closing_tags_are_skipped_too() {
        let html = b"</notmeta><meta charset=windows-1251>";
        assert_eq!(scan(html), Some("windows-1251"));
    }

    #[test]
    fn doctype_and_processing_instructions_are_skipped() {
        let html = b"<!DOCTYPE html><?xml-stylesheet foo?><meta charset=windows-1251>";
        assert_eq!(scan(html), Some("windows-1251"));
    }

    #[test]
    fn x_user_defined_becomes_windows_1252() {
        assert_eq!(scan(b"<meta charset=x-user-defined>"), Some("windows-1252"));
    }

    #[test]
    fn utf16_meta_label_becomes_utf8() {
        assert_eq!(scan(b"<meta charset=utf-16>"), Some("UTF-8"));
    }

    #[test]
    fn empty_and_tiny_inputs_do_not_panic() {
        assert_eq!(scan(b""), None);
        assert_eq!(scan(b"<"), None);
        assert_eq!(scan(b"<m"), None);
        assert_eq!(scan(b"<meta"), None);
        assert_eq!(scan(b"<meta "), None);
        assert_eq!(scan(b"<!--"), None);
        assert_eq!(scan(b"<!-- unterminated"), None);
    }

    #[test]
    fn truncated_meta_tag_aborts_without_a_false_match() {
        // The `charset` attribute is complete, but the tag's closing quote/`>` never arrives —
        // per the spec's "runs out of bytes -> abort the whole prescan" rule this must not
        // resolve, even though a naive reader might see a plausible-looking label go by.
        assert_eq!(scan(br#"<meta charset="windows-1251"#), None);
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(512))]

        #[test]
        fn prescan_should_never_panic_on_arbitrary_bytes(bytes in proptest::collection::vec(any::<u8>(), 0..2048)) {
            let _ = prescan_meta_charset(&bytes);
        }

        #[test]
        fn prescan_ignores_bytes_beyond_the_1024_byte_window(
            window in proptest::collection::vec(any::<u8>(), 1024),
            tail in proptest::collection::vec(any::<u8>(), 0..512),
        ) {
            // Whatever comes after the first 1024 bytes must never change the result: it's
            // outside the window the spec tells us to look at.
            let mut full = window.clone();
            full.extend_from_slice(&tail);
            let windowed = prescan_meta_charset(&window).map(Encoding::name);
            let with_tail = prescan_meta_charset(&full).map(Encoding::name);
            prop_assert_eq!(windowed, with_tail);
        }
    }
}
