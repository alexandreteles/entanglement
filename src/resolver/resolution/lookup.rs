use super::super::{ImportEntry, Index, ModuleKey, Symbol, Target};
use super::targets;

impl Index {
    pub(super) fn lookup_name(
        &self,
        file: usize,
        access_module: &ModuleKey,
        position: usize,
        module: &ModuleKey,
        name: &str,
    ) -> Vec<Target> {
        let mut choices = self.symbol_choices(file, access_module, position, module, name);
        let import_choices = self
            .imports
            .iter()
            .enumerate()
            .filter(|(_, import)| import.module == *module && import.name == name)
            .filter_map(|(index, import)| {
                import_rank(file, access_module, position, import).map(|rank| (index, rank))
            })
            .flat_map(|(index, rank)| {
                self.import_targets[index]
                    .iter()
                    .cloned()
                    .map(move |target| (rank, target))
            });
        choices.extend(import_choices);
        choose_best_targets(choices)
    }

    fn symbol_choices(
        &self,
        file: usize,
        access_module: &ModuleKey,
        position: usize,
        module: &ModuleKey,
        name: &str,
    ) -> Vec<(usize, Target)> {
        self.symbols
            .get(module)
            .and_then(|names| names.get(name))
            .into_iter()
            .flatten()
            .filter_map(|symbol| {
                symbol_rank(file, access_module, position, module, symbol)
                    .map(|rank| (rank, self.symbol_target(module, name, symbol)))
            })
            .collect()
    }

    fn symbol_target(&self, module: &ModuleKey, name: &str, symbol: &Symbol) -> Target {
        if symbol.id.kind != crate::model::DefinitionKind::Module {
            return Target::Symbol(symbol.id.clone());
        }
        let ids = self
            .symbols
            .get(module)
            .and_then(|names| names.get(name))
            .into_iter()
            .flatten()
            .filter(|candidate| candidate.id.kind == crate::model::DefinitionKind::Module)
            .map(|candidate| candidate.id.clone())
            .collect();
        let ids = targets::unique_symbols(ids);
        if ids.len() > 1 {
            return Target::Ambiguous(ids);
        }
        let mut child = module.clone();
        child.path.push(name.to_owned());
        if self.modules.contains(&child) {
            Target::Module(child)
        } else {
            Target::Symbol(symbol.id.clone())
        }
    }
}

fn symbol_rank(
    file: usize,
    access_module: &ModuleKey,
    position: usize,
    module: &ModuleKey,
    symbol: &Symbol,
) -> Option<usize> {
    if !symbol.is_public && !can_access_private(module, access_module) {
        return None;
    }
    if !symbol.is_local {
        return Some(usize::MAX);
    }
    (symbol.file == file && symbol.scope_start <= position && position <= symbol.scope_end)
        .then(|| symbol.scope_end.saturating_sub(symbol.scope_start))
}

fn import_rank(
    file: usize,
    access_module: &ModuleKey,
    position: usize,
    import: &ImportEntry,
) -> Option<usize> {
    let in_scope =
        import.file == file && import.scope_start <= position && position <= import.scope_end;
    if in_scope {
        return Some(if import.is_local {
            import.scope_end.saturating_sub(import.scope_start)
        } else {
            usize::MAX
        });
    }
    (!import.is_local && (import.is_public || can_access_private(&import.module, access_module)))
        .then_some(usize::MAX)
}

fn choose_best_targets(choices: Vec<(usize, Target)>) -> Vec<Target> {
    let Some(best_rank) = choices.iter().map(|(rank, _)| *rank).min() else {
        return Vec::new();
    };
    targets::unique_targets(
        choices
            .into_iter()
            .filter_map(|(rank, target)| (rank == best_rank).then_some(target))
            .collect(),
    )
}

fn can_access_private(definition: &ModuleKey, access: &ModuleKey) -> bool {
    definition.crate_root == access.crate_root && access.path.starts_with(&definition.path)
}

pub(super) fn is_external_root(name: &str) -> bool {
    matches!(name, "std" | "core" | "alloc")
}
