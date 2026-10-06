use crate::model::{FileFacts, Import};

#[derive(Clone, Copy)]
pub(super) enum ImportLookup<'a> {
    None,
    One(&'a Import, usize),
    Ambiguous(usize),
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
                && position <= import.scope_end
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
    fact.locals
        .iter()
        .filter(|local| {
            local.name == name
                && local.scope_start <= position
                && position <= local.scope_end
                && local.context_id == context_id
                && (local.scope_start > 0 || local.scope_end < fact.source.len())
        })
        .map(|local| local.scope_end.saturating_sub(local.scope_start))
        .min()
}
