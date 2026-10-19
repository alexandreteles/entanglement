use super::*;

pub(crate) fn project_dependencies(
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

pub(crate) fn source_set_reaches(
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

pub(crate) fn conventional_source_edge(caller: &str, target: &str) -> bool {
    if caller == "commonTest" && target == "commonMain" {
        return true;
    }
    if caller.ends_with("Test") {
        let main = caller.trim_end_matches("Test").to_owned() + "Main";
        return target == main || target == "commonTest" || target == "commonMain";
    }
    caller.ends_with("Main") && target == "commonMain"
}

pub(crate) fn transitive_closure(
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
        build::DependencyScope::Test(name) | build::DependencyScope::Variant(name) => {
            source_set.is_some_and(|set| set == name)
        }
        build::DependencyScope::Other => false,
    }
}
