pub mod cyclomatic;
pub mod cyclomatic_density;
pub mod nloc;

use std::ops::Range;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SyntaxRole {
    /// A syntax node used to find concrete source rows.
    Node,
    /// A function boundary.
    Function,
    /// A control-flow decision.
    Condition,
    /// A short-circuit Boolean decision.
    LogicalCondition,
    /// A decision with multiple cases.
    Multiway,
    /// One case in a multiway decision.
    Case,
    /// A comment range.
    Comment,
    /// A name definition.
    Definition,
    /// A name reference.
    Reference,
    /// An import declaration.
    Import,
    /// The content range for an injected language.
    InjectionContent,
    /// A language label for an injected range.
    InjectionLanguage,
}

/// One syntax fact with a role and source position.
#[derive(Debug, Clone, Copy)]
pub struct SyntaxEvent {
    /// The role that the language query gives this node.
    pub role: SyntaxRole,
    /// The node's start byte in the source.
    pub start_byte: usize,
    /// The node's exclusive end byte in the source.
    pub end_byte: usize,
    /// The zero-based row for the node start.
    pub start_row: usize,
    /// The zero-based row for the node end.
    pub end_row: usize,
    /// The byte column for the node end.
    pub end_column: usize,
    /// The node identity used by this syntax tree.
    pub node_id: usize,
    /// The parent node identity, when the node has a parent.
    pub parent_id: Option<usize>,
    /// True when the node has no children.
    pub terminal: bool,
}

impl SyntaxEvent {
    /// Return the half-open source byte range for this fact.
    pub fn range(self) -> Range<usize> {
        self.start_byte..self.end_byte
    }
}
