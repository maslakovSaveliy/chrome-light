//! A stable, human-readable rendering of a [`DisplayList`], for snapshot tests and for
//! eyeballing what [`crate::build::build`] actually produced — the same role
//! `cl_layout::dump::fragment_tree_dump` plays for a `FragmentTree`.
//!
//! Unlike the fragment/box-tree dumps this mirrors, a [`DisplayList`] is already flat (no
//! tree to walk with an explicit stack): [`display_list_dump`] is a single linear pass over
//! [`DisplayList::items`], tracking only the current clip depth so a
//! [`crate::list::DisplayItem::PushClip`]/[`crate::list::DisplayItem::PopClip`] pair reads as
//! a nested block rather than two same-indent lines.
//!
//! # Format
//!
//! ```text
//! DisplayList bounds=(0.00, 0.00, 800.00, 600.00) items=3
//! Rect (0.00, 0.00, 800.00, 600.00) #0000ffff
//! PushClip (8.00, 8.00, 100.00, 50.00)
//!   Text font=FontKey(1) size=16.00 origin=(8.00, 24.00) glyphs=2 #000000ff
//! PopClip
//! ```
//!
//! The first line is always `DisplayList bounds=(x, y, w, h) items=N` (`bounds`, like every
//! other rect below, as CSS pixels to two decimal places; `N` = `DisplayList::items.len()`).
//! Every item after it is indented two spaces per current clip depth (a `PushClip` and its
//! matching `PopClip` print at the *same* depth; only what is nested between them is one
//! level deeper — matching this module's own doc-comment code block above). Colors print as
//! lowercase `#rrggbbaa`. A [`cl_fonts::FontKey`] prints via its `Debug` impl (`FontKey(1)`):
//! it has no public accessor for its inner index, and this crate is not the place to add one
//! (`cl-fonts`'s docs already call the type "opaque" — see [`cl_fonts::FontKey`]), so `Debug`
//! is the only stable-enough representation available without widening that crate's API.

use cl_layout::{Au, Rect, Rgba8};

use crate::list::{DisplayItem, DisplayList};

/// Renders `dl` in the format documented above.
#[must_use]
pub fn display_list_dump(dl: &DisplayList) -> String {
    let mut lines = vec![format!(
        "DisplayList bounds={} items={}",
        rect_str(dl.bounds),
        dl.items.len()
    )];

    // A plain counter, not a stack: `PushClip`/`PopClip` are never mismatched in a
    // `DisplayList` `crate::build::build` produced from a well-formed tree (see that
    // function's docs), and a `PopClip` with no matching `PushClip` — reachable only from a
    // hand-built `DisplayList`, never from `build` — saturates to zero rather than
    // underflowing.
    let mut depth: usize = 0;
    for item in &dl.items {
        if matches!(item, DisplayItem::PopClip) {
            depth = depth.saturating_sub(1);
        }
        lines.push(format!("{}{}", indent(depth), item_line(item)));
        if matches!(item, DisplayItem::PushClip { .. }) {
            depth += 1;
        }
    }

    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        out.push_str(line);
    }
    out
}

/// Two spaces per level, matching `cl_layout::dump`'s indent unit (that module's own lines
/// also carry a leading `| ` marker this one omits — a `DisplayList` has no box/fragment kind
/// label needing one, just draw commands).
fn indent(depth: usize) -> String {
    "  ".repeat(depth)
}

/// One item's own line, without indentation (the caller in [`display_list_dump`] adds that).
fn item_line(item: &DisplayItem) -> String {
    match item {
        DisplayItem::Rect { rect, color } => {
            format!("Rect {} {}", rect_str(*rect), rgba_str(*color))
        }
        DisplayItem::Border {
            rect,
            widths,
            colors,
        } => format!(
            "Border {} widths=({}, {}, {}, {}) colors=({}, {}, {}, {})",
            rect_str(*rect),
            px_str(widths.top),
            px_str(widths.right),
            px_str(widths.bottom),
            px_str(widths.left),
            rgba_str(colors.top),
            rgba_str(colors.right),
            rgba_str(colors.bottom),
            rgba_str(colors.left),
        ),
        DisplayItem::Text { run } => format!(
            "Text font={:?} size={} origin=({:.2}, {:.2}) glyphs={} {}",
            run.font,
            px_str(run.size),
            run.origin.x.to_px(),
            run.origin.y.to_px(),
            run.glyphs.len(),
            rgba_str(run.color),
        ),
        DisplayItem::PushClip { rect } => format!("PushClip {}", rect_str(*rect)),
        DisplayItem::PopClip => String::from("PopClip"),
    }
}

/// One rect as `(x, y, w, h)`, each coordinate a CSS pixel value to two decimal places —
/// matching `cl_layout::dump`'s own `rect_str` convention for the same reason: column-like
/// alignment across a run of geometry-heavy lines.
fn rect_str(r: Rect) -> String {
    format!(
        "({:.2}, {:.2}, {:.2}, {:.2})",
        r.origin.x.to_px(),
        r.origin.y.to_px(),
        r.size.w.to_px(),
        r.size.h.to_px()
    )
}

/// One `Au` value as CSS pixels to two decimal places.
fn px_str(au: Au) -> String {
    format!("{:.2}", au.to_px())
}

/// One color as lowercase `#rrggbbaa`.
fn rgba_str(c: Rgba8) -> String {
    format!("#{:02x}{:02x}{:02x}{:02x}", c.r, c.g, c.b, c.a)
}
