use std::ops::Range;

use tree_sitter::Node;

mod context;
pub(super) use context::context_parameters;

mod events;
mod recovery;

pub(super) use events::{append_kotlin_decision_events, remove_else_events_for_else_if};
pub(super) use recovery::{recovered_type_for, recovered_types};

#[derive(Clone, Debug)]
pub(super) struct ContextParameters {
    /// The `context(...)` prefix, including its keyword and closing delimiter.
    pub range: Range<usize>,
    pub parameters: Vec<ContextParameter>,
}

#[derive(Clone, Debug)]
pub(super) struct ContextParameter {
    pub name: Option<String>,
    pub name_range: Option<Range<usize>>,
    pub type_name: String,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(super) enum RecoveredTypeKind {
    Class,
    Interface,
    Object,
    CompanionObject,
}

#[derive(Clone, Debug)]
pub(super) struct RecoveredType {
    pub error_range: Range<usize>,
    pub declaration_range: Range<usize>,
    pub keyword_range: Range<usize>,
    pub name_range: Option<Range<usize>>,
    pub body_range: Range<usize>,
    pub kind: RecoveredTypeKind,
}

pub(in crate::languages::kotlin) fn context_for_declaration<'a>(
    node: Node<'_>,
    contexts: &'a [ContextParameters],
) -> Option<&'a ContextParameters> {
    contexts
        .iter()
        .find(|context| context.range.start == node.start_byte())
}
