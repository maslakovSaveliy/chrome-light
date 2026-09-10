//! Error type for `cl-layout`.
//!
//! Uninhabited today: [`crate::style_adapt::adapt`] is total over any `&ComputedValues`
//! stylo hands back (every computed-value shape it reads has a documented fold onto
//! [`crate::geom::Length`]/[`crate::geom::Display`]/etc., even for values M1a's CSS scope
//! does not implement — see that function's docs), and [`crate::box_tree::build`] is total
//! over any [`cl_style::StyledDocument`], including one built from an empty [`cl_dom::Document`]
//! or one whose styles were hand-assembled into an inconsistent shape — see that function's
//! docs for the defensive fallbacks it takes instead of failing. `LayoutError` is kept as a
//! real, `#[non_exhaustive]` type anyway, mirroring `cl_html::HtmlError` and (before Task 13)
//! `cl_style::StyleError`, so its identity in `cl-layout`'s public API is stable for whenever
//! a future task (a resource limit worth aborting on, rather than recovering from) gives it
//! its first inhabited variant.

/// Errors produced by `cl-layout`.
///
/// Uninhabited today — see the module documentation above for why.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LayoutError {}
