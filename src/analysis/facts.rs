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
use crate::model::{
    Definition, Export, FileAnalysis, FileFacts, FunctionAnalysis, Import, InjectionAnalysis,
    LocalBinding, ModulePath, Reference,
};

use super::injections::{self, ParsedInjection};
use super::metrics;

pub(super) struct TreeSummary {
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
    pub(super) definitions: Vec<Definition>,
    pub(super) imports: Vec<Import>,
    pub(super) exports: Vec<Export>,
    references: Vec<Reference>,
    locals: Vec<LocalBinding>,
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
    let mut file_rows = BTreeSet::new();
    for item in &summaries {
        file_rows.extend(nloc::rows(&item.events, 0..source.len(), &item.excluded));
    }

    let mut by_metric = BTreeMap::<String, Vec<usize>>::new();
    let mut functions = BTreeMap::<(usize, usize, String, String), FunctionScope>::new();
    for (index, item) in summaries.iter().enumerate() {
        by_metric
            .entry(item.metric_id.clone())
            .or_default()
            .push(index);
        for function in &item.functions {
            functions
                .entry((
                    function.range.start,
                    function.range.end,
                    function.name.clone(),
                    item.metric_id.clone(),
                ))
                .or_insert_with(|| function.clone());
        }
    }

    let mut functions_by_metric = BTreeMap::<String, Vec<FunctionScope>>::new();
    for ((_, _, _, metric), function) in functions {
        functions_by_metric
            .entry(metric)
            .or_default()
            .push(function);
    }

    let mut analyses = Vec::new();
    let mut file_cyclomatic_complexity = 0;
    for (metric, scopes) in functions_by_metric {
        let indexes = &by_metric[&metric];
        let language_metrics =
            metrics::LanguageMetrics::calculate(&scopes, &summaries, indexes, selection);
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
            let nloc = rows.len();
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
    let mut definitions = Vec::new();
    let mut imports = Vec::new();
    let mut exports = Vec::new();
    let mut references = Vec::new();
    let mut locals = Vec::new();
    for item in summaries
        .into_iter()
        .filter(|item| item.resolution_family == resolution_family)
    {
        definitions.extend(item.definitions);
        imports.extend(item.imports);
        exports.extend(item.exports);
        references.extend(item.references);
        locals.extend(item.locals);
    }
    FileFacts {
        target: path.to_path_buf(),
        aliases: vec![path.to_path_buf()],
        path: path.to_path_buf(),
        module: ModulePath::default(),
        resolution_family,
        file_module_rules,
        definitions,
        imports,
        exports,
        references,
        locals,
        analysis: FileAnalysis {
            path: path.display().to_string(),
            hash: blake3::hash(&source).to_hex().to_string(),
            language: root_choice.name.clone(),
            nloc: file_rows.len(),
            maintainability_index: selection.needs_mi().then(|| {
                MaintainabilityIndex::calculate(
                    file_volume,
                    file_cyclomatic_complexity,
                    file_rows.len(),
                )
            }),
            halstead: file_halstead,
            functions: analyses,
            injections,
            resolution: Vec::new(),
        },
        source,
    }
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
        .definitions
        .iter()
        .all(|item| item.context_id == summary.semantic_context)
        && summary
            .imports
            .iter()
            .all(|item| item.context_id == summary.semantic_context)
        && summary
            .exports
            .iter()
            .all(|item| item.context_id == summary.semantic_context)
        && summary
            .references
            .iter()
            .all(|item| item.context_id == summary.semantic_context)
        && summary
            .locals
            .iter()
            .all(|item| item.context_id == summary.semantic_context)
}
