use super::super::model::RelationshipKind;
use super::{containing_type, type_id};
use crate::model::{FileFacts, Resolution, SemanticStore};

/// Return the declaration that owns the source position.
///
/// An implementation in another file uses the canonical resolved target from
/// its header. An unresolved or ambiguous target has no inferred owner.
pub(crate) fn type_owner(
    facts: &[FileFacts],
    fact: &FileFacts,
    store: &SemanticStore,
    position: usize,
) -> Option<String> {
    let implementation = store
        .semantic
        .structure
        .iter()
        .filter(|record| record.kind == "impl" && record.start <= position && position < record.end)
        .min_by_key(|record| record.end - record.start);
    let Some(implementation) = implementation else {
        return containing_type(fact, store, position).map(|record| type_id(fact, store, record));
    };
    let target = implementation
        .relations
        .iter()
        .find(|relation| relation.kind == RelationshipKind::Type)?;
    fact.analysis
        .resolution
        .iter()
        .filter(|reference| {
            target.start <= reference.start_byte && reference.end_byte <= target.end
        })
        .find_map(|reference| {
            let Resolution::Exact(symbol) = &reference.resolution else {
                return None;
            };
            let target_fact = facts
                .iter()
                .find(|fact| fact.analysis.path == symbol.file)?;
            target_fact.contexts.iter().find_map(|target_store| {
                let record = target_store
                    .semantic
                    .structure
                    .iter()
                    .filter(|record| {
                        record.is_type
                            && record.name == symbol.name
                            && record.start <= symbol.start_byte
                            && symbol.start_byte < record.end
                    })
                    .min_by_key(|record| record.end - record.start)?;
                Some(type_id(target_fact, target_store, record))
            })
        })
}
