//! Test fixtures shared by the testshell's integration tests.

/// One flat repeating unit — never nested — so N copies make a *wide* document, not a deep
/// one; a deep one would test the DOM/box/fragment tree's iterative-walk guarantees (already
/// covered by their own crates' fuzz targets) rather than this crate's raw throughput.
pub const UNIT: &str = "<div><p>lorem ipsum dolor sit amet consectetur adipiscing elit</p></div>\n";

/// Builds a `<!doctype html><body>` document of repeated [`UNIT`]s at least `1_000_000` bytes
/// long — the M1a smoke page.
pub fn generate_1mb_html() -> String {
    let mut html = String::from("<!doctype html><body>\n");
    while html.len() < 1_000_000 {
        html.push_str(UNIT);
    }
    html
}
