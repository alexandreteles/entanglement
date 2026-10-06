use crate::model::{FileFacts, Import};

use super::Target;
use super::symbols::local_symbols;

#[derive(Clone, Copy)]
pub(super) enum ImportLookup<'a> {
    None,
    One(&'a Import, usize),
    Ambiguous(usize),
}

pub(super) enum BindingTarget<'a> {
    Local(Vec<Target>),
    Import(&'a Import),
    Unresolved,
}

pub(super) fn binding_target<'a>(
    fact: &'a FileFacts,
    name: &str,
    position: usize,
    context_id: usize,
) -> BindingTarget<'a> {
    let local = local_symbols(fact, name, position, context_id);
    let local_rank = local.as_ref().map(|(rank, _)| *rank);
    let shadow_rank = local_binding_rank(fact, name, position, context_id);
    let import = imported_binding(fact, name, position, context_id);
    let import_rank = match import {
        ImportLookup::One(_, rank) | ImportLookup::Ambiguous(rank) => Some(rank),
        ImportLookup::None => None,
    };
    let best_rank = [local_rank, shadow_rank, import_rank]
        .into_iter()
        .flatten()
        .min();
    let Some(best_rank) = best_rank else {
        return BindingTarget::Unresolved;
    };
    let winners = usize::from(local_rank == Some(best_rank))
        + usize::from(shadow_rank == Some(best_rank))
        + usize::from(import_rank == Some(best_rank));
    if winners != 1 || shadow_rank == Some(best_rank) {
        return BindingTarget::Unresolved;
    }
    if local_rank == Some(best_rank) {
        return local
            .map(|(_, targets)| BindingTarget::Local(targets))
            .unwrap_or(BindingTarget::Unresolved);
    }
    match import {
        ImportLookup::One(import, _) if import_rank == Some(best_rank) => {
            BindingTarget::Import(import)
        }
        _ => BindingTarget::Unresolved,
    }
}

pub(super) fn imported_binding<'a>(
    fact: &'a FileFacts,
    name: &str,
    position: usize,
    context_id: usize,
) -> ImportLookup<'a> {
    let imports = fact
        .imports
        .iter()
        .filter(|import| {
            import.source.is_some()
                && import.alias.as_deref().or(import.imported_name.as_deref()) == Some(name)
                && import.scope_start <= position
                && position < import.scope_end
                && import.context_id == context_id
        })
        .collect::<Vec<_>>();
    let Some(rank) = imports
        .iter()
        .map(|import| import.scope_end.saturating_sub(import.scope_start))
        .min()
    else {
        return ImportLookup::None;
    };
    let mut best = imports
        .into_iter()
        .filter(|import| import.scope_end.saturating_sub(import.scope_start) == rank);
    let Some(import) = best.next() else {
        return ImportLookup::None;
    };
    if best.next().is_some() {
        ImportLookup::Ambiguous(rank)
    } else {
        ImportLookup::One(import, rank)
    }
}

pub(super) fn local_binding_rank(
    fact: &FileFacts,
    name: &str,
    position: usize,
    context_id: usize,
) -> Option<usize> {
    let rules = fact.file_module_rules;
    let locals = fact
        .locals
        .iter()
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
        .imports
        .iter()
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
    fact: &FileFacts,
    local: &crate::model::LocalBinding,
    name: &str,
    position: usize,
    context_id: usize,
) -> bool {
    fact.imports.iter().any(|import| {
        import.source.is_some()
            && import.alias.as_deref().or(import.imported_name.as_deref()) == Some(name)
            && import.context_id == context_id
            && import.start_byte <= local.start_byte
            && local.end_byte <= import.end_byte
            && import.scope_start <= position
            && position < import.scope_end
    })
}
