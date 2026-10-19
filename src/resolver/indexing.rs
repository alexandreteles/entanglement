use std::collections::{BTreeMap, BTreeSet};

use crate::graph::ProgramGraph;
use crate::languages::ResolutionFamily;
use crate::model::{DefinitionKind, Import, ModulePath, SymbolId};

use super::{ImportEntry, Index, ModuleKey, Symbol, joined_path};

impl Index {
    pub(super) fn new(graph: &ProgramGraph<'_>) -> Self {
        let contexts = super::module_graph::file_contexts(graph);
        let mut index = Self {
            contexts,
            symbols: BTreeMap::new(),
            modules: BTreeSet::new(),
            imports: Vec::new(),
            import_targets: Vec::new(),
        };
        index.add_symbols(graph);
        index.add_imports(graph);
        index
    }

    fn add_symbols(&mut self, graph: &ProgramGraph<'_>) {
        for (file_index, fact) in graph
            .files()
            .iter()
            .enumerate()
            .filter(|(_, fact)| fact.resolution_family == ResolutionFamily::RustCrates)
        {
            for context in &self.contexts[file_index] {
                let file_module = joined_path(&context.module, &fact.module.0);
                self.modules
                    .extend([Vec::new(), file_module.clone()].map(|path| ModuleKey {
                        crate_root: context.crate_root.clone(),
                        path,
                    }));

                for graph_symbol in graph.symbols_for_file(file_index).filter(|symbol| {
                    !matches!(symbol.kind, DefinitionKind::Method | DefinitionKind::Other)
                }) {
                    let definition = graph
                        .definition(graph_symbol.id)
                        .expect("eligible symbol has canonical definition");
                    let module = joined_path(&file_module, &definition.module.0);
                    let module_key = ModuleKey {
                        crate_root: context.crate_root.clone(),
                        path: module.clone(),
                    };
                    if definition.kind == DefinitionKind::Module {
                        let mut child = module_key.clone();
                        child.path.push(definition.name.clone());
                        self.modules.insert(module_key.clone());
                        self.modules.insert(child);
                    }
                    let id = SymbolId {
                        module: ModulePath(module),
                        ..graph_symbol.semantic_id.clone()
                    };
                    self.symbols
                        .entry(module_key)
                        .or_default()
                        .entry(definition.name.clone())
                        .or_default()
                        .push(Symbol {
                            id,
                            file: file_index,
                            scope_start: definition.scope_start,
                            scope_end: definition.scope_end,
                            is_local: graph.is_local_definition(file_index, definition),
                            is_public: definition.is_public,
                        });
                }
            }
        }
        for symbols in self
            .symbols
            .values_mut()
            .flat_map(|names| names.values_mut())
        {
            symbols.sort_by(|left, right| left.id.cmp(&right.id));
            symbols.dedup_by(|left, right| left.id == right.id);
        }
    }

    fn add_imports(&mut self, graph: &ProgramGraph<'_>) {
        for (file_index, fact) in graph
            .files()
            .iter()
            .enumerate()
            .filter(|(_, fact)| fact.resolution_family == ResolutionFamily::RustCrates)
        {
            for context in &self.contexts[file_index] {
                let file_module = joined_path(&context.module, &fact.module.0);
                for import in graph.imports(file_index) {
                    let Some((name, path)) = import_name_and_path(import) else {
                        continue;
                    };
                    self.imports.push(ImportEntry {
                        file: file_index,
                        module: ModuleKey {
                            crate_root: context.crate_root.clone(),
                            path: joined_path(&file_module, &import.module.0),
                        },
                        name,
                        path,
                        scope_start: import.scope_start,
                        scope_end: import.scope_end,
                        is_local: graph.is_local_scope(
                            file_index,
                            import.context_id,
                            import.scope_start,
                            import.scope_end,
                            import.start_byte,
                        ),
                        is_public: import.is_public,
                    });
                }
            }
        }
        self.imports.sort_by(|left, right| {
            (
                &left.module,
                &left.name,
                left.file,
                left.scope_start,
                &left.path,
            )
                .cmp(&(
                    &right.module,
                    &right.name,
                    right.file,
                    right.scope_start,
                    &right.path,
                ))
        });
        self.import_targets = vec![Vec::new(); self.imports.len()];
    }
}

fn import_name_and_path(import: &Import) -> Option<(String, Vec<String>)> {
    let mut path = import.path.clone();
    if path.is_empty() || path.last().is_some_and(|part| part == "*") {
        return None;
    }
    if path.last().is_some_and(|part| part == "self") && path.len() > 1 {
        path.pop();
    }
    let name = import.alias.clone().or_else(|| path.last().cloned())?;
    Some((name, path))
}
