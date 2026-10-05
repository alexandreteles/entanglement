use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use crate::languages::{CapturedTree, LanguageChoice};
use crate::metrics::selection::Selection;
use crate::metrics::{
    SyntaxEvent, cyclomatic::FunctionScope, halstead::HalsteadToken,
    maintainability::MaintainabilityIndex, nloc,
};
use crate::model::{
    Definition, FileAnalysis, FileFacts, FunctionAnalysis, Import, InjectionAnalysis, LocalBinding,
    ModulePath, Reference,
};

use super::injections::{self, ParsedInjection};
use super::metrics;

pub(super) struct TreeSummary {
    language_id: String,
    pub(super) events: Vec<SyntaxEvent>,
    pub(super) parents: HashMap<usize, Option<usize>>,
    pub(super) cognitive_parent: Option<usize>,
    functions: Vec<FunctionScope>,
    pub(super) tokens: Vec<HalsteadToken>,
    pub(super) excluded: Vec<Range<usize>>,
    definitions: Vec<Definition>,
    imports: Vec<Import>,
    references: Vec<Reference>,
    locals: Vec<LocalBinding>,
}

pub(super) fn summary(
    choice: &LanguageChoice,
    captured: CapturedTree,
    excluded: Vec<Range<usize>>,
    inherited_module: &ModulePath,
    owner_range: &Range<usize>,
    inherited_scope: Option<&Range<usize>>,
) -> TreeSummary {
    let prefix = &inherited_module.0;
    let mut definitions = captured.definitions;
    definitions.retain(|item| !inside_any(item.start_byte..item.end_byte, &excluded));
    for definition in &mut definitions {
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
        inherit_scope(
            &mut import.scope_start,
            &mut import.scope_end,
            owner_range,
            inherited_scope,
        );
        prepend_module(&mut import.module, prefix);
    }
    let mut references = captured.references;
    references.retain(|item| !inside_any(item.start_byte..item.end_byte, &excluded));
    for reference in &mut references {
        inherit_scope(
            &mut reference.scope_start,
            &mut reference.scope_end,
            owner_range,
            inherited_scope,
        );
        prepend_module(&mut reference.module, prefix);
    }
    let mut locals = captured.locals;
    locals.retain(|item| !inside_any(item.start_byte..item.end_byte, &excluded));
    for local in &mut locals {
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
        language_id: choice.id.clone(),
        events: captured.events,
        parents: captured.parents,
        cognitive_parent: None,
        functions,
        tokens,
        excluded,
        definitions,
        imports,
        references,
        locals,
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
    let (file_volume, file_halstead) = metrics::file_halstead(&summaries, selection);
    let mut file_rows = BTreeSet::new();
    for item in &summaries {
        file_rows.extend(nloc::rows(&item.events, 0..source.len(), &item.excluded));
    }

    let mut by_language = BTreeMap::<String, Vec<usize>>::new();
    let mut functions = BTreeMap::<(usize, usize, String, String), FunctionScope>::new();
    for (index, item) in summaries.iter().enumerate() {
        by_language
            .entry(item.language_id.clone())
            .or_default()
            .push(index);
        for function in &item.functions {
            functions
                .entry((
                    function.range.start,
                    function.range.end,
                    function.name.clone(),
                    item.language_id.clone(),
                ))
                .or_insert_with(|| function.clone());
        }
    }

    let mut functions_by_language = BTreeMap::<String, Vec<FunctionScope>>::new();
    for ((_, _, _, language), function) in functions {
        functions_by_language
            .entry(language)
            .or_default()
            .push(function);
    }

    let mut analyses = Vec::new();
    let mut file_cyclomatic_complexity = 0;
    for (language, scopes) in functions_by_language {
        let indexes = &by_language[&language];
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
    let mut references = Vec::new();
    let mut locals = Vec::new();
    for item in summaries
        .into_iter()
        .filter(|item| item.language_id == root_choice.id)
    {
        definitions.extend(item.definitions);
        imports.extend(item.imports);
        references.extend(item.references);
        locals.extend(item.locals);
    }
    FileFacts {
        target: path.to_path_buf(),
        aliases: vec![path.to_path_buf()],
        path: path.to_path_buf(),
        module: ModulePath::default(),
        definitions,
        imports,
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
