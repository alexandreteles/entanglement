//! Program graphs built from semantic facts emitted by language adapters.

mod facts;
mod program;

pub(crate) use facts::{
    CallArgumentFact, CallConvention, CallDispatch, CallFacts, CallableParameter,
    CallableSignature, CallableTypeParameter, DeclarationExecutionFacts, DeclarationFacts,
    DeclarationKind, DeclarationModifiers, PhpDeclaration, PhpFacts, PhpImport, PhpInclude,
    PhpNamespace, PhpReference, PhpReferenceKind, PhpSymbolKind, PhpTraitAdaptation, PhpTraitUse,
    ReceiverScope, SemanticContext, SemanticFacts, SemanticVisibility,
};
pub(crate) use program::{FileFactsView, ProgramGraph, ProgramSymbol, ProgramSymbolId};

/// One proven execution relation between reported callable scopes.
///
/// Edges point from caller to callee. Call-site ranges preserve repeated
/// invocations; exact duplicate edges are removed. Self-edges are retained.
/// Cross-context edges require the same resolution and dispatch proof as
/// same-context edges.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct CallEdge {
    pub relation: ExecutionRelation,
    pub caller_file: usize,
    pub caller_start_byte: usize,
    pub callee_file: usize,
    pub callee_start_byte: usize,
    pub call_start_byte: usize,
    pub call_end_byte: usize,
}

/// The reason that one executable body can directly enter another.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) enum ExecutionRelation {
    Invocation,
    Initialization,
}

/// A reported function in one immutable analysis snapshot.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct ProgramFunctionId {
    pub file_index: usize,
    pub function_index: usize,
}

/// The shared execution graph, including call-site evidence on each edge.
///
/// Petgraph provides node access and forward/reverse adjacency. Parallel edges
/// preserve distinct call sites and relation kinds. Unresolved, ambiguous, and
/// virtual references remain in file analyses and do not enter this graph.
/// A node uses the first reported function for each file and start byte.
/// Duplicate edges have equal relation kinds, endpoints, and source ranges.
pub(crate) type ExecutionGraph = petgraph::graph::DiGraph<ProgramFunctionId, CallEdge>;
