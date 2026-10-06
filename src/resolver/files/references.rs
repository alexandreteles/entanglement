use std::collections::BTreeSet;

use crate::languages::ResolutionFamily;
use crate::model::{FileFacts, Import, Reference, ReferenceAnalysis, ReferenceKind, Resolution};

use super::bindings::{BindingTarget, binding_target};
use super::layout::resolve_submodule;
use super::{Index, Target, to_resolution};

impl Index<'_> {
    pub(super) fn resolve_file(
        &self,
        file: usize,
        fact: &FileFacts,
    ) -> Option<Vec<ReferenceAnalysis>> {
        (fact.resolution_family == ResolutionFamily::FileModules).then(|| {
            fact.references
                .iter()
                .map(|reference| ReferenceAnalysis {
                    path: reference.path.clone(),
                    start_byte: reference.start_byte,
                    end_byte: reference.end_byte,
                    resolution: self.resolve_reference(file, fact, reference),
                })
                .collect()
        })
    }

    fn resolve_reference(
        &self,
        file: usize,
        fact: &FileFacts,
        reference: &Reference,
    ) -> Resolution {
        if reference.kind == ReferenceKind::Method || reference.path.is_empty() {
            return Resolution::Unresolved;
        }
        let name = &reference.path[0];
        match binding_target(fact, name, reference.start_byte, reference.context_id) {
            BindingTarget::Local(targets) if reference.path.len() == 1 => to_resolution(targets),
            BindingTarget::Import(import) => self.resolve_import_reference(file, import, reference),
            BindingTarget::Local(_) | BindingTarget::Unresolved => Resolution::Unresolved,
        }
    }

    fn resolve_import_reference(
        &self,
        file: usize,
        import: &Import,
        reference: &Reference,
    ) -> Resolution {
        if import.source.is_none() {
            return Resolution::Unresolved;
        }
        let targets = self.resolve_import_binding(file, import, &mut BTreeSet::new());
        let targets = reference
            .path
            .iter()
            .skip(1)
            .fold(targets, |targets, name| {
                self.resolve_namespace_member(targets, name)
            });
        to_resolution(targets)
    }

    fn resolve_namespace_member(&self, targets: Vec<Target>, name: &str) -> Vec<Target> {
        targets
            .into_iter()
            .flat_map(|target| match target {
                Target::Namespace(module_file) => {
                    let mut targets = self.resolve_export(module_file, name, &mut BTreeSet::new());
                    if targets.is_empty() {
                        targets = resolve_submodule(self, module_file, name);
                    }
                    targets
                }
                Target::External => vec![Target::External],
                _ => vec![Target::Unresolved],
            })
            .collect()
    }

    pub(super) fn resolve_named_import(
        &self,
        module_file: usize,
        name: &str,
        visiting: &mut BTreeSet<(usize, String)>,
    ) -> Vec<Target> {
        let targets = self.resolve_export(module_file, name, visiting);
        if targets.is_empty() {
            resolve_submodule(self, module_file, name)
        } else {
            targets
        }
    }

    pub(super) fn resolve_import_binding(
        &self,
        file: usize,
        import: &Import,
        visiting: &mut BTreeSet<(usize, String)>,
    ) -> Vec<Target> {
        let Some(source) = import.source.as_deref() else {
            return vec![Target::Unresolved];
        };
        self.resolve_module(file, source)
            .into_iter()
            .flat_map(|module| match module {
                Target::Namespace(module_file) if import.namespace => {
                    vec![Target::Namespace(module_file)]
                }
                Target::Namespace(module_file) => import
                    .imported_name
                    .as_deref()
                    .map(|name| self.resolve_named_import(module_file, name, visiting))
                    .unwrap_or_else(|| vec![Target::Unresolved]),
                target => vec![target],
            })
            .collect()
    }
}
