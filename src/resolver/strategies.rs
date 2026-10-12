//! Dispatch semantic contexts independently, including mixed-language files.

use crate::graph::ProgramGraph;
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
    let layer = fact.semantic.iter().find(|layer| {
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
    fact.semantic
        .iter()
        .map(|layer| layer.context_id)
        .chain(fact.references.iter().map(|item| item.context_id))
        .chain(fact.definitions.iter().map(|item| item.context_id))
        .chain(fact.imports.iter().map(|item| item.context_id))
        .chain(fact.exports.iter().map(|item| item.context_id))
        .chain(fact.locals.iter().map(|item| item.context_id))
}

pub(super) fn resolve(
    facts: &[FileFacts],
    snapshot: &Snapshot,
    graph: &ProgramGraph<'_>,
) -> Vec<Vec<ReferenceAnalysis>> {
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
        return super::resolve_references(facts, snapshot, graph);
    }
    let mut strategies = Vec::<Strategy>::new();
    for fact in facts {
        for context_id in context_ids(fact) {
            let strategy = for_context(fact, context_id);
            if strategy.family != ResolutionFamily::None
                && !strategies.iter().any(|other| strategy.same(*other))
            {
                strategies.push(strategy);
            }
        }
    }
    let mut result = facts
        .iter()
        .map(|fact| {
            fact.references
                .iter()
                .map(|reference| ReferenceAnalysis {
                    path: reference.path.clone(),
                    start_byte: reference.start_byte,
                    end_byte: reference.end_byte,
                    resolution: Resolution::Unresolved,
                    dispatch: None,
                    execution_targets: Vec::new(),
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    for strategy in strategies {
        let projected = facts
            .iter()
            .map(|fact| {
                let belongs = |context_id| strategy.same(for_context(fact, context_id));
                let mut projected = fact.clone();
                let present = context_ids(fact).any(belongs);
                projected.resolution_family = if present {
                    strategy.family
                } else {
                    ResolutionFamily::None
                };
                projected.file_module_rules = present.then_some(strategy.rules).flatten();
                projected
                    .definitions
                    .retain(|item| belongs(item.context_id));
                projected.imports.retain(|item| belongs(item.context_id));
                projected.exports.retain(|item| belongs(item.context_id));
                projected.references.retain(|item| belongs(item.context_id));
                projected.locals.retain(|item| belongs(item.context_id));
                projected.semantic.retain(|layer| belongs(layer.context_id));
                projected
            })
            .collect::<Vec<_>>();
        let projected_graph = ProgramGraph::new(&projected);
        let resolved = super::resolve_references(&projected, snapshot, &projected_graph);
        for (file_index, fact) in facts.iter().enumerate() {
            let indices = fact
                .references
                .iter()
                .enumerate()
                .filter(|(_, reference)| strategy.same(for_context(fact, reference.context_id)))
                .map(|(index, _)| index);
            for (index, resolution) in indices.zip(resolved[file_index].iter()) {
                result[file_index][index] = resolution.clone();
            }
        }
    }
    result
}
