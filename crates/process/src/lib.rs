//! `ChromeLight` process layer: spawning same-binary children, sandbox type-state, crash handling.
//! Allowed to hold platform-specific code and (later) `unsafe` FFI to OS sandbox APIs; M0 has none.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod error;
pub mod sandbox;

pub use error::ProcessError;
pub use sandbox::{Applied, NotImplementedPolicy, Sandbox, SandboxError, SandboxPolicy, Unapplied};
