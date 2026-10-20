use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::graph::CallFacts;
use crate::languages::ResolutionFamily;
use crate::model::{Definition, Import, SemanticStore, SymbolId};

use super::{FileFactsView, ProgramGraph, ProgramSymbol, ProgramSymbolId};

impl<'a> ProgramGraph<'a> {
    pub(crate) fn file_count(&self) -> usize {
        self.facts.len()
    }

    pub(crate) fn file(&self, file_index: usize) -> &FileFactsView<'a> {
        &self.facts[file_index]
    }

    pub(crate) fn files(&self) -> &[FileFactsView<'a>] {
        &self.facts
    }

    pub(crate) fn aliases(&self) -> &BTreeMap<PathBuf, BTreeSet<usize>> {
        &self.aliases
    }

    pub(crate) fn symbols_for_file(
        &self,
        file_index: usize,
    ) -> impl Iterator<Item = &ProgramSymbol<'a>> {
        self.symbols_by_file[file_index]
            .iter()
            .map(|index| &self.symbols[*index])
            .filter(move |symbol| self.file(file_index).allows(symbol.scope.context_id))
    }

    pub(crate) fn symbols_named(
        &self,
        file_index: usize,
        name: &str,
    ) -> impl Iterator<Item = &ProgramSymbol<'a>> {
        self.symbols_by_name[file_index]
            .get(name)
            .into_iter()
            .flatten()
            .map(|index| &self.symbols[*index])
            .filter(move |symbol| self.file(file_index).allows(symbol.scope.context_id))
    }

    pub(crate) fn symbol(&self, id: ProgramSymbolId) -> Option<&ProgramSymbol<'a>> {
        let definition_index = self
            .symbols_by_file
            .get(id.file_index)?
            .get(id.definition_index)?;
        self.symbols
            .get(*definition_index)
            .filter(|symbol| self.file(id.file_index).allows(symbol.scope.context_id))
    }

    pub(crate) fn definition(&self, id: ProgramSymbolId) -> Option<&'a Definition> {
        self.symbol(id).map(|symbol| symbol.definition)
    }

    pub(crate) fn definitions(
        &self,
        file_index: usize,
    ) -> impl Iterator<Item = &'a Definition> + '_ {
        self.facts[file_index].definitions()
    }

    pub(crate) fn imports(&self, file_index: usize) -> impl Iterator<Item = &'a Import> + '_ {
        self.facts[file_index].imports()
    }

    pub(crate) fn semantic(
        &self,
        file_index: usize,
        context_id: usize,
        family: Option<ResolutionFamily>,
    ) -> impl Iterator<Item = &'a SemanticStore> + Clone {
        let file = self.file(file_index);
        let first_family = file
            .semantic()
            .find(|semantic| {
                semantic.semantic.context_id == context_id
                    && family
                        .is_none_or(|family| semantic.semantic.resolution_family == Some(family))
            })
            .map(|semantic| semantic.semantic.resolution_family);
        file.semantic().filter(move |semantic| {
            file.allows(context_id)
                && semantic.semantic.context_id == context_id
                && Some(semantic.semantic.resolution_family) == first_family
        })
    }

    pub(crate) fn call_facts(
        &self,
        file_index: usize,
        context_id: usize,
        family: Option<ResolutionFamily>,
    ) -> impl Iterator<Item = &'a CallFacts> + Clone {
        self.semantic(file_index, context_id, family)
            .flat_map(|semantic| &semantic.semantic.calls)
    }

    pub(super) fn unique_symbol_for_target(&self, target: &SymbolId) -> Option<ProgramSymbolId> {
        let exact = self.symbol_by_semantic_id.get(target);
        if let Some(exact) = exact {
            // SymbolId has no layer identity. If several semantic layers
            // emitted the same ID, it cannot prove which declaration matched.
            let mut eligible = exact
                .iter()
                .copied()
                .filter(|id| self.symbol(*id).is_some());
            if let Some(symbol) = eligible.next() {
                return eligible.next().is_none().then_some(symbol);
            }
            // An exact key present only in excluded contexts is absent from
            // this view, so the family-specific module fallback still applies.
        }

        // Some resolver families (notably Rust modules) qualify the target
        // module during resolution. Preserve that family-specific identity
        // while still mapping a unique source declaration back to the graph.
        let mut matches = self
            .symbols
            .iter()
            .filter(|symbol| {
                self.file(symbol.id.file_index)
                    .allows(symbol.scope.context_id)
                    && symbol.semantic_id.file == target.file
                    && symbol.semantic_id.name == target.name
                    && symbol.semantic_id.kind == target.kind
                    && symbol.semantic_id.start_byte == target.start_byte
            })
            .map(|symbol| symbol.id);
        let symbol = matches.next()?;
        matches.next().is_none().then_some(symbol)
    }
}
