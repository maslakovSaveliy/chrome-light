//! `ChromeLight` network process (M1a subset): a validated [`Url`] newtype over the `url` crate
//! and a size-capped `file://` loader. This crate grows into the full network process in
//! M1d/M2 (HTTP, caching, cookies); its public shape here is load-bearing for later tasks.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod error;
pub mod file;
pub mod url;

pub use error::NetError;
pub use file::{MAX_FILE_BYTES, load_file, load_file_with_cap};
pub use url::Url;
