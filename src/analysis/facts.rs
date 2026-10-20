mod summary;

pub(super) use summary::summary;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use crate::languages::{FileModuleRules, LanguageChoice, ResolutionFamily};
use crate::metrics::selection::Selection;
use crate::metrics::{
    SyntaxEvent, cyclomatic::FunctionScope, halstead::HalsteadToken,
    maintainability::MaintainabilityIndex, nloc,
};
use crate::model::{FileAnalysis, FileFacts, FunctionAnalysis, InjectionAnalysis, ModulePath};

use super::injections::{self, ParsedInjection};
use super::metrics;

pub(super) struct TreeSummary {
    pub(super) facts: crate::languages::SemanticStore,
    metric_id: String,
    resolution_family: ResolutionFamily,
    file_module_rules: Option<&'static FileModuleRules>,
    pub(super) semantic_context: usize,
    pub(super) events: Vec<SyntaxEvent>,
    pub(super) parents: HashMap<usize, Option<usize>>,
    pub(super) cognitive_parent: Option<usize>,
    functions: Vec<FunctionScope>,
    pub(super) tokens: Vec<HalsteadToken>,
    pub(super) excluded: Vec<Range<usize>>,
}

pub(super) struct SummaryContext<'a> {
    pub(super) semantic_context: usize,
    pub(super) metric_id: String,
    pub(super) excluded: Vec<Range<usize>>,
    pub(super) inherited_module: &'a ModulePath,
    pub(super) owner_range: &'a Range<usize>,
    pub(super) inherited_scope: Option<&'a Range<usize>>,
}

pub(super) fn build_facts(
    path: &Path,
    source: Arc<[u8]>,
    root_choice: &LanguageChoice,
    summaries: Vec<TreeSummary>,
    parsed_injections: &[ParsedInjection],
    selection: Selection,
) -> FileFacts {
    debug_assert!(summaries.iter().all(summary_context_is_consistent));
    let (resolution_family, file_module_rules) = summary_descriptor(root_choice, &summaries);
    let (file_volume, file_halstead) = metrics::file_halstead(&summaries, selection);
    let file_nloc = summary_nloc(summaries.iter(), 0..source.len());
    type MetricScopes<'a> = (
        Vec<usize>,
        BTreeMap<(usize, usize, &'a str), &'a FunctionScope>,
    );
    let mut by_metric = BTreeMap::<&str, MetricScopes<'_>>::new();
    for (index, item) in summaries.iter().enumerate() {
        let (indexes, scopes) = by_metric.entry(&item.metric_id).or_default();
        indexes.push(index);
        for function in &item.functions {
            scopes
                .entry((function.range.start, function.range.end, &function.name))
                .or_insert(function);
        }
    }

    let mut analyses = Vec::new();
    let mut file_cyclomatic_complexity = 0;
    for (indexes, scopes) in by_metric.into_values() {
        let scopes = scopes.into_values().cloned().collect::<Vec<_>>();
        let language_metrics =
            metrics::LanguageMetrics::calculate(&scopes, &summaries, &indexes, selection);
        for (index, function) in scopes.into_iter().enumerate() {
            let nloc = summary_nloc(
                indexes.iter().map(|index| &summaries[*index]),
                function.range.clone(),
            );
            let function_metrics = language_metrics.for_function(&function, index, nloc);
            file_cyclomatic_complexity += function_metrics.cyclomatic_for_file;
            analyses.push(FunctionAnalysis {
                name: function.name,
                start_byte: function.range.start,
                end_byte: function.range.end,
                nloc,
                cyclomatic_complexity: function_metrics.cyclomatic_complexity,
                cyclomatic_density: function_metrics.cyclomatic_density,
                contributions: function_metrics.contributions,
                cognitive_complexity: function_metrics.cognitive_complexity,
                cognitive_contributions: function_metrics.cognitive_contributions,
                maintainability_index: function_metrics.maintainability_index,
                halstead: function_metrics.halstead,
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
    let mut injections = Vec::<InjectionAnalysis>::new();
    injections::collect_injection_analysis(parsed_injections, &mut injections);
    FileFacts {
        contexts: summaries.into_iter().map(|summary| summary.facts).collect(),
        context_order: Vec::new(),
        target: path.to_path_buf(),
        aliases: vec![path.to_path_buf()],
        path: path.to_path_buf(),
        module: ModulePath::default(),
        resolution_family,
        file_module_rules,
        analysis: FileAnalysis {
            path: path.display().to_string(),
            hash: blake3::hash(&source).to_hex().to_string(),
            language: root_choice.name.clone(),
            nloc: file_nloc,
            maintainability_index: selection.needs_mi().then(|| {
                MaintainabilityIndex::calculate(file_volume, file_cyclomatic_complexity, file_nloc)
            }),
            halstead: file_halstead,
            functions: analyses,
            injections,
            resolution: Vec::new(),
        },
        source,
    }
    .index_contexts()
}

fn summary_nloc<'a>(
    summaries: impl Iterator<Item = &'a TreeSummary>,
    range: Range<usize>,
) -> usize {
    summaries
        .flat_map(|item| nloc::rows(&item.events, range.clone(), &item.excluded))
        .collect::<BTreeSet<_>>()
        .len()
}

fn summary_descriptor(
    root: &LanguageChoice,
    summaries: &[TreeSummary],
) -> (ResolutionFamily, Option<&'static FileModuleRules>) {
    let family = if root.resolution_family != ResolutionFamily::None {
        root.resolution_family
    } else {
        let Some(family) = summaries
            .iter()
            .map(|item| item.resolution_family)
            .find(|family| *family != ResolutionFamily::None)
        else {
            return (ResolutionFamily::None, None);
        };
        let one_family = summaries
            .iter()
            .map(|item| item.resolution_family)
            .filter(|candidate| *candidate != ResolutionFamily::None)
            .all(|candidate| candidate == family);
        if !one_family {
            return (ResolutionFamily::None, None);
        }
        family
    };
    let rules = (family == ResolutionFamily::FileModules)
        .then(|| {
            if root.resolution_family == family {
                root.file_module_rules
            } else {
                summaries
                    .iter()
                    .filter(|item| item.resolution_family == family)
                    .find_map(|item| item.file_module_rules)
            }
        })
        .flatten();
    (family, rules)
}

fn summary_context_is_consistent(summary: &TreeSummary) -> bool {
    summary
        .facts
        .definitions
        .iter()
        .all(|item| item.context_id == summary.semantic_context)
        && summary
            .facts
            .imports
            .iter()
            .all(|item| item.context_id == summary.semantic_context)
        && summary
            .facts
            .exports
            .iter()
            .all(|item| item.context_id == summary.semantic_context)
        && summary
            .facts
            .references
            .iter()
            .all(|item| item.context_id == summary.semantic_context)
        && summary
            .facts
            .locals
            .iter()
            .all(|item| item.context_id == summary.semantic_context)
}
