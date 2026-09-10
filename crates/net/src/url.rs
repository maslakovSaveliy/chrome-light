//! `Url`: an immutable, validated URL newtype over the `url` crate.
//!
//! We do not re-implement URL parsing: `url` already implements the WHATWG URL Standard.
//! This module exists to (a) give the rest of `ChromeLight` a stable, crate-local type
//! (`cl_net::Url`) instead of leaking a third-party type through every public API, and
//! (b) narrow that crate's surface to exactly what the browser needs.

use std::path::{Path, PathBuf};

use crate::error::NetError;

/// A parsed, absolute URL.
///
/// Construct with [`Url::parse`], [`Url::parse_with_base`], or [`Url::from_file_path`] — there
/// is no way to build one without going through the WHATWG URL parser, so a `Url` in hand is
/// always valid.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Url(url::Url);

impl Url {
    /// Parse `input` as an absolute URL (WHATWG "basic URL parser" with no base).
    ///
    /// # Errors
    /// Returns [`NetError::Url`] if `input` is not a valid absolute URL. Never panics: malformed
    /// input (including attacker-controlled `href` values) is always an `Err`.
    pub fn parse(input: &str) -> Result<Self, NetError> {
        url::Url::parse(input)
            .map(Self)
            .map_err(|e| NetError::Url(e.to_string()))
    }

    /// Parse `input` against `base` — the WHATWG "URL parser with base URL" algorithm. This
    /// lets a relative reference (`../a.css`, `?q=1`, `#frag`) resolve the way an `href` in a
    /// document resolves against that document's own URL.
    ///
    /// # Errors
    /// Returns [`NetError::Url`] if the resolved result is not a valid URL. Never panics.
    pub fn parse_with_base(input: &str, base: &Url) -> Result<Self, NetError> {
        // `Url::options().base_url(..).parse(..)` is the `url` crate's direct implementation of
        // the WHATWG "URL parser with base URL" algorithm (the same algorithm a browser runs
        // when resolving an `href`, `src`, `@import`, etc. against the current document's URL).
        // We call it explicitly here rather than only via `Url::join` below, because this is the
        // primitive: `join` is defined in terms of it, not the other way around, matching how
        // the `url` crate itself defines `Url::join` as `self.options().base_url(Some(self))
        // .parse(input)`.
        url::Url::options()
            .base_url(Some(&base.0))
            .parse(input)
            .map(Self)
            .map_err(|e| NetError::Url(e.to_string()))
    }

    /// Build a `file:` URL from an absolute filesystem path.
    ///
    /// # Errors
    /// Returns [`NetError::Url`] if `p` is not absolute (the `url` crate requires this to
    /// build an unambiguous `file:` URL).
    pub fn from_file_path(p: &Path) -> Result<Self, NetError> {
        url::Url::from_file_path(p)
            .map(Self)
            .map_err(|()| NetError::Url(format!("not a valid absolute file path: {}", p.display())))
    }

    /// The URL's scheme, e.g. `"file"`, `"http"`, `"https"`.
    #[must_use]
    pub fn scheme(&self) -> &str {
        self.0.scheme()
    }

    /// The full serialized URL.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Convert a `file:` URL back into a filesystem path.
    ///
    /// Returns `None` when this is not a `file:` URL, or when it is a `file:` URL that does not
    /// map to a local path on this platform — notably one with a non-empty, non-`localhost`
    /// host (`file://example.com/x`). We treat that as "no path" rather than guessing at UNC or
    /// remote-share semantics; see [`crate::file`] for how callers should report it.
    #[must_use]
    pub fn to_file_path(&self) -> Option<PathBuf> {
        // A `file:` URL with a real host names a path on *another* machine. The `url` crate's
        // answer here is platform-dependent: on Windows `file://server/share/x` maps to the UNC
        // path `\\server\share\x`, while on Unix it is an error. ChromeLight refuses it on every
        // platform, for two reasons: the same document must resolve identically on every machine
        // the engine runs on (determinism is the point of M1a's testing story), and a document
        // must not be able to turn a "local file" load into an SMB fetch that bypasses the
        // network policy `load_file` does not have. `localhost` and the empty host both mean
        // "this machine" per the URL Standard and are accepted.
        match self.0.host_str() {
            None | Some("" | "localhost") => self.0.to_file_path().ok(),
            Some(_) => None,
        }
    }

    /// Resolve `relative` against `self` (WHATWG "URL parser with base URL", base = `self`).
    /// Equivalent to `Url::parse_with_base(relative, self)`; kept as a method because resolving
    /// a link against "the current document's URL" is the common case call sites reach for.
    ///
    /// # Errors
    /// Returns [`NetError::Url`] if the resolved result is not a valid URL. Never panics.
    pub fn join(&self, relative: &str) -> Result<Self, NetError> {
        Self::parse_with_base(relative, self)
    }
}

impl std::fmt::Display for Url {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.0, f)
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn parse_with_base_should_resolve_relative_path() {
        let base = Url::parse("file:///x/y/z.html").expect("base parses");
        let resolved = Url::parse_with_base("../a.css", &base).expect("resolves");
        assert_eq!(resolved.as_str(), "file:///x/a.css");
    }

    #[test]
    fn join_should_keep_query_and_fragment_semantics() {
        let base = Url::parse("http://example.com/a/b?x=1#f").expect("base parses");

        // A relative reference with its own query and fragment replaces both.
        let full = base.join("c?y=2#g").expect("joins");
        assert_eq!(full.as_str(), "http://example.com/a/c?y=2#g");

        // A query-only reference keeps the base's path but replaces the query, drops fragment.
        let query_only = base.join("?y=2").expect("joins");
        assert_eq!(query_only.as_str(), "http://example.com/a/b?y=2");

        // A fragment-only reference keeps the base's path *and* query.
        let frag_only = base.join("#g2").expect("joins");
        assert_eq!(frag_only.as_str(), "http://example.com/a/b?x=1#g2");
    }

    #[test]
    fn from_file_path_round_trip() {
        let original = std::env::temp_dir().join("cl-net-round-trip-does-not-need-to-exist.txt");
        let url = Url::from_file_path(&original).expect("absolute path converts");
        assert_eq!(url.scheme(), "file");
        let back = url.to_file_path().expect("file url maps back to a path");
        assert_eq!(back, original);
    }

    #[test]
    fn to_file_path_should_be_none_for_http() {
        let url = Url::parse("http://example.com/a").expect("parses");
        assert_eq!(url.to_file_path(), None);
    }

    #[test]
    fn to_file_path_should_accept_localhost_and_empty_host() {
        // Built from a real local path so the test is meaningful on every platform: a
        // Unix-shaped literal like `file:///tmp/a` is not a valid path on Windows, where
        // `to_file_path` legitimately requires a drive letter.
        let path = std::env::temp_dir().join("cl-net-host-test");
        let empty = Url::from_file_path(&path).expect("temp dir is absolute");
        assert!(empty.to_file_path().is_some());

        // Same URL with an explicit `localhost` host: per the URL Standard it still means
        // "this machine", so it must resolve to the same path.
        let with_localhost = Url::parse(&empty.as_str().replacen("file://", "file://localhost", 1))
            .expect("still a valid file URL");
        assert_eq!(with_localhost.to_file_path(), empty.to_file_path());
    }

    #[test]
    fn to_file_path_should_be_none_for_file_url_with_host() {
        // file://example.com/a has a real (non-localhost) host: it does not name a path on
        // this machine, so we treat it as "no local path" rather than guessing.
        let url = Url::parse("file://example.com/a").expect("parses");
        assert_eq!(url.to_file_path(), None);
    }

    fn arbitrary_string() -> impl Strategy<Value = String> {
        proptest::collection::vec(any::<char>(), 0..256).prop_map(|cs| cs.into_iter().collect())
    }

    proptest! {
        #[test]
        fn parse_should_reject_garbage_without_panicking(s in arbitrary_string()) {
            let _ = Url::parse(&s);
        }

        #[test]
        fn parse_with_base_should_reject_garbage_without_panicking(s in arbitrary_string()) {
            let base = Url::parse("http://example.com/").expect("fixed base parses");
            let _ = Url::parse_with_base(&s, &base);
        }
    }
}
