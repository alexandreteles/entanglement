use super::*;

#[derive(Clone, Debug, Default)]
pub(crate) struct SourceContext {
    pub(crate) path: PathBuf,
    pub(crate) project_root: PathBuf,
    pub(crate) module_root: PathBuf,
    /// Project-unique identity used to prevent unrelated roots colliding.
    pub(crate) module: String,
    /// Gradle's module coordinate relative to project_root (for example `:lib`).
    pub(crate) gradle_module: String,
    pub(crate) source_set: Option<String>,
    pub(crate) isolated_source: bool,
}

/// Project evidence belongs to one resolution snapshot, never to a global cache.
#[derive(Default)]
pub(in crate::resolver) struct ProjectCache {
    modules: HashMap<PathBuf, Vec<(String, PathBuf)>>,
    scripts: HashMap<PathBuf, build::BuildFacts>,
}

impl ProjectCache {
    pub(super) fn modules(&mut self, root: &Path, snapshot: &Snapshot) -> &[(String, PathBuf)] {
        self.modules.entry(root.to_path_buf()).or_insert_with(|| {
            let (source, kind) = ["settings.gradle.kts", "settings.gradle"]
                .iter()
                .find_map(|name| {
                    read_text(snapshot, &root.join(name))
                        .map(|source| (source, build::ScriptKind::for_path(Path::new(name))))
                })
                .unwrap_or_else(|| (String::new(), build::ScriptKind::Kotlin));
            let settings = build::parse(&source, kind).0;
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
                build::parse(&source, build::ScriptKind::for_path(&path)).1
            })
        })
    }
}

pub(crate) fn source_context(
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

fn read_text(snapshot: &Snapshot, path: &Path) -> Option<String> {
    snapshot
        .read(path)
        .ok()
        .flatten()
        .and_then(|bytes| String::from_utf8(bytes.to_vec()).ok())
}
