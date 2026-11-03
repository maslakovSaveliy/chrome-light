//! The subresource policy (ADR-0016 §6): which files the browser reads for a renderer.
//!
//! The renderer names the URLs, so every one is hostile input. A URL is served only if it is a
//! `file:` URL without a remote host whose canonical path — every symlink resolved — lies under
//! the canonical directory of the top-level document and is a regular file; at most
//! [`MAX_SUBRESOURCES_PER_DOCUMENT`] URLs per document and
//! [`MAX_SUBRESOURCE_BYTES_PER_DOCUMENT`] bytes in total. A refusal is a warning in the page,
//! never an error. The IO layer (Task 10) runs [`load_subresources`] on a worker thread, so a
//! slow disk never blocks the browser.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use cl_ipc::proto::MAX_SUBRESOURCES_PER_DOCUMENT;
use cl_ipc::proto::renderer::SubresourceRefusal;

/// Most bytes of subresources the browser reads for one document.
pub const MAX_SUBRESOURCE_BYTES_PER_DOCUMENT: u64 = 64 * 1024 * 1024;

/// The answer for one requested URL, by its index in the request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedSubresource {
    /// Index into the request's URL list.
    pub index: u32,
    /// The file's bytes, or why it was not served.
    pub result: Result<Vec<u8>, SubresourceRefusal>,
}

/// The file a renderer may read for `url`, given the canonical directory of the top-level
/// document (`doc_dir`, canonicalised by the caller once per document; a directory that is not
/// absolute serves nothing).
///
/// The check runs on the canonical path, so `..` segments and symlinks out of the directory
/// are refused however they are spelled, and the file type is checked before anything opens
/// it: opening a FIFO for reading would block until a writer appeared. (A file swapped between
/// this check and the read is outside the threat model: the renderer cannot write files.)
///
/// # Errors
/// [`SubresourceRefusal::NotFile`] for anything but a `file:` URL with no remote host;
/// [`SubresourceRefusal::Io`] if the path does not exist or cannot be canonicalised;
/// [`SubresourceRefusal::OutsideDocumentDirectory`]; [`SubresourceRefusal::NotRegularFile`].
pub fn resolve_subresource(doc_dir: &Path, url: &str) -> Result<PathBuf, SubresourceRefusal> {
    // Every path starts with `""`, and a relative directory would be resolved against the
    // browser's working directory: neither names a document directory.
    if !doc_dir.is_absolute() {
        return Err(SubresourceRefusal::OutsideDocumentDirectory);
    }
    let url = cl_net::Url::parse(url).map_err(|_| SubresourceRefusal::NotFile)?;
    if url.scheme() != "file" {
        return Err(SubresourceRefusal::NotFile);
    }
    // `to_file_path` refuses a remote host on every OS (a Windows UNC path would be an SMB
    // request).
    let path = url.to_file_path().ok_or(SubresourceRefusal::NotFile)?;
    let canonical = path.canonicalize().map_err(|_| SubresourceRefusal::Io)?;
    if !canonical.starts_with(doc_dir) {
        return Err(SubresourceRefusal::OutsideDocumentDirectory);
    }
    let metadata = std::fs::metadata(&canonical).map_err(|_| SubresourceRefusal::Io)?;
    if !metadata.is_file() {
        return Err(SubresourceRefusal::NotRegularFile);
    }
    Ok(canonical)
}

/// Answers one `NeedSubresources` batch: every URL in order, each resolved with
/// [`resolve_subresource`] and read within what is left of `max_total_bytes` (production:
/// [`MAX_SUBRESOURCE_BYTES_PER_DOCUMENT`]). URLs past [`MAX_SUBRESOURCES_PER_DOCUMENT`] are
/// refused without being looked at. Blocking file IO — run it off the browser's event loop.
pub fn load_subresources(
    doc_dir: &Path,
    urls: &[String],
    max_total_bytes: u64,
) -> Vec<LoadedSubresource> {
    let mut remaining = max_total_bytes;
    urls.iter()
        .enumerate()
        .map(|(i, url)| {
            let result = if i < MAX_SUBRESOURCES_PER_DOCUMENT {
                load_one(doc_dir, url, &mut remaining)
            } else {
                Err(SubresourceRefusal::TooMany)
            };
            LoadedSubresource {
                index: u32::try_from(i).unwrap_or(u32::MAX),
                result,
            }
        })
        .collect()
}

/// Resolves and reads one URL, charging its size to `remaining`.
fn load_one(doc_dir: &Path, url: &str, remaining: &mut u64) -> Result<Vec<u8>, SubresourceRefusal> {
    let path = resolve_subresource(doc_dir, url)?;
    let file = File::open(&path).map_err(|_| SubresourceRefusal::Io)?;
    // The opened file, not the path, decides from here on.
    let metadata = file.metadata().map_err(|_| SubresourceRefusal::Io)?;
    if !metadata.is_file() {
        return Err(SubresourceRefusal::NotRegularFile);
    }
    if metadata.len() > *remaining {
        return Err(SubresourceRefusal::TooLarge);
    }
    // Read at most one byte past the budget, so a file that grew after `metadata` is caught
    // without reading all of it.
    let mut bytes = Vec::new();
    file.take(remaining.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|_| SubresourceRefusal::Io)?;
    let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if len > *remaining {
        return Err(SubresourceRefusal::TooLarge);
    }
    *remaining -= len;
    Ok(bytes)
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "a failed setup step should abort the test, loudly"
)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use cl_ipc::proto::MAX_SUBRESOURCES_PER_DOCUMENT;
    use cl_ipc::proto::renderer::SubresourceRefusal;

    use super::{LoadedSubresource, load_subresources, resolve_subresource};

    /// `<tmp>/<tag>/doc/{page.html, a.css, b.css, sub/c.css}` and `<tmp>/<tag>/secret.css`,
    /// canonical (on macOS the temp dir is behind a `/var` → `/private/var` symlink).
    struct Fixture {
        root: PathBuf,
        doc: PathBuf,
    }

    impl Fixture {
        fn new(tag: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "cl-browser-subresources-{}-{tag}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&root);
            let doc = root.join("doc");
            std::fs::create_dir_all(doc.join("sub")).expect("mkdir");
            for (path, bytes) in [
                (doc.join("page.html"), "<p>x</p>"),
                (doc.join("a.css"), "0123456789"),
                (doc.join("b.css"), "abcdefghij"),
                (doc.join("sub/c.css"), "p{}"),
                (root.join("secret.css"), "secret"),
            ] {
                std::fs::write(path, bytes).expect("write");
            }
            let root = root.canonicalize().expect("canonical root");
            let doc = root.join("doc");
            Fixture { root, doc }
        }
    }

    fn url(path: &Path) -> String {
        cl_net::Url::from_file_path(path)
            .expect("file url")
            .as_str()
            .to_owned()
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    /// Runs `program args…`, returning whether it exists and succeeded — creating a FIFO or a
    /// symlink without platform `cfg` (tools/check-platform-cfg.sh).
    fn run(program: &str, args: &[&Path]) -> bool {
        Command::new(program)
            .args(args)
            .status()
            .is_ok_and(|status| status.success())
    }

    #[test]
    fn a_file_under_the_document_directory_is_served() {
        let fx = Fixture::new("ok");
        let a = fx.doc.join("a.css");
        assert_eq!(resolve_subresource(&fx.doc, &url(&a)), Ok(a));
        let c = fx.doc.join("sub/c.css");
        assert_eq!(resolve_subresource(&fx.doc, &url(&c)), Ok(c));
    }

    #[test]
    fn path_escape_and_fifo_refused() {
        let fx = Fixture::new("escape");
        // `..` out of the directory. The URL parser removes a literal `/../`, but an encoded
        // slash survives it and `to_file_path` decodes it into a real `..` component, so only
        // the canonical path catches this one.
        let dotdot = format!("{}/..%2Fsecret.css", url(&fx.doc));
        assert_eq!(
            resolve_subresource(&fx.doc, &dotdot),
            Err(SubresourceRefusal::OutsideDocumentDirectory)
        );
        let secret = fx.root.join("secret.css");
        assert_eq!(
            resolve_subresource(&fx.doc, &url(&secret)),
            Err(SubresourceRefusal::OutsideDocumentDirectory)
        );
        // A symlink inside the directory that points out of it.
        let link = fx.doc.join("link.css");
        // Only a real symlink counts: Git for Windows' `ln -s` may copy the file instead.
        let made_symlink = run("ln", &[Path::new("-s"), &secret, &link])
            && std::fs::symlink_metadata(&link).is_ok_and(|m| m.file_type().is_symlink());
        if made_symlink {
            assert_eq!(
                resolve_subresource(&fx.doc, &url(&link)),
                Err(SubresourceRefusal::OutsideDocumentDirectory)
            );
        }
        // Not a regular file: a directory, and a FIFO where `mkfifo` exists (opening a FIFO
        // for reading would block the reader until a writer appears).
        let sub = fx.doc.join("sub");
        assert_eq!(
            resolve_subresource(&fx.doc, &url(&sub)),
            Err(SubresourceRefusal::NotRegularFile)
        );
        let fifo = fx.doc.join("fifo.css");
        if run("mkfifo", &[&fifo]) {
            let result = resolve_subresource(&fx.doc, &url(&fifo));
            if std::fs::metadata(&fifo).is_ok_and(|m| !m.is_file() && !m.is_dir()) {
                // A real FIFO (Unix): refused for what it is, before anything opens it.
                assert_eq!(result, Err(SubresourceRefusal::NotRegularFile));
            } else {
                // Git for Windows ships an `mkfifo` whose result `canonicalize` cannot
                // resolve (windows-2022 CI): refused as Io — never served either way.
                assert!(result.is_err(), "{result:?}");
            }
        }
    }

    #[test]
    fn a_document_directory_that_is_not_absolute_serves_nothing() {
        // `Path::new("page.html").parent()` is `""`, and every path starts with `""`.
        let fx = Fixture::new("relative");
        let a = url(&fx.doc.join("a.css"));
        for dir in ["", "doc", "."] {
            assert_eq!(
                resolve_subresource(Path::new(dir), &a),
                Err(SubresourceRefusal::OutsideDocumentDirectory),
                "{dir:?}"
            );
        }
    }

    #[test]
    fn non_file_urls_are_refused() {
        let fx = Fixture::new("scheme");
        for url in [
            "http://example.com/a.css",
            "data:text/css,p{}",
            "file://remote.example/a.css",
            "not a url",
        ] {
            assert_eq!(
                resolve_subresource(&fx.doc, url),
                Err(SubresourceRefusal::NotFile),
                "{url}"
            );
        }
    }

    #[test]
    fn a_missing_file_is_an_io_refusal() {
        let fx = Fixture::new("missing");
        let missing = fx.doc.join("missing.css");
        assert_eq!(
            resolve_subresource(&fx.doc, &url(&missing)),
            Err(SubresourceRefusal::Io)
        );
    }

    #[test]
    fn a_batch_is_answered_per_index_within_the_byte_budget() {
        let fx = Fixture::new("budget");
        let urls = [
            url(&fx.doc.join("a.css")),
            url(&fx.root.join("secret.css")),
            url(&fx.doc.join("b.css")),
            url(&fx.doc.join("sub/c.css")),
        ];
        // 15 bytes: a.css (10) fits, b.css (10) would not, c.css (3) still does.
        let replies = load_subresources(&fx.doc, &urls, 15);
        assert_eq!(
            replies,
            [
                LoadedSubresource {
                    index: 0,
                    result: Ok(b"0123456789".to_vec())
                },
                LoadedSubresource {
                    index: 1,
                    result: Err(SubresourceRefusal::OutsideDocumentDirectory)
                },
                LoadedSubresource {
                    index: 2,
                    result: Err(SubresourceRefusal::TooLarge)
                },
                LoadedSubresource {
                    index: 3,
                    result: Ok(b"p{}".to_vec())
                },
            ]
        );
    }

    #[test]
    fn urls_past_the_count_limit_are_refused() {
        let fx = Fixture::new("count");
        let url = url(&fx.doc.join("sub/c.css"));
        let urls = vec![url; MAX_SUBRESOURCES_PER_DOCUMENT + 2];
        let replies = load_subresources(&fx.doc, &urls, 1024 * 1024);
        assert_eq!(replies.len(), MAX_SUBRESOURCES_PER_DOCUMENT + 2);
        for (i, reply) in replies.iter().enumerate() {
            assert_eq!(reply.index, u32::try_from(i).expect("small"));
            if i < MAX_SUBRESOURCES_PER_DOCUMENT {
                assert_eq!(reply.result, Ok(b"p{}".to_vec()));
            } else {
                assert_eq!(reply.result, Err(SubresourceRefusal::TooMany));
            }
        }
    }
}
