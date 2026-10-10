use crate::model::{DefinitionKind, Resolution, SymbolId};

use super::super::{Index, ModuleKey, Target};

impl Index {
    pub(in crate::resolver) fn to_resolution(&self, targets: Vec<Target>) -> Resolution {
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

pub(super) fn unique_targets(mut targets: Vec<Target>) -> Vec<Target> {
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

pub(super) fn unique_symbols(mut symbols: Vec<SymbolId>) -> Vec<SymbolId> {
    symbols.sort();
    symbols.dedup();
    symbols
}
