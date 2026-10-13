use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::languages::ResolutionFamily;
use crate::model::{Definition, FileFacts, Import, SymbolId};

use super::{ProgramGraph, ProgramSymbol, ProgramSymbolId};
use crate::graph::{CallFacts, DeclarationFacts, SemanticFacts};

impl<'a> ProgramGraph<'a> {
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

    pub(super) fn unique_symbol_for_target(&self, target: &SymbolId) -> Option<ProgramSymbolId> {
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
