mod bindings;
mod scopes;
mod symbols;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use crate::model::FileFacts;

use super::{ProgramGraph, ProgramScopeId};

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
            relations: Vec::new(),
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
        self.index_local_bindings(file_index, fact, scope_ids);
        self.index_semantic_bindings(file_index, fact, scope_ids);
        self.index_callable_scopes(file_index, scope_ids);
    }
}
