mod lookup;
mod targets;

use lookup::is_external_root;
use targets::unique_targets;

use crate::model::{FileFacts, Reference, ReferenceKind, Resolution};

use super::{FileContext, ImportEntry, Index, ModuleKey, Target, joined_path};

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
}
