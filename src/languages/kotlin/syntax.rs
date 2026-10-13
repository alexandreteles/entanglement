use std::ops::Range;

mod context;
mod events;
mod lexer;
mod parameters;
mod recovery;
mod tokens;

#[cfg(test)]
pub(super) use context::context_parameters;
pub(super) use context::{
    context_for_declaration, context_parameters_at, context_start, mask_context_parameters,
};
pub(super) use events::{append_kotlin_decision_events, remove_else_events_for_else_if};
pub(super) use recovery::{recovered_type_for, recovered_types};
pub(super) use tokens::{append_context_terminals, context_terminal_ranges};

#[derive(Clone, Debug)]
pub(super) struct ContextParameters {
    /// The `context(...)` prefix, including its keyword and closing delimiter.
    pub range: Range<usize>,
    /// First non-trivia byte after the prefix, usually a modifier or annotation.
    pub declaration_start: usize,
    /// The declaration keyword after any recognized modifiers.
    pub declaration_keyword: usize,
    pub parameters: Vec<ContextParameter>,
}

#[derive(Clone, Debug)]
pub(super) struct ContextParameter {
    pub name: Option<String>,
    pub name_range: Option<Range<usize>>,
    pub type_name: String,
    pub type_range: Range<usize>,
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
