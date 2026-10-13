use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use crate::model::{DefinitionKind, FileFacts, SymbolId};

mod execution;
mod indexing;
mod lookup;

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

impl ProgramScopeId {
    fn new(file_index: usize, context_id: usize, range: std::ops::Range<usize>) -> Self {
        Self {
            file_index,
            context_id,
            start_byte: range.start,
            end_byte: range.end,
        }
    }

    fn width(self) -> usize {
        self.end_byte.saturating_sub(self.start_byte)
    }
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

impl ProgramBinding {
    fn new(scope: ProgramScopeId, name: String, range: std::ops::Range<usize>) -> Self {
        Self {
            name,
            start_byte: range.start,
            end_byte: range.end,
            scope,
            type_name: None,
            alias_target: None,
            callable_reference: false,
            immutable: false,
            semantic_evidence: false,
            conflicting_evidence: false,
        }
    }
}

/// An indexed view of all normalized program facts across analyzed files.
///
/// This owns common symbol, scope and binding indexes while borrowing the
/// canonical language facts. Resolver families keep their lookup rules, but
/// share symbol identities, scope selection, path aliases, and call-edge validation.
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
}
