use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use crate::languages::{CapturedTree, FileModuleRules, LanguageChoice, ResolutionFamily};
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

pub(super) fn summary(
    choice: &LanguageChoice,
    captured: CapturedTree,
    context: SummaryContext<'_>,
) -> TreeSummary {
    let SummaryContext {
        semantic_context,
        metric_id,
        excluded,
        inherited_module,
        owner_range,
        inherited_scope,
    } = context;
    let prefix = &inherited_module.0;
    let mut definitions = captured.definitions;
    definitions.retain(|item| !inside_any(item.start_byte..item.end_byte, &excluded));
    for definition in &mut definitions {
        definition.context_id = semantic_context;
        inherit_scope(
            &mut definition.scope_start,
            &mut definition.scope_end,
            owner_range,
            inherited_scope,
        );
        prepend_module(&mut definition.module, prefix);
    }
    let mut imports = captured.imports;
    imports.retain(|item| !inside_any(item.start_byte..item.end_byte, &excluded));
    for import in &mut imports {
        import.context_id = semantic_context;
        if import.scope_start == 0 && import.scope_end == 0 {
            import.scope_start = owner_range.start;
            import.scope_end = owner_range.end;
        } else {
            inherit_scope(
                &mut import.scope_start,
                &mut import.scope_end,
                owner_range,
                inherited_scope,
            );
        }
        prepend_module(&mut import.module, prefix);
    }
    let mut exports = captured.exports;
    exports.retain(|item| !inside_any(item.start_byte..item.end_byte, &excluded));
    for export in &mut exports {
        export.context_id = semantic_context;
        inherit_scope(
            &mut export.scope_start,
            &mut export.scope_end,
            owner_range,
            inherited_scope,
        );
    }
    let mut references = captured.references;
    references.retain(|item| !inside_any(item.start_byte..item.end_byte, &excluded));
    for reference in &mut references {
        reference.context_id = semantic_context;
        inherit_scope(
            &mut reference.scope_start,
            &mut reference.scope_end,
            owner_range,
            inherited_scope,
        );
        if reference.call_owner.is_none()
            && reference.kind == crate::model::ReferenceKind::Call
            && let Some(scope) = inherited_scope
        {
            reference.call_owner = Some(scope.start);
        }
        prepend_module(&mut reference.module, prefix);
    }
    let mut locals = captured.locals;
    locals.retain(|item| !inside_any(item.start_byte..item.end_byte, &excluded));
    for local in &mut locals {
        local.context_id = semantic_context;
        inherit_scope(
            &mut local.scope_start,
            &mut local.scope_end,
            owner_range,
            inherited_scope,
        );
    }
    let functions = captured
        .functions
        .into_iter()
        .filter(|item| !inside_any(item.range.clone(), &excluded))
        .collect();
    let mut tokens = captured.tokens;
    tokens.retain(|token| !inside_any(token.start_byte..token.end_byte, &excluded));
    TreeSummary {
        metric_id,
        resolution_family: choice.resolution_family,
        file_module_rules: choice.file_module_rules,
        semantic_context,
        events: captured.events,
        parents: captured.parents,
        cognitive_parent: None,
        functions,
        tokens,
        excluded,
        definitions,
        imports,
        exports,
        references,
        locals,
    }
}

impl TreeSummary {
    pub(super) fn discard_exports(&mut self) {
        self.exports.clear();
    }

    pub(super) fn share_top_level_bindings_into(
        &self,
        target: &mut TreeSummary,
        owner_range: &Range<usize>,
        target_scope: &Range<usize>,
    ) {
        target.definitions.extend(
            self.definitions
                .iter()
                .filter(|item| {
                    item.scope_start <= owner_range.start && owner_range.end <= item.scope_end
                })
                .cloned()
                .map(|mut item| {
                    item.context_id = target.semantic_context;
                    item.scope_start = target_scope.start;
                    item.scope_end = target_scope.end;
                    item
                }),
        );
        target.imports.extend(
            self.imports
                .iter()
                .filter(|item| {
                    item.scope_start <= owner_range.start && owner_range.end <= item.scope_end
                })
                .cloned()
                .map(|mut item| {
                    item.context_id = target.semantic_context;
                    item.scope_start = target_scope.start;
                    item.scope_end = target_scope.end;
                    item
                }),
        );
    }
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

fn prepend_module(module: &mut ModulePath, prefix: &[String]) {
    if !prefix.is_empty() {
        let mut path = prefix.to_vec();
        path.append(&mut module.0);
        module.0 = path;
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

fn inherit_scope(
    scope_start: &mut usize,
    scope_end: &mut usize,
    owner_range: &Range<usize>,
    inherited_scope: Option<&Range<usize>>,
) {
    if let Some(scope) = inherited_scope
        .filter(|_| *scope_start <= owner_range.start && owner_range.end <= *scope_end)
    {
        *scope_start = scope.start;
        *scope_end = scope.end;
    }
}

fn inside_any(range: Range<usize>, excluded: &[Range<usize>]) -> bool {
    excluded
        .iter()
        .any(|item| item.start <= range.start && range.end <= item.end)
}
