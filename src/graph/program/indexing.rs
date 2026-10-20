use std::cmp::Reverse;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use crate::model::{Definition, DefinitionKind, FileFacts, SymbolId};

use super::bindings::binding_identity;
use super::{
    CallEdge, ExecutionGraph, FileFactsView, ProgramBinding, ProgramFunctionId, ProgramGraph,
    ProgramIndexes, ProgramScope, ProgramScopeId, ProgramSymbol, ProgramSymbolId,
};

impl<'a> ProgramGraph<'a> {
    pub(crate) fn new(facts: &'a [FileFacts]) -> Self {
        let mut graph = Self::empty(facts);
        for (file_index, fact) in facts.iter().enumerate() {
            graph.index_file(file_index, fact);
        }
        graph.link_scope_parents();
        graph
    }

    /// Restrict resolver eligibility while reusing all canonical indexes.
    pub(crate) fn project(&self, facts: Vec<FileFactsView<'a>>) -> Self {
        assert_eq!(facts.len(), self.facts.len());
        assert!(
            facts
                .iter()
                .zip(&self.facts)
                .all(|(view, canonical)| std::ptr::eq(view.canonical, canonical.canonical)),
            "projected views must borrow the indexed canonical files in order"
        );
        Self {
            facts,
            indexes: self.indexes.clone(),
        }
    }

    fn empty(facts: &'a [FileFacts]) -> Self {
        Self {
            facts: facts.iter().map(FileFactsView::new).collect(),
            indexes: std::sync::Arc::new(ProgramIndexes {
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
            }),
        }
    }

    fn index_file(&mut self, file_index: usize, fact: &'a FileFacts) {
        for alias in &fact.aliases {
            self.aliases
                .entry(alias.clone())
                .or_default()
                .insert(file_index);
        }
        self.index_definitions(file_index, fact);
        self.index_declared_scopes(file_index, fact);
        self.index_bindings(file_index, fact);
        self.index_callable_scopes(file_index, fact);
    }
}
impl<'a> ProgramGraph<'a> {
    fn index_definitions(&mut self, file_index: usize, fact: &'a FileFacts) {
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
            self.add_scope(scope);
            let semantic_id = semantic_symbol_id(fact, definition);
            self.symbol_by_semantic_id
                .entry(semantic_id.clone())
                .or_default()
                .push(id);
            let symbol_index = self.symbols.len();
            self.symbols.push(ProgramSymbol {
                id,
                semantic_id,
                definition,
                scope,
            });
            self.symbols_by_file[file_index].push(symbol_index);
            self.symbols_by_name[file_index]
                .entry(definition.name.clone())
                .or_default()
                .push(symbol_index);
        }
    }
    fn index_callable_scopes(&mut self, file_index: usize, fact: &FileFacts) {
        for definition in fact.definitions.iter().filter(|definition| {
            matches!(
                definition.kind,
                DefinitionKind::Function | DefinitionKind::Method
            )
        }) {
            let scope = ProgramScopeId::new(
                file_index,
                definition.context_id,
                definition.start_byte..definition.end_byte,
            );
            self.function_starts[file_index].insert((scope.context_id, scope.start_byte));
            self.callable_scopes[file_index].insert(scope);
            self.add_scope(scope);
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
    fn index_declared_scopes(&mut self, file_index: usize, fact: &FileFacts) {
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
            self.add_scope(ProgramScopeId::new(file_index, context_id, start..end));
        }
    }

    fn add_scope(&mut self, id: ProgramScopeId) {
        let indexes = std::sync::Arc::get_mut(&mut self.indexes)
            .expect("scope indexing runs before any graph projections");
        if let std::collections::hash_map::Entry::Vacant(entry) = indexes.scope_by_id.entry(id) {
            let index = indexes.scopes.len();
            indexes.scopes.push(ProgramScope { id, parent: None });
            indexes.scopes_by_file[id.file_index].push(index);
            entry.insert(index);
        }
    }
    fn link_scope_parents(&mut self) {
        for file_index in 0..self.scopes_by_file.len() {
            let mut ordered = self.scopes_by_file[file_index].clone();
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
    fn index_bindings(&mut self, file_index: usize, fact: &FileFacts) {
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
            self.index_binding(binding);
        }
        for semantic in &fact.semantic {
            for binding in &semantic.bindings {
                self.index_binding(ProgramBinding {
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
                });
            }
            for receiver in &semantic.receivers {
                self.add_scope(ProgramScopeId::new(
                    file_index,
                    semantic.context_id,
                    receiver.start_byte..receiver.end_byte,
                ));
            }
        }
    }
    fn index_binding(&mut self, binding: ProgramBinding) {
        let scope = binding.scope;
        self.add_binding(binding);
        self.add_scope(scope);
    }

    fn add_binding(&mut self, binding: ProgramBinding) {
        let existing = self.bindings_by_file_name[binding.scope.file_index]
            .get(&binding.name)
            .into_iter()
            .flatten()
            .copied()
            .find(|index| binding_identity(&self.bindings[*index]) == binding_identity(&binding));
        if let Some(index) = existing {
            let existing = &mut self.bindings[index];
            match (binding.semantic_evidence, existing.semantic_evidence) {
                (true, false) => *existing = binding,
                (true, true) => {
                    existing.conflicting_evidence |=
                        conflicting_option(&existing.type_name, &binding.type_name)
                            || conflicting_option(&existing.alias_target, &binding.alias_target)
                            || existing.callable_reference != binding.callable_reference
                            || existing.immutable != binding.immutable;
                    existing.type_name = existing.type_name.take().or(binding.type_name);
                    existing.alias_target = existing.alias_target.take().or(binding.alias_target);
                    existing.callable_reference |= binding.callable_reference;
                    existing.immutable &= binding.immutable;
                }
                _ => {}
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

impl ProgramGraph<'_> {
    /// Index reported functions and proven execution edges for shared graph consumers.
    pub(super) fn index_execution_relations(&self, calls: Vec<CallEdge>) -> ExecutionGraph {
        let mut graph = ExecutionGraph::new();
        let mut nodes = std::collections::BTreeMap::new();
        for (file_index, fact) in self.facts.iter().enumerate() {
            for (function_index, function) in fact.analysis.functions.iter().enumerate() {
                nodes
                    .entry((file_index, function.start_byte))
                    .or_insert_with(|| {
                        graph.add_node(ProgramFunctionId {
                            file_index,
                            function_index,
                        })
                    });
            }
        }
        for edge in calls {
            if let (Some(&caller), Some(&callee)) = (
                nodes.get(&(edge.caller_file, edge.caller_start_byte)),
                nodes.get(&(edge.callee_file, edge.callee_start_byte)),
            ) {
                graph.add_edge(caller, callee, edge);
            }
        }
        graph
    }
}
