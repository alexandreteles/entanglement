use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use crate::languages::ResolutionFamily;
use crate::model::{
    Definition, DefinitionKind, ExecutionTarget, FileFacts, Import, Reference, ReferenceAnalysis,
    ReferenceKind, Resolution, SourceRange, SymbolId,
};

use super::{CallDispatch, CallFacts, DeclarationExecutionFacts, DeclarationFacts, SemanticFacts};

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

impl<'a> ProgramGraph<'a> {
    /// Build shared indexes once from the language-neutral file facts.
    pub(crate) fn new(facts: &'a [FileFacts]) -> Self {
        let mut graph = Self {
            facts,
            symbols: Vec::new(),
            symbols_by_file: vec![Vec::new(); facts.len()],
            symbols_by_name: (0..facts.len()).map(|_| HashMap::new()).collect(),
            symbol_by_semantic_id: HashMap::new(),
            function_starts: (0..facts.len()).map(|_| HashSet::new()).collect(),
            callable_scopes: (0..facts.len()).map(|_| HashSet::new()).collect(),
            scopes: Vec::new(),
            scopes_by_file: vec![Vec::new(); facts.len()],
            scope_by_id: HashMap::new(),
            bindings: Vec::new(),
            bindings_by_file_name: (0..facts.len()).map(|_| HashMap::new()).collect(),
            aliases: BTreeMap::new(),
            relations: Vec::new(),
        };

        let mut scope_ids = HashSet::new();
        for (file_index, fact) in facts.iter().enumerate() {
            for alias in &fact.aliases {
                graph
                    .aliases
                    .entry(alias.clone())
                    .or_default()
                    .insert(file_index);
            }
            for (definition_index, definition) in fact.definitions.iter().enumerate() {
                let id = ProgramSymbolId {
                    file_index,
                    definition_index,
                };
                let scope = ProgramScopeId {
                    file_index,
                    context_id: definition.context_id,
                    start_byte: definition.scope_start,
                    end_byte: definition.scope_end,
                };
                graph.add_scope(scope, &mut scope_ids);
                let semantic_id = semantic_symbol_id(fact, definition);
                graph
                    .symbol_by_semantic_id
                    .entry(semantic_id.clone())
                    .or_default()
                    .push(id);
                let symbol_index = graph.symbols.len();
                graph.symbols.push(ProgramSymbol {
                    id,
                    semantic_id,
                    name: definition.name.clone(),
                    kind: definition.kind.clone(),
                    scope,
                    is_public: definition.is_public,
                });
                graph.symbols_by_file[file_index].push(symbol_index);
                graph.symbols_by_name[file_index]
                    .entry(definition.name.clone())
                    .or_default()
                    .push(symbol_index);
            }
            for import in &fact.imports {
                graph.add_scope(
                    ProgramScopeId {
                        file_index,
                        context_id: import.context_id,
                        start_byte: import.scope_start,
                        end_byte: import.scope_end,
                    },
                    &mut scope_ids,
                );
            }
            for export in &fact.exports {
                graph.add_scope(
                    ProgramScopeId {
                        file_index,
                        context_id: export.context_id,
                        start_byte: export.scope_start,
                        end_byte: export.scope_end,
                    },
                    &mut scope_ids,
                );
            }
            for reference in &fact.references {
                graph.add_scope(
                    ProgramScopeId {
                        file_index,
                        context_id: reference.context_id,
                        start_byte: reference.scope_start,
                        end_byte: reference.scope_end,
                    },
                    &mut scope_ids,
                );
            }
            for local in &fact.locals {
                graph.add_binding(
                    ProgramBinding {
                        file_index,
                        context_id: local.context_id,
                        name: local.name.clone(),
                        start_byte: local.start_byte,
                        end_byte: local.end_byte,
                        scope: ProgramScopeId {
                            file_index,
                            context_id: local.context_id,
                            start_byte: local.scope_start,
                            end_byte: local.scope_end,
                        },
                        type_name: None,
                        alias_target: None,
                        callable_reference: false,
                        immutable: false,
                        semantic_evidence: false,
                        conflicting_evidence: false,
                    },
                    false,
                );
                graph.add_scope(
                    ProgramScopeId {
                        file_index,
                        context_id: local.context_id,
                        start_byte: local.scope_start,
                        end_byte: local.scope_end,
                    },
                    &mut scope_ids,
                );
            }
            for semantic in &fact.semantic {
                for binding in &semantic.bindings {
                    graph.add_binding(
                        ProgramBinding {
                            file_index,
                            context_id: semantic.context_id,
                            name: binding.name.clone(),
                            start_byte: binding.name_start_byte,
                            end_byte: binding.name_end_byte,
                            scope: ProgramScopeId {
                                file_index,
                                context_id: semantic.context_id,
                                start_byte: binding.scope_start,
                                end_byte: binding.scope_end,
                            },
                            type_name: binding.type_name.clone(),
                            alias_target: binding.alias_target.clone(),
                            callable_reference: binding.callable_reference,
                            immutable: binding.immutable,
                            semantic_evidence: false,
                            conflicting_evidence: false,
                        },
                        true,
                    );
                    graph.add_scope(
                        ProgramScopeId {
                            file_index,
                            context_id: semantic.context_id,
                            start_byte: binding.scope_start,
                            end_byte: binding.scope_end,
                        },
                        &mut scope_ids,
                    );
                }
                for receiver in &semantic.receivers {
                    graph.add_scope(
                        ProgramScopeId {
                            file_index,
                            context_id: semantic.context_id,
                            start_byte: receiver.start_byte,
                            end_byte: receiver.end_byte,
                        },
                        &mut scope_ids,
                    );
                }
            }
            let callable_ranges = graph
                .symbols_for_file(file_index)
                .filter(|symbol| {
                    matches!(
                        symbol.kind,
                        DefinitionKind::Function | DefinitionKind::Method
                    )
                })
                .filter_map(|symbol| {
                    graph.definition(symbol.id).map(|definition| {
                        (
                            symbol.scope.context_id,
                            symbol.semantic_id.start_byte,
                            definition.end_byte,
                        )
                    })
                })
                .collect::<Vec<_>>();
            for (context_id, start_byte, end_byte) in callable_ranges {
                graph.function_starts[file_index].insert((context_id, start_byte));
                let scope = ProgramScopeId {
                    file_index,
                    context_id,
                    start_byte,
                    end_byte,
                };
                graph.callable_scopes[file_index].insert(scope);
                graph.add_scope(scope, &mut scope_ids);
            }
        }
        graph.link_scope_parents();
        graph
    }

    fn add_scope(&mut self, id: ProgramScopeId, seen: &mut HashSet<ProgramScopeId>) {
        if seen.insert(id) {
            let index = self.scopes.len();
            self.scopes.push(ProgramScope { id, parent: None });
            self.scopes_by_file[id.file_index].push(index);
            self.scope_by_id.insert(id, index);
        }
    }

    fn add_binding(&mut self, mut binding: ProgramBinding, semantic_evidence: bool) {
        let existing = self.bindings_by_file_name[binding.file_index]
            .get(&binding.name)
            .into_iter()
            .flatten()
            .copied()
            .find(|index| binding_identity(&self.bindings[*index]) == binding_identity(&binding));
        if let Some(index) = existing {
            let existing = &mut self.bindings[index];
            if semantic_evidence && !existing.semantic_evidence {
                binding.semantic_evidence = true;
                *existing = binding;
            } else if semantic_evidence && existing.semantic_evidence {
                existing.conflicting_evidence |=
                    conflicting_option(&existing.type_name, &binding.type_name)
                        || conflicting_option(&existing.alias_target, &binding.alias_target)
                        || existing.callable_reference != binding.callable_reference
                        || existing.immutable != binding.immutable;
                existing.type_name = existing.type_name.clone().or(binding.type_name);
                existing.alias_target = existing.alias_target.clone().or(binding.alias_target);
                existing.callable_reference |= binding.callable_reference;
                existing.immutable &= binding.immutable;
            }
            return;
        }
        binding.semantic_evidence = semantic_evidence;
        let index = self.bindings.len();
        self.bindings_by_file_name[binding.file_index]
            .entry(binding.name.clone())
            .or_default()
            .push(index);
        self.bindings.push(binding);
    }

    fn link_scope_parents(&mut self) {
        for file_scopes in &self.scopes_by_file {
            let mut ordered = file_scopes.clone();
            ordered.sort_by_key(|index| {
                let scope = self.scopes[*index].id;
                (scope.context_id, scope.start_byte, Reverse(scope.end_byte))
            });
            let mut ancestors = Vec::<usize>::new();
            let mut context_id = None;
            for child_index in ordered {
                let child = self.scopes[child_index].id;
                if context_id != Some(child.context_id) {
                    ancestors.clear();
                    context_id = Some(child.context_id);
                }
                while let Some(parent_index) = ancestors.last().copied() {
                    let parent = self.scopes[parent_index].id;
                    if parent.start_byte <= child.start_byte && child.end_byte <= parent.end_byte {
                        break;
                    }
                    ancestors.pop();
                }
                self.scopes[child_index].parent = ancestors
                    .last()
                    .map(|parent_index| self.scopes[*parent_index].id);
                ancestors.push(child_index);
            }
        }
    }

    pub(crate) fn file_count(&self) -> usize {
        self.facts.len()
    }

    pub(crate) fn file(&self, file_index: usize) -> &'a FileFacts {
        &self.facts[file_index]
    }

    pub(crate) fn files(&self) -> &'a [FileFacts] {
        self.facts
    }

    pub(crate) fn aliases(&self) -> &BTreeMap<PathBuf, BTreeSet<usize>> {
        &self.aliases
    }

    pub(crate) fn symbols_for_file(
        &self,
        file_index: usize,
    ) -> impl Iterator<Item = &ProgramSymbol> {
        self.symbols_by_file[file_index]
            .iter()
            .map(|index| &self.symbols[*index])
    }

    pub(crate) fn symbols_named(
        &self,
        file_index: usize,
        name: &str,
    ) -> impl Iterator<Item = &ProgramSymbol> {
        self.symbols_by_name[file_index]
            .get(name)
            .into_iter()
            .flatten()
            .map(|index| &self.symbols[*index])
    }

    pub(crate) fn symbol(&self, id: ProgramSymbolId) -> Option<&ProgramSymbol> {
        let definition_index = self
            .symbols_by_file
            .get(id.file_index)?
            .get(id.definition_index)?;
        self.symbols.get(*definition_index)
    }

    pub(crate) fn symbol_id(&self, id: ProgramSymbolId) -> Option<&SymbolId> {
        Some(&self.symbol(id)?.semantic_id)
    }

    pub(crate) fn definition(&self, id: ProgramSymbolId) -> Option<&'a Definition> {
        self.facts
            .get(id.file_index)?
            .definitions
            .get(id.definition_index)
    }

    pub(crate) fn definitions(&self, file_index: usize) -> &'a [Definition] {
        &self.facts[file_index].definitions
    }

    pub(crate) fn imports(&self, file_index: usize) -> &'a [Import] {
        &self.facts[file_index].imports
    }

    pub(crate) fn bindings_named(
        &self,
        file_index: usize,
        name: &str,
        context_id: usize,
        position: usize,
    ) -> impl Iterator<Item = &ProgramBinding> {
        self.bindings_by_file_name[file_index]
            .get(name)
            .into_iter()
            .flatten()
            .map(|index| &self.bindings[*index])
            .filter(move |binding| {
                binding.context_id == context_id
                    && binding.start_byte <= position
                    && binding.scope.start_byte <= position
                    && position < binding.scope.end_byte
            })
    }

    /// Return the unique nearest binding for a name at a source position.
    /// A narrower lexical scope wins; declarations in that same scope use
    /// the latest declaration before the position. Equal-ranked alternatives
    /// are ambiguous and do not provide evidence.
    fn binding_at(
        &self,
        file_index: usize,
        name: &str,
        context_id: usize,
        position: usize,
    ) -> Option<&ProgramBinding> {
        let bindings = self
            .bindings_named(file_index, name, context_id, position)
            .collect::<Vec<_>>();
        let narrowest = bindings
            .iter()
            .map(|binding| {
                binding
                    .scope
                    .end_byte
                    .saturating_sub(binding.scope.start_byte)
            })
            .min()?;
        let nearest = bindings
            .into_iter()
            .filter(|binding| {
                binding
                    .scope
                    .end_byte
                    .saturating_sub(binding.scope.start_byte)
                    == narrowest
            })
            .collect::<Vec<_>>();
        let scope = nearest.first()?.scope;
        if nearest.iter().any(|binding| {
            binding.scope.start_byte != scope.start_byte || binding.scope.end_byte != scope.end_byte
        }) {
            return None;
        }
        let latest = nearest.iter().map(|binding| binding.start_byte).max()?;
        let mut winners = nearest
            .into_iter()
            .filter(|binding| binding.start_byte == latest);
        let winner = winners.next()?;
        (winners.next().is_none() && !winner.conflicting_evidence).then_some(winner)
    }

    /// Resolve a binding's declared type, following only bounded immutable
    /// aliases that are visible at each alias declaration in the same
    /// semantic context.
    pub(crate) fn binding_type(
        &self,
        file_index: usize,
        name: &str,
        context_id: usize,
        position: usize,
    ) -> Option<String> {
        let mut binding = self.binding_at(file_index, name, context_id, position)?;
        let mut seen = HashSet::new();
        loop {
            if let Some(type_name) = &binding.type_name {
                return Some(type_name.clone());
            }
            if !binding.immutable || !seen.insert(binding_identity(binding)) {
                return None;
            }
            let target = binding.alias_target.as_deref()?;
            if !is_simple_binding_name(target) {
                return None;
            }
            binding = self.binding_at(file_index, target, context_id, binding.end_byte)?;
            if seen.len() > self.bindings.len() {
                return None;
            }
        }
    }

    /// Follow a simple immutable alias chain and return its terminal source
    /// path. The terminal name is left for the owning resolver to check
    /// against its imports, package, and visibility rules.
    pub(crate) fn binding_alias(
        &self,
        file_index: usize,
        name: &str,
        context_id: usize,
        position: usize,
    ) -> Option<String> {
        let mut binding = self.binding_at(file_index, name, context_id, position)?;
        let mut seen = HashSet::new();
        loop {
            if !binding.immutable || !seen.insert(binding_identity(binding)) {
                return None;
            }
            let target = binding.alias_target.as_deref()?;
            if !is_simple_binding_name(target) {
                let first = target.split('.').next()?;
                if self.has_local_binding(file_index, first, context_id, binding.end_byte) {
                    return None;
                }
                return Some(target.to_owned());
            }
            let Some(next) = self.binding_at(file_index, target, context_id, binding.end_byte)
            else {
                return (!self.has_local_binding(file_index, target, context_id, binding.end_byte))
                    .then(|| target.to_owned());
            };
            if !next.immutable {
                return None;
            }
            next.alias_target.as_ref()?;
            binding = next;
            if seen.len() > self.bindings.len() {
                return None;
            }
        }
    }

    /// Return the terminal path only when an immutable alias chain begins at
    /// an explicit callable-reference value such as `::handler`.
    pub(crate) fn binding_callable_alias(
        &self,
        file_index: usize,
        name: &str,
        context_id: usize,
        position: usize,
    ) -> Option<String> {
        let mut binding = self.binding_at(file_index, name, context_id, position)?;
        let mut seen = HashSet::new();
        let mut contains_callable_reference = false;
        loop {
            if !binding.immutable || !seen.insert(binding_identity(binding)) {
                return None;
            }
            contains_callable_reference |= binding.callable_reference;
            let target = binding.alias_target.as_deref()?;
            if !is_simple_binding_name(target) {
                let first = target.split('.').next()?;
                if self.has_local_binding(file_index, first, context_id, binding.end_byte) {
                    return None;
                }
                return contains_callable_reference.then(|| target.to_owned());
            }
            let Some(next) = self.binding_at(file_index, target, context_id, binding.end_byte)
            else {
                if self.has_local_binding(file_index, target, context_id, binding.end_byte) {
                    return None;
                }
                return contains_callable_reference.then(|| target.to_owned());
            };
            if !next.immutable || next.alias_target.is_none() {
                return None;
            }
            binding = next;
            if seen.len() > self.bindings.len() {
                return None;
            }
        }
    }

    pub(crate) fn has_local_binding(
        &self,
        file_index: usize,
        name: &str,
        context_id: usize,
        position: usize,
    ) -> bool {
        self.bindings_named(file_index, name, context_id, position)
            .next()
            .is_some()
    }

    pub(crate) fn scopes(&self, file_index: usize) -> impl Iterator<Item = ProgramScope> + '_ {
        self.scopes_by_file[file_index]
            .iter()
            .map(|index| self.scopes[*index])
    }

    pub(crate) fn scope_at(
        &self,
        file_index: usize,
        context_id: usize,
        position: usize,
    ) -> Option<ProgramScopeId> {
        self.scopes(file_index)
            .map(|scope| scope.id)
            .filter(|scope| {
                scope.context_id == context_id
                    && scope.start_byte <= position
                    && position < scope.end_byte
            })
            .min_by_key(|scope| scope.end_byte - scope.start_byte)
    }

    fn scope(&self, id: ProgramScopeId) -> Option<ProgramScope> {
        self.scope_by_id.get(&id).map(|index| self.scopes[*index])
    }

    fn within_callable_scope(
        &self,
        file_index: usize,
        context_id: usize,
        position: usize,
        start_byte: usize,
        end_byte: usize,
    ) -> bool {
        let Some(mut current) = self.scope_at(file_index, context_id, position) else {
            return false;
        };
        loop {
            let Some(scope) = self.scope(current) else {
                return false;
            };
            if current.start_byte < position
                && current.start_byte <= start_byte
                && end_byte <= current.end_byte
                && self.callable_scopes[file_index].contains(&current)
            {
                return true;
            }
            let Some(parent) = scope.parent else {
                return false;
            };
            current = parent;
        }
    }

    pub(crate) fn is_local_definition(&self, file_index: usize, definition: &Definition) -> bool {
        self.within_callable_scope(
            file_index,
            definition.context_id,
            definition.start_byte,
            definition.scope_start,
            definition.scope_end,
        )
    }

    pub(crate) fn is_local_scope(
        &self,
        file_index: usize,
        context_id: usize,
        scope_start: usize,
        scope_end: usize,
        position: usize,
    ) -> bool {
        self.within_callable_scope(file_index, context_id, position, scope_start, scope_end)
    }

    pub(crate) fn semantic(
        &self,
        file_index: usize,
        context_id: usize,
        family: Option<ResolutionFamily>,
    ) -> Option<&'a SemanticFacts> {
        self.file(file_index).semantic.iter().find(|semantic| {
            semantic.context_id == context_id
                && family.is_none_or(|family| semantic.resolution_family == Some(family))
        })
    }

    pub(crate) fn declaration(
        &self,
        file_index: usize,
        context_id: usize,
        start_byte: usize,
        family: Option<ResolutionFamily>,
    ) -> Option<&'a DeclarationFacts> {
        self.semantic(file_index, context_id, family)?
            .declarations
            .iter()
            .find(|item| item.start_byte == start_byte)
    }

    pub(crate) fn call_facts(
        &self,
        file_index: usize,
        context_id: usize,
        family: Option<ResolutionFamily>,
    ) -> &'a [CallFacts] {
        self.semantic(file_index, context_id, family)
            .map_or(&[], |semantic| semantic.calls.as_slice())
    }

    pub(crate) fn confirmed_relations(
        &mut self,
        resolutions: &[Vec<ReferenceAnalysis>],
    ) -> Vec<CallEdge> {
        self.relations.clear();
        let mut calls = Vec::new();
        for (file_index, fact) in self.facts.iter().enumerate() {
            let Some(analyses) = resolutions.get(file_index) else {
                continue;
            };
            for (reference, analysis) in fact.references.iter().zip(analyses) {
                if analysis.start_byte != reference.start_byte
                    || analysis.end_byte != reference.end_byte
                    || analysis.path != reference.path
                {
                    continue;
                }
                let Resolution::Exact(target) = &analysis.resolution else {
                    continue;
                };
                let Some(symbol) = self.unique_symbol_for_target(target) else {
                    continue;
                };
                if analysis.dispatch == Some(CallDispatch::Direct) {
                    for execution in &analysis.execution_targets {
                        if self.valid_execution_target(symbol, execution)
                            && self.is_reported_execution_caller(
                                file_index,
                                reference,
                                execution.caller_start_byte,
                            )
                        {
                            calls.push(CallEdge {
                                caller_file: file_index,
                                caller_start_byte: execution.caller_start_byte,
                                callee_file: execution.file_index,
                                callee_start_byte: execution.range.start_byte,
                                call_start_byte: reference.start_byte,
                                call_end_byte: reference.end_byte,
                            });
                        }
                    }
                }
                let is_call = matches!(
                    reference.kind,
                    ReferenceKind::Call
                        | ReferenceKind::Method
                        | ReferenceKind::Qualified
                        | ReferenceKind::Value
                );
                let Some(caller_start_byte) = reference.call_owner else {
                    self.relations.push(ProgramRelation {
                        source_file: file_index,
                        source_start_byte: reference.start_byte,
                        source_end_byte: reference.end_byte,
                        caller_start_byte: None,
                        target: symbol,
                        kind: ProgramRelationKind::Reference,
                    });
                    continue;
                };
                let Some(symbol_info) = self.symbol(symbol) else {
                    continue;
                };
                let callee_file = symbol.file_index;
                let callee_start_byte = symbol_info.semantic_id.start_byte;
                let callee_context = symbol_info.scope.context_id;
                let dispatch_is_confirmed = match &symbol_info.kind {
                    DefinitionKind::Function => {
                        matches!(analysis.dispatch, None | Some(CallDispatch::Direct))
                    }
                    DefinitionKind::Method => analysis.dispatch == Some(CallDispatch::Direct),
                    _ => false,
                };
                let confirmed_call = is_call
                    && dispatch_is_confirmed
                    && self.function_starts[callee_file]
                        .contains(&(callee_context, callee_start_byte))
                    && self.is_reported_caller(file_index, reference, caller_start_byte);
                self.relations.push(ProgramRelation {
                    source_file: file_index,
                    source_start_byte: reference.start_byte,
                    source_end_byte: reference.end_byte,
                    caller_start_byte: confirmed_call.then_some(caller_start_byte),
                    target: symbol,
                    kind: if confirmed_call {
                        ProgramRelationKind::Call
                    } else {
                        ProgramRelationKind::Reference
                    },
                });
            }
        }
        calls.extend(self.relations.iter().filter_map(|relation| {
            if relation.kind != ProgramRelationKind::Call {
                return None;
            }
            let caller_start_byte = relation.caller_start_byte?;
            let callee = self.symbol(relation.target)?;
            Some(CallEdge {
                caller_file: relation.source_file,
                caller_start_byte,
                callee_file: callee.id.file_index,
                callee_start_byte: callee.semantic_id.start_byte,
                call_start_byte: relation.source_start_byte,
                call_end_byte: relation.source_end_byte,
            })
        }));
        calls.extend(self.implicit_initialization_edges());
        calls.sort_by_key(|edge| {
            (
                edge.caller_file,
                edge.caller_start_byte,
                edge.callee_file,
                edge.callee_start_byte,
                edge.call_start_byte,
                edge.call_end_byte,
            )
        });
        calls.dedup();
        calls
    }

    fn is_reported_callable(
        &self,
        file_index: usize,
        context_id: usize,
        start_byte: usize,
    ) -> bool {
        self.facts[file_index]
            .analysis
            .functions
            .iter()
            .filter(|function| function.start_byte == start_byte)
            .any(|function| {
                let declared_function = self.function_starts[file_index]
                    .contains(&(context_id, start_byte))
                    && self
                        .symbols_for_file(file_index)
                        .filter(|symbol| {
                            symbol.scope.context_id == context_id
                                && symbol.semantic_id.start_byte == start_byte
                                && matches!(
                                    symbol.kind,
                                    DefinitionKind::Function | DefinitionKind::Method
                                )
                        })
                        .filter_map(|symbol| self.definition(symbol.id))
                        .any(|definition| definition.end_byte == function.end_byte);
                let declared_execution = self.facts[file_index]
                    .semantic
                    .iter()
                    .filter(|semantic| semantic.context_id == context_id)
                    .flat_map(|semantic| semantic.declarations.iter())
                    .filter_map(|declaration| declaration.execution.as_ref())
                    .flat_map(execution_ranges)
                    .any(|range| {
                        range.start_byte == function.start_byte
                            && range.end_byte == function.end_byte
                    });
                declared_function || declared_execution
            })
    }

    fn is_reported_caller(
        &self,
        file_index: usize,
        reference: &Reference,
        caller_start_byte: usize,
    ) -> bool {
        reference.call_owner == Some(caller_start_byte)
            && self.is_reported_scope_owner(file_index, reference, caller_start_byte)
    }

    fn is_reported_execution_caller(
        &self,
        file_index: usize,
        reference: &Reference,
        caller_start_byte: usize,
    ) -> bool {
        reference.execution_owner.or(reference.call_owner) == Some(caller_start_byte)
            && self.is_reported_scope_owner(file_index, reference, caller_start_byte)
    }

    fn is_reported_scope_owner(
        &self,
        file_index: usize,
        reference: &Reference,
        caller_start_byte: usize,
    ) -> bool {
        if reference.scope_start > reference.start_byte || reference.end_byte > reference.scope_end
        {
            return false;
        }
        let nearest = self.facts[file_index]
            .analysis
            .functions
            .iter()
            .filter(|function| {
                function.start_byte <= reference.start_byte
                    && reference.end_byte <= function.end_byte
                    && self.is_reported_callable(
                        file_index,
                        reference.context_id,
                        function.start_byte,
                    )
            })
            .min_by_key(|function| function.end_byte.saturating_sub(function.start_byte));
        nearest.is_some_and(|function| function.start_byte == caller_start_byte)
    }

    fn valid_execution_target(
        &self,
        resolved_symbol: ProgramSymbolId,
        target: &ExecutionTarget,
    ) -> bool {
        let Some(symbol) = self.symbol(resolved_symbol) else {
            return false;
        };
        if target.file_index != symbol.id.file_index || target.context_id != symbol.scope.context_id
        {
            return false;
        }
        let Some(definition) = self.definition(resolved_symbol) else {
            return false;
        };
        let Some(declaration) = self.declaration(
            symbol.id.file_index,
            symbol.scope.context_id,
            definition.start_byte,
            None,
        ) else {
            return false;
        };
        let Some(execution) = declaration.execution.as_ref() else {
            return false;
        };
        let range_is_declared = execution_ranges(execution).any(|range| range == target.range);
        range_is_declared
            && self.facts[target.file_index]
                .analysis
                .functions
                .iter()
                .any(|function| {
                    function.start_byte == target.range.start_byte
                        && function.end_byte == target.range.end_byte
                })
    }

    fn implicit_initialization_edges(&self) -> Vec<CallEdge> {
        let mut edges = Vec::new();
        for (file_index, fact) in self.facts.iter().enumerate() {
            for semantic in &fact.semantic {
                for declaration in &semantic.declarations {
                    if !matches!(
                        declaration.kind,
                        super::DeclarationKind::Class
                            | super::DeclarationKind::Object
                            | super::DeclarationKind::CompanionObject
                    ) {
                        continue;
                    }
                    let Some(execution) = declaration.execution.as_ref() else {
                        continue;
                    };
                    for constructor in &execution.constructors {
                        if !self.is_reported_execution_range(
                            file_index,
                            semantic.context_id,
                            *constructor,
                        ) {
                            continue;
                        }
                        for initializer in &execution.initializers {
                            if self.is_reported_execution_range(
                                file_index,
                                semantic.context_id,
                                *initializer,
                            ) {
                                edges.push(CallEdge {
                                    caller_file: file_index,
                                    caller_start_byte: constructor.start_byte,
                                    callee_file: file_index,
                                    callee_start_byte: initializer.start_byte,
                                    call_start_byte: constructor.start_byte,
                                    call_end_byte: constructor.end_byte,
                                });
                            }
                        }
                    }
                }
            }
        }
        edges
    }

    fn is_reported_execution_range(
        &self,
        file_index: usize,
        context_id: usize,
        range: crate::model::SourceRange,
    ) -> bool {
        self.is_reported_callable(file_index, context_id, range.start_byte)
            && self.facts[file_index]
                .analysis
                .functions
                .iter()
                .any(|function| {
                    function.start_byte == range.start_byte && function.end_byte == range.end_byte
                })
    }

    fn unique_symbol_for_target(&self, target: &SymbolId) -> Option<ProgramSymbolId> {
        let exact = self.symbol_by_semantic_id.get(target);
        if let Some(exact) = exact {
            // SymbolId has no layer identity. If several semantic layers
            // emitted the same ID, it cannot prove which declaration matched.
            return match exact.as_slice() {
                [symbol] => Some(*symbol),
                _ => None,
            };
        }

        // Some resolver families (notably Rust modules) qualify the target
        // module during resolution. Preserve that family-specific identity
        // while still mapping a unique source declaration back to the graph.
        let mut matches = self
            .symbols
            .iter()
            .filter(|symbol| {
                symbol.semantic_id.file == target.file
                    && symbol.semantic_id.name == target.name
                    && symbol.semantic_id.kind == target.kind
                    && symbol.semantic_id.start_byte == target.start_byte
            })
            .map(|symbol| symbol.id);
        let symbol = matches.next()?;
        matches.next().is_none().then_some(symbol)
    }
}

fn binding_identity(binding: &ProgramBinding) -> (usize, usize, &str, usize, usize, usize, usize) {
    (
        binding.file_index,
        binding.context_id,
        &binding.name,
        binding.start_byte,
        binding.end_byte,
        binding.scope.start_byte,
        binding.scope.end_byte,
    )
}

fn conflicting_option<T: PartialEq>(left: &Option<T>, right: &Option<T>) -> bool {
    matches!((left, right), (Some(left), Some(right)) if left != right)
}

fn execution_ranges(
    execution: &DeclarationExecutionFacts,
) -> impl Iterator<Item = SourceRange> + '_ {
    [execution.callable_value, execution.getter, execution.setter]
        .into_iter()
        .flatten()
        .chain(execution.constructors.iter().copied())
        .chain(execution.initializers.iter().copied())
}

fn is_simple_binding_name(name: &str) -> bool {
    !name.is_empty() && !name.contains('.')
}

fn semantic_symbol_id(fact: &FileFacts, definition: &Definition) -> SymbolId {
    SymbolId {
        file: fact.analysis.path.clone(),
        module: definition.module.clone(),
        name: definition.name.clone(),
        kind: definition.kind.clone(),
        start_byte: definition.start_byte,
    }
}
