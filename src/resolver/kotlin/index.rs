use std::path::Path;

use super::*;

type ProjectIndexes = (
    HashMap<PathBuf, BTreeMap<String, BTreeSet<build::ProjectDependency>>>,
    HashMap<PathBuf, BTreeMap<String, BTreeSet<String>>>,
);

impl<'a> Index<'a> {
    pub(super) fn new(
        facts: &'a [FileFacts],
        snapshot: &'a Snapshot,
        graph: &'a ProgramGraph<'a>,
    ) -> Self {
        let sources = facts
            .iter()
            .map(|fact| source_context(&fact.target, snapshot))
            .collect::<Vec<_>>();
        let (dependencies, source_edges) = project_indexes(&sources, snapshot);
        let (candidates, by_name) = indexed_candidates(&sources, graph);
        Self {
            graph,
            snapshot,
            sources,
            candidates,
            by_name,
            dependencies,
            source_edges,
        }
    }
}

fn project_indexes(sources: &[SourceContext], snapshot: &Snapshot) -> ProjectIndexes {
    let mut dependencies = HashMap::new();
    let mut source_edges = HashMap::new();
    for source in sources {
        dependencies
            .entry(source.project_root.clone())
            .or_insert_with(|| project_dependencies(&source.project_root, snapshot));
        source_edges
            .entry(source.module_root.clone())
            .or_insert_with(|| source_set_edges(&source.module_root, snapshot));
    }
    (dependencies, source_edges)
}

fn source_set_edges(module_root: &Path, snapshot: &Snapshot) -> BTreeMap<String, BTreeSet<String>> {
    let mut edges = BTreeMap::new();
    for name in ["build.gradle.kts", "build.gradle"] {
        if let Some(text) = read_text(snapshot, &module_root.join(name)) {
            build::parse_source_edges(&text, &mut edges);
        }
    }
    edges
}

fn indexed_candidates(
    sources: &[SourceContext],
    graph: &ProgramGraph<'_>,
) -> (Vec<Candidate>, HashMap<String, Vec<usize>>) {
    let mut candidates = Vec::new();
    let mut by_name = HashMap::<String, Vec<usize>>::new();
    for (file, source) in sources.iter().enumerate().take(graph.file_count()) {
        for symbol in graph.symbols_for_file(file) {
            let Some(candidate) =
                candidate_for_symbol(candidates.len(), file, source, symbol, graph)
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

fn candidate_for_symbol(
    candidate_index: usize,
    file: usize,
    source: &SourceContext,
    symbol: &crate::graph::ProgramSymbol,
    graph: &ProgramGraph<'_>,
) -> Option<Candidate> {
    let context_id = symbol.scope.context_id;
    let semantic = graph.semantic(file, context_id, Some(ResolutionFamily::KotlinModules))?;
    let definition = graph.definition(symbol.id)?;
    let package = semantic.context.as_ref().map_or_else(
        || definition.module.0.clone(),
        |context| context.package.clone(),
    );
    let declaration = graph
        .declaration(
            file,
            context_id,
            definition.start_byte,
            Some(ResolutionFamily::KotlinModules),
        )
        .cloned();
    Some(Candidate {
        candidate_index,
        graph_id: symbol.id,
        symbol: symbol.semantic_id.clone(),
        file,
        context_id,
        name: symbol.name.clone(),
        package,
        owner: declaration
            .as_ref()
            .map_or_else(Vec::new, |declaration| declaration.owner.clone()),
        kind: symbol.kind.clone(),
        declaration,
        source: source.clone(),
        public: symbol.is_public,
        start: definition.start_byte,
        end: definition.end_byte,
        scope_start: definition.scope_start,
        scope_end: definition.scope_end,
    })
}
