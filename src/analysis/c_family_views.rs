use super::{ParsedFile, Worker};
use crate::languages::c_family::{
    project::{FileView, Project, view::preprocess},
    semantic,
};
use crate::model::{
    Definition, DefinitionKind, ModulePath, Reference, ReferenceKind, SemanticStore,
};
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
            view.definitions.keys().copied().collect::<Vec<_>>(),
            view.uses.keys().copied().collect::<Vec<_>>(),
        );
        let store = match cache.entry(key) {
            std::collections::btree_map::Entry::Occupied(entry) => Arc::clone(entry.get()),
            std::collections::btree_map::Entry::Vacant(entry) => {
                let choice = worker
                    .registry
                    .select_injection(if cpp { "cpp" } else { "c" })?
                    .ok_or("Missing C-family grammar")?;
                let tree = worker.parse_tree(&choice, bytes, &[], None)?;
                let mut captured = semantic::capture(&tree, bytes, cpp);
                if structure {
                    crate::structure::extract::capture(&tree, bytes, &choice.id, &mut captured);
                }
                projection.semantic(&mut captured);
                capture_macros(view, source, &mut captured);
                if let Some(data) = captured.semantic.c_family.as_ref() {
                    errors.extend(data.errors.iter().copied());
                    if let Some(physical) = host.semantic.c_family.as_mut() {
                        physical.virtual_bases.extend(&data.virtual_bases);
                        physical.visibility.extend(data.visibility.clone());
                    }
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
        data.missing_build = project
            .units
            .iter()
            .filter(|unit| unit.files.contains_key(&file.facts.target))
            .all(|unit| !unit.configured);
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

/// Record written macro uses after mapping parser offsets back to the source.
fn capture_macros(view: &FileView, source: &[u8], store: &mut SemanticStore) {
    store.definitions.extend(
        view.definitions
            .iter()
            .map(|(&start, (name, end))| Definition {
                name: name.clone(),
                kind: DefinitionKind::Macro,
                module: ModulePath::default(),
                start_byte: start,
                end_byte: *end,
                scope_start: 0,
                scope_end: source.len(),
                is_public: true,
                inline_module: false,
                external_module: false,
                context_id: 0,
                declaration: None,
            }),
    );
    if view.uses.is_empty() {
        return;
    }
    store.references.retain(|item| {
        !view.uses.get(&item.start_byte).is_some_and(|usage| {
            usage.range.end == item.end_byte
                && item.path.len() == 1
                && item.path[0].as_bytes() == &source[usage.range.clone()]
        })
    });
    store
        .references
        .extend(view.uses.values().map(|usage| Reference {
            path: vec![String::from_utf8_lossy(&source[usage.range.clone()]).into_owned()],
            module: ModulePath::default(),
            kind: ReferenceKind::Macro,
            start_byte: usage.range.start,
            end_byte: usage.range.end,
            scope_start: 0,
            scope_end: source.len(),
            call_owner: None,
            execution_owner: None,
            access: None,
            context_id: 0,
        }));
}
