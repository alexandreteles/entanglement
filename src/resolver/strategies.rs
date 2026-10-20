//! Dispatch semantic contexts independently, including mixed-language files.

use super::analysis;
use crate::graph::{FileFactsView, ProgramGraph};
use crate::languages::{FileModuleRules, ResolutionFamily};
use crate::model::{FileFacts, ReferenceAnalysis, Resolution};
use crate::snapshot::Snapshot;

#[derive(Clone, Copy)]
struct Strategy {
    family: ResolutionFamily,
    rules: Option<&'static FileModuleRules>,
}

impl Strategy {
    fn same(self, other: Self) -> bool {
        self.family == other.family
            && match (self.rules, other.rules) {
                (Some(left), Some(right)) => std::ptr::eq(left, right),
                (None, None) => true,
                _ => false,
            }
    }
}

fn for_context(fact: &FileFacts, context_id: usize) -> Strategy {
    let layer = fact
        .contexts
        .iter()
        .map(|context| &context.semantic)
        .find(|layer| {
            layer.context_id == context_id
                && layer
                    .resolution_family
                    .is_some_and(|family| family != ResolutionFamily::None)
        });
    Strategy {
        family: layer
            .and_then(|layer| layer.resolution_family)
            .unwrap_or(fact.resolution_family),
        rules: layer.map_or(fact.file_module_rules, |layer| layer.file_module_rules),
    }
}

fn context_ids(fact: &FileFacts) -> impl Iterator<Item = usize> + '_ {
    fact.contexts
        .iter()
        .map(|context| context.semantic.context_id)
}

pub(super) fn resolve(
    facts: &[FileFacts],
    snapshot: &Snapshot,
    graph: &ProgramGraph<'_>,
) -> Vec<Vec<ReferenceAnalysis>> {
    let mut cache = super::kotlin::ProjectCache::default();
    let homogeneous = facts.iter().all(|fact| {
        let native = Strategy {
            family: fact.resolution_family,
            rules: fact.file_module_rules,
        };
        context_ids(fact).all(|context_id| {
            let strategy = for_context(fact, context_id);
            strategy.family == ResolutionFamily::None || native.same(strategy)
        })
    });
    if homogeneous {
        return super::resolve_references(graph.files(), snapshot, graph, &mut cache);
    }
    let mut strategies = Vec::<Strategy>::new();
    for strategy in facts
        .iter()
        .flat_map(|fact| context_ids(fact).map(|context_id| for_context(fact, context_id)))
    {
        if strategy.family != ResolutionFamily::None
            && !strategies.iter().any(|other| strategy.same(*other))
        {
            strategies.push(strategy);
        }
    }
    let mut result = facts
        .iter()
        .map(|fact| {
            FileFactsView::new(fact)
                .references()
                .map(|reference| analysis(reference, Resolution::Unresolved, None))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    for strategy in strategies {
        let projected = facts
            .iter()
            .map(|fact| {
                let contexts = context_ids(fact)
                    .filter(|context_id| strategy.same(for_context(fact, *context_id)))
                    .collect();
                FileFactsView::projected(fact, contexts, strategy.family, strategy.rules)
            })
            .collect();
        let projected_graph = graph.project(projected);
        let resolved = super::resolve_references(
            projected_graph.files(),
            snapshot,
            &projected_graph,
            &mut cache,
        );
        for (file_index, (fact, resolutions)) in facts.iter().zip(resolved).enumerate() {
            let view = FileFactsView::new(fact);
            let indices = view
                .references()
                .enumerate()
                .filter(|(_, reference)| strategy.same(for_context(fact, reference.context_id)))
                .map(|(index, _)| index);
            let indices = indices.collect::<Vec<_>>();
            // A family with no semantic evidence may produce no analyses;
            // preserve the prefilled unresolved entries in that case.
            assert!(
                resolutions.is_empty() || indices.len() == resolutions.len(),
                "strategy results must preserve reference cardinality"
            );
            for (index, resolution) in indices.into_iter().zip(resolutions) {
                result[file_index][index] = resolution;
            }
        }
    }
    result
}
