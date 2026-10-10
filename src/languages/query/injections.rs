use tree_sitter::QueryCapture;

use crate::metrics::SyntaxRole;

use super::events::{event, node_text};
use super::ranges;
use super::{InjectionRequest, QueryAnalyzer, QueryFacts};

impl QueryFacts {
    pub(super) fn record_injections(
        &mut self,
        analyzer: &QueryAnalyzer,
        pattern_index: usize,
        captures: &[QueryCapture<'_>],
        source: &[u8],
    ) {
        let Some(content_id) = analyzer.injection_content else {
            return;
        };
        let mut properties = analyzer
            .injection_patterns
            .get(&pattern_index)
            .cloned()
            .unwrap_or_default();
        infer_language_capture(analyzer, captures, &mut properties);
        let Some(language) = injection_language(&properties, captures, source) else {
            return;
        };
        let host_ranges = host_ranges(analyzer, captures);
        for content in captures.iter().filter(|item| item.index == content_id) {
            let node = content.node;
            self.events.push(event(SyntaxRole::InjectionLanguage, node));
            let content_range = node.range();
            let guest_ranges = if properties.include_children {
                ranges::subtract(content_range, &host_ranges)
            } else {
                let mut cursor = node.walk();
                let children = node
                    .named_children(&mut cursor)
                    .map(|child| child.range())
                    .collect::<Vec<_>>();
                ranges::subtract(content_range, &children)
            };
            self.injections.push(InjectionRequest {
                language: language.clone(),
                range: node.start_byte()..node.end_byte(),
                guest_ranges,
                priority: properties.priority,
                inherit_scope: properties.inherit_scope,
                inherit_metrics: properties.inherit_metrics,
                inherit_context: properties.inherit_context,
                share_bindings: properties.share_bindings,
                publish_exports: properties.publish_exports,
                registered_only: properties.registered_only,
                serialized_bindings: Vec::new(),
            });
        }
    }
}

fn infer_language_capture(
    analyzer: &QueryAnalyzer,
    captures: &[QueryCapture<'_>],
    properties: &mut super::InjectionProperties,
) {
    if properties.language.is_some() || properties.language_capture.is_some() {
        return;
    }
    properties.language_capture = analyzer
        .injection_language
        .filter(|id| captures.iter().any(|capture| capture.index == *id));
}

fn injection_language(
    properties: &super::InjectionProperties,
    captures: &[QueryCapture<'_>],
    source: &[u8],
) -> Option<String> {
    properties.language.clone().or_else(|| {
        properties.language_capture.and_then(|id| {
            captures
                .iter()
                .find(|capture| capture.index == id)
                .map(|capture| node_text(capture.node, source))
        })
    })
}

fn host_ranges(analyzer: &QueryAnalyzer, captures: &[QueryCapture<'_>]) -> Vec<tree_sitter::Range> {
    analyzer.injection_host.map_or_else(Vec::new, |host_id| {
        captures
            .iter()
            .filter(|item| item.index == host_id)
            .map(|item| item.node.range())
            .collect()
    })
}

pub(super) fn merge(injections: &mut Vec<InjectionRequest>) {
    injections.sort_by_key(|item| (item.range.start, item.range.end, item.language.clone()));
    let mut merged: Vec<InjectionRequest> = Vec::with_capacity(injections.len());
    for item in injections.drain(..) {
        match merged
            .last_mut()
            .filter(|current| current.range == item.range && current.language == item.language)
        {
            Some(current) => current.merge_duplicate(&item),
            None => merged.push(item),
        }
    }
    for item in &mut merged {
        ranges::normalize(&mut item.guest_ranges);
    }
    *injections = merged;
}
