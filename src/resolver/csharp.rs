mod calls;
mod expressions;
mod index;
mod names;
mod overloads;

use crate::graph::{CallDispatch, FileFactsView};
use crate::languages::{
    ResolutionFamily,
    csharp::{facts::Usage, project::Project},
};
use crate::model::{ModulePath, ReferenceAnalysis, Resolution, SymbolId};
use index::{Index, Site, Symbol};
use rayon::prelude::*;
use std::collections::BTreeSet;

pub(super) fn resolve(
    files: &[FileFactsView<'_>],
    project: &Project,
) -> Vec<Option<Vec<ReferenceAnalysis>>> {
    if !files
        .iter()
        .any(|f| f.resolution_family == ResolutionFamily::CSharp)
    {
        return (0..files.len()).map(|_| None).collect();
    }
    let index = Index::new(files, project);
    files
        .par_iter()
        .enumerate()
        .map(|(file, f)| {
            (f.resolution_family == ResolutionFamily::CSharp).then(|| {
                f.references()
                    .map(|reference| {
                        let usage = index.sources[file]
                            .uses
                            .get(&(reference.start_byte, reference.end_byte));
                        let Some(usage) = usage else {
                            return super::analysis(reference, Resolution::Unresolved, None);
                        };
                        let (resolution, dispatch) = index.resolution(file, usage);
                        let mut report = super::analysis(reference, resolution, dispatch);
                        if let Resolution::Exact(symbol) = &report.resolution
                            && let Some((file, context, decl)) = index
                                .locations
                                .get(&(symbol.file.as_str(), symbol.start_byte))
                            && let Some(execution) = &decl.execution
                            && let Some(caller) = reference.execution_owner
                        {
                            let target = if usage.constructor {
                                execution.constructors.first().copied()
                            } else if usage.write {
                                execution.setter
                            } else {
                                execution.getter
                            };
                            if let Some(range) = target {
                                report
                                    .execution_targets
                                    .push(crate::model::ExecutionTarget {
                                        caller_start_byte: caller,
                                        file_index: *file,
                                        context_id: *context,
                                        range,
                                    });
                            }
                        }
                        report
                    })
                    .collect()
            })
        })
        .collect()
}

impl Index<'_> {
    fn resolution(&self, file: usize, usage: &Usage) -> (Resolution, Option<CallDispatch>) {
        if self.sources[file].has_error(&usage.range) {
            return (Resolution::Unresolved, None);
        }
        let at = usage.range.start;
        let contexts = self.project.owners(self.files[file].target);
        let mut options = Vec::new();
        if contexts.is_empty() {
            options.push(None);
        } else {
            options.extend(
                contexts
                    .iter()
                    .filter(|v| self.project.active(self.files[file].target, **v, at))
                    .copied()
                    .map(Some),
            );
        }
        let mut symbols = BTreeSet::new();
        let mut missing = false;
        let mut external = false;
        let mut direct = true;
        for view in options {
            let site = Site {
                file,
                view,
                at,
                owner: &usage.owner,
                namespace: &usage.namespace,
            };
            let candidates = self.select(site, usage, 0);
            if candidates.is_empty() {
                external |= names::builtin(&usage.name)
                    || usage.name == "System"
                    || usage.name.starts_with("System.")
                    || self.external_receiver(site, &usage.receiver);
                missing = true;
            }
            for symbol in candidates {
                if file != symbol.file
                    && view.is_some_and(|v| {
                        self.project.views[v]
                            .limits
                            .iter()
                            .any(|s| s.starts_with("msbuild_"))
                    })
                {
                    missing = true;
                }
                direct &= self.direct(site, usage, symbol);
                symbols.insert(self.symbol_id(symbol));
            }
        }
        let resolution = match symbols.len() {
            0 if external => Resolution::External,
            0 => Resolution::Unresolved,
            1 if !missing => Resolution::Exact(symbols.pop_first().expect("unique C# symbol")),
            _ => Resolution::Ambiguous(symbols.into_iter().collect()),
        };
        let dispatch = (usage.kind != crate::model::ReferenceKind::Type).then_some(
            if !matches!(resolution, Resolution::Exact(_)) {
                CallDispatch::Unknown
            } else if direct {
                CallDispatch::Direct
            } else {
                CallDispatch::Virtual
            },
        );
        (resolution, dispatch)
    }

    fn direct(&self, site: Site<'_>, usage: &Usage, symbol: Symbol<'_>) -> bool {
        let decl = symbol.decl;
        if usage.kind == crate::model::ReferenceKind::Value && decl.execution.is_none() {
            return false;
        }
        if usage.constructor && decl.is_type {
            return decl.execution.is_some();
        }
        if decl.body.is_none() || decl.modifiers.contains("abstract") {
            return false;
        }
        if matches!(&usage.receiver,crate::languages::csharp::facts::Expression::Name(name) if name=="base")
        {
            return true;
        }
        if decl.modifiers.contains("sealed") || decl.modifiers.contains("static") {
            return true;
        }
        if self
            .named_types(site, &decl.owner)
            .iter()
            .any(|s| s.decl.category == "interface")
        {
            return false;
        }
        !decl.modifiers.contains("virtual") && !decl.modifiers.contains("override")
    }

    fn symbol_id(&self, symbol: Symbol<'_>) -> SymbolId {
        let decl = symbol.decl;
        SymbolId {
            file: self.files[symbol.file].analysis.path.clone(),
            module: ModulePath(
                decl.owner
                    .split('.')
                    .filter(|s| !s.is_empty())
                    .map(str::to_owned)
                    .collect(),
            ),
            name: decl.name.clone(),
            kind: decl.kind.clone(),
            start_byte: decl.range.start,
        }
    }
}
