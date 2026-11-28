use crate::graph::FileFactsView;
use crate::languages::csharp::{
    facts::{Binding, Decl, Usage, Using},
    project::{self, Project},
};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Range;
use std::path::Path;

#[derive(Clone, Copy)]
pub(super) struct Symbol<'a> {
    pub file: usize,
    pub decl: &'a Decl,
}
#[derive(Clone, Copy)]
pub(super) struct Site<'a> {
    pub file: usize,
    pub view: Option<usize>,
    pub at: usize,
    pub owner: &'a str,
    pub namespace: &'a str,
}
#[derive(Default)]
pub(super) struct Source<'a> {
    pub errors: Vec<&'a Range<usize>>,
    pub uses: BTreeMap<(usize, usize), &'a Usage>,
    pub bindings: HashMap<&'a str, Vec<&'a Binding>>,
    pub imports: Vec<&'a Using>,
    pub loads: BTreeSet<usize>,
}

impl Source<'_> {
    pub fn has_error(&self, range: &Range<usize>) -> bool {
        self.errors.iter().any(|error| {
            range.contains(&error.start) || error.start < range.start && range.start < error.end
        })
    }
}
pub(super) struct Index<'a> {
    pub files: &'a [FileFactsView<'a>],
    pub project: &'a Project,
    pub sources: Vec<Source<'a>>,
    pub types: HashMap<String, Vec<Symbol<'a>>>,
    pub members: HashMap<&'a str, HashMap<&'a str, Vec<Symbol<'a>>>>,
    pub extensions: HashMap<&'a str, Vec<Symbol<'a>>>,
    pub globals: HashMap<usize, Vec<(usize, &'a Using)>>,
    pub locations: HashMap<(&'a str, usize), (usize, usize, &'a Decl)>,
}

impl<'a> Index<'a> {
    pub fn new(files: &'a [FileFactsView<'a>], project: &'a Project) -> Self {
        let paths: HashMap<_, _> = files
            .iter()
            .enumerate()
            .map(|(i, f)| (f.target.as_path(), i))
            .collect();
        let mut result = Self {
            files,
            project,
            sources: (0..files.len()).map(|_| Source::default()).collect(),
            types: HashMap::new(),
            members: HashMap::new(),
            extensions: HashMap::new(),
            globals: HashMap::new(),
            locations: HashMap::new(),
        };
        for (index, file) in files.iter().enumerate() {
            for (context, data) in file.semantic().filter_map(|s| {
                s.semantic
                    .csharp
                    .as_deref()
                    .map(|d| (s.semantic.context_id, d))
            }) {
                result.sources[index].errors.extend(&data.errors);
                for import in &data.imports {
                    result.sources[index].imports.push(import);
                    if import.global {
                        for owner in project.owners(file.target) {
                            result
                                .globals
                                .entry(*owner)
                                .or_default()
                                .push((index, import));
                        }
                    }
                }
                for binding in &data.bindings {
                    result.sources[index]
                        .bindings
                        .entry(&binding.name)
                        .or_default()
                        .push(binding);
                }
                result.sources[index]
                    .uses
                    .extend(data.uses.iter().map(|(k, v)| (*k, v)));
                for directive in data.script.iter().filter(|d| d.load) {
                    let relative = file
                        .target
                        .parent()
                        .unwrap_or(Path::new(""))
                        .join(directive.path.replace('\\', "/"));
                    if let Ok(key) = crate::input::path_identity(&relative)
                        && let Some(loaded) = paths.get(key.as_path())
                    {
                        result.sources[index].loads.insert(*loaded);
                    }
                }
                for decl in data.declarations.values() {
                    let symbol = Symbol { file: index, decl };
                    result.locations.insert(
                        (file.analysis.path.as_str(), decl.range.start),
                        (index, context, decl),
                    );
                    if decl.is_type {
                        result
                            .types
                            .entry(decl.qualified())
                            .or_default()
                            .push(symbol);
                    } else {
                        result
                            .members
                            .entry(&decl.owner)
                            .or_default()
                            .entry(&decl.name)
                            .or_default()
                            .push(symbol);
                        if decl.extension.is_some() {
                            result
                                .extensions
                                .entry(&decl.name)
                                .or_default()
                                .push(symbol);
                        }
                    }
                }
            }
        }
        let direct = result
            .sources
            .iter()
            .map(|s| s.loads.clone())
            .collect::<Vec<_>>();
        for source in &mut result.sources {
            let mut pending = source.loads.iter().copied().collect::<Vec<_>>();
            while let Some(item) = pending.pop() {
                for other in &direct[item] {
                    if source.loads.insert(*other) {
                        pending.push(*other);
                    }
                }
            }
        }
        result
    }

    pub fn visible(&self, site: Site<'_>, symbol: Symbol<'_>) -> bool {
        let target = self.files[symbol.file].target;
        let decl = symbol.decl;
        let header_end = decl.body.as_ref().map_or(decl.range.end, |body| body.start);
        if self.sources[symbol.file].has_error(&(decl.range.start..header_end)) {
            return false;
        }
        if decl.visibility == "file" && symbol.file != site.file {
            return false;
        }
        if project::extension(self.files[site.file].target, "csx")
            || project::extension(target, "csx")
        {
            return site.file == symbol.file
                || self.sources[site.file].loads.contains(&symbol.file);
        }
        let Some(view) = site.view else {
            return if self.project.views.is_empty() {
                true
            } else {
                site.file == symbol.file
            };
        };
        self.project.owners(target).iter().any(|candidate| {
            self.project.reachable[view].contains(candidate)
                && self.project.active(target, *candidate, decl.range.start)
                && (self.project.views[*candidate].project == self.project.views[view].project
                    || !matches!(decl.visibility.as_str(), "internal" | "private protected"))
        })
    }

    pub fn binding(&self, site: Site<'_>, name: &str) -> Option<&'a Binding> {
        self.sources[site.file]
            .bindings
            .get(name)?
            .iter()
            .copied()
            .filter(|b| {
                b.position <= site.at
                    && b.scope.contains(&site.at)
                    && site.view.is_none_or(|v| {
                        self.project
                            .active(self.files[site.file].target, v, b.position)
                    })
            })
            .min_by_key(|b| (b.scope.len(), std::cmp::Reverse(b.position)))
    }

    pub fn imports(&self, site: Site<'_>) -> Vec<(&str, &str, bool)> {
        let mut imports = self.sources[site.file]
            .imports
            .iter()
            .filter(|u| {
                u.scope.contains(&site.at)
                    && (u.namespace.is_empty()
                        || site.namespace == u.namespace
                        || site.namespace.starts_with(&format!("{}.", u.namespace)))
                    && site.view.is_none_or(|v| {
                        self.project
                            .active(self.files[site.file].target, v, u.position)
                    })
            })
            .map(|u| (u.alias.as_str(), u.target.as_str(), u.is_static))
            .collect::<Vec<_>>();
        if let Some(view) = site.view {
            imports.extend(
                self.globals
                    .get(&view)
                    .into_iter()
                    .flatten()
                    .filter(|(file, u)| {
                        self.project
                            .active(self.files[*file].target, view, u.position)
                    })
                    .map(|(_, u)| (u.alias.as_str(), u.target.as_str(), u.is_static)),
            );
            imports.extend(
                self.project.views[view]
                    .usings
                    .iter()
                    .map(|(a, t, s)| (a.as_str(), t.as_str(), *s)),
            );
        }
        imports
    }
}
