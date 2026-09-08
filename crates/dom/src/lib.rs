//! `ChromeLight` DOM: an arena-backed [`Document`] of [`Node`]s addressed by [`NodeId`].
//!
//! No `Rc`/`RefCell` here — see `docs/CODING_STANDARDS.md` §1. Tree edges are `NodeId`
//! indices into a flat `Vec<Node>` owned by `Document`; every tree algorithm in this
//! crate walks them iteratively (no recursion), since a hostile or merely very deep
//! document can nest arbitrarily far, and a recursive walk would be a stack-overflow
//! `DoS` on that input.
//!
//! `document`, `element`, `node` and `error` are implemented here (Task 3 of the M1a
//! plan). DOM-order traversal and HTML re-serialization are separate concerns added by
//! Task 4, once `cl-html`'s tree builder exists to exercise them against.
//!
//! `QualName`, `LocalName`, `Namespace` (plus the `ns!`/`local_name!`/`namespace_url!`
//! atom macros) and `StrTendril` are re-exported here from `markup5ever` so every
//! downstream crate (`cl-html`, `cl-style`, `cl-layout`, ...) names the same types rather
//! than each pulling its own, possibly version-skewed, copies.
#![forbid(unsafe_code)]
#![deny(missing_docs)]

pub mod document;
pub mod element;
pub mod error;
pub mod node;

pub use document::{Document, QuirksMode};
pub use element::{Attr, Element};
pub use error::DomError;
pub use node::{Doctype, Node, NodeId, NodeKind};

/// A `Tendril` specialised for UTF-8 text: the string type used throughout the DOM for
/// text nodes, comments, attribute values and processing-instruction data.
///
/// Re-exported via `markup5ever`'s own `pub use tendril;` (rather than a direct `tendril`
/// dependency of this crate) so it is always exactly the type `markup5ever` itself uses —
/// see the "Design decisions" section of the Task 3 report for why a direct dependency
/// would silently diverge in version.
pub use markup5ever::tendril::StrTendril;
pub use markup5ever::{LocalName, Namespace, QualName, local_name, namespace_url, ns};
