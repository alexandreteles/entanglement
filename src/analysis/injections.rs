mod requests;
mod trees;

use requests::{
    can_analyze_injection, excluded_ranges, injection_contexts, pending_injection,
    selected_requests,
};
pub(super) use trees::{collect_old_trees, edit_tree};
use trees::{injection_ranges, remap_node_ids};

use std::collections::{BTreeMap, HashSet};
use std::ops::Range;

use tree_sitter::Tree;

use crate::Result;
use crate::languages::{InjectionRequest, LanguageChoice};
use crate::model::{InjectionAnalysis, ModulePath};

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
        child_summaries[0].add_serialized_bindings(request.serialized_bindings);
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
