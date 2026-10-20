use crate::graph::FileFactsView;
use crate::graph::ProgramGraph;
use crate::model::Import;

use super::Target;
use super::symbols::local_symbols;

pub(super) enum BindingTarget<'a> {
    Local(Vec<Target>),
    Import(&'a Import),
    Unresolved,
}

pub(super) fn binding_target<'a>(
    fact: &'a FileFactsView<'_>,
    graph: &ProgramGraph<'_>,
    file_index: usize,
    name: &str,
    position: usize,
    context_id: usize,
) -> BindingTarget<'a> {
    let local = local_symbols(graph, file_index, name, position, context_id)
        .map(|(rank, targets)| (rank, BindingTarget::Local(targets)));
    let shadow = local_binding_rank(fact, name, position, context_id)
        .map(|rank| (rank, BindingTarget::Unresolved));
    let imports = fact
        .imports()
        .filter(|import| {
            import.source.is_some()
                && import.alias.as_deref().or(import.imported_name.as_deref()) == Some(name)
                && import.scope_start <= position
                && position < import.scope_end
                && import.context_id == context_id
        })
        .map(|import| {
            (
                import.scope_end.saturating_sub(import.scope_start),
                BindingTarget::Import(import),
            )
        });
    match best_ranked(local.into_iter().chain(shadow).chain(imports)) {
        Some((_, mut targets)) if targets.len() == 1 => targets.pop().unwrap(),
        _ => BindingTarget::Unresolved,
    }
}

/// Retain all candidates in the nearest scope, including ties.
pub(super) fn best_ranked<T>(items: impl Iterator<Item = (usize, T)>) -> Option<(usize, Vec<T>)> {
    let mut best: Option<(usize, Vec<T>)> = None;
    for (rank, item) in items {
        match &mut best {
            Some((nearest, items)) if rank == *nearest => items.push(item),
            Some((nearest, _)) if rank > *nearest => {}
            _ => best = Some((rank, vec![item])),
        }
    }
    best
}

pub(super) fn local_binding_rank(
    fact: &FileFactsView<'_>,
    name: &str,
    position: usize,
    context_id: usize,
) -> Option<usize> {
    let rules = fact.file_module_rules;
    let locals = fact
        .locals()
        .filter(|local| {
            local.name == name
                && local.scope_start <= position
                && position < local.scope_end
                && local.context_id == context_id
                && (local.scope_start > 0
                    || local.scope_end < fact.source.len()
                    || rules.is_some_and(|rules| rules.module_bindings_shadow))
                && !import_local_is_visible(fact, local, name, position, context_id)
        })
        .map(|local| local.scope_end.saturating_sub(local.scope_start));
    let wildcard_imports = fact
        .imports()
        .filter(|import| {
            rules.is_some_and(|rules| rules.wildcard_imports_shadow)
                && import.imported_name.as_deref() == Some("*")
                && import.context_id == context_id
                && import.scope_start <= position
                && position < import.scope_end
        })
        .map(|import| import.scope_end.saturating_sub(import.scope_start));
    locals.chain(wildcard_imports).min()
}

fn import_local_is_visible(
    fact: &FileFactsView<'_>,
    local: &crate::model::LocalBinding,
    name: &str,
    position: usize,
    context_id: usize,
) -> bool {
    fact.imports().any(|import| {
        import.source.is_some()
            && import.alias.as_deref().or(import.imported_name.as_deref()) == Some(name)
            && import.context_id == context_id
            && import.start_byte <= local.start_byte
            && local.end_byte <= import.end_byte
            && import.scope_start <= position
            && position < import.scope_end
    })
}
