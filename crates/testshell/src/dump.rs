//! Stage selection for the `dump` CLI command: which of [`crate::pipeline::Stages`]' fields
//! to print, and parsing the `--stage` flag's value into one.

use crate::ShellError;
use crate::pipeline::Stages;

/// One pipeline stage `dump --stage` can print, in pipeline order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// `dom` — [`cl_dom::serialize::dom_dump`] of the parsed document.
    Dom,
    /// `style` — [`cl_style::dump::computed_style_dump`] of the resolved cascade.
    Style,
    /// `box-tree` — [`cl_layout::dump::box_tree_dump`] of the box tree.
    BoxTree,
    /// `fragments` — [`cl_layout::dump::fragment_tree_dump`] of the laid-out fragment tree.
    Fragments,
    /// `display-list` — [`cl_paint::dump::display_list_dump`] of the display list.
    DisplayList,
}

impl Stage {
    /// Parses a `--stage` value: `dom`, `style`, `box-tree`, `fragments`, or `display-list`.
    ///
    /// # Errors
    /// [`ShellError::InvalidStage`] for anything else.
    pub fn parse(s: &str) -> Result<Self, ShellError> {
        match s {
            "dom" => Ok(Stage::Dom),
            "style" => Ok(Stage::Style),
            "box-tree" => Ok(Stage::BoxTree),
            "fragments" => Ok(Stage::Fragments),
            "display-list" => Ok(Stage::DisplayList),
            other => Err(ShellError::InvalidStage(other.to_owned())),
        }
    }
}

/// Selects `stage`'s dump text out of `stages`.
#[must_use]
pub fn select(stages: &Stages, stage: Stage) -> &str {
    match stage {
        Stage::Dom => &stages.dom,
        Stage::Style => &stages.style,
        Stage::BoxTree => &stages.box_tree,
        Stage::Fragments => &stages.fragments,
        Stage::DisplayList => &stages.display_list,
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn stages() -> Stages {
        Stages {
            dom: "dom-text".to_owned(),
            style: "style-text".to_owned(),
            box_tree: "box-tree-text".to_owned(),
            fragments: "fragments-text".to_owned(),
            display_list: "display-list-text".to_owned(),
        }
    }

    #[test]
    fn parse_should_accept_every_documented_stage_name() {
        assert_eq!(Stage::parse("dom").expect("ok"), Stage::Dom);
        assert_eq!(Stage::parse("style").expect("ok"), Stage::Style);
        assert_eq!(Stage::parse("box-tree").expect("ok"), Stage::BoxTree);
        assert_eq!(Stage::parse("fragments").expect("ok"), Stage::Fragments);
        assert_eq!(
            Stage::parse("display-list").expect("ok"),
            Stage::DisplayList
        );
    }

    #[test]
    fn parse_should_reject_unknown_stage_names() {
        assert!(matches!(
            Stage::parse("boxtree"),
            Err(ShellError::InvalidStage(_))
        ));
    }

    #[test]
    fn select_should_pick_the_matching_field() {
        let stages = stages();
        assert_eq!(select(&stages, Stage::Dom), "dom-text");
        assert_eq!(select(&stages, Stage::Style), "style-text");
        assert_eq!(select(&stages, Stage::BoxTree), "box-tree-text");
        assert_eq!(select(&stages, Stage::Fragments), "fragments-text");
        assert_eq!(select(&stages, Stage::DisplayList), "display-list-text");
    }
}
