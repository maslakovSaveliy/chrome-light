//! [`RenderError`]: why a document could not be parsed or rendered.

/// A failure of [`crate::ParsedDocument::parse`] or [`crate::LoadedDocument::render`].
///
/// A missing or refused subresource is never one of these: it becomes a warning in
/// [`crate::Rendered::warnings`] and the page still renders.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    /// `cl-html` failed to parse the document (uninhabited today — see
    /// [`cl_html::HtmlError`]).
    #[error("html: {0}")]
    Html(#[from] cl_html::HtmlError),
    /// The style engine could not be built for the viewport (zero or non-finite size), a
    /// stylesheet could not be built, or the document's base URL is invalid.
    #[error("style: {0}")]
    Style(#[from] cl_style::StyleError),
    /// Layout failed (font plumbing only — see [`cl_layout::LayoutError`]).
    #[error("layout: {0}")]
    Layout(#[from] cl_layout::LayoutError),
    /// An earlier render failed inside the cascade ([`cl_style::StyleEngine::resolve`],
    /// which takes the document by value), so this [`crate::LoadedDocument`] cannot render
    /// again. Every other failure gives the document back. Not reachable today (`resolve`
    /// never fails), kept so a fallible cascade cannot silently leave an empty document
    /// behind.
    #[error("the document was lost in an earlier failed render")]
    DocumentLost,
}
