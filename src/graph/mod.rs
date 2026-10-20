//! Program graphs built from semantic facts emitted by language adapters.

mod facts;
mod program;

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
