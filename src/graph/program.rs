use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use crate::languages::{FileModuleRules, ResolutionFamily};
use crate::model::{
    Definition, DefinitionKind, Export, FileAnalysis, FileFacts, Import, LocalBinding, ModulePath,
    Reference, SymbolId,
};
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

mod bindings;
mod execution;
mod indexing;
mod lookup;

/// A strategy's borrowed view of one canonical file. Metadata is shared; only
/// layer eligibility and effective resolution policy belong to the view.
pub(crate) struct FileFactsView<'a> {
    canonical: &'a FileFacts,
    contexts: Option<HashSet<usize>>,
    pub resolution_family: ResolutionFamily,
    pub file_module_rules: Option<&'static FileModuleRules>,
    pub target: &'a PathBuf,
    pub aliases: &'a [PathBuf],
    pub module: &'a ModulePath,
    pub source: &'a Arc<[u8]>,
    pub analysis: &'a FileAnalysis,
}

impl<'a> FileFactsView<'a> {
    pub(crate) fn new(fact: &'a FileFacts) -> Self {
        Self::with_contexts(fact, None, fact.resolution_family, fact.file_module_rules)
    }

    pub(crate) fn projected(
        fact: &'a FileFacts,
        contexts: HashSet<usize>,
        family: ResolutionFamily,
        rules: Option<&'static FileModuleRules>,
    ) -> Self {
        let present = !contexts.is_empty();
        Self::with_contexts(
            fact,
            Some(contexts),
            if present {
                family
            } else {
                ResolutionFamily::None
            },
            if present { rules } else { None },
        )
    }

    fn with_contexts(
        fact: &'a FileFacts,
        contexts: Option<HashSet<usize>>,
        resolution_family: ResolutionFamily,
        file_module_rules: Option<&'static FileModuleRules>,
    ) -> Self {
        Self {
            canonical: fact,
            contexts,
            resolution_family,
            file_module_rules,
            target: &fact.target,
            aliases: &fact.aliases,
            module: &fact.module,
            source: &fact.source,
            analysis: &fact.analysis,
        }
    }

    pub(crate) fn allows(&self, context_id: usize) -> bool {
        self.contexts
            .as_ref()
            .is_none_or(|ids| ids.contains(&context_id))
    }

    pub(crate) fn definitions(&self) -> impl Iterator<Item = &'a Definition> + '_ {
        self.canonical
            .definitions
            .iter()
            .filter(|item| self.allows(item.context_id))
    }

    pub(crate) fn imports(&self) -> impl Iterator<Item = &'a Import> + '_ {
        self.canonical
            .imports
            .iter()
            .filter(|item| self.allows(item.context_id))
    }

    pub(crate) fn exports(&self) -> impl Iterator<Item = &'a Export> + '_ {
        self.canonical
            .exports
            .iter()
            .filter(|item| self.allows(item.context_id))
    }

    pub(crate) fn references(&self) -> impl Iterator<Item = &'a Reference> + '_ {
        self.canonical
            .references
            .iter()
            .filter(|item| self.allows(item.context_id))
    }

    pub(crate) fn locals(&self) -> impl Iterator<Item = &'a LocalBinding> + '_ {
        self.canonical
            .locals
            .iter()
            .filter(|item| self.allows(item.context_id))
    }

    pub(crate) fn semantic(&self) -> impl Iterator<Item = &'a super::SemanticFacts> + '_ {
        self.canonical
            .semantic
            .iter()
            .filter(|item| self.allows(item.context_id))
    }
}

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
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
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
    facts: Vec<FileFactsView<'a>>,
    indexes: Arc<ProgramIndexes>,
}

pub(crate) struct ProgramIndexes {
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

impl Deref for ProgramGraph<'_> {
    type Target = ProgramIndexes;
    fn deref(&self) -> &Self::Target {
        &self.indexes
    }
}

impl DerefMut for ProgramGraph<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        Arc::get_mut(&mut self.indexes).expect("only canonical graph construction mutates indexes")
    }
}
