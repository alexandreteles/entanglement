use std::collections::{BTreeMap, HashMap};

use crate::metrics::selection::{Metric, Selection};
use crate::metrics::{
    SyntaxEvent, SyntaxRole, cognitive,
    cyclomatic::{self, FunctionScope},
    cyclomatic_density,
    halstead::{self, HalsteadMetrics, HalsteadToken},
    maintainability::MaintainabilityIndex,
};
use crate::model::ComplexityContribution;

use super::facts::TreeSummary;

pub(super) struct LanguageMetrics {
    parents: HashMap<usize, Option<usize>>,
    assigned_events: BTreeMap<usize, Vec<SyntaxEvent>>,
    assigned_tokens: BTreeMap<usize, Vec<HalsteadToken>>,
    cognitive: Option<Vec<(usize, Vec<ComplexityContribution>)>>,
    selection: Selection,
}

pub(super) struct FunctionMetrics {
    pub cyclomatic_complexity: usize,
    pub cyclomatic_for_file: usize,
    pub cyclomatic_density: f64,
    pub contributions: Vec<ComplexityContribution>,
    pub cognitive_complexity: usize,
    pub cognitive_contributions: Vec<ComplexityContribution>,
    pub halstead: HalsteadMetrics,
    pub maintainability_index: Option<MaintainabilityIndex>,
}

impl LanguageMetrics {
    pub(super) fn calculate(
        scopes: &[FunctionScope],
        summaries: &[TreeSummary],
        indexes: &[usize],
        selection: Selection,
    ) -> Self {
        let inputs = collect_language_inputs(summaries, indexes, selection);
        let assigned_events = if selection.needs_cc() {
            cyclomatic::assign_events(scopes, &inputs.events)
        } else {
            BTreeMap::new()
        };
        let assigned_tokens = if selection.needs_halstead() {
            halstead::assign_tokens(scopes, &inputs.tokens)
        } else {
            BTreeMap::new()
        };
        let cognitive = selection.needs_cognitive().then(|| {
            analyze_cognitive(scopes, &inputs.events, &inputs.parents, summaries, indexes)
        });

        Self {
            parents: inputs.parents,
            assigned_events,
            assigned_tokens,
            cognitive,
            selection,
        }
    }

    pub(super) fn for_function(
        &self,
        function: &FunctionScope,
        index: usize,
        nloc: usize,
    ) -> FunctionMetrics {
        let (cyclomatic_for_file, cyclomatic_complexity, contributions) = cyclomatic_metrics(
            function,
            index,
            &self.assigned_events,
            &self.parents,
            self.selection,
        );
        let (halstead_volume, halstead) =
            halstead_metrics(index, &self.assigned_tokens, self.selection);
        let (cognitive_complexity, cognitive_contributions) = self
            .cognitive
            .as_ref()
            .and_then(|results| results.get(index))
            .cloned()
            .unwrap_or_default();

        FunctionMetrics {
            cyclomatic_complexity,
            cyclomatic_for_file,
            cyclomatic_density: if self.selection.needs_density() {
                cyclomatic_density::calculate(cyclomatic_for_file, nloc)
            } else {
                0.0
            },
            contributions,
            cognitive_complexity,
            cognitive_contributions,
            halstead,
            maintainability_index: self.selection.needs_mi().then(|| {
                MaintainabilityIndex::calculate(halstead_volume, cyclomatic_for_file, nloc)
            }),
        }
    }
}

struct LanguageInputs {
    events: Vec<SyntaxEvent>,
    parents: HashMap<usize, Option<usize>>,
    tokens: Vec<HalsteadToken>,
}

fn collect_language_inputs(
    summaries: &[TreeSummary],
    indexes: &[usize],
    selection: Selection,
) -> LanguageInputs {
    let mut inputs = LanguageInputs {
        events: Vec::new(),
        parents: HashMap::new(),
        tokens: Vec::new(),
    };
    for index in indexes {
        let summary = &summaries[*index];
        inputs
            .events
            .extend(summary.events.iter().copied().filter(|event| {
                selected_event(event, selection)
                    && (is_host_structure(event.role)
                        || !inside_any(event.range(), &summary.excluded))
            }));
        if selection.needs_cc() || selection.needs_cognitive() {
            inputs
                .parents
                .extend(summary.parents.iter().map(|(key, value)| (*key, *value)));
        }
        if selection.needs_halstead() {
            inputs.tokens.extend(summary.tokens.iter().cloned());
        }
    }
    inputs
}

fn is_host_structure(role: SyntaxRole) -> bool {
    matches!(
        role,
        SyntaxRole::CognitiveConditionBoundary | SyntaxRole::CognitiveElseIf
    )
}

fn selected_event(event: &SyntaxEvent, selection: Selection) -> bool {
    selection.needs_cc() && is_complexity_event(event.role)
        || selection.needs_cognitive() && cognitive::is_event(event.role)
}

fn analyze_cognitive(
    scopes: &[FunctionScope],
    events: &[SyntaxEvent],
    parents: &HashMap<usize, Option<usize>>,
    summaries: &[TreeSummary],
    indexes: &[usize],
) -> Vec<(usize, Vec<ComplexityContribution>)> {
    let mut cognitive_parents = std::borrow::Cow::Borrowed(parents);
    for index in indexes {
        let summary = &summaries[*index];
        if let Some(parent) = summary.cognitive_parent {
            for (root, ancestor) in &summary.parents {
                if ancestor.is_none() {
                    cognitive_parents.to_mut().insert(*root, Some(parent));
                }
            }
        }
    }
    cognitive::Context::new(events, cognitive_parents.as_ref()).analyze_functions(scopes)
}

pub(super) fn file_halstead(
    summaries: &[TreeSummary],
    selection: Selection,
) -> (f64, HalsteadMetrics) {
    if !selection.needs_halstead() {
        return (0.0, empty_halstead());
    }
    let metrics = HalsteadMetrics::from_tokens(
        summaries
            .iter()
            .flat_map(|item| item.tokens.iter().cloned()),
    );
    let volume = metrics.volume;
    let metrics = if selection.includes(Metric::Halstead) {
        metrics
    } else {
        empty_halstead()
    };
    (volume, metrics)
}

fn cyclomatic_metrics(
    function: &FunctionScope,
    index: usize,
    assigned_events: &BTreeMap<usize, Vec<SyntaxEvent>>,
    parents: &HashMap<usize, Option<usize>>,
    selection: Selection,
) -> (usize, usize, Vec<ComplexityContribution>) {
    if !selection.needs_cc() {
        return (0, 0, Vec::new());
    }
    let (value, contributions) = cyclomatic::analyze(
        function,
        assigned_events.get(&index).map_or(&[], Vec::as_slice),
        parents,
    );
    if selection.includes(Metric::Cc) {
        (value, value, contributions)
    } else {
        (value, 0, Vec::new())
    }
}

fn halstead_metrics(
    index: usize,
    assigned_tokens: &BTreeMap<usize, Vec<HalsteadToken>>,
    selection: Selection,
) -> (f64, HalsteadMetrics) {
    if !selection.needs_halstead() {
        return (0.0, empty_halstead());
    }
    let metrics =
        HalsteadMetrics::from_tokens(assigned_tokens.get(&index).cloned().unwrap_or_default());
    let volume = metrics.volume;
    let metrics = if selection.includes(Metric::Halstead) {
        metrics
    } else {
        empty_halstead()
    };
    (volume, metrics)
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

fn inside_any(range: std::ops::Range<usize>, excluded: &[std::ops::Range<usize>]) -> bool {
    excluded
        .iter()
        .any(|item| item.start <= range.start && range.end <= item.end)
}

fn empty_halstead() -> HalsteadMetrics {
    HalsteadMetrics::calculate(0, 0, 0, 0)
}
