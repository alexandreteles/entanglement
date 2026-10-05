use crate::model::{DefinitionKind, FileFacts, Reference, ReferenceKind, Resolution, SymbolId};

use super::{FileContext, ImportEntry, Index, ModuleKey, Symbol, Target, joined_path};

impl Index {
    pub(super) fn resolve_imports(&mut self) {
        for _ in 0..=self.imports.len() {
            let next = self
                .imports
                .iter()
                .map(|import| self.resolve_import(import))
                .collect::<Vec<_>>();
            if next == self.import_targets {
                break;
            }
            self.import_targets = next;
        }
    }

    fn resolve_import(&self, import: &ImportEntry) -> Vec<Target> {
        let external_root = import.path.first().filter(|name| is_external_root(name));
        if external_root.is_some()
            && self
                .lookup_name(
                    import.file,
                    &import.module,
                    import.scope_start,
                    &import.module,
                    &import.path[0],
                )
                .is_empty()
        {
            return vec![Target::External];
        }
        let Some((base, path)) = self.path_base(&import.module, &import.path) else {
            return Vec::new();
        };
        self.resolve_segments(
            import.file,
            &import.module,
            import.scope_start,
            &base,
            &path,
        )
    }

    pub(super) fn resolve_reference(
        &self,
        file: usize,
        context: &FileContext,
        reference: &Reference,
        fact: &FileFacts,
    ) -> Resolution {
        if reference.kind == ReferenceKind::Method || reference.path.is_empty() {
            return Resolution::Unresolved;
        }
        let external_root = reference.path.first().filter(|name| is_external_root(name));
        let explicit_module_path = reference.path.len() > 1
            && matches!(reference.path[0].as_str(), "crate" | "self" | "super");
        if !explicit_module_path
            && fact.locals.iter().any(|local| {
                local.name == reference.path[0]
                    && local.scope_start <= reference.start_byte
                    && reference.start_byte <= local.scope_end
            })
        {
            return Resolution::Unresolved;
        }

        let current = ModuleKey {
            crate_root: context.crate_root.clone(),
            path: joined_path(
                &joined_path(&context.module, &fact.module.0),
                &reference.module.0,
            ),
        };
        if external_root.is_some()
            && self
                .lookup_name(
                    file,
                    &current,
                    reference.start_byte,
                    &current,
                    &reference.path[0],
                )
                .is_empty()
        {
            return Resolution::External;
        }
        let Some((base, path)) = self.path_base(&current, &reference.path) else {
            return Resolution::Unresolved;
        };
        let targets = self.resolve_segments(file, &current, reference.start_byte, &base, &path);
        self.to_resolution(targets)
    }

    fn path_base(&self, current: &ModuleKey, path: &[String]) -> Option<(ModuleKey, Vec<String>)> {
        let mut base = current.clone();
        let mut rest = path;
        match rest.first().map(String::as_str) {
            Some("crate") => {
                base.path.clear();
                rest = &rest[1..];
            }
            Some("self") => rest = &rest[1..],
            Some("super") => {
                while rest.first().is_some_and(|part| part == "super") {
                    base.path.pop()?;
                    rest = &rest[1..];
                }
            }
            _ => {}
        }
        Some((base, rest.to_vec()))
    }

    fn resolve_segments(
        &self,
        file: usize,
        access_module: &ModuleKey,
        position: usize,
        base: &ModuleKey,
        path: &[String],
    ) -> Vec<Target> {
        if path.is_empty() {
            return vec![Target::Module(base.clone())];
        }

        let mut modules = vec![base.clone()];
        for segment in &path[..path.len() - 1] {
            let targets = modules
                .iter()
                .flat_map(|module| self.lookup_name(file, access_module, position, module, segment))
                .collect::<Vec<_>>();
            let targets = unique_targets(targets);
            if targets.iter().any(|target| {
                matches!(
                    target,
                    Target::Ambiguous(_) | Target::External | Target::Unresolved
                )
            }) {
                return targets;
            }
            modules = targets
                .into_iter()
                .filter_map(|target| match target {
                    Target::Module(module) => Some(module),
                    _ => None,
                })
                .collect();
            modules.sort();
            modules.dedup();
            if modules.is_empty() {
                return Vec::new();
            }
        }

        let name = path.last().expect("Non-empty path has a final segment");
        unique_targets(
            modules
                .iter()
                .flat_map(|module| self.lookup_name(file, access_module, position, module, name))
                .collect(),
        )
    }

    fn lookup_name(
        &self,
        file: usize,
        access_module: &ModuleKey,
        position: usize,
        module: &ModuleKey,
        name: &str,
    ) -> Vec<Target> {
        let mut choices = self
            .symbols
            .get(module)
            .and_then(|names| names.get(name))
            .into_iter()
            .flatten()
            .filter_map(|symbol| {
                if !symbol.is_public && !can_access_private(module, access_module) {
                    return None;
                }
                let rank = if symbol.is_local {
                    if symbol.file != file
                        || position < symbol.scope_start
                        || position > symbol.scope_end
                    {
                        return None;
                    }
                    symbol.scope_end.saturating_sub(symbol.scope_start)
                } else {
                    usize::MAX
                };
                Some((rank, self.symbol_target(module, name, symbol)))
            })
            .collect::<Vec<_>>();

        for (import_index, import) in self.imports.iter().enumerate() {
            if import.module != *module || import.name != name {
                continue;
            }
            let rank = if import.file == file
                && import.scope_start <= position
                && position <= import.scope_end
            {
                if import.is_local {
                    import.scope_end.saturating_sub(import.scope_start)
                } else {
                    usize::MAX
                }
            } else if !import.is_local
                && (import.is_public || can_access_private(&import.module, access_module))
            {
                usize::MAX
            } else {
                continue;
            };
            choices.extend(
                self.import_targets[import_index]
                    .iter()
                    .cloned()
                    .map(|target| (rank, target)),
            );
        }

        let Some(best_rank) = choices.iter().map(|(rank, _)| *rank).min() else {
            return Vec::new();
        };
        unique_targets(
            choices
                .into_iter()
                .filter_map(|(rank, target)| (rank == best_rank).then_some(target))
                .collect(),
        )
    }

    fn symbol_target(&self, module: &ModuleKey, name: &str, symbol: &Symbol) -> Target {
        if symbol.id.kind != DefinitionKind::Module {
            return Target::Symbol(symbol.id.clone());
        }
        let ids = self
            .symbols
            .get(module)
            .and_then(|names| names.get(name))
            .into_iter()
            .flatten()
            .filter(|candidate| candidate.id.kind == DefinitionKind::Module)
            .map(|candidate| candidate.id.clone())
            .collect::<Vec<_>>();
        let ids = unique_symbols(ids);
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

    pub(super) fn to_resolution(&self, targets: Vec<Target>) -> Resolution {
        let targets = unique_targets(targets)
            .into_iter()
            .map(|target| match target {
                Target::Module(module) => self
                    .module_symbol(&module)
                    .map(Target::Symbol)
                    .unwrap_or(Target::Module(module)),
                target => target,
            })
            .collect::<Vec<_>>();
        match targets.as_slice() {
            [Target::Symbol(symbol)] => Resolution::Exact(symbol.clone()),
            [Target::Module(_)] => Resolution::Unresolved,
            [Target::External] => Resolution::External,
            [Target::Ambiguous(symbols)] => Resolution::Ambiguous(symbols.clone()),
            [] => Resolution::Unresolved,
            _ if targets
                .iter()
                .all(|target| matches!(target, Target::Symbol(_) | Target::Ambiguous(_))) =>
            {
                let symbols = targets.iter().flat_map(target_symbols).collect();
                Resolution::Ambiguous(unique_symbols(symbols))
            }
            _ => Resolution::Unresolved,
        }
    }

    fn module_symbol(&self, module: &ModuleKey) -> Option<SymbolId> {
        let name = module.path.last()?;
        let parent = ModuleKey {
            crate_root: module.crate_root.clone(),
            path: module.path[..module.path.len() - 1].to_vec(),
        };
        let symbols = self
            .symbols
            .get(&parent)?
            .get(name)?
            .iter()
            .filter(|symbol| symbol.id.kind == DefinitionKind::Module)
            .map(|symbol| symbol.id.clone())
            .collect::<Vec<_>>();
        match unique_symbols(symbols).as_slice() {
            [symbol] => Some(symbol.clone()),
            _ => None,
        }
    }
}

fn can_access_private(definition: &ModuleKey, access: &ModuleKey) -> bool {
    definition.crate_root == access.crate_root && access.path.starts_with(&definition.path)
}

fn is_external_root(name: &str) -> bool {
    matches!(name, "std" | "core" | "alloc")
}

fn unique_targets(mut targets: Vec<Target>) -> Vec<Target> {
    targets.sort();
    targets.dedup();
    targets
}

fn target_symbols(target: &Target) -> Vec<SymbolId> {
    match target {
        Target::Symbol(symbol) => vec![symbol.clone()],
        Target::Ambiguous(symbols) => symbols.clone(),
        Target::Module(_) | Target::External | Target::Unresolved => Vec::new(),
    }
}

fn unique_symbols(mut symbols: Vec<SymbolId>) -> Vec<SymbolId> {
    symbols.sort();
    symbols.dedup();
    symbols
}
