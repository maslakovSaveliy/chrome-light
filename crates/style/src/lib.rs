//! `ChromeLight` style system: the stylo cascade over an author stylesheet set.
//!
//! M1a task 2 is a build spike only — no DOM is wired up yet ([`StyleEngine`] just proves a
//! `Stylist` accepts stylesheets). Task 11 adds the `cl-dom` adapter (`store.rs`, `handle.rs`,
//! `stylo_dom.rs`) and is where `unsafe` is introduced for this crate, per ADR-0015.
#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(clippy::undocumented_unsafe_blocks)]
#![deny(missing_docs)]

pub mod engine;
pub mod error;

pub use engine::StyleEngine;
pub use error::StyleError;
