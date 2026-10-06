use std::collections::{BTreeMap, BTreeSet};

use crate::languages::ResolutionFamily;
use crate::model::{Definition, DefinitionKind, FileFacts, Import, ModulePath, SymbolId};

use super::{ImportEntry, Index, ModuleKey, Symbol, joined_path};

impl Index {
    pub(super) fn new(facts: &[FileFacts]) -> Self {
        let contexts = super::module_graph::file_contexts(facts);
        let mut index = Self {
            contexts,
            symbols: BTreeMap::new(),
            modules: BTreeSet::new(),
            imports: Vec::new(),
            import_targets: Vec::new(),
        };
        index.add_symbols(facts);
        index.add_imports(facts);
        index
    }

    fn add_symbols(&mut self, facts: &[FileFacts]) {
        for (file_index, fact) in facts.iter().enumerate() {
            if fact.resolution_family != ResolutionFamily::RustCrates {
                continue;
            }
            for context in &self.contexts[file_index] {
                let file_module = joined_path(&context.module, &fact.module.0);
                self.modules.insert(ModuleKey {
                    crate_root: context.crate_root.clone(),
                    path: Vec::new(),
                });
                if !file_module.is_empty() {
                    self.modules.insert(ModuleKey {
                        crate_root: context.crate_root.clone(),
                        path: file_module.clone(),
                    });
                }

                for definition in &fact.definitions {
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
                    if !is_resolvable_kind(&definition.kind) {
                        continue;
                    }
                    let id = SymbolId {
                        file: fact.analysis.path.clone(),
                        module: ModulePath(module),
                        name: definition.name.clone(),
                        kind: definition.kind.clone(),
                        start_byte: definition.start_byte,
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
                            is_local: is_local_definition(fact, definition),
                            is_public: definition.is_public,
                        });
                }
            }
        }
        for names in self.symbols.values_mut() {
            for symbols in names.values_mut() {
                symbols.sort_by(|left, right| left.id.cmp(&right.id));
                symbols.dedup_by(|left, right| left.id == right.id);
            }
        }
    }

    fn add_imports(&mut self, facts: &[FileFacts]) {
        for (file_index, fact) in facts.iter().enumerate() {
            if fact.resolution_family != ResolutionFamily::RustCrates {
                continue;
            }
            for context in &self.contexts[file_index] {
                let file_module = joined_path(&context.module, &fact.module.0);
                for import in &fact.imports {
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
                        is_local: is_local_scope(
                            fact,
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

fn is_local_definition(fact: &FileFacts, definition: &Definition) -> bool {
    is_local_scope(
        fact,
        definition.scope_start,
        definition.scope_end,
        definition.start_byte,
    )
}

fn is_local_scope(fact: &FileFacts, scope_start: usize, scope_end: usize, position: usize) -> bool {
    fact.analysis.functions.iter().any(|function| {
        function.start_byte <= scope_start
            && scope_end <= function.end_byte
            && (function.start_byte < scope_start
                || scope_end < function.end_byte
                || (function.start_byte < position && position < function.end_byte))
    })
}

fn is_resolvable_kind(kind: &DefinitionKind) -> bool {
    !matches!(kind, DefinitionKind::Method | DefinitionKind::Other)
}
