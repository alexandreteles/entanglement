use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use crate::model::{DefinitionKind, FileFacts, SymbolId};

mod bindings;
mod execution;
mod indexing;
mod initialization;
mod relations;
mod scopes;
mod symbols;

/// A stable, graph-local identity for one normalized declaration.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct ProgramSymbolId {
    pub file_index: usize,
    pub definition_index: usize,
}

/// A lexical scope in one file and semantic context.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub(crate) struct ProgramScopeId {
    pub file_index: usize,
    pub context_id: usize,
    pub start_byte: usize,
    pub end_byte: usize,
}

/// A generic source relationship between a reference and a local symbol.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct ProgramRelation {
    pub source_file: usize,
    pub source_start_byte: usize,
    pub source_end_byte: usize,
    pub caller_start_byte: Option<usize>,
    pub target: ProgramSymbolId,
    pub kind: ProgramRelationKind,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ProgramRelationKind {
    Reference,
    Call,
}

/// A resolved direct call between two callable metric scopes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct CallEdge {
    pub caller_file: usize,
    pub caller_start_byte: usize,
    pub callee_file: usize,
    pub callee_start_byte: usize,
    pub call_start_byte: usize,
    pub call_end_byte: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct ProgramSymbol {
    pub id: ProgramSymbolId,
    pub semantic_id: SymbolId,
    pub name: String,
    pub kind: DefinitionKind,
    pub scope: ProgramScopeId,
    pub is_public: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ProgramScope {
    pub id: ProgramScopeId,
    pub parent: Option<ProgramScopeId>,
}

#[derive(Clone, Debug)]
pub(crate) struct ProgramBinding {
    pub file_index: usize,
    pub context_id: usize,
    pub name: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub scope: ProgramScopeId,
    pub type_name: Option<String>,
    pub alias_target: Option<String>,
    pub callable_reference: bool,
    pub immutable: bool,
    semantic_evidence: bool,
    conflicting_evidence: bool,
}

/// An indexed view of all normalized program facts across analyzed files.
///
/// This owns the common indexes and graph relationships while borrowing the
/// canonical language facts. Resolver families keep their lookup rules, but
/// share symbol identities, scope selection, path aliases, and relationships.
pub(crate) struct ProgramGraph<'a> {
    facts: &'a [FileFacts],
    symbols: Vec<ProgramSymbol>,
    symbols_by_file: Vec<Vec<usize>>,
    symbols_by_name: Vec<HashMap<String, Vec<usize>>>,
    symbol_by_semantic_id: HashMap<SymbolId, Vec<ProgramSymbolId>>,
    function_starts: Vec<HashSet<(usize, usize)>>,
    callable_scopes: Vec<HashSet<ProgramScopeId>>,
    scopes: Vec<ProgramScope>,
    scopes_by_file: Vec<Vec<usize>>,
    scope_by_id: HashMap<ProgramScopeId, usize>,
    bindings: Vec<ProgramBinding>,
    bindings_by_file_name: Vec<HashMap<String, Vec<usize>>>,
    aliases: BTreeMap<PathBuf, BTreeSet<usize>>,
    relations: Vec<ProgramRelation>,
}
