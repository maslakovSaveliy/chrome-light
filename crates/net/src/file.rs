//! The `file://` loader: reads a local file into memory, capped so a malicious or misbehaving
//! page (or a hostile `file://` link) cannot exhaust memory by pointing at `/dev/zero`, a huge
//! file, or a growing/streaming special file.

use std::io::Read;

use crate::error::NetError;
use crate::url::Url;

/// Default cap on how much of a `file:` resource [`load_file`] will read into memory: 32 MiB.
/// Chosen as a generous bound for the static documents/stylesheets/scripts this crate loads in
/// M1a — large enough for any real page, small enough that a hostile `file://` target cannot
/// turn a single load into an unbounded allocation.
pub const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;

/// Read the contents of a `file:` URL, capped at [`MAX_FILE_BYTES`].
///
/// # Errors
/// - [`NetError::UnsupportedScheme`] if `url`'s scheme is not `file`.
/// - [`NetError::Url`] if `url` is a `file:` URL that does not map to a local path (see
///   [`Url::to_file_path`]).
/// - [`NetError::Io`] if the file cannot be stat'd, opened, or read.
/// - [`NetError::TooLarge`] if the file is bigger than [`MAX_FILE_BYTES`].
pub fn load_file(url: &Url) -> Result<Vec<u8>, NetError> {
    load_file_with_cap(url, MAX_FILE_BYTES)
}

/// Like [`load_file`], but with a caller-supplied cap instead of [`MAX_FILE_BYTES`].
///
/// This is `pub` so tests (in this crate and downstream) can exercise the cap-rejection path
/// without allocating a multi-megabyte fixture; it is not meant to be a general tuning knob in
/// product code, which should go through [`load_file`].
///
/// Enforcement is two-layered, both required:
/// 1. We `stat` first and reject before opening/reading at all if the reported size already
///    exceeds `cap` — the common case, and it avoids doing any read work for an obviously
///    oversized file.
/// 2. We then read through a [`Read::take`] bounded to `cap + 1` bytes and check what we
///    actually got. `stat` and `read` are not atomic: between them a FIFO can be fed more data,
///    a symlink can be swapped, or a concurrent writer can extend the file. Trusting the `stat`
///    alone would let such a file grow past `cap` while we read it, defeating the cap. Reading
///    at most `cap + 1` bytes bounds our worst-case allocation to `cap + 1` regardless of how
///    large the file actually is, and getting more than `cap` bytes back is itself the signal
///    that the file exceeded the cap.
///
/// # Errors
/// Same as [`load_file`], against `cap` instead of [`MAX_FILE_BYTES`].
pub fn load_file_with_cap(url: &Url, cap: u64) -> Result<Vec<u8>, NetError> {
    if url.scheme() != "file" {
        return Err(NetError::UnsupportedScheme(url.scheme().to_owned()));
    }
    let path = url.to_file_path().ok_or_else(|| {
        NetError::Url(format!(
            "file url does not map to a local path (host?): {url}"
        ))
    })?;

    // Layer 1: fail fast on the common case without opening the file.
    let metadata = std::fs::metadata(&path)?;
    if metadata.len() > cap {
        return Err(NetError::TooLarge {
            size: metadata.len(),
            max: cap,
        });
    }

    // Layer 2: bound the actual read to `cap + 1` bytes so a file that grows between the stat
    // above and this read (FIFO, symlink swap, concurrent writer) cannot make us allocate more
    // than `cap + 1` bytes. If we read back more than `cap` bytes, the file exceeded the cap by
    // the time we finished reading it, regardless of what `stat` reported earlier.
    let file = std::fs::File::open(&path)?;
    let mut buf = Vec::new();
    file.take(cap.saturating_add(1)).read_to_end(&mut buf)?;
    let read_len = buf.len() as u64;
    if read_len > cap {
        return Err(NetError::TooLarge {
            size: read_len,
            max: cap,
        });
    }
    Ok(buf)
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    /// A unique path under the system temp dir, following the convention in
    /// `crates/testshell/tests/cli.rs`: pid plus a per-test tag to avoid collisions between
    /// concurrently-running tests.
    fn temp_path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("cl-net-{}-{tag}", std::process::id()))
    }

    #[test]
    fn load_file_should_reject_http_scheme() {
        let url = Url::parse("http://example.com/a").expect("parses");
        let err = load_file(&url).expect_err("http must be rejected");
        assert!(matches!(err, NetError::UnsupportedScheme(scheme) if scheme == "http"));
    }

    #[test]
    fn load_file_should_reject_files_over_cap() {
        let path = temp_path("over-cap.txt");
        std::fs::write(&path, b"this is more than sixteen bytes long").expect("write fixture");
        let url = Url::from_file_path(&path).expect("path converts");

        let result = load_file_with_cap(&url, 16);

        std::fs::remove_file(&path).ok();
        let err = result.expect_err("oversized file must be rejected");
        assert!(matches!(err, NetError::TooLarge { max: 16, .. }));
    }

    #[test]
    fn load_file_should_read_bytes() {
        let path = temp_path("read-bytes.txt");
        let contents = b"hello, cl-net";
        std::fs::write(&path, contents).expect("write fixture");
        let url = Url::from_file_path(&path).expect("path converts");

        let result = load_file(&url);

        std::fs::remove_file(&path).ok();
        assert_eq!(result.expect("read succeeds"), contents.to_vec());
    }

    #[test]
    fn load_file_should_reject_file_url_with_host() {
        let url = Url::parse("file://example.com/a").expect("parses");
        let err = load_file(&url).expect_err("must be rejected: no local path");
        assert!(matches!(err, NetError::Url(_)));
    }
}
