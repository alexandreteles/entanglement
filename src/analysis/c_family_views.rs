use super::{ParsedFile, Worker};
use crate::languages::c_family::{
    project::{Project, view::preprocess},
    semantic,
};
use crate::model::SemanticStore;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// Store each distinct preprocessing view once. Keep the physical report's
/// source identities available to the shared call and structural graphs.
pub(super) fn collect(
    file: &mut ParsedFile,
    project: &mut Project,
    worker: &mut Worker,
) -> crate::Result<()> {
    let mut cache = BTreeMap::new();
    let source = &file.facts.source;
    let Some(host) = file
        .facts
        .contexts
        .iter_mut()
        .find(|store| store.semantic.context_id == 0)
    else {
        return Ok(());
    };
    let mut definitions = BTreeSet::new();
    let mut references = BTreeSet::new();
    let structure = worker
        .selection
        .includes(crate::metrics::selection::Metric::Structure);
    let mut records = BTreeSet::new();
    let mut errors = BTreeSet::new();
    host.semantic.structure.clear();
    host.definitions.clear();
    host.references.clear();
    for unit in &mut project.units {
        let Some(view) = unit.files.get_mut(&file.facts.target) else {
            continue;
        };
        let cpp = unit.cpp;
        let projection = preprocess(source, view);
        let bytes = &projection.bytes;
        let key = (
            cpp,
            *blake3::hash(bytes).as_bytes(),
            projection.mapping_key(),
        );
        let store = match cache.entry(key) {
            std::collections::btree_map::Entry::Occupied(entry) => Arc::clone(entry.get()),
            std::collections::btree_map::Entry::Vacant(entry) => {
                let choice = worker
                    .registry
                    .select_injection(if cpp { "cpp" } else { "c" })?
                    .ok_or("Missing C-family grammar")?;
                let tree = worker.parse_tree(&choice, bytes, &[], None)?;
                let mut captured = semantic::capture(&tree, bytes);
                if structure {
                    crate::structure::extract::capture(&tree, bytes, &choice.id, &mut captured);
                }
                projection.semantic(&mut captured);
                if let Some(data) = captured.semantic.c_family.as_ref() {
                    errors.extend(data.errors.iter().copied());
                }
                host.semantic.structure.extend(
                    captured
                        .semantic
                        .structure
                        .iter()
                        .filter(|record| {
                            records.insert((
                                record.start,
                                record.name.clone(),
                                record.owner.clone(),
                                record.kind.clone(),
                            ))
                        })
                        .cloned(),
                );
                let store = Arc::new(captured);
                merge(host, &store, &mut definitions, &mut references);
                entry.insert(Arc::clone(&store));
                store
            }
        };
        view.semantic = Some(store);
    }
    host.semantic.structure_errors = !errors.is_empty();
    if let Some(data) = host.semantic.c_family.as_mut() {
        data.errors = errors.into_iter().collect();
    }
    host.references
        .sort_by_key(|reference| (reference.start_byte, reference.end_byte));
    Ok(())
}

fn merge(
    host: &mut SemanticStore,
    view: &SemanticStore,
    definitions: &mut BTreeSet<(usize, String, crate::model::DefinitionKind)>,
    references: &mut BTreeSet<(usize, usize, Vec<String>)>,
) {
    host.definitions.extend(
        view.definitions
            .iter()
            .filter(|def| definitions.insert((def.start_byte, def.name.clone(), def.kind.clone())))
            .cloned(),
    );
    host.references.extend(
        view.references
            .iter()
            .filter(|item| references.insert((item.start_byte, item.end_byte, item.path.clone())))
            .cloned(),
    );
}
