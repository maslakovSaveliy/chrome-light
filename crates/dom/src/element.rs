//! Element and attribute data embedded in [`crate::NodeKind::Element`].

use markup5ever::QualName;

use crate::StrTendril;
use crate::node::NodeId;

/// A single attribute on an [`Element`].
#[derive(Debug, Clone)]
pub struct Attr {
    /// The attribute's qualified name (namespace, optional prefix, and local name).
    pub name: QualName,
    /// The attribute's value.
    pub value: StrTendril,
}

/// An element's tag name, attributes, and (for `<template>`) its contents subtree.
#[derive(Debug, Clone)]
pub struct Element {
    /// The element's qualified tag name.
    pub name: QualName,
    /// The element's attributes, in source (parse) order.
    pub attrs: Vec<Attr>,
    /// For a `<template>` element, the id of the root [`crate::NodeKind::DocumentFragment`]
    /// holding its (inert, not part of the main tree) contents. `None` for every other
    /// element.
    pub template_contents: Option<NodeId>,
}
