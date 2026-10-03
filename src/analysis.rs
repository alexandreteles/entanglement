//! Parse source files and derive language-independent metrics.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use rayon::prelude::*;
use tree_sitter::{InputEdit, Parser, Range as TsRange, Tree};

use crate::Result;
use crate::languages::{CapturedTree, InjectionRequest, LanguageChoice, Registry};
use crate::metrics::{
    SyntaxEvent, SyntaxRole,
    cyclomatic::{self, FunctionScope},
    cyclomatic_density, nloc,
};
use crate::model::{
    Definition, FileAnalysis, FileFacts, FunctionAnalysis, Import, LocalBinding, ModulePath,
    Reference,
};

const MAX_INJECTION_DEPTH: usize = 64;

/// Keep parser state for one file task and reuse it for later files.
pub(crate) struct Worker {
    registry: Registry,
    parsers: HashMap<String, Parser>,
}

/// Keep a parsed file so a later patch can reuse its syntax trees.
pub(crate) struct ParsedFile {
    pub source: Vec<u8>,
    pub language: String,
    pub tree: Tree,
    pub injections: Vec<ParsedInjection>,
    pub facts: FileFacts,
}

/// Keep one injected tree and its nested injected trees.
pub(crate) struct ParsedInjection {
    language: String,
    language_id: Option<String>,
    range: Range<usize>,
    tree: Option<Tree>,
    children: Vec<ParsedInjection>,
}

struct TreeSummary {
    language_id: String,
    events: Vec<SyntaxEvent>,
    parents: HashMap<usize, Option<usize>>,
    functions: Vec<FunctionScope>,
    excluded: Vec<Range<usize>>,
    definitions: Vec<Definition>,
    imports: Vec<Import>,
    references: Vec<Reference>,
    locals: Vec<LocalBinding>,
}

type TreeKey = (String, usize, usize);

impl Worker {
    /// Create a worker with the registered grammars and empty parser state.
    pub fn new() -> Result<Self> {
        Ok(Self {
            registry: Registry::new()?,
            parsers: HashMap::new(),
        })
    }

    /// Return whether Tree-sitter has an analyzer for this file.
    pub fn supports(&mut self, path: &Path, source: &[u8]) -> Result<bool> {
        Ok(self.registry.select_file(path, source)?.is_some())
    }

    /// Parse a file and derive metrics and resolution facts.
    ///
    /// `previous` may hold the prior parse of this file. Apply each `InputEdit`
    /// to its trees in order before the parser reuses them. The method does not
    /// write source bytes. It returns an error when no registered analyzer can
    /// parse `path`, or when Tree-sitter cannot parse an edited tree.
    pub fn analyze_source(
        &mut self,
        path: &Path,
        source: &[u8],
        previous: Option<&ParsedFile>,
        edits: &[InputEdit],
    ) -> Result<ParsedFile> {
        let choice = self
            .registry
            .select_file(path, source)?
            .ok_or_else(|| format!("No registered grammar supports {}", path.display()))?;
        let reusable = previous.filter(|file| {
            file.language == choice.id && (!edits.is_empty() || file.source == source)
        });
        let mut old_trees = BTreeMap::new();
        if let Some(previous) = reusable {
            collect_old_trees(&previous.injections, edits, &mut old_trees);
        }
        let old_root = reusable.map(|file| edit_tree(file.tree.clone(), edits));
        let tree = self.parse_tree(&choice, source, &[], old_root.as_ref())?;
        let mut active = HashSet::from([(choice.id.clone(), 0, source.len())]);
        let mut next_node_id = 0;
        let (injections, summaries) = self.analyze_layer(
            &choice,
            &tree,
            source,
            0..source.len(),
            &ModulePath::default(),
            None,
            &mut active,
            &mut next_node_id,
            &mut old_trees,
            0,
        )?;
        let facts = build_facts(path, source, &choice, summaries, &injections);
        Ok(ParsedFile {
            source: source.to_vec(),
            language: choice.id,
            tree,
            injections,
            facts,
        })
    }

    fn parse_tree(
        &mut self,
        choice: &LanguageChoice,
        source: &[u8],
        included_ranges: &[TsRange],
        old_tree: Option<&Tree>,
    ) -> Result<Tree> {
        let parser = self.parsers.entry(choice.id.clone()).or_default();
        parser.set_language(&choice.language)?;
        parser.set_included_ranges(included_ranges)?;
        parser
            .parse(source, old_tree)
            .ok_or_else(|| format!("Tree-sitter canceled parsing {}", choice.name).into())
    }

    #[allow(clippy::too_many_arguments)]
    fn analyze_layer(
        &mut self,
        choice: &LanguageChoice,
        tree: &Tree,
        source: &[u8],
        owner_range: Range<usize>,
        inherited_module: &ModulePath,
        inherited_scope: Option<Range<usize>>,
        active: &mut HashSet<(String, usize, usize)>,
        next_node_id: &mut usize,
        old_trees: &mut BTreeMap<TreeKey, Tree>,
        depth: usize,
    ) -> Result<(Vec<ParsedInjection>, Vec<TreeSummary>)> {
        let mut captured = self.registry.capture(&choice.id, tree, source)?;
        remap_node_ids(&mut captured, next_node_id);
        let requests = selected_requests(std::mem::take(&mut captured.injections));
        let excluded = requests
            .iter()
            .map(|item| item.range.clone())
            .collect::<Vec<_>>();
        let child_modules = requests
            .iter()
            .map(|request| {
                inherited_module_for_range(inherited_module, &captured.definitions, &request.range)
            })
            .collect::<Vec<_>>();
        let mut summaries = vec![summary(
            choice,
            &captured,
            excluded,
            inherited_module,
            &owner_range,
            inherited_scope.as_ref(),
        )];
        let mut injections = Vec::with_capacity(requests.len());

        for (request, child_module) in requests.into_iter().zip(child_modules) {
            let Some(child_choice) = self.registry.select_injection(&request.language)? else {
                injections.push(ParsedInjection {
                    language: request.language,
                    language_id: None,
                    range: request.range,
                    tree: None,
                    children: Vec::new(),
                });
                continue;
            };
            let key = (
                child_choice.id.clone(),
                request.range.start,
                request.range.end,
            );
            if depth >= MAX_INJECTION_DEPTH
                || !is_smaller(&owner_range, &request.range)
                || active.contains(&key)
            {
                injections.push(ParsedInjection {
                    language: request.language,
                    language_id: Some(child_choice.id),
                    range: request.range,
                    tree: None,
                    children: Vec::new(),
                });
                continue;
            }
            let included_ranges = injection_ranges(tree, &request);
            if included_ranges.is_empty() {
                injections.push(ParsedInjection {
                    language: request.language,
                    language_id: Some(child_choice.id),
                    range: request.range,
                    tree: None,
                    children: Vec::new(),
                });
                continue;
            }
            let old_tree = old_trees.remove(&key);
            let child_tree =
                self.parse_tree(&child_choice, source, &included_ranges, old_tree.as_ref())?;
            let child_scope = enclosing_function(&captured.functions, &request.range)
                .or_else(|| inherited_scope.clone());
            active.insert(key.clone());
            let (children, child_summaries) = self.analyze_layer(
                &child_choice,
                &child_tree,
                source,
                request.range.clone(),
                &child_module,
                child_scope,
                active,
                next_node_id,
                old_trees,
                depth + 1,
            )?;
            active.remove(&key);
            summaries.extend(child_summaries);
            injections.push(ParsedInjection {
                language: request.language,
                language_id: Some(child_choice.id),
                range: request.range,
                tree: Some(child_tree),
                children,
            });
        }

        Ok((injections, summaries))
    }
}

/// Parse supported files in parallel, with one reusable worker per Rayon task.
pub(crate) fn analyze_paths(paths: &[PathBuf]) -> Result<Vec<ParsedFile>> {
    let parsed = paths
        .par_iter()
        .map_init(
            || Worker::new().map_err(|error| error.to_string()),
            |worker, path| match worker {
                Ok(worker) => worker.analyze_path(path),
                Err(error) => Err(error.clone().into()),
            },
        )
        .collect::<Result<Vec<_>>>()?;
    Ok(parsed.into_iter().flatten().collect())
}

impl Worker {
    fn analyze_path(&mut self, path: &Path) -> Result<Option<ParsedFile>> {
        let source = fs::read(path)?;
        if !self.supports(path, &source)? {
            return Ok(None);
        }
        self.analyze_source(path, &source, None, &[]).map(Some)
    }
}

fn summary(
    choice: &LanguageChoice,
    captured: &CapturedTree,
    excluded: Vec<Range<usize>>,
    inherited_module: &ModulePath,
    owner_range: &Range<usize>,
    inherited_scope: Option<&Range<usize>>,
) -> TreeSummary {
    let prefix = &inherited_module.0;
    let mut definitions = captured.definitions.clone();
    definitions.retain(|item| !inside_any(item.start_byte..item.end_byte, &excluded));
    for definition in &mut definitions {
        inherit_scope(
            &mut definition.scope_start,
            &mut definition.scope_end,
            owner_range,
            inherited_scope,
        );
        prepend_module(&mut definition.module, prefix);
    }
    let mut imports = captured.imports.clone();
    imports.retain(|item| !inside_any(item.start_byte..item.end_byte, &excluded));
    for import in &mut imports {
        inherit_scope(
            &mut import.scope_start,
            &mut import.scope_end,
            owner_range,
            inherited_scope,
        );
        prepend_module(&mut import.module, prefix);
    }
    let mut references = captured.references.clone();
    references.retain(|item| !inside_any(item.start_byte..item.end_byte, &excluded));
    for reference in &mut references {
        inherit_scope(
            &mut reference.scope_start,
            &mut reference.scope_end,
            owner_range,
            inherited_scope,
        );
        prepend_module(&mut reference.module, prefix);
    }
    let mut locals = captured.locals.clone();
    locals.retain(|item| !inside_any(item.start_byte..item.end_byte, &excluded));
    for local in &mut locals {
        inherit_scope(
            &mut local.scope_start,
            &mut local.scope_end,
            owner_range,
            inherited_scope,
        );
    }
    let functions = captured
        .functions
        .iter()
        .filter(|item| !inside_any(item.range.clone(), &excluded))
        .cloned()
        .collect();
    TreeSummary {
        language_id: choice.id.clone(),
        events: captured.events.clone(),
        parents: captured.parents.clone(),
        functions,
        excluded,
        definitions,
        imports,
        references,
        locals,
    }
}

fn build_facts(
    path: &Path,
    source: &[u8],
    root_choice: &LanguageChoice,
    summaries: Vec<TreeSummary>,
    parsed_injections: &[ParsedInjection],
) -> FileFacts {
    let mut file_rows = BTreeSet::new();
    for item in &summaries {
        file_rows.extend(nloc::rows(&item.events, 0..source.len(), &item.excluded));
    }

    let mut by_language = BTreeMap::<String, Vec<usize>>::new();
    let mut functions = BTreeMap::<(usize, usize, String, String), FunctionScope>::new();
    for (index, item) in summaries.iter().enumerate() {
        by_language
            .entry(item.language_id.clone())
            .or_default()
            .push(index);
        for function in &item.functions {
            functions
                .entry((
                    function.range.start,
                    function.range.end,
                    function.name.clone(),
                    item.language_id.clone(),
                ))
                .or_insert_with(|| function.clone());
        }
    }

    let mut functions_by_language = BTreeMap::<String, Vec<FunctionScope>>::new();
    for ((_, _, _, language), function) in functions {
        functions_by_language
            .entry(language)
            .or_default()
            .push(function);
    }

    let mut analyses = Vec::new();
    for (language, scopes) in functions_by_language {
        let indexes = &by_language[&language];
        let mut events = Vec::new();
        let mut parents = HashMap::new();
        for index in indexes {
            let item = &summaries[*index];
            events.extend(item.events.iter().copied().filter(|event| {
                is_complexity_event(event.role) && !inside_any(event.range(), &item.excluded)
            }));
            parents.extend(item.parents.iter().map(|(key, value)| (*key, *value)));
        }
        let assigned = cyclomatic::assign_events(&scopes, &events);
        for (index, function) in scopes.into_iter().enumerate() {
            let mut rows = BTreeSet::new();
            for summary_index in indexes {
                let item = &summaries[*summary_index];
                rows.extend(nloc::rows(
                    &item.events,
                    function.range.clone(),
                    &item.excluded,
                ));
            }
            let (cyclomatic_complexity, contributions) = cyclomatic::analyze(
                &function,
                assigned.get(&index).map_or(&[], Vec::as_slice),
                &parents,
            );
            let nloc = rows.len();
            analyses.push(FunctionAnalysis {
                name: function.name,
                start_byte: function.range.start,
                end_byte: function.range.end,
                nloc,
                cyclomatic_complexity,
                cyclomatic_density: cyclomatic_density::calculate(cyclomatic_complexity, nloc),
                contributions,
            });
        }
    }
    analyses.sort_by(|left, right| {
        (left.start_byte, left.end_byte, &left.name).cmp(&(
            right.start_byte,
            right.end_byte,
            &right.name,
        ))
    });

    let mut injections = Vec::new();
    collect_injection_analysis(parsed_injections, &mut injections);
    let mut definitions = Vec::new();
    let mut imports = Vec::new();
    let mut references = Vec::new();
    let mut locals = Vec::new();
    for item in summaries
        .into_iter()
        .filter(|item| item.language_id == root_choice.id)
    {
        definitions.extend(item.definitions);
        imports.extend(item.imports);
        references.extend(item.references);
        locals.extend(item.locals);
    }
    FileFacts {
        path: path.to_path_buf(),
        module: ModulePath::default(),
        definitions,
        imports,
        references,
        locals,
        analysis: FileAnalysis {
            path: path.display().to_string(),
            hash: blake3::hash(source).to_hex().to_string(),
            language: root_choice.name.clone(),
            nloc: file_rows.len(),
            functions: analyses,
            injections,
            resolution: Vec::new(),
        },
    }
}

fn collect_injection_analysis(
    parsed: &[ParsedInjection],
    result: &mut Vec<crate::model::InjectionAnalysis>,
) {
    for injection in parsed {
        result.push(crate::model::InjectionAnalysis {
            language: injection.language.clone(),
            start_byte: injection.range.start,
            end_byte: injection.range.end,
            analyzed: injection.tree.is_some(),
        });
        collect_injection_analysis(&injection.children, result);
    }
}

fn selected_requests(requests: Vec<InjectionRequest>) -> Vec<InjectionRequest> {
    let mut selected = BTreeMap::<(usize, usize), InjectionRequest>::new();
    for request in requests {
        let key = (request.range.start, request.range.end);
        match selected.get(&key) {
            Some(current)
                if current.priority > request.priority
                    || (current.priority == request.priority
                        && current.language <= request.language) => {}
            _ => {
                selected.insert(key, request);
            }
        }
    }
    selected.into_values().collect()
}

fn remap_node_ids(captured: &mut CapturedTree, next_id: &mut usize) {
    let mut old_ids = BTreeSet::new();
    old_ids.extend(captured.parents.keys().copied());
    old_ids.extend(captured.parents.values().flatten().copied());
    for event in &captured.events {
        old_ids.insert(event.node_id);
        old_ids.extend(event.parent_id);
    }
    let mapping = old_ids
        .into_iter()
        .map(|old| {
            let new = *next_id;
            *next_id += 1;
            (old, new)
        })
        .collect::<HashMap<_, _>>();
    for event in &mut captured.events {
        event.node_id = mapping[&event.node_id];
        event.parent_id = event.parent_id.map(|id| mapping[&id]);
    }
    captured.parents = captured
        .parents
        .drain()
        .map(|(node, parent)| (mapping[&node], parent.map(|id| mapping[&id])))
        .collect();
}

fn inherited_module_for_range(
    inherited: &ModulePath,
    definitions: &[Definition],
    range: &Range<usize>,
) -> ModulePath {
    let nested = definitions
        .iter()
        .filter(|definition| {
            definition.inline_module
                && definition.start_byte <= range.start
                && range.end <= definition.end_byte
        })
        .min_by_key(|definition| definition.end_byte - definition.start_byte);
    let mut path = inherited.0.clone();
    if let Some(module) = nested {
        path.extend(module.module.0.iter().cloned());
        path.push(module.name.clone());
    }
    ModulePath(path)
}

fn prepend_module(module: &mut ModulePath, prefix: &[String]) {
    if !prefix.is_empty() {
        let mut path = prefix.to_vec();
        path.append(&mut module.0);
        module.0 = path;
    }
}

fn inherit_scope(
    scope_start: &mut usize,
    scope_end: &mut usize,
    owner_range: &Range<usize>,
    inherited_scope: Option<&Range<usize>>,
) {
    if let Some(scope) = inherited_scope
        .filter(|_| *scope_start <= owner_range.start && owner_range.end <= *scope_end)
    {
        *scope_start = scope.start;
        *scope_end = scope.end;
    }
}

fn enclosing_function(functions: &[FunctionScope], range: &Range<usize>) -> Option<Range<usize>> {
    functions
        .iter()
        .filter(|function| function.range.start <= range.start && range.end <= function.range.end)
        .min_by_key(|function| function.range.end - function.range.start)
        .map(|function| function.range.clone())
}

fn injection_ranges(tree: &Tree, request: &InjectionRequest) -> Vec<TsRange> {
    let content = TsRange {
        start_byte: request.range.start,
        start_point: request.start_point,
        end_byte: request.range.end,
        end_point: request.end_point,
    };
    let mut ranges = Vec::new();
    if request.include_children {
        ranges.push(content);
    } else {
        let mut children = request.child_ranges.clone();
        children.sort_by_key(|range| (range.start_byte, range.end_byte));
        let mut start_byte = content.start_byte;
        let mut start_point = content.start_point;
        for child in children {
            let child_start = child.start_byte.max(content.start_byte);
            let child_end = child.end_byte.min(content.end_byte);
            if start_byte < child_start {
                ranges.push(TsRange {
                    start_byte,
                    start_point,
                    end_byte: child_start,
                    end_point: if child_start == child.start_byte {
                        child.start_point
                    } else {
                        content.start_point
                    },
                });
            }
            if child_end > start_byte {
                start_byte = child_end;
                start_point = if child_end == child.end_byte {
                    child.end_point
                } else {
                    content.end_point
                };
            }
        }
        if start_byte < content.end_byte {
            ranges.push(TsRange {
                start_byte,
                start_point,
                end_byte: content.end_byte,
                end_point: content.end_point,
            });
        }
    }
    let parent_ranges = tree.included_ranges();
    let parent_ranges = if parent_ranges.is_empty() {
        vec![tree.root_node().range()]
    } else {
        parent_ranges
    };
    ranges
        .into_iter()
        .flat_map(|range| {
            parent_ranges
                .iter()
                .filter_map(move |parent| intersect(range, *parent))
        })
        .collect()
}

fn intersect(left: TsRange, right: TsRange) -> Option<TsRange> {
    let start_byte = left.start_byte.max(right.start_byte);
    let end_byte = left.end_byte.min(right.end_byte);
    if start_byte >= end_byte {
        return None;
    }
    let start_point = if start_byte == left.start_byte {
        left.start_point
    } else {
        right.start_point
    };
    let end_point = if end_byte == left.end_byte {
        left.end_point
    } else {
        right.end_point
    };
    Some(TsRange {
        start_byte,
        start_point,
        end_byte,
        end_point,
    })
}

fn is_smaller(parent: &Range<usize>, child: &Range<usize>) -> bool {
    parent.start <= child.start
        && child.end <= parent.end
        && child.start < child.end
        && child.end - child.start < parent.end - parent.start
}

fn is_complexity_event(role: SyntaxRole) -> bool {
    matches!(
        role,
        SyntaxRole::Condition
            | SyntaxRole::LogicalCondition
            | SyntaxRole::Multiway
            | SyntaxRole::Case
    )
}

fn inside_any(range: Range<usize>, excluded: &[Range<usize>]) -> bool {
    excluded
        .iter()
        .any(|item| item.start <= range.start && range.end <= item.end)
}

fn collect_old_trees(
    injections: &[ParsedInjection],
    edits: &[InputEdit],
    trees: &mut BTreeMap<TreeKey, Tree>,
) {
    for injection in injections {
        if let (Some(language), Some(tree), Some(range)) = (
            injection.language_id.as_ref(),
            injection.tree.as_ref(),
            map_range(injection.range.clone(), edits),
        ) {
            let edited = edit_tree(tree.clone(), edits);
            trees
                .entry((language.clone(), range.start, range.end))
                .or_insert(edited);
        }
        collect_old_trees(&injection.children, edits, trees);
    }
}

fn edit_tree(mut tree: Tree, edits: &[InputEdit]) -> Tree {
    for edit in edits {
        tree.edit(edit);
    }
    tree
}

fn map_range(mut range: Range<usize>, edits: &[InputEdit]) -> Option<Range<usize>> {
    for edit in edits {
        if edit.old_end_byte <= range.start {
            range.start = shift(range.start, edit.old_end_byte, edit.new_end_byte)?;
            range.end = shift(range.end, edit.old_end_byte, edit.new_end_byte)?;
        } else if edit.start_byte >= range.end {
            continue;
        } else if range.start <= edit.start_byte && edit.old_end_byte <= range.end {
            range.end = shift(range.end, edit.old_end_byte, edit.new_end_byte)?;
        } else {
            return None;
        }
    }
    Some(range)
}

fn shift(value: usize, old_end: usize, new_end: usize) -> Option<usize> {
    if new_end >= old_end {
        value.checked_add(new_end - old_end)
    } else {
        value.checked_sub(old_end - new_end)
    }
}
