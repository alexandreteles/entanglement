use std::collections::{BTreeMap, HashSet};
use std::ops::Range;

use crate::languages::query::ranges;
use crate::languages::{CapturedTree, InjectionRequest};
use crate::metrics::{SyntaxEvent, SyntaxRole, cyclomatic::FunctionScope};
use crate::model::{Definition, ModulePath};

use super::trees::is_smaller;
use super::{InjectionContext, LayerContext, MAX_INJECTION_DEPTH, ParsedInjection, TreeKey};

pub(super) fn excluded_ranges(requests: &[InjectionRequest]) -> Vec<Range<usize>> {
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

pub(super) fn injection_contexts(
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

pub(super) fn can_analyze_injection(
    layer: &LayerContext<'_>,
    request: &InjectionRequest,
    key: &TreeKey,
    active: &HashSet<TreeKey>,
) -> bool {
    layer.depth < MAX_INJECTION_DEPTH
        && is_smaller(layer.owner_range, &request.range)
        && !active.contains(key)
}

pub(super) fn pending_injection(
    request: InjectionRequest,
    language_id: Option<String>,
) -> ParsedInjection {
    ParsedInjection {
        language: request.language,
        language_id,
        range: request.range,
        tree: None,
        children: Vec::new(),
    }
}

pub(super) fn selected_requests(requests: Vec<InjectionRequest>) -> Vec<InjectionRequest> {
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
