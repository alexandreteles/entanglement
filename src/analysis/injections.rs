use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ops::Range;

use tree_sitter::{InputEdit, Range as TsRange, Tree};

use crate::Result;
use crate::languages::{CapturedTree, InjectionRequest, LanguageChoice};
use crate::metrics::{SyntaxRole, cyclomatic::FunctionScope};
use crate::model::{Definition, InjectionAnalysis, ModulePath};

use super::{
    facts::{self, TreeSummary},
    worker::Worker,
};

const MAX_INJECTION_DEPTH: usize = 64;

/// Keep one injected tree and its nested injected trees.
pub(super) struct ParsedInjection {
    language: String,
    language_id: Option<String>,
    range: Range<usize>,
    tree: Option<Tree>,
    children: Vec<ParsedInjection>,
}

pub(super) type TreeKey = (String, usize, usize);

pub(super) fn collect_injection_analysis(
    parsed: &[ParsedInjection],
    result: &mut Vec<InjectionAnalysis>,
) {
    for injection in parsed {
        result.push(InjectionAnalysis {
            language: injection.language.clone(),
            start_byte: injection.range.start,
            end_byte: injection.range.end,
            analyzed: injection.tree.is_some(),
        });
        collect_injection_analysis(&injection.children, result);
    }
}

impl Worker {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn analyze_layer(
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
        let mut captured =
            self.registry
                .capture(&choice.id, tree, source, self.selection.needs_halstead())?;
        remap_node_ids(&mut captured, next_node_id);
        let requests = selected_requests(std::mem::take(&mut captured.injections));
        let excluded = requests
            .iter()
            .map(|item| item.range.clone())
            .collect::<Vec<_>>();
        let child_contexts = requests
            .iter()
            .map(|request| {
                (
                    captured
                        .events
                        .iter()
                        .filter(|event| {
                            event.role == SyntaxRole::Node
                                && event.start_byte <= request.range.start
                                && event.end_byte >= request.range.end
                                && event.range() != request.range
                        })
                        .min_by_key(|event| event.end_byte - event.start_byte)
                        .map(|event| event.node_id),
                    inherited_module_for_range(
                        inherited_module,
                        &captured.definitions,
                        &request.range,
                    ),
                    enclosing_function(&captured.functions, &request.range)
                        .or_else(|| inherited_scope.clone()),
                )
            })
            .collect::<Vec<_>>();
        let mut summaries = vec![facts::summary(
            choice,
            captured,
            excluded,
            inherited_module,
            &owner_range,
            inherited_scope.as_ref(),
        )];
        let mut injections = Vec::with_capacity(requests.len());

        for (request, (cognitive_parent, child_module, child_scope)) in
            requests.into_iter().zip(child_contexts)
        {
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
            active.insert(key.clone());
            let (children, mut child_summaries) = self.analyze_layer(
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
            if child_choice.id == choice.id {
                child_summaries[0].cognitive_parent = cognitive_parent;
            }
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

pub(super) fn collect_old_trees(
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

pub(super) fn edit_tree(mut tree: Tree, edits: &[InputEdit]) -> Tree {
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
