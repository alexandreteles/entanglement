//! Resolve Rust names across the analyzed files.
//!
//! The resolver uses Tree-sitter facts from each file. It does not infer types
//! or resolve method dispatch. It leaves uncertain names unresolved.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};

use crate::model::{
    Definition, DefinitionKind, FileFacts, Import, ModulePath, Reference, ReferenceAnalysis,
    ReferenceKind, Resolution, SymbolId,
};

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ModuleKey {
    crate_root: PathBuf,
    path: Vec<String>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Target {
    Symbol(SymbolId),
    Module(ModuleKey),
    External,
    Ambiguous(Vec<SymbolId>),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct FileContext {
    crate_root: PathBuf,
    module: Vec<String>,
}

#[derive(Clone, Debug)]
struct Symbol {
    id: SymbolId,
    file: usize,
    scope_start: usize,
    scope_end: usize,
    is_local: bool,
    is_public: bool,
}

#[derive(Clone, Debug)]
struct ImportEntry {
    file: usize,
    module: ModuleKey,
    name: String,
    path: Vec<String>,
    scope_start: usize,
    scope_end: usize,
    is_local: bool,
    is_public: bool,
}

struct Index {
    contexts: Vec<FileContext>,
    symbols: BTreeMap<ModuleKey, BTreeMap<String, Vec<Symbol>>>,
    modules: BTreeSet<ModuleKey>,
    imports: Vec<ImportEntry>,
    import_targets: Vec<Vec<Target>>,
}

/// Resolve references in all facts and store their results in each analysis.
///
/// Call this after all files have been parsed. The function sorts symbols and
/// ambiguity results to keep output stable. It leaves method calls and names
/// that cannot be resolved without guessing as `Unresolved`.
pub(crate) fn resolve(facts: &mut [FileFacts]) {
    let mut index = Index::new(facts);
    index.resolve_imports();
    for (file_index, fact) in facts.iter_mut().enumerate() {
        let context = &index.contexts[file_index];
        fact.analysis.resolution = fact
            .references
            .iter()
            .map(|reference| ReferenceAnalysis {
                path: reference.path.clone(),
                start_byte: reference.start_byte,
                end_byte: reference.end_byte,
                resolution: index.resolve_reference(file_index, context, reference, fact),
            })
            .collect();
    }
}

impl Index {
    fn new(facts: &[FileFacts]) -> Self {
        let contexts = file_contexts(facts);
        let mut index = Self {
            contexts,
            symbols: BTreeMap::new(),
            modules: BTreeSet::new(),
            imports: Vec::new(),
            import_targets: Vec::new(),
        };
        index.add_symbols(facts);
        index.add_imports(facts);
        index
    }

    fn add_symbols(&mut self, facts: &[FileFacts]) {
        for (file_index, fact) in facts.iter().enumerate() {
            let context = &self.contexts[file_index];
            let file_module = joined_path(&context.module, &fact.module.0);
            self.modules.insert(ModuleKey {
                crate_root: context.crate_root.clone(),
                path: Vec::new(),
            });
            if !file_module.is_empty() {
                self.modules.insert(ModuleKey {
                    crate_root: context.crate_root.clone(),
                    path: file_module.clone(),
                });
            }

            for definition in &fact.definitions {
                let module = joined_path(&file_module, &definition.module.0);
                let module_key = ModuleKey {
                    crate_root: context.crate_root.clone(),
                    path: module.clone(),
                };
                if definition.kind == DefinitionKind::Module {
                    let mut child = module_key.clone();
                    child.path.push(definition.name.clone());
                    self.modules.insert(module_key.clone());
                    self.modules.insert(child);
                }
                if !is_resolvable_kind(&definition.kind) {
                    continue;
                }
                let id = SymbolId {
                    file: fact.analysis.path.clone(),
                    module: ModulePath(module),
                    name: definition.name.clone(),
                    kind: definition.kind.clone(),
                    start_byte: definition.start_byte,
                };
                self.symbols
                    .entry(module_key)
                    .or_default()
                    .entry(definition.name.clone())
                    .or_default()
                    .push(Symbol {
                        id,
                        file: file_index,
                        scope_start: definition.scope_start,
                        scope_end: definition.scope_end,
                        is_local: is_local_definition(fact, definition),
                        is_public: definition.is_public,
                    });
            }
        }
        for names in self.symbols.values_mut() {
            for symbols in names.values_mut() {
                symbols.sort_by(|left, right| left.id.cmp(&right.id));
                symbols.dedup_by(|left, right| left.id == right.id);
            }
        }
    }

    fn add_imports(&mut self, facts: &[FileFacts]) {
        for (file_index, fact) in facts.iter().enumerate() {
            let context = &self.contexts[file_index];
            let file_module = joined_path(&context.module, &fact.module.0);
            for import in &fact.imports {
                let Some((name, path)) = import_name_and_path(import) else {
                    continue;
                };
                self.imports.push(ImportEntry {
                    file: file_index,
                    module: ModuleKey {
                        crate_root: context.crate_root.clone(),
                        path: joined_path(&file_module, &import.module.0),
                    },
                    name,
                    path,
                    scope_start: import.scope_start,
                    scope_end: import.scope_end,
                    is_local: is_local_scope(
                        fact,
                        import.scope_start,
                        import.scope_end,
                        import.start_byte,
                    ),
                    is_public: import.is_public,
                });
            }
        }
        self.imports.sort_by(|left, right| {
            (
                &left.module,
                &left.name,
                left.file,
                left.scope_start,
                &left.path,
            )
                .cmp(&(
                    &right.module,
                    &right.name,
                    right.file,
                    right.scope_start,
                    &right.path,
                ))
        });
        self.import_targets = vec![Vec::new(); self.imports.len()];
    }

    fn resolve_imports(&mut self) {
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

    fn resolve_reference(
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
            if targets
                .iter()
                .any(|target| matches!(target, Target::Ambiguous(_) | Target::External))
            {
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

    fn lookup_name(
        &self,
        file: usize,
        access_module: &ModuleKey,
        position: usize,
        module: &ModuleKey,
        name: &str,
    ) -> Vec<Target> {
        let mut choices = self
            .symbols
            .get(module)
            .and_then(|names| names.get(name))
            .into_iter()
            .flatten()
            .filter_map(|symbol| {
                if !symbol.is_public && !can_access_private(module, access_module) {
                    return None;
                }
                let rank = if symbol.is_local {
                    if symbol.file != file
                        || position < symbol.scope_start
                        || position > symbol.scope_end
                    {
                        return None;
                    }
                    symbol.scope_end.saturating_sub(symbol.scope_start)
                } else {
                    usize::MAX
                };
                Some((rank, self.symbol_target(module, name, symbol)))
            })
            .collect::<Vec<_>>();

        for (import_index, import) in self.imports.iter().enumerate() {
            if import.module != *module || import.name != name {
                continue;
            }
            let rank = if import.file == file
                && import.scope_start <= position
                && position <= import.scope_end
            {
                if import.is_local {
                    import.scope_end.saturating_sub(import.scope_start)
                } else {
                    usize::MAX
                }
            } else if !import.is_local
                && (import.is_public || can_access_private(&import.module, access_module))
            {
                usize::MAX
            } else {
                continue;
            };
            choices.extend(
                self.import_targets[import_index]
                    .iter()
                    .cloned()
                    .map(|target| (rank, target)),
            );
        }

        let Some(best_rank) = choices.iter().map(|(rank, _)| *rank).min() else {
            return Vec::new();
        };
        unique_targets(
            choices
                .into_iter()
                .filter_map(|(rank, target)| (rank == best_rank).then_some(target))
                .collect(),
        )
    }

    fn symbol_target(&self, module: &ModuleKey, name: &str, symbol: &Symbol) -> Target {
        if symbol.id.kind != DefinitionKind::Module {
            return Target::Symbol(symbol.id.clone());
        }
        let ids = self
            .symbols
            .get(module)
            .and_then(|names| names.get(name))
            .into_iter()
            .flatten()
            .filter(|candidate| candidate.id.kind == DefinitionKind::Module)
            .map(|candidate| candidate.id.clone())
            .collect::<Vec<_>>();
        let ids = unique_symbols(ids);
        if ids.len() > 1 {
            return Target::Ambiguous(ids);
        }
        let mut child = module.clone();
        child.path.push(name.to_owned());
        if self.modules.contains(&child) {
            Target::Module(child)
        } else {
            Target::Symbol(symbol.id.clone())
        }
    }

    fn to_resolution(&self, targets: Vec<Target>) -> Resolution {
        let targets = unique_targets(targets)
            .into_iter()
            .map(|target| match target {
                Target::Module(module) => self
                    .module_symbol(&module)
                    .map(Target::Symbol)
                    .unwrap_or(Target::Module(module)),
                target => target,
            })
            .collect::<Vec<_>>();
        match targets.as_slice() {
            [Target::Symbol(symbol)] => Resolution::Exact(symbol.clone()),
            [Target::Module(_)] => Resolution::Unresolved,
            [Target::External] => Resolution::External,
            [Target::Ambiguous(symbols)] => Resolution::Ambiguous(symbols.clone()),
            [] => Resolution::Unresolved,
            _ if targets
                .iter()
                .all(|target| matches!(target, Target::Symbol(_) | Target::Ambiguous(_))) =>
            {
                let symbols = targets.iter().flat_map(target_symbols).collect();
                Resolution::Ambiguous(unique_symbols(symbols))
            }
            _ => Resolution::Unresolved,
        }
    }

    fn module_symbol(&self, module: &ModuleKey) -> Option<SymbolId> {
        let name = module.path.last()?;
        let parent = ModuleKey {
            crate_root: module.crate_root.clone(),
            path: module.path[..module.path.len() - 1].to_vec(),
        };
        let symbols = self
            .symbols
            .get(&parent)?
            .get(name)?
            .iter()
            .filter(|symbol| symbol.id.kind == DefinitionKind::Module)
            .map(|symbol| symbol.id.clone())
            .collect::<Vec<_>>();
        match unique_symbols(symbols).as_slice() {
            [symbol] => Some(symbol.clone()),
            _ => None,
        }
    }
}

fn file_contexts(facts: &[FileFacts]) -> Vec<FileContext> {
    let by_path = facts.iter().enumerate().fold(
        BTreeMap::<PathBuf, Vec<usize>>::new(),
        |mut paths, (index, fact)| {
            paths.entry(fact.path.clone()).or_default().push(index);
            paths
        },
    );
    let mut roots = facts
        .iter()
        .enumerate()
        .filter(|(_, fact)| is_crate_root(&fact.path))
        .map(|(index, fact)| (fact.path.clone(), index))
        .collect::<Vec<_>>();
    roots.sort_by(|left, right| left.0.cmp(&right.0));

    let mut assignments = vec![BTreeSet::new(); facts.len()];
    let mut pending = VecDeque::new();
    for (root_path, index) in roots {
        pending.push_back((
            index,
            FileContext {
                crate_root: root_path,
                module: Vec::new(),
            },
        ));
    }
    let mut visited = BTreeSet::new();
    walk_module_graph(
        facts,
        &by_path,
        &mut pending,
        &mut visited,
        &mut assignments,
    );

    let mut incoming = BTreeSet::new();
    for fact in facts {
        for declaration in fact.definitions.iter().filter(|definition| {
            definition.kind == DefinitionKind::Module && definition.external_module
        }) {
            let relative_module = joined_path(&fact.module.0, &declaration.module.0);
            let targets = module_files(&fact.path, &relative_module, &declaration.name)
                .into_iter()
                .filter_map(|path| by_path.get(&path))
                .flatten()
                .copied()
                .collect::<BTreeSet<_>>();
            if targets.len() == 1 {
                incoming.insert(*targets.first().expect("One target is present"));
            }
        }
    }

    for index in 0..facts.len() {
        if assignments[index].is_empty() && !incoming.contains(&index) {
            pending.push_back((
                index,
                FileContext {
                    crate_root: facts[index].path.clone(),
                    module: Vec::new(),
                },
            ));
        }
    }
    walk_module_graph(
        facts,
        &by_path,
        &mut pending,
        &mut visited,
        &mut assignments,
    );

    facts
        .iter()
        .enumerate()
        .map(
            |(index, fact)| match assignments[index].iter().collect::<Vec<_>>().as_slice() {
                [context] => (*context).clone(),
                _ => FileContext {
                    crate_root: fact.path.clone(),
                    module: Vec::new(),
                },
            },
        )
        .collect()
}

fn walk_module_graph(
    facts: &[FileFacts],
    by_path: &BTreeMap<PathBuf, Vec<usize>>,
    pending: &mut VecDeque<(usize, FileContext)>,
    visited: &mut BTreeSet<(usize, FileContext)>,
    assignments: &mut [BTreeSet<FileContext>],
) {
    while let Some((file, context)) = pending.pop_front() {
        if !visited.insert((file, context.clone())) {
            continue;
        }
        assignments[file].insert(context.clone());
        let file_module = joined_path(&context.module, &facts[file].module.0);
        for declaration in facts[file].definitions.iter().filter(|definition| {
            definition.kind == DefinitionKind::Module && definition.external_module
        }) {
            let module = joined_path(&file_module, &declaration.module.0);
            let child_module = joined_path(&module, std::slice::from_ref(&declaration.name));
            let relative_module = joined_path(&facts[file].module.0, &declaration.module.0);
            let candidates = module_files(&facts[file].path, &relative_module, &declaration.name)
                .into_iter()
                .filter_map(|path| by_path.get(&path))
                .flatten()
                .copied()
                .collect::<BTreeSet<_>>();
            if candidates.len() == 1 {
                let child_file = *candidates.first().expect("One candidate is present");
                pending.push_back((
                    child_file,
                    FileContext {
                        crate_root: context.crate_root.clone(),
                        module: child_module,
                    },
                ));
            }
        }
    }
}

fn module_files(file: &Path, inline_module: &[String], name: &str) -> Vec<PathBuf> {
    let Some(parent) = file.parent() else {
        return Vec::new();
    };
    let base = if file.file_name().and_then(|file_name| file_name.to_str()) == Some("mod.rs") {
        parent.to_path_buf()
    } else if file.extension().is_some_and(|extension| extension == "rs")
        && !file
            .file_name()
            .and_then(|file_name| file_name.to_str())
            .is_some_and(|file_name| matches!(file_name, "lib.rs" | "main.rs"))
    {
        file.file_stem()
            .map_or_else(|| parent.to_path_buf(), |stem| parent.join(stem))
    } else {
        parent.to_path_buf()
    };
    let base = inline_module
        .iter()
        .fold(base, |path, part| path.join(part));
    vec![
        base.join(format!("{name}.rs")),
        base.join(name).join("mod.rs"),
    ]
}

fn is_crate_root(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name, "lib.rs" | "main.rs"))
}

fn import_name_and_path(import: &Import) -> Option<(String, Vec<String>)> {
    let mut path = import.path.clone();
    if path.is_empty() || path.last().is_some_and(|part| part == "*") {
        return None;
    }
    if path.last().is_some_and(|part| part == "self") && path.len() > 1 {
        path.pop();
    }
    let name = import.alias.clone().or_else(|| path.last().cloned())?;
    Some((name, path))
}

fn is_local_definition(fact: &FileFacts, definition: &Definition) -> bool {
    is_local_scope(
        fact,
        definition.scope_start,
        definition.scope_end,
        definition.start_byte,
    )
}

fn is_local_scope(fact: &FileFacts, scope_start: usize, scope_end: usize, position: usize) -> bool {
    fact.analysis.functions.iter().any(|function| {
        function.start_byte <= scope_start
            && scope_end <= function.end_byte
            && (function.start_byte < scope_start
                || scope_end < function.end_byte
                || (function.start_byte < position && position < function.end_byte))
    })
}

fn can_access_private(definition: &ModuleKey, access: &ModuleKey) -> bool {
    definition.crate_root == access.crate_root && access.path.starts_with(&definition.path)
}

fn is_resolvable_kind(kind: &DefinitionKind) -> bool {
    !matches!(kind, DefinitionKind::Method | DefinitionKind::Other)
}

fn is_external_root(name: &str) -> bool {
    matches!(name, "std" | "core" | "alloc")
}

fn joined_path(base: &[String], relative: &[String]) -> Vec<String> {
    base.iter().chain(relative).cloned().collect()
}

fn unique_targets(mut targets: Vec<Target>) -> Vec<Target> {
    targets.sort();
    targets.dedup();
    targets
}

fn target_symbols(target: &Target) -> Vec<SymbolId> {
    match target {
        Target::Symbol(symbol) => vec![symbol.clone()],
        Target::Ambiguous(symbols) => symbols.clone(),
        Target::Module(_) | Target::External => Vec::new(),
    }
}

fn unique_symbols(mut symbols: Vec<SymbolId>) -> Vec<SymbolId> {
    symbols.sort();
    symbols.dedup();
    symbols
}
