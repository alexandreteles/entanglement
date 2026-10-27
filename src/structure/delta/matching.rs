use super::*;

pub(super) fn rename_candidates(
    before: &StructureSnapshot,
    after: &StructureSnapshot,
    previous_units: &BTreeSet<String>,
    current_units: &BTreeSet<String>,
    granularity: UnitKind,
) -> BTreeSet<String> {
    let previous = before
        .units
        .iter()
        .filter(|unit| unit.kind == granularity && !current_units.contains(&unit.id));
    let current: Vec<_> = after
        .units
        .iter()
        .filter(|unit| unit.kind == granularity && !previous_units.contains(&unit.id))
        .collect();
    previous
        .flat_map(|previous| {
            current
                .iter()
                .filter(move |current| {
                    let same_source = previous.source_hash.as_ref().is_some_and(|hash| {
                        !hash.is_empty() && current.source_hash.as_ref() == Some(hash)
                    }) && (granularity == UnitKind::Module
                        || previous.name == current.name);
                    let same_structure =
                        previous
                            .structural_fingerprint
                            .as_ref()
                            .is_some_and(|fingerprint| {
                                !fingerprint.is_empty()
                                    && current.structural_fingerprint.as_ref() == Some(fingerprint)
                            });
                    same_source || same_structure
                })
                .flat_map(|current| [previous.id.clone(), current.id.clone()])
        })
        .collect()
}
