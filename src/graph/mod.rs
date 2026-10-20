//! Shared syntax and program graph primitives.
//!
//! Syntax adapters keep grammar-specific decisions locally and use these
//! node identities when captures need later lookup. The normalized program graph is
//! built from semantic facts after language adapters have emitted them.

mod facts;
mod program;
mod syntax;

pub(crate) use facts::{
    BindingType, CallArgumentFact, CallConvention, CallDispatch, CallFacts, CallableParameter,
    CallableSignature, CallableTypeParameter, DeclarationExecutionFacts, DeclarationFacts,
    DeclarationKind, DeclarationModifiers, PhpDeclaration, PhpFacts, PhpImport, PhpInclude,
    PhpNamespace, PhpReference, PhpReferenceKind, PhpSymbolKind, PhpTraitAdaptation, PhpTraitUse,
    ReceiverScope, SemanticContext, SemanticFacts, SemanticVisibility,
};
pub(crate) use program::{
    ExecutionGraph, FileFactsView, ProgramGraph, ProgramSymbol, ProgramSymbolId,
};
pub(crate) use syntax::SyntaxGraph;
