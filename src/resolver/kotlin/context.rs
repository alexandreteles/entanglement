use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use super::*;

use super::{Snapshot, build};

mod dependencies;
mod source;

pub(super) use dependencies::{conventional_source_edge, source_set_reaches};
use dependencies::{project_dependencies, transitive_closure};
pub(in crate::resolver) use source::ProjectCache;
pub(super) use source::SourceContext;
use source::source_context;

type ProjectIndexes = (
    HashMap<PathBuf, BTreeMap<String, BTreeSet<build::ProjectDependency>>>,
    HashMap<PathBuf, BTreeMap<String, BTreeSet<String>>>,
);

impl<'a> Index<'a> {
    pub(super) fn new(
        facts: &'a [FileFactsView<'a>],
        snapshot: &'a Snapshot,
        graph: &'a ProgramGraph<'a>,
        cache: &mut ProjectCache,
        contexts: &[HashSet<usize>],
    ) -> Self {
        let sources = facts
            .iter()
            .zip(contexts)
            .map(|(fact, contexts)| {
                if contexts.is_empty() {
                    SourceContext::default()
                } else {
                    source_context(fact.target, snapshot, cache)
                }
            })
            .collect::<Vec<_>>();
        let active_sources = sources
            .iter()
            .zip(contexts)
            .filter(|(_, contexts)| !contexts.is_empty())
            .map(|(source, _)| source);
        let (dependencies, source_edges) = project_indexes(active_sources.clone(), snapshot, cache);
        let mut visible_modules =
            HashMap::<String, HashMap<Option<String>, BTreeSet<String>>>::new();
        for source in active_sources {
            visible_modules
                .entry(source.module.clone())
                .or_default()
                .entry(source.source_set.clone())
                .or_insert_with(|| {
                    dependencies
                        .get(&source.project_root)
                        .map_or_else(BTreeSet::new, |graph| {
                            transitive_closure(
                                &source.gradle_module,
                                graph,
                                source.source_set.as_deref(),
                            )
                        })
                });
        }
        let (candidates, by_name) = indexed_candidates(&sources, graph);
        Self {
            graph,
            snapshot,
            sources,
            candidates,
            by_name,
            visible_modules,
            source_edges,
            argument_evidence: RefCell::default(),
        }
    }
}

fn project_indexes<'source>(
    sources: impl Iterator<Item = &'source SourceContext>,
    snapshot: &Snapshot,
    cache: &mut ProjectCache,
) -> ProjectIndexes {
    let mut dependencies = HashMap::new();
    let mut source_edges = HashMap::new();
    for source in sources {
        dependencies
            .entry(source.project_root.clone())
            .or_insert_with(|| project_dependencies(&source.project_root, snapshot, cache));
        source_edges
            .entry(source.module_root.clone())
            .or_insert_with(|| source_set_edges(&source.module_root, snapshot, cache));
    }
    (dependencies, source_edges)
}

fn source_set_edges(
    module_root: &Path,
    snapshot: &Snapshot,
    cache: &mut ProjectCache,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut edges = BTreeMap::new();
    for name in ["build.gradle.kts", "build.gradle"] {
        for (child, parents) in &cache.script(module_root.join(name), snapshot).source_edges {
            edges
                .entry(child.clone())
                .or_insert_with(BTreeSet::new)
                .extend(parents.iter().cloned());
        }
    }
    edges
}

fn indexed_candidates<'a>(
    sources: &[SourceContext],
    graph: &'a ProgramGraph<'a>,
) -> (Vec<Candidate<'a>>, HashMap<String, Vec<usize>>) {
    let mut candidates = Vec::new();
    let mut by_name = HashMap::<String, Vec<usize>>::new();
    for file in 0..sources.len().min(graph.file_count()) {
        for symbol in graph.symbols_for_file(file) {
            let Some(candidate) = candidate_for_symbol(candidates.len(), file, symbol, graph)
            else {
                continue;
            };
            by_name
                .entry(candidate.name.clone())
                .or_default()
                .push(candidate.candidate_index);
            candidates.push(candidate);
        }
    }
    (candidates, by_name)
}

fn candidate_for_symbol<'a>(
    candidate_index: usize,
    file: usize,
    symbol: &'a crate::graph::ProgramSymbol<'a>,
    graph: &'a ProgramGraph<'a>,
) -> Option<Candidate<'a>> {
    let context_id = symbol.scope.context_id;
    let mut semantic = graph.semantic(file, context_id, Some(ResolutionFamily::KotlinModules));
    semantic.clone().next()?;
    let definition = symbol.definition;
    let package = semantic
        .find_map(|semantic| semantic.context.as_ref())
        .map_or(definition.module.0.as_slice(), |context| {
            context.package.as_slice()
        });
    let declaration = definition.declaration.as_deref();
    Some(Candidate {
        candidate_index,
        graph_id: symbol.id,
        symbol: &symbol.semantic_id,
        file,
        definition,
        package,
        owner: declaration.map_or(&[][..], |declaration| declaration.owner.as_slice()),
        declaration,
    })
}
