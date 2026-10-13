use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

use super::*;

use super::{Snapshot, build};

#[derive(Clone, Debug, Default)]
pub(super) struct SourceContext {
    pub(super) path: PathBuf,
    pub(super) project_root: PathBuf,
    pub(super) module_root: PathBuf,
    /// Project-unique identity used to prevent unrelated roots colliding.
    pub(super) module: String,
    /// Gradle's module coordinate relative to project_root (for example `:lib`).
    pub(super) gradle_module: String,
    pub(super) source_set: Option<String>,
    pub(super) isolated_source: bool,
}

/// Project evidence belongs to one resolution snapshot, never to a global cache.
#[derive(Default)]
pub(in crate::resolver) struct ProjectCache {
    modules: HashMap<PathBuf, Vec<(String, PathBuf)>>,
    scripts: HashMap<PathBuf, build::BuildFacts>,
}

impl ProjectCache {
    fn modules(&mut self, root: &Path, snapshot: &Snapshot) -> &[(String, PathBuf)] {
        self.modules.entry(root.to_path_buf()).or_insert_with(|| {
            let source = ["settings.gradle.kts", "settings.gradle"]
                .iter()
                .find_map(|name| read_text(snapshot, &root.join(name)))
                .unwrap_or_default();
            let settings = build::Settings::parse(&source);
            settings
                .modules
                .into_iter()
                .map(|module| {
                    let directory =
                        settings
                            .directories
                            .get(&module)
                            .cloned()
                            .unwrap_or_else(|| {
                                PathBuf::from(module.trim_start_matches(':').replace(':', "/"))
                            });
                    (module, root.join(directory))
                })
                .collect()
        })
    }

    pub(super) fn script(&mut self, path: PathBuf, snapshot: &Snapshot) -> &build::BuildFacts {
        self.scripts.entry(path.clone()).or_insert_with(|| {
            read_text(snapshot, &path).map_or_else(build::BuildFacts::default, |source| {
                build::BuildFacts::parse(&source)
            })
        })
    }
}

pub(super) fn source_context(
    path: &Path,
    snapshot: &Snapshot,
    cache: &mut ProjectCache,
) -> SourceContext {
    let absolute = path.to_path_buf();
    let settings_root = manifest_root(path, snapshot, &["settings.gradle.kts", "settings.gradle"]);
    let source_parts = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => part.to_str().map(str::to_owned),
            _ => None,
        })
        .collect::<Vec<_>>();
    let source_index = source_parts.iter().rposition(|part| part == "src");
    let source_set = source_index
        .filter(|index| *index + 2 < source_parts.len())
        .filter(|index| matches!(source_parts[*index + 2].as_str(), "kotlin" | "java"))
        .map(|index| source_parts[index + 1].clone());
    let inferred_module_root = source_index
        .and_then(|index| {
            path.ancestors()
                .nth(path.components().count().saturating_sub(index + 1))
        })
        .map(Path::to_path_buf);
    let build_root = manifest_root(path, snapshot, &["build.gradle.kts", "build.gradle"]);
    let settings_root = settings_root.or_else(|| build_root.clone());
    let project_root = settings_root
        .clone()
        .or_else(|| inferred_module_root.clone())
        .unwrap_or_else(|| absolute.parent().unwrap_or(&absolute).to_path_buf());
    let module_root = build_root
        .or(inferred_module_root)
        .unwrap_or_else(|| project_root.clone());
    let gradle_module = module_name(
        &project_root,
        &module_root,
        cache.modules(&project_root, snapshot),
    );
    let module = format!("{}{}", project_root.display(), gradle_module);
    SourceContext {
        path: absolute.clone(),
        project_root,
        module_root,
        module,
        gradle_module,
        source_set,
        isolated_source: path.extension().and_then(|extension| extension.to_str()) != Some("kt"),
    }
}

fn manifest_root(path: &Path, snapshot: &Snapshot, names: &[&str]) -> Option<PathBuf> {
    path.ancestors()
        .find(|directory| {
            names.iter().any(|name| {
                snapshot
                    .read(&directory.join(name))
                    .ok()
                    .flatten()
                    .is_some()
            })
        })
        .map(Path::to_path_buf)
}

fn module_name(project_root: &Path, module_root: &Path, modules: &[(String, PathBuf)]) -> String {
    let normalized_root = normalize_path(module_root);
    if let Some((module, _)) = modules
        .iter()
        .find(|(_, path)| normalize_path(path) == normalized_root)
    {
        return module.clone();
    }
    if normalize_path(project_root) == normalized_root {
        return ":".to_owned();
    }
    module_root
        .strip_prefix(project_root)
        .ok()
        .map(|relative| {
            relative
                .components()
                .filter_map(|part| part.as_os_str().to_str())
                .collect::<Vec<_>>()
        })
        .filter(|parts| !parts.is_empty())
        .map(|parts| format!(":{}", parts.join(":")))
        .unwrap_or_else(|| ":".to_owned())
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

pub(super) fn read_text(snapshot: &Snapshot, path: &Path) -> Option<String> {
    snapshot
        .read(path)
        .ok()
        .flatten()
        .and_then(|bytes| String::from_utf8(bytes.to_vec()).ok())
}

pub(super) fn project_dependencies(
    root: &Path,
    snapshot: &Snapshot,
    cache: &mut ProjectCache,
) -> BTreeMap<String, BTreeSet<build::ProjectDependency>> {
    let mut dependencies = BTreeMap::<String, BTreeSet<build::ProjectDependency>>::new();
    let mut module_roots = vec![(":".to_owned(), root.to_path_buf())];
    module_roots.extend_from_slice(cache.modules(root, snapshot));
    for (module, path) in module_roots {
        let mut deps = BTreeSet::new();
        for name in ["build.gradle.kts", "build.gradle"] {
            deps.extend(
                cache
                    .script(path.join(name), snapshot)
                    .dependencies
                    .iter()
                    .cloned(),
            );
        }
        dependencies.insert(module, deps);
    }
    dependencies
}

pub(super) fn source_set_reaches(
    caller: &str,
    target: &str,
    edges: &BTreeMap<String, BTreeSet<String>>,
) -> bool {
    let mut pending = vec![caller.to_owned()];
    let mut seen = HashSet::new();
    while let Some(current) = pending.pop() {
        if current == target {
            return true;
        }
        if !seen.insert(current.clone()) {
            continue;
        }
        pending.extend(edges.get(&current).into_iter().flatten().cloned());
    }
    false
}

pub(super) fn conventional_source_edge(caller: &str, target: &str) -> bool {
    if caller == "commonTest" && target == "commonMain" {
        return true;
    }
    if caller.ends_with("Test") {
        let main = caller.trim_end_matches("Test").to_owned() + "Main";
        return target == main || target == "commonTest" || target == "commonMain";
    }
    caller.ends_with("Main") && target == "commonMain"
}

pub(super) fn transitive_closure(
    root: &str,
    graph: &BTreeMap<String, BTreeSet<build::ProjectDependency>>,
    source_set: Option<&str>,
) -> BTreeSet<String> {
    let mut result = BTreeSet::new();
    let mut pending = graph
        .get(root)
        .into_iter()
        .flatten()
        .filter(|dependency| {
            dependency_visible_in_source_set(&dependency.scope, source_set)
                && dependency_compile_visible(&dependency.exposure)
        })
        .map(|dependency| dependency.path.clone())
        .collect::<Vec<_>>();
    while let Some(item) = pending.pop() {
        if result.insert(item.clone()) {
            pending.extend(
                graph
                    .get(&item)
                    .into_iter()
                    .flatten()
                    .filter(|dependency| {
                        dependency_visible_in_source_set(&dependency.scope, source_set)
                            && dependency.exposure == build::DependencyExposure::Api
                    })
                    .map(|dependency| dependency.path.clone()),
            );
        }
    }
    result
}

fn dependency_compile_visible(exposure: &build::DependencyExposure) -> bool {
    matches!(
        exposure,
        build::DependencyExposure::Api
            | build::DependencyExposure::Implementation
            | build::DependencyExposure::CompileOnly
    )
}

fn dependency_visible_in_source_set(
    scope: &build::DependencyScope,
    source_set: Option<&str>,
) -> bool {
    match scope {
        build::DependencyScope::Production => true,
        build::DependencyScope::Test(test_source_set) => {
            source_set.is_some_and(|set| set == test_source_set)
        }
        build::DependencyScope::Variant(variant) => source_set.is_some_and(|set| set == variant),
        build::DependencyScope::Other => false,
    }
}

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
                .entry(candidate.name.to_owned())
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
    symbol: &'a crate::graph::ProgramSymbol,
    graph: &'a ProgramGraph<'a>,
) -> Option<Candidate<'a>> {
    let context_id = symbol.scope.context_id;
    let semantic = graph.semantic(file, context_id, Some(ResolutionFamily::KotlinModules))?;
    let definition = graph.definition(symbol.id)?;
    let package = semantic
        .context
        .as_ref()
        .map_or(definition.module.0.as_slice(), |context| {
            context.package.as_slice()
        });
    let declaration = graph.declaration(
        file,
        context_id,
        definition.start_byte,
        Some(ResolutionFamily::KotlinModules),
    );
    Some(Candidate {
        candidate_index,
        graph_id: symbol.id,
        symbol: &symbol.semantic_id,
        file,
        context_id,
        name: &symbol.name,
        package,
        owner: declaration.map_or(&[][..], |declaration| declaration.owner.as_slice()),
        kind: symbol.kind.clone(),
        declaration,
        public: symbol.is_public,
        start: definition.start_byte,
        end: definition.end_byte,
        scope_start: definition.scope_start,
        scope_end: definition.scope_end,
    })
}
