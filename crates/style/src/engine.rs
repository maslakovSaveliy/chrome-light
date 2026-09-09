//! Owns the stylo `Stylist`, `Device` and the shared lock. Sheet bookkeeping is manual
//! (`stylist.append_stylesheet`) like Blitz, not `DocumentStylesheetSet`.

use style::context::QuirksMode;
use style::device::Device;
use style::device::servo::FontMetricsProvider;
use style::font_metrics::FontMetrics;
use style::media_queries::{MediaList, MediaType};
use style::properties::ComputedValues;
use style::properties::style_structs::Font;
use style::queries::values::PrefersColorScheme;
use style::servo::media_features::PointerCapabilities;
use style::servo_arc::Arc as StyloArc;
use style::shared_lock::SharedRwLock;
use style::stylesheets::{AllowImportRules, DocumentStyleSheet, Origin, Stylesheet, UrlExtraData};
use style::stylist::Stylist;
use style::values::computed::font::{GenericFontFamily, QueryFontMetricsFlags};
use style::values::computed::{CSSPixelLength, Length};

use crate::error::StyleError;

/// A [`FontMetricsProvider`] that always reports zeroed metrics with a fixed x-height
/// (`0.5em`, per the brief). M1a has no font shaping yet — a real provider backed by
/// `cl-fonts` lands with the DOM/layout integration in Task 11 and beyond.
#[derive(Debug)]
struct NullFontMetricsProvider;

impl FontMetricsProvider for NullFontMetricsProvider {
    fn query_font_metrics(
        &self,
        _vertical: bool,
        _font: &Font,
        base_size: CSSPixelLength,
        _flags: QueryFontMetricsFlags,
    ) -> FontMetrics {
        FontMetrics {
            x_height: Some(base_size.scale_by(0.5)),
            ..FontMetrics::default()
        }
    }

    fn base_size_for_generic(&self, _generic: GenericFontFamily) -> Length {
        Length::new(16.0)
    }
}

/// Owns one document's stylo `Stylist`: the `Device` (viewport, media type, font metrics),
/// the `SharedRwLock` protecting every stylesheet reachable from it, and the user-agent
/// and author stylesheets appended so far (tracked manually, mirroring Blitz rather than
/// stylo's `DocumentStylesheetSet`).
pub struct StyleEngine {
    lock: SharedRwLock,
    stylist: Stylist,
    /// `Origin::UserAgent` sheets, in the order they were appended. Populated by
    /// `add_ua_sheet` (`sheets.rs`).
    ua: Vec<DocumentStyleSheet>,
    /// `Origin::Author` sheets, in the order they were appended: author stylesheets from
    /// [`StyleEngine::add_author_sheet`] plus, once collected, every `<style>`/`<link>`
    /// sheet `collect_document_sheets` (`sheets.rs`) finds in a document, in document
    /// order.
    author: Vec<DocumentStyleSheet>,
}

impl StyleEngine {
    /// Construct a new engine for a `viewport` of `(width, height)` CSS pixels at device
    /// pixel ratio `dpr`.
    ///
    /// # Errors
    /// Returns [`StyleError::Device`] if `viewport` or `dpr` are not finite, positive values.
    pub fn new(viewport: (f32, f32), dpr: f32) -> Result<Self, StyleError> {
        let (width, height) = viewport;
        let dimensions_valid =
            width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0;
        let dpr_valid = dpr.is_finite() && dpr > 0.0;
        if !dimensions_valid || !dpr_valid {
            return Err(StyleError::Device);
        }

        let viewport_size = euclid::Size2D::new(width, height);
        let device_size = euclid::Size2D::new(width, height) * dpr;
        let device_pixel_ratio = euclid::Scale::new(dpr);

        let device = Device::new(
            MediaType::screen(),
            QuirksMode::NoQuirks,
            viewport_size,
            device_size,
            device_pixel_ratio,
            Box::new(NullFontMetricsProvider),
            ComputedValues::initial_values_with_font_override(Font::initial_values()),
            PrefersColorScheme::Light,
            PointerCapabilities::default(),
            PointerCapabilities::default(),
        );

        let stylist = Stylist::new(device, QuirksMode::NoQuirks);

        Ok(Self {
            lock: SharedRwLock::new(),
            stylist,
            ua: Vec::new(),
            author: Vec::new(),
        })
    }

    /// Parse `css` as an author-origin stylesheet resolved against base URL `base`, and
    /// append it to the stylist.
    ///
    /// # Errors
    /// Returns [`StyleError::Url`] if `base` cannot be parsed as an absolute URL.
    pub fn add_author_sheet(&mut self, css: &str, base: &str) -> Result<(), StyleError> {
        let sheet = self.build_and_append(css, base, Origin::Author)?;
        self.author.push(sheet);
        Ok(())
    }

    /// Parse `css` as a user-agent-origin stylesheet resolved against base URL `base`, and
    /// append it to the stylist. `pub(crate)`: `sheets.rs`'s public `add_ua_sheet` is the
    /// real entry point (it always passes the bundled `assets/ua.css`); this exists so that
    /// code lives in `sheets.rs` per the task brief while still reaching the private
    /// `stylist`/`lock`/`ua` fields declared here.
    ///
    /// # Errors
    /// Returns [`StyleError::Url`] if `base` cannot be parsed as an absolute URL.
    pub(crate) fn add_ua_sheet_from(&mut self, css: &str, base: &str) -> Result<(), StyleError> {
        let sheet = self.build_and_append(css, base, Origin::UserAgent)?;
        self.ua.push(sheet);
        Ok(())
    }

    /// Parses `css` as a stylesheet of the given `origin` resolved against base URL `base`,
    /// and appends it to the stylist. Shared by [`StyleEngine::add_author_sheet`] and
    /// [`StyleEngine::add_ua_sheet_from`], which differ only in `origin` and which
    /// per-origin list they push the returned handle onto.
    ///
    /// # Errors
    /// Returns [`StyleError::Url`] if `base` cannot be parsed as an absolute URL.
    fn build_and_append(
        &mut self,
        css: &str,
        base: &str,
        origin: Origin,
    ) -> Result<DocumentStyleSheet, StyleError> {
        let base_url = url::Url::parse(base).map_err(|e| StyleError::Url(e.to_string()))?;
        let url_data = UrlExtraData::from(base_url);

        let sheet = Stylesheet::from_str(
            css,
            url_data,
            origin,
            StyloArc::new(self.lock.wrap(MediaList::empty())),
            self.lock.clone(),
            /* stylesheet_loader = */ None,
            /* error_reporter = */ None,
            QuirksMode::NoQuirks,
            AllowImportRules::No,
        );
        let sheet = DocumentStyleSheet(StyloArc::new(sheet));

        self.stylist
            .append_stylesheet(sheet.clone(), &self.lock.read());

        Ok(sheet)
    }

    /// Total number of top-level CSS rules across every author stylesheet appended so far
    /// (via [`StyleEngine::add_author_sheet`] or `sheets.rs`'s `collect_document_sheets`).
    pub fn author_rule_count(&self) -> usize {
        self.rule_counts(&self.author).iter().sum()
    }

    /// Per-sheet top-level CSS rule counts of every author stylesheet, in the order the
    /// sheets were appended.
    ///
    /// Exists so callers (mainly tests) can confirm sheets were appended in a particular
    /// order — e.g. document order, for `collect_document_sheets` — without needing the
    /// cascade itself to observe the effect.
    pub fn author_rule_counts(&self) -> Vec<usize> {
        self.rule_counts(&self.author)
    }

    /// Total number of top-level CSS rules across every user-agent stylesheet appended so
    /// far (in practice, just the one bundled `assets/ua.css`).
    pub fn ua_rule_count(&self) -> usize {
        self.rule_counts(&self.ua).iter().sum()
    }

    /// Per-sheet top-level CSS rule counts of `sheets`, in order.
    fn rule_counts(&self, sheets: &[DocumentStyleSheet]) -> Vec<usize> {
        let guard = self.lock.read();
        sheets
            .iter()
            .map(|sheet| {
                sheet
                    .0
                    .contents
                    .read_with(&guard)
                    .rules
                    .read_with(&guard)
                    .0
                    .len()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[allow(clippy::expect_used)]
    fn stylist_should_accept_one_author_rule() {
        let mut engine = StyleEngine::new((800.0, 600.0), 1.0).expect("engine");
        engine
            .add_author_sheet("p { color: red }", "file:///test.html")
            .expect("sheet");
        assert_eq!(engine.author_rule_count(), 1);
    }

    #[test]
    #[allow(clippy::expect_used)]
    fn add_author_sheet_should_reject_invalid_base_url() {
        let mut engine = StyleEngine::new((800.0, 600.0), 1.0).expect("engine");
        let result = engine.add_author_sheet("p { color: red }", "not a url");
        assert!(matches!(result, Err(StyleError::Url(_))));
    }
}
