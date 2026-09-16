//! HTML/CSS whitespace processing (CSS Text 3 §4.1.1), applied *before* shaping.
//!
//! [`crate::inline`] concatenates a block's inline content into one string and hands that
//! string to `parley`; everything `white-space` says about which of the source's spaces,
//! tabs and newlines survive into that string happens here, first, so the shaper only ever
//! sees text that is already in its final form. `parley` is never asked to collapse
//! whitespace itself (its `TreeBuilder::set_white_space_mode` is not used): the CSS rules are
//! per-element and interact with the box tree's structure, which is this crate's knowledge,
//! not the shaper's.
//!
//! # What is implemented
//!
//! * [`WhiteSpace::Normal`]: every *segment break* (`\r\n`, `\r`, `\n`) and tab is treated as
//!   a space, then each run of spaces collapses to one, and a space that would land where the
//!   processed text already ends in one — including the very start of a block, and the start
//!   of a line after a forced break — is dropped entirely. A space at the *end* of a line is
//!   not removed from the string here — which line it falls on is not known until the shaper
//!   has broken the text — but simply hangs: [`crate::text`] drops the glyphs of a line's
//!   trailing collapsible spaces when it builds that line's runs, so they neither paint nor
//!   count toward the line's width.
//! * [`WhiteSpace::Pre`]: nothing is collapsed or dropped. Only the segment-break spellings
//!   are normalized (`\r\n` and a lone `\r` both become `\n`), per the HTML parser's own
//!   newline normalization, so a forced break is always exactly one `\n` for the line breaker
//!   to find.
//!
//! # What is not
//!
//! M1a's `white-space` scope is `normal | pre` (`crate::geom::WhiteSpace`), so there is no
//! `pre-wrap`/`pre-line`/`break-spaces` handling here, and no `text-transform`, no
//! bidi-control-character insertion, and no East Asian "collapse the space between two
//! wide characters away entirely" rule (CSS Text 3 §4.1.2's segment-break transformation for
//! CJK) — a segment break in `normal` text always becomes a space, whatever surrounds it.

use crate::geom::WhiteSpace;

/// The characters CSS Text 3 §4.1.1 calls *collapsible* in `white-space: normal`: the space,
/// the tab, and the three segment-break spellings (a lone `\r`, a lone `\n`, and the `\r`
/// and `\n` of a `\r\n` pair, each of which this treats individually — the result is the same
/// single space either way). The form feed is included for the same reason the HTML spec's
/// "ASCII whitespace" set does: the parser can hand one through in text content.
fn is_collapsible(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{0C}')
}

/// The result of processing one run of text: the processed text, plus whether the *next* run
/// of text must suppress a space it would otherwise start with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Processed {
    /// The processed text, ready to be concatenated into the block's shaping string.
    pub(crate) text: String,
    /// Whether a leading collapsible space in whatever comes next must be dropped, because
    /// the processed text so far already ends in a space (or has not emitted anything yet,
    /// which is the start-of-block/start-of-line case).
    pub(crate) suppress_next_space: bool,
}

/// Processes one run of source text per `mode`, given whether a leading collapsible space
/// must be suppressed because the block's processed text so far already ends in a space (or
/// is still empty — the start of a block, and the position just after a forced line break).
///
/// Pure: the only state is the `suppress_leading_space` argument in and the
/// [`Processed::suppress_next_space`] flag out, so a block's inline content is processed by
/// threading that flag through its runs in document order — which is exactly what makes
/// `<span>a </span><span> b</span>` collapse to `"a b"` rather than `"a  b"`.
///
/// Total over any input: no indexing, no recursion, no panic — a 10 MB text node costs one
/// allocation of at most its own length.
pub(crate) fn process(text: &str, mode: WhiteSpace, suppress_leading_space: bool) -> Processed {
    match mode {
        WhiteSpace::Normal => process_normal(text, suppress_leading_space),
        WhiteSpace::Pre => Processed {
            text: normalize_segment_breaks(text),
            // Preserved whitespace is not collapsible, so it neither is nor creates a
            // collapsible space for what follows: the next run starts with a clean slate.
            // (An M1a simplification: CSS Text 3 §4.1.1's rules for a collapsible space
            // *adjacent to* a preserved one are not modelled — mixing `white-space` values
            // within one block is out of M1a's scope.)
            suppress_next_space: false,
        },
    }
}

/// The [`WhiteSpace::Normal`] half of [`process`].
fn process_normal(text: &str, suppress_leading_space: bool) -> Processed {
    let mut out = String::with_capacity(text.len());
    let mut suppress = suppress_leading_space;
    let mut in_space_run = false;

    for c in text.chars() {
        if is_collapsible(c) {
            in_space_run = true;
            continue;
        }
        if in_space_run {
            in_space_run = false;
            if !suppress {
                out.push(' ');
            }
        }
        out.push(c);
        suppress = false;
    }

    // A run of collapsible space at the end of this text is emitted as a single space (it
    // may well separate two words across a run boundary). Whether it should have been
    // dropped as a *trailing* space of the whole block is not knowable here — that is
    // `crate::inline`'s call, once the concatenation is complete.
    if in_space_run && !suppress {
        out.push(' ');
        suppress = true;
    }

    Processed {
        text: out,
        suppress_next_space: suppress,
    }
}

/// Normalizes `\r\n` and a lone `\r` to `\n`, leaving every other character (spaces and tabs
/// included) untouched — the [`WhiteSpace::Pre`] half of [`process`].
fn normalize_segment_breaks(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut after_cr = false;
    for c in text.chars() {
        match c {
            '\r' => {
                out.push('\n');
                after_cr = true;
            }
            // The `\n` of a `\r\n` pair: the `\r` already produced the one newline.
            '\n' if after_cr => after_cr = false,
            other => {
                after_cr = false;
                out.push(other);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shorthand: process `text` in `mode` as if it were the first content of a block.
    fn first(text: &str, mode: WhiteSpace) -> Processed {
        process(text, mode, true)
    }

    #[test]
    fn normal_should_collapse_runs_of_spaces_to_one() {
        assert_eq!(first("a   b", WhiteSpace::Normal).text, "a b");
        assert_eq!(first("a\t\t b", WhiteSpace::Normal).text, "a b");
    }

    #[test]
    fn normal_should_turn_segment_breaks_into_spaces() {
        assert_eq!(first("a\nb", WhiteSpace::Normal).text, "a b");
        assert_eq!(first("a\r\nb", WhiteSpace::Normal).text, "a b");
        assert_eq!(first("a\rb", WhiteSpace::Normal).text, "a b");
        assert_eq!(first("a\n\n\nb", WhiteSpace::Normal).text, "a b");
    }

    #[test]
    fn normal_should_drop_a_leading_space_at_the_start_of_a_block() {
        assert_eq!(first("   hello", WhiteSpace::Normal).text, "hello");
        assert_eq!(first("\n  hello", WhiteSpace::Normal).text, "hello");
    }

    #[test]
    fn normal_should_keep_a_leading_space_after_a_word() {
        // The same text, but with something already emitted before it: the space now
        // separates two words and must survive.
        assert_eq!(
            process("   hello", WhiteSpace::Normal, false).text,
            " hello"
        );
    }

    #[test]
    fn normal_should_emit_one_trailing_space_for_the_next_run_to_use() {
        let p = process("hello   ", WhiteSpace::Normal, false);
        assert_eq!(p.text, "hello ");
        assert!(p.suppress_next_space, "the next run must not add another");
    }

    #[test]
    fn normal_should_collapse_a_space_split_across_two_runs() {
        let a = process("a ", WhiteSpace::Normal, true);
        assert_eq!(a.text, "a ");
        let b = process(" b", WhiteSpace::Normal, a.suppress_next_space);
        assert_eq!(
            b.text, "b",
            "the second run's space collapses into the first's"
        );
    }

    #[test]
    fn normal_should_handle_empty_and_all_space_input() {
        let empty = process("", WhiteSpace::Normal, false);
        assert_eq!(empty.text, "");
        assert!(
            !empty.suppress_next_space,
            "an empty run must not change the state it was handed"
        );

        let empty_suppressed = process("", WhiteSpace::Normal, true);
        assert_eq!(empty_suppressed.text, "");
        assert!(empty_suppressed.suppress_next_space);

        // A whitespace-only run at the start of a block produces nothing at all — this is
        // the `<body>\n  <p>` case, whose text node must not generate a line box.
        let only_spaces = first("   \n\t ", WhiteSpace::Normal);
        assert_eq!(only_spaces.text, "");
        assert!(only_spaces.suppress_next_space);

        // The same run *between* two words is the single space separating them.
        let between = process("   \n\t ", WhiteSpace::Normal, false);
        assert_eq!(between.text, " ");
        assert!(between.suppress_next_space);
    }

    #[test]
    fn pre_should_preserve_every_space_and_newline() {
        let p = first("  two spaces\nand a newline", WhiteSpace::Pre);
        assert_eq!(p.text, "  two spaces\nand a newline");
        assert!(!p.suppress_next_space);
    }

    #[test]
    fn pre_should_normalize_crlf_and_lone_cr_to_lf() {
        assert_eq!(first("a\r\nb", WhiteSpace::Pre).text, "a\nb");
        assert_eq!(first("a\rb", WhiteSpace::Pre).text, "a\nb");
        assert_eq!(first("a\r\r\nb", WhiteSpace::Pre).text, "a\n\nb");
        assert_eq!(first("a\n\rb", WhiteSpace::Pre).text, "a\n\nb");
    }

    #[test]
    fn pre_should_pass_empty_input_through() {
        assert_eq!(first("", WhiteSpace::Pre).text, "");
    }

    #[test]
    fn processing_should_not_panic_on_large_or_exotic_input() {
        let big = "a \t\n".repeat(50_000);
        assert!(!process(&big, WhiteSpace::Normal, false).text.is_empty());
        assert!(!process(&big, WhiteSpace::Pre, false).text.is_empty());
        // Multi-byte characters must never be split (`chars` only, no slicing), and the only
        // non-ASCII-whitespace character CSS treats as collapsible is none of them: the zero
        // width space is ordinary content, the form feed collapses like a space.
        let exotic = "  日本語\u{0C}\u{200B}  ";
        assert_eq!(
            process(exotic, WhiteSpace::Normal, true).text,
            "日本語 \u{200B} "
        );
    }
}
