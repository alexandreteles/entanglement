use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::ops::Range;

use tree_sitter::{InputEdit, Range as TsRange, Tree};

use crate::Result;
use crate::languages::query::ranges;
use crate::languages::{CapturedTree, InjectionRequest, LanguageChoice};
use crate::metrics::{SyntaxEvent, SyntaxRole, cyclomatic::FunctionScope};
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

struct InjectionContext {
    cognitive_parent: Option<usize>,
    module: ModulePath,
    scope: Option<Range<usize>>,
}

struct LayerContext<'a> {
    choice: &'a LanguageChoice,
    tree: &'a Tree,
    source: &'a [u8],
    owner_range: &'a Range<usize>,
    context_id: usize,
    metric_id: &'a str,
    depth: usize,
}

struct TraversalState<'a> {
    active: &'a mut HashSet<(String, usize, usize)>,
    next_node_id: &'a mut usize,
    next_context_id: &'a mut usize,
    old_trees: &'a mut BTreeMap<TreeKey, Tree>,
}

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
        inherited_context: Option<usize>,
        metric_id: String,
        active: &mut HashSet<(String, usize, usize)>,
        next_node_id: &mut usize,
        next_context_id: &mut usize,
        old_trees: &mut BTreeMap<TreeKey, Tree>,
        depth: usize,
    ) -> Result<(Vec<ParsedInjection>, Vec<TreeSummary>)> {
        let context_id = inherited_context.unwrap_or_else(|| {
            let id = *next_context_id;
            *next_context_id += 1;
            id
        });
        let mut captured =
            self.registry
                .capture(&choice.id, tree, source, self.selection.needs_halstead())?;
        remap_node_ids(&mut captured, next_node_id);
        let requests = self.injection_requests(std::mem::take(&mut captured.injections))?;
        let excluded = excluded_ranges(&requests);
        let child_contexts = injection_contexts(
            &requests,
            &captured,
            inherited_module,
            inherited_scope.as_ref(),
        );
        let mut summaries = vec![facts::summary(
            choice,
            captured,
            facts::SummaryContext {
                semantic_context: context_id,
                metric_id: metric_id.clone(),
                excluded,
                inherited_module,
                owner_range: &owner_range,
                inherited_scope: inherited_scope.as_ref(),
            },
        )];
        let mut injections = Vec::with_capacity(requests.len());
        let layer = LayerContext {
            choice,
            tree,
            source,
            owner_range: &owner_range,
            context_id,
            metric_id: &metric_id,
            depth,
        };
        let mut state = TraversalState {
            active,
            next_node_id,
            next_context_id,
            old_trees,
        };
        for (request, context) in requests.into_iter().zip(child_contexts) {
            let (injection, child_summaries) =
                self.analyze_injection(request, context, &layer, &mut summaries[0], &mut state)?;
            summaries.extend(child_summaries);
            injections.push(injection);
        }

        Ok((injections, summaries))
    }

    fn analyze_injection(
        &mut self,
        request: InjectionRequest,
        context: InjectionContext,
        layer: &LayerContext<'_>,
        host_summary: &mut TreeSummary,
        state: &mut TraversalState<'_>,
    ) -> Result<(ParsedInjection, Vec<TreeSummary>)> {
        let Some(child_choice) = self.registry.select_injection(&request.language)? else {
            return Ok((pending_injection(request, None), Vec::new()));
        };
        let child_id = child_choice.id.clone();
        let key = (child_id.clone(), request.range.start, request.range.end);
        if !can_analyze_injection(layer, &request, &key, state.active) {
            return Ok((pending_injection(request, Some(child_id)), Vec::new()));
        }
        let included_ranges = injection_ranges(layer.tree, &request);
        if included_ranges.is_empty() {
            return Ok((pending_injection(request, Some(child_id)), Vec::new()));
        }

        let old_tree = state.old_trees.remove(&key);
        let child_tree = self.parse_tree(
            &child_choice,
            layer.source,
            &included_ranges,
            old_tree.as_ref(),
        )?;
        state.active.insert(key.clone());
        let child_metric_id = if request.inherit_metrics {
            layer.metric_id.to_owned()
        } else {
            child_id.clone()
        };
        let (children, mut child_summaries) = self.analyze_layer(
            &child_choice,
            &child_tree,
            layer.source,
            request.range.clone(),
            &context.module,
            context.scope,
            request.inherit_context.then_some(layer.context_id),
            child_metric_id,
            &mut *state.active,
            &mut *state.next_node_id,
            &mut *state.next_context_id,
            &mut *state.old_trees,
            layer.depth + 1,
        )?;
        state.active.remove(&key);
        if child_id == layer.choice.id || request.inherit_metrics {
            child_summaries[0].cognitive_parent = context.cognitive_parent;
        }
        if !request.publish_exports {
            child_summaries[0].discard_exports();
        }
        if request.share_bindings {
            child_summaries[0].share_top_level_bindings_into(
                host_summary,
                &request.range,
                layer.owner_range,
            );
        }
        Ok((
            ParsedInjection {
                language: request.language,
                language_id: Some(child_id),
                range: request.range,
                tree: Some(child_tree),
                children,
            },
            child_summaries,
        ))
    }

    /// Optional helper labels affect host ownership only when supported.
    fn injection_requests(&self, requests: Vec<InjectionRequest>) -> Result<Vec<InjectionRequest>> {
        let mut eligible = Vec::with_capacity(requests.len());
        for request in requests {
            if !request.registered_only
                || self.registry.select_injection(&request.language)?.is_some()
            {
                eligible.push(request);
            }
        }
        Ok(selected_requests(eligible))
    }
}

fn excluded_ranges(requests: &[InjectionRequest]) -> Vec<Range<usize>> {
    requests
        .iter()
        .flat_map(|request| {
            request
                .guest_ranges
                .iter()
                .map(|range| range.start_byte..range.end_byte)
        })
        .collect()
}

fn injection_contexts(
    requests: &[InjectionRequest],
    captured: &CapturedTree,
    inherited_module: &ModulePath,
    inherited_scope: Option<&Range<usize>>,
) -> Vec<InjectionContext> {
    requests
        .iter()
        .map(|request| InjectionContext {
            cognitive_parent: cognitive_parent(&captured.events, &request.range),
            module: inherited_module_for_range(
                inherited_module,
                &captured.definitions,
                &request.range,
            ),
            scope: injection_scope(
                request.inherit_scope,
                &captured.functions,
                &request.range,
                inherited_scope,
            ),
        })
        .collect()
}

fn cognitive_parent(events: &[SyntaxEvent], range: &Range<usize>) -> Option<usize> {
    events
        .iter()
        .filter(|event| {
            event.role == SyntaxRole::Node
                && event.start_byte <= range.start
                && event.end_byte >= range.end
                && event.range() != *range
        })
        .min_by_key(|event| event.end_byte - event.start_byte)
        .map(|event| event.node_id)
}

fn injection_scope(
    inherit_scope: bool,
    functions: &[FunctionScope],
    range: &Range<usize>,
    inherited_scope: Option<&Range<usize>>,
) -> Option<Range<usize>> {
    if inherit_scope {
        enclosing_function(functions, range).or_else(|| inherited_scope.cloned())
    } else {
        None
    }
}

fn can_analyze_injection(
    layer: &LayerContext<'_>,
    request: &InjectionRequest,
    key: &TreeKey,
    active: &HashSet<TreeKey>,
) -> bool {
    layer.depth < MAX_INJECTION_DEPTH
        && is_smaller(layer.owner_range, &request.range)
        && !active.contains(key)
}

fn pending_injection(request: InjectionRequest, language_id: Option<String>) -> ParsedInjection {
    ParsedInjection {
        language: request.language,
        language_id,
        range: request.range,
        tree: None,
        children: Vec::new(),
    }
}

fn selected_requests(requests: Vec<InjectionRequest>) -> Vec<InjectionRequest> {
    let mut selected = BTreeMap::<(usize, usize), InjectionRequest>::new();
    for request in requests {
        let key = (request.range.start, request.range.end);
        match selected.get_mut(&key) {
            Some(current) if current.language == request.language => {
                current.merge_duplicate(&request);
            }
            Some(current)
                if current.priority > request.priority
                    || (current.priority == request.priority
                        && current.language <= request.language) => {}
            _ => {
                selected.insert(key, request);
            }
        }
    }
    selected
        .into_values()
        .map(|mut request| {
            ranges::normalize(&mut request.guest_ranges);
            request
        })
        .collect()
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
    let ranges = request.guest_ranges.clone();
    let parent_ranges = tree.included_ranges();
    let parent_ranges = if parent_ranges.is_empty() {
        vec![tree.root_node().range()]
    } else {
        parent_ranges
    };
    let mut ranges = ranges
        .into_iter()
        .flat_map(|range| {
            parent_ranges
                .iter()
                .filter_map(move |parent| intersect(range, *parent))
        })
        .collect::<Vec<_>>();
    ranges::normalize(&mut ranges);
    ranges
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
