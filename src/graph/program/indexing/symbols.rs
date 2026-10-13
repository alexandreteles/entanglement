use std::collections::HashSet;

use super::super::{ProgramGraph, ProgramScopeId, ProgramSymbol, ProgramSymbolId};
use crate::model::{Definition, DefinitionKind, FileFacts, SymbolId};

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
