use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use crate::languages::{FileModuleRules, ResolutionFamily};
use crate::model::{
    BindingEvidence, Definition, Export, FileAnalysis, FileFacts, Import, LocalBinding, ModulePath,
    Reference, SemanticStore, SymbolId,
};
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use super::{CallEdge, ExecutionGraph, ExecutionRelation, ProgramFunctionId};

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
            .contexts
            .iter()
            .flat_map(|store| &store.definitions)
            .filter(|item| self.allows(item.context_id))
    }

    pub(crate) fn imports(&self) -> impl Iterator<Item = &'a Import> + '_ {
        self.canonical
            .contexts
            .iter()
            .flat_map(|store| &store.imports)
            .filter(|item| self.allows(item.context_id))
    }

    pub(crate) fn exports(&self) -> impl Iterator<Item = &'a Export> + '_ {
        self.canonical
            .contexts
            .iter()
            .flat_map(|store| &store.exports)
            .filter(|item| self.allows(item.context_id))
    }

    pub(crate) fn references(&self) -> impl Iterator<Item = &'a Reference> + '_ {
        self.canonical
            .contexts
            .iter()
            .flat_map(|store| &store.references)
            .filter(|item| self.allows(item.context_id))
    }

    pub(crate) fn locals(&self) -> impl Iterator<Item = &'a LocalBinding> + '_ {
        self.canonical
            .contexts
            .iter()
            .flat_map(|store| &store.locals)
            .filter(|item| {
                item.evidence != BindingEvidence::Semantic && self.allows(item.context_id)
            })
    }

    pub(crate) fn semantic(&self) -> impl Iterator<Item = &'a SemanticStore> + Clone + '_ {
        self.canonical
            .context_order
            .iter()
            .map(|index| &self.canonical.contexts[*index])
            .filter(|item| self.allows(item.semantic.context_id))
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

#[derive(Clone, Debug)]
pub(crate) struct ProgramSymbol<'a> {
    pub id: ProgramSymbolId,
    pub semantic_id: SymbolId,
    pub definition: &'a Definition,
    pub scope: ProgramScopeId,
}

impl std::ops::Deref for ProgramSymbol<'_> {
    type Target = Definition;

    fn deref(&self) -> &Self::Target {
        self.definition
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ProgramScope {
    pub id: ProgramScopeId,
    pub parent: Option<ProgramScopeId>,
}

#[derive(Clone, Debug)]
pub(crate) struct ProgramBinding<'a> {
    local: &'a LocalBinding,
    pub scope: ProgramScopeId,
    pub type_name: Option<&'a str>,
    pub alias_target: Option<&'a str>,
    pub callable_reference: bool,
    pub immutable: bool,
    semantic_evidence: bool,
    conflicting_evidence: bool,
}

impl<'a> ProgramBinding<'a> {
    fn new(file_index: usize, local: &'a LocalBinding) -> Self {
        Self {
            local,
            scope: ProgramScopeId::new(
                file_index,
                local.context_id,
                local.scope_start..local.scope_end,
            ),
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
    indexes: Arc<ProgramIndexes<'a>>,
}

pub(crate) struct ProgramIndexes<'a> {
    symbols: Vec<ProgramSymbol<'a>>,
    symbols_by_file: Vec<Vec<usize>>,
    symbols_by_name: Vec<HashMap<String, Vec<usize>>>,
    symbol_by_semantic_id: HashMap<SymbolId, Vec<ProgramSymbolId>>,
    callable_scopes: Vec<HashSet<ProgramScopeId>>,
    reported_scopes: Vec<BTreeSet<(usize, usize, usize)>>,
    scopes: Vec<ProgramScope>,
    scopes_by_file: Vec<Vec<usize>>,
    scope_by_id: HashMap<ProgramScopeId, usize>,
    bindings: Vec<ProgramBinding<'a>>,
    bindings_by_file_name: Vec<HashMap<&'a str, Vec<usize>>>,
    aliases: BTreeMap<PathBuf, BTreeSet<usize>>,
}

impl<'a> Deref for ProgramGraph<'a> {
    type Target = ProgramIndexes<'a>;
    fn deref(&self) -> &Self::Target {
        &self.indexes
    }
}

impl DerefMut for ProgramGraph<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        Arc::get_mut(&mut self.indexes).expect("only canonical graph construction mutates indexes")
    }
}
