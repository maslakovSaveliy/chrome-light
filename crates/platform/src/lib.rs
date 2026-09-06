//! `ChromeLight` OS abstraction layer. The only crate (besides `cl-process` and
//! `cl-gfx` backends) allowed to contain platform-specific code. M0 contains no `unsafe`.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod process_type;

pub use process_type::{ProcessType, UnknownProcessType};
