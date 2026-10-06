use std::collections::BTreeSet;

use crate::model::Export;

use super::bindings::{BindingTarget, binding_target, local_binding_rank};
use super::symbols::anonymous_export_symbols;
use super::{Index, Target, unique_targets};

impl Index<'_> {
    pub(super) fn resolve_export(
        &self,
        file: usize,
        name: &str,
        visiting: &mut BTreeSet<(usize, String)>,
    ) -> Vec<Target> {
        let key = (file, name.to_owned());
        if !visiting.insert(key.clone()) {
            return Vec::new();
        }
        let exports = &self.facts[file].exports;
        let explicit = exports
            .iter()
            .filter(|export| export.exported_name == name)
            .collect::<Vec<_>>();
        let targets = if explicit.is_empty() {
            self.resolve_star_exports(file, name, visiting)
        } else {
            explicit
                .into_iter()
                .flat_map(|export| self.resolve_export_entry(file, export, visiting))
                .collect()
        };
        visiting.remove(&key);
        unique_targets(targets)
    }

    fn resolve_star_exports(
        &self,
        file: usize,
        name: &str,
        visiting: &mut BTreeSet<(usize, String)>,
    ) -> Vec<Target> {
        if name == "default" {
            return vec![Target::Unresolved];
        }
        self.facts[file]
            .exports
            .iter()
            .filter(|export| export.exported_name == "*" && !export.namespace)
            .flat_map(|export| {
                self.resolve_export_entry(file, export, visiting)
                    .into_iter()
                    .flat_map(|target| match target {
                        Target::Namespace(module) => self.resolve_export(module, name, visiting),
                        target => vec![target],
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn resolve_export_entry(
        &self,
        file: usize,
        export: &Export,
        visiting: &mut BTreeSet<(usize, String)>,
    ) -> Vec<Target> {
        let Some(source) = export.source.as_deref() else {
            return self.resolve_local_export(file, export, visiting);
        };
        self.resolve_export_source(file, source, export, visiting)
    }

    fn resolve_local_export(
        &self,
        file: usize,
        export: &Export,
        visiting: &mut BTreeSet<(usize, String)>,
    ) -> Vec<Target> {
        let Some(name) = export.local_name.as_deref() else {
            return anonymous_export_symbols(&self.facts[file], export);
        };
        match binding_target(
            &self.facts[file],
            name,
            export.start_byte,
            export.context_id,
        ) {
            BindingTarget::Local(targets) => targets,
            BindingTarget::Import(import) => self.resolve_import_binding(file, import, visiting),
            BindingTarget::Unresolved => vec![Target::Unresolved],
        }
    }

    fn resolve_export_source(
        &self,
        file: usize,
        source: &str,
        export: &Export,
        visiting: &mut BTreeSet<(usize, String)>,
    ) -> Vec<Target> {
        if export.local_name.as_deref().is_some_and(|name| {
            local_binding_rank(
                &self.facts[file],
                name,
                export.start_byte,
                export.context_id,
            )
            .is_some()
        }) {
            return vec![Target::Unresolved];
        }
        self.resolve_module(file, source)
            .into_iter()
            .flat_map(|module| match module {
                Target::Namespace(module_file) if export.namespace => {
                    vec![Target::Namespace(module_file)]
                }
                Target::Namespace(module_file) if export.exported_name == "*" => {
                    vec![Target::Namespace(module_file)]
                }
                Target::Namespace(module_file) => export
                    .imported_name
                    .as_deref()
                    .map(|name| self.resolve_named_import(module_file, name, visiting))
                    .unwrap_or_else(|| vec![Target::Unresolved]),
                target => vec![target],
            })
            .collect()
    }
}
