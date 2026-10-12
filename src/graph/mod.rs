//! Shared syntax and program graph primitives.
//!
//! Syntax adapters keep grammar-specific decisions locally and use these
//! indexes for common tree relationships. The normalized program graph is
//! built from semantic facts after language adapters have emitted them.

mod facts;
mod program;
mod syntax;

pub(crate) use facts::{
    BindingType, CallArgumentFact, CallConvention, CallDispatch, CallFacts, CallableParameter,
    CallableSignature, DeclarationExecutionFacts, DeclarationFacts, DeclarationKind,
    DeclarationModifiers, ReceiverScope, SemanticContext, SemanticFacts, SemanticVisibility,
};
pub(crate) use program::{CallEdge, ProgramGraph, ProgramSymbolId};
pub(crate) use syntax::SyntaxGraph;
