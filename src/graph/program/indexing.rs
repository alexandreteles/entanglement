use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use crate::model::{Definition, DefinitionKind, FileFacts, SymbolId};

use super::lookup::binding_identity;
use super::{
    ProgramBinding, ProgramGraph, ProgramScope, ProgramScopeId, ProgramSymbol, ProgramSymbolId,
};

impl<'a> ProgramGraph<'a> {
    pub(crate) fn new(facts: &'a [FileFacts]) -> Self {
        let mut graph = Self::empty(facts);
        let mut scope_ids = HashSet::new();
        for (file_index, fact) in facts.iter().enumerate() {
            graph.index_file(file_index, fact, &mut scope_ids);
        }
        graph.link_scope_parents();
        graph
    }

    fn empty(facts: &'a [FileFacts]) -> Self {
        Self {
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
            aliases: BTreeMap::<PathBuf, std::collections::BTreeSet<usize>>::new(),
        }
    }

    fn index_file(
        &mut self,
        file_index: usize,
        fact: &FileFacts,
        scope_ids: &mut HashSet<ProgramScopeId>,
    ) {
        for alias in &fact.aliases {
            self.aliases
                .entry(alias.clone())
                .or_default()
                .insert(file_index);
        }
        self.index_definitions(file_index, fact, scope_ids);
        self.index_declared_scopes(file_index, fact, scope_ids);
        self.index_bindings(file_index, fact, scope_ids);
        self.index_callable_scopes(file_index, scope_ids);
    }
}
impl<'a> ProgramGraph<'a> {
    pub(super) fn index_definitions(
        &mut self,
        file_index: usize,
        fact: &FileFacts,
        scope_ids: &mut HashSet<ProgramScopeId>,
    ) {
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
            self.add_scope(scope, scope_ids);
            let semantic_id = semantic_symbol_id(fact, definition);
            self.symbol_by_semantic_id
                .entry(semantic_id.clone())
                .or_default()
                .push(id);
            let symbol_index = self.symbols.len();
            self.symbols.push(ProgramSymbol {
                id,
                semantic_id,
                name: definition.name.clone(),
                kind: definition.kind.clone(),
                scope,
                is_public: definition.is_public,
            });
            self.symbols_by_file[file_index].push(symbol_index);
            self.symbols_by_name[file_index]
                .entry(definition.name.clone())
                .or_default()
                .push(symbol_index);
        }
    }
    pub(super) fn index_callable_scopes(
        &mut self,
        file_index: usize,
        scope_ids: &mut HashSet<ProgramScopeId>,
    ) {
        let callable_ranges = self
            .symbols_for_file(file_index)
            .filter(|symbol| {
                matches!(
                    symbol.kind,
                    DefinitionKind::Function | DefinitionKind::Method
                )
            })
            .filter_map(|symbol| {
                self.definition(symbol.id).map(|definition| {
                    (
                        symbol.scope.context_id,
                        symbol.semantic_id.start_byte,
                        definition.end_byte,
                    )
                })
            })
            .collect::<Vec<_>>();
        for (context_id, start_byte, end_byte) in callable_ranges {
            self.function_starts[file_index].insert((context_id, start_byte));
            let scope = ProgramScopeId {
                file_index,
                context_id,
                start_byte,
                end_byte,
            };
            self.callable_scopes[file_index].insert(scope);
            self.add_scope(scope, scope_ids);
        }
    }
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
impl<'a> ProgramGraph<'a> {
    pub(super) fn index_declared_scopes(
        &mut self,
        file_index: usize,
        fact: &FileFacts,
        scope_ids: &mut HashSet<ProgramScopeId>,
    ) {
        let ranges = fact
            .imports
            .iter()
            .map(|item| (item.context_id, item.scope_start, item.scope_end))
            .chain(
                fact.exports
                    .iter()
                    .map(|item| (item.context_id, item.scope_start, item.scope_end)),
            )
            .chain(
                fact.references
                    .iter()
                    .map(|item| (item.context_id, item.scope_start, item.scope_end)),
            );
        for (context_id, start, end) in ranges {
            self.add_scope(
                ProgramScopeId::new(file_index, context_id, start..end),
                scope_ids,
            );
        }
    }

    pub(super) fn add_scope(&mut self, id: ProgramScopeId, seen: &mut HashSet<ProgramScopeId>) {
        if seen.insert(id) {
            let index = self.scopes.len();
            self.scopes.push(ProgramScope { id, parent: None });
            self.scopes_by_file[id.file_index].push(index);
            self.scope_by_id.insert(id, index);
        }
    }
    pub(super) fn link_scope_parents(&mut self) {
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
}
impl<'a> ProgramGraph<'a> {
    fn index_bindings(
        &mut self,
        file_index: usize,
        fact: &FileFacts,
        scope_ids: &mut HashSet<ProgramScopeId>,
    ) {
        let locals = fact.locals.iter().map(|local| {
            ProgramBinding::new(
                ProgramScopeId::new(
                    file_index,
                    local.context_id,
                    local.scope_start..local.scope_end,
                ),
                local.name.clone(),
                local.start_byte..local.end_byte,
            )
        });
        for binding in locals {
            self.index_binding(binding, scope_ids);
        }
        for semantic in &fact.semantic {
            for binding in &semantic.bindings {
                self.index_binding(
                    ProgramBinding {
                        type_name: binding.type_name.clone(),
                        alias_target: binding.alias_target.clone(),
                        callable_reference: binding.callable_reference,
                        immutable: binding.immutable,
                        semantic_evidence: true,
                        ..ProgramBinding::new(
                            ProgramScopeId::new(
                                file_index,
                                semantic.context_id,
                                binding.scope_start..binding.scope_end,
                            ),
                            binding.name.clone(),
                            binding.name_start_byte..binding.name_end_byte,
                        )
                    },
                    scope_ids,
                );
            }
            for receiver in &semantic.receivers {
                self.add_scope(
                    ProgramScopeId::new(
                        file_index,
                        semantic.context_id,
                        receiver.start_byte..receiver.end_byte,
                    ),
                    scope_ids,
                );
            }
        }
    }
    fn index_binding(&mut self, binding: ProgramBinding, scope_ids: &mut HashSet<ProgramScopeId>) {
        let scope = binding.scope;
        self.add_binding(binding);
        self.add_scope(scope, scope_ids);
    }

    fn add_binding(&mut self, binding: ProgramBinding) {
        let semantic_evidence = binding.semantic_evidence;
        let existing = self.bindings_by_file_name[binding.scope.file_index]
            .get(&binding.name)
            .into_iter()
            .flatten()
            .copied()
            .find(|index| binding_identity(&self.bindings[*index]) == binding_identity(&binding));
        if let Some(index) = existing {
            let existing = &mut self.bindings[index];
            if semantic_evidence && !existing.semantic_evidence {
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
        let index = self.bindings.len();
        self.bindings_by_file_name[binding.scope.file_index]
            .entry(binding.name.clone())
            .or_default()
            .push(index);
        self.bindings.push(binding);
    }
}

fn conflicting_option<T: PartialEq>(left: &Option<T>, right: &Option<T>) -> bool {
    matches!((left, right), (Some(left), Some(right)) if left != right)
}
