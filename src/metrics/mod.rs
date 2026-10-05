pub mod cognitive;
pub mod cyclomatic;
pub mod cyclomatic_density;
pub mod halstead;
pub mod nloc;
pub(crate) mod recursion;

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
    /// A structural `if` decision for cognitive complexity.
    CognitiveIf,
    /// An expression scanned before its owning conditional increases nesting.
    CognitiveConditionBoundary,
    /// An `else` branch for cognitive complexity.
    CognitiveElse,
    /// An `else if` chain boundary, which suppresses the enclosing `if` depth.
    CognitiveElseIf,
    /// A loop for cognitive complexity.
    CognitiveLoop,
    /// A Rust `let ... else` conditional.
    CognitiveLetElse,
    /// A `match` expression for cognitive complexity.
    CognitiveMultiway,
    /// A binary expression joining logical operators.
    CognitiveLogicalExpression,
    /// A logical AND operator.
    CognitiveLogicalAnd,
    /// A logical OR operator.
    CognitiveLogicalOr,
    /// Parentheses, which are transparent when flattening logical groups.
    CognitiveParentheses,
    /// A closure boundary that increases nesting for its contents.
    CognitiveClosure,
    /// A labeled `break` or `continue` jump.
    CognitiveLabeledJump,
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
