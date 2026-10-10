use std::collections::HashSet;
use std::ops::Range;

use crate::languages::{CapturedTree, LanguageChoice};
use crate::model::{LocalBinding, ModulePath};

use super::{SummaryContext, TreeSummary};

pub(in crate::analysis) fn summary(
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
    /// Add explicit runtime bindings to this layer's isolated semantic context.
    pub(in crate::analysis) fn add_serialized_bindings(&mut self, mut bindings: Vec<LocalBinding>) {
        let mut seen = HashSet::new();
        bindings.retain(|binding| {
            seen.insert((
                binding.name.clone(),
                binding.start_byte,
                binding.end_byte,
                binding.scope_start,
                binding.scope_end,
            ))
        });
        self.locals.extend(bindings.into_iter().map(|mut binding| {
            binding.context_id = self.semantic_context;
            binding
        }));
    }

    pub(in crate::analysis) fn discard_exports(&mut self) {
        self.exports.clear();
    }

    pub(in crate::analysis) fn share_top_level_bindings_into(
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
