use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

use crate::languages::ResolutionFamily;
use crate::model::{
    Definition, DefinitionKind, FileFacts, Import, ReferenceAnalysis, ReferenceKind, Resolution,
    SymbolId,
};

use super::{CallDispatch, CallFacts, DeclarationFacts, SemanticFacts};

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
    pub immutable: bool,
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
                graph.add_binding(ProgramBinding {
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
                    immutable: false,
                });
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
                    graph.add_binding(ProgramBinding {
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
                        immutable: binding.immutable,
                    });
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

    fn add_binding(&mut self, binding: ProgramBinding) {
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
                let caller_is_callable = self.symbols_for_file(file_index).any(|candidate| {
                    matches!(
                        candidate.kind,
                        DefinitionKind::Function | DefinitionKind::Method
                    ) && candidate.semantic_id.start_byte == caller_start_byte
                        && candidate.scope.context_id == reference.context_id
                });
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
                    && self.function_starts[file_index]
                        .contains(&(reference.context_id, caller_start_byte))
                    && caller_is_callable;
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

fn semantic_symbol_id(fact: &FileFacts, definition: &Definition) -> SymbolId {
    SymbolId {
        file: fact.analysis.path.clone(),
        module: definition.module.clone(),
        name: definition.name.clone(),
        kind: definition.kind.clone(),
        start_byte: definition.start_byte,
    }
}
