use crate::graph::{CallDispatch, FileFactsView};
use crate::languages::ResolutionFamily;
use crate::languages::c_family::{
    conditional::Active,
    project::{FileView, Project},
    semantic::{Data, Invocation, Symbol, calls::binding},
};
use crate::model::{
    Definition, DefinitionKind, Reference, ReferenceAnalysis, ReferenceKind, Resolution,
    SemanticStore, SymbolId,
};
use std::collections::{BTreeMap, BTreeSet};

mod hierarchy;
mod lookup;
pub(crate) mod modules;
mod overload;

struct Entry<'a> {
    id: SymbolId,
    unit: usize,
    position: (Active, usize, usize),
    def: &'a Definition,
    symbol: &'a Symbol,
    cpp: bool,
    module: Option<&'a str>,
}

struct Context<'a> {
    cpp: bool,
    reportable: bool,
    unit: usize,
    view: Option<&'a FileView>,
    store: &'a SemanticStore,
    references: BTreeSet<(usize, &'a [String])>,
}

struct Index<'a> {
    classes: hierarchy::Classes,
    names: BTreeMap<&'a str, Vec<Entry<'a>>>,
    contexts: Vec<Vec<Context<'a>>>,
    modules: modules::Modules,
}

impl<'a> Index<'a> {
    fn new(files: &'a [FileFactsView<'a>], project: &'a Project) -> Self {
        let mut names: BTreeMap<_, Vec<_>> = BTreeMap::new();
        let mut contexts = Vec::new();
        for (index, file) in files.iter().enumerate() {
            let mut views = Vec::new();
            if file.resolution_family == ResolutionFamily::CFamily {
                for (unit_id, unit) in project.units.iter().enumerate() {
                    if let Some(view) = unit.files.get(file.target)
                        && let Some(store) = view.semantic.as_deref()
                    {
                        views.push((unit_id, Some(view), store, unit.cpp));
                    }
                }
                if views.is_empty() {
                    views.extend(
                        file.semantic()
                            .filter(|store| store.semantic.c_family.is_some())
                            .map(|store| {
                                (
                                    project.units.len() + index,
                                    None,
                                    store,
                                    file.analysis.language != "c",
                                )
                            }),
                    );
                }
            }
            let mut indexed = Vec::new();
            for (unit, view, store, cpp) in views {
                let Some(data) = store.semantic.c_family.as_deref() else {
                    continue;
                };
                for def in &store.definitions {
                    let Some(symbol) = data.symbols.get(&(def.start_byte, def.name.clone())) else {
                        continue;
                    };
                    let Some(position) = position(view, def.start_byte) else {
                        continue;
                    };
                    let id = SymbolId {
                        file: file.analysis.path.clone(),
                        module: def.module.clone(),
                        name: def.name.clone(),
                        kind: def.kind.clone(),
                        start_byte: def.start_byte,
                    };
                    names.entry(def.name.as_str()).or_default().push(Entry {
                        id,
                        unit,
                        position,
                        def,
                        symbol,
                        cpp,
                        module: data.module.as_deref(),
                    });
                }
                let references = store
                    .references
                    .iter()
                    .map(|item| (item.start_byte, item.path.as_slice()))
                    .collect();
                indexed.push(Context {
                    cpp,
                    reportable: project
                        .units
                        .get(unit)
                        .is_none_or(|unit| !unit.fallback || !project.has_includer(file.target)),
                    unit,
                    view,
                    store,
                    references,
                });
            }
            contexts.push(indexed);
        }
        let modules = modules::Modules::new(&contexts);
        let classes = hierarchy::classes(&contexts);
        Self {
            classes,
            names,
            contexts,
            modules,
        }
    }
}

pub(super) fn resolve(
    files: &[FileFactsView<'_>],
    project: &Project,
) -> Vec<Option<Vec<ReferenceAnalysis>>> {
    let index = Index::new(files, project);
    files
        .iter()
        .enumerate()
        .map(|(file, facts)| {
            (facts.resolution_family == ResolutionFamily::CFamily).then(|| {
                facts
                    .references()
                    .map(|reference| {
                        let mut outcomes = Vec::new();
                        for context in &index.contexts[file] {
                            if !context.reportable
                                || context.store.semantic.context_id != reference.context_id
                            {
                                continue;
                            }
                            outcomes.push(
                                if context
                                    .references
                                    .contains(&(reference.start_byte, reference.path.as_slice()))
                                {
                                    index.lookup(context, reference)
                                } else {
                                    (Resolution::Unresolved, CallDispatch::Unknown)
                                },
                            );
                        }
                        let (resolution, dispatch) = combine(outcomes);
                        super::analysis(reference, resolution, Some(dispatch))
                    })
                    .collect()
            })
        })
        .collect()
}

fn position(view: Option<&FileView>, at: usize) -> Option<(Active, usize, usize)> {
    view.map_or(Some((Active::Yes, 0, at)), |view| view.position(at))
}

type Outcome = (Resolution, CallDispatch);
fn combine(results: Vec<Outcome>) -> Outcome {
    let mut symbols = BTreeSet::new();
    let mut uncertain = results.is_empty();
    let mut direct = true;
    let mut external = !results.is_empty();
    for (answer, dispatch) in results {
        direct &= dispatch == CallDispatch::Direct;
        external &= matches!(answer, Resolution::External);
        match answer {
            Resolution::Exact(symbol) => {
                symbols.insert(symbol);
            }
            Resolution::Ambiguous(candidates) => symbols.extend(candidates),
            _ => uncertain = true,
        }
    }
    let answer = if symbols.len() > 1 {
        Resolution::Ambiguous(symbols.into_iter().collect())
    } else if !uncertain && let Some(symbol) = symbols.pop_first() {
        Resolution::Exact(symbol)
    } else if external {
        Resolution::External
    } else {
        Resolution::Unresolved
    };
    (
        answer,
        if direct && !uncertain {
            CallDispatch::Direct
        } else {
            CallDispatch::Unknown
        },
    )
}

pub(crate) fn module_files(facts: &[crate::model::FileFacts]) -> BTreeMap<String, BTreeSet<&str>> {
    let mut result: BTreeMap<_, BTreeSet<_>> = BTreeMap::new();
    for file in facts {
        for module in file
            .contexts
            .iter()
            .filter_map(|store| store.semantic.c_family.as_ref()?.module.as_ref())
        {
            result
                .entry(module.clone())
                .or_default()
                .insert(file.analysis.path.as_str());
        }
    }
    result
}
