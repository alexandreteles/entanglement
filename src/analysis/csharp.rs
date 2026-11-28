mod physical;
use super::{ParsedFile, Worker, facts};
use crate::languages::csharp::{conditional, facts::Data, project::Project};
use crate::{Result, metrics::selection::Selection, model::SemanticStore, snapshot::Snapshot};
use std::collections::BTreeSet;

pub(super) fn prepare(
    files: &mut [ParsedFile],
    other: &[crate::model::FileFacts],
    selection: Selection,
    snapshot: &mut Snapshot,
) -> Result<()> {
    let paths = files
        .iter()
        .filter(|f| matches!(f.language.as_str(), "source.cs" | "source.razor"))
        .map(|file| &file.facts)
        .chain(
            other
                .iter()
                .filter(|fact| crate::languages::csharp::is_source(&fact.path)),
        )
        .map(|fact| fact.target.clone())
        .collect::<Vec<_>>();
    if paths.is_empty() {
        return Ok(());
    }
    let root = snapshot
        .csharp_root
        .as_deref()
        .or_else(|| paths[0].parent())
        .ok_or("Missing C# source directory")?;
    let mut project = Project::load(root, &paths, snapshot)?;
    for fact in files
        .iter()
        .map(|file| &file.facts)
        .chain(other)
        .filter(|fact| fact.aliases.iter().any(|alias| alias != &fact.target))
    {
        project.include_aliases(&fact.target, &fact.aliases);
    }
    let mut worker = Worker::new(selection)?;
    for file in files.iter_mut().filter(|f| f.language == "source.cs") {
        let lines = conditional::lines(file.tree.root_node(), &file.facts.source);
        if !lines.iter().any(|line| line.command == "if") {
            continue;
        }
        let owners = project.owners(&file.facts.target).to_vec();
        let mut masks = BTreeSet::new();
        for view in owners {
            let mask = conditional::mask(
                &lines,
                file.facts.source.len(),
                &project.views[view].symbols,
            );
            masks.insert(mask.clone());
            project
                .masks
                .entry(file.facts.target.clone())
                .or_default()
                .insert(view, mask);
        }
        let detached = masks.is_empty();
        let choice = worker
            .select_file(&file.facts.path)?
            .ok_or("Missing C# grammar")?;
        let mut merged = SemanticStore::default();
        let mut physical = physical::Physical::default();
        let original = worker.registry.capture(
            &choice.id,
            &file.tree,
            &file.facts.source,
            selection.needs_halstead(),
        )?;
        for mask in conditional::coverage(&lines, file.facts.source.len(), &masks) {
            let bytes = mask.bytes(&file.facts.source);
            let tree = worker.parse_tree(&choice, &bytes, &[], None)?;
            let mut captured =
                worker
                    .registry
                    .capture(&choice.id, &tree, &bytes, selection.needs_halstead())?;
            if detached || masks.contains(&mask) {
                if selection.includes(crate::metrics::selection::Metric::Structure) {
                    crate::structure::extract::capture(
                        &tree,
                        &bytes,
                        &choice.id,
                        &mut captured.facts,
                    );
                }
                merged.semantic.structure_errors |= tree.root_node().has_error() || mask.uncertain;
                merge(&mut merged, std::mem::take(&mut captured.facts));
            }
            physical.add(&tree, captured);
        }
        physical.directives(&lines, original);
        let range = 0..file.facts.source.len();
        let summary = facts::summary(
            &choice,
            physical.captured,
            facts::SummaryContext {
                semantic_context: 0,
                metric_id: choice.id.clone(),
                excluded: vec![],
                inherited_module: &crate::model::ModulePath::default(),
                owner_range: &range,
                inherited_scope: None,
            },
        );
        file.facts.analysis = facts::build_facts(
            &file.facts.path,
            file.facts.source.clone(),
            &choice,
            vec![summary],
            &[],
            selection,
        )
        .analysis;
        let host = &mut file.facts.contexts[0];
        host.definitions = merged.definitions;
        host.definitions.sort_by_key(|d| d.start_byte);
        host.definitions.dedup_by_key(|d| d.start_byte);
        host.references = merged.references;
        host.references.sort_by_key(|r| (r.start_byte, r.end_byte));
        host.references.dedup_by(|a, b| {
            a.start_byte == b.start_byte && a.end_byte == b.end_byte && a.kind == b.kind
        });
        let data = merged
            .semantic
            .csharp
            .as_deref_mut()
            .expect("projected C# facts");
        data.imports.sort_by_key(|u| u.position);
        data.imports.dedup_by_key(|u| u.position);
        data.bindings.sort_by_key(|b| b.position);
        data.bindings.dedup_by_key(|b| b.position);
        data.errors.sort_by_key(|r| (r.start, r.end));
        data.errors.dedup();
        data.directives = lines;
        if let Some(raw) = host.semantic.csharp.as_ref() {
            data.script = raw.script.clone();
        }
        host.semantic.csharp = merged.semantic.csharp;
        host.semantic.structure_errors = merged.semantic.structure_errors;
    }
    for file in files.iter_mut().filter(|f| f.language == "source.razor") {
        if file.facts.contexts[0].semantic.csharp.is_some() {
            let choice = worker
                .select_file(&file.facts.path)?
                .ok_or("Missing Razor grammar")?;
            *file = worker.analyze_selected_source(
                &file.facts.path,
                file.facts.source.to_vec(),
                Some(file),
                &[],
                choice,
            )?;
        }
        crate::languages::csharp::razor::bind(&mut file.facts, &file.tree, &project, snapshot);
    }
    snapshot.csharp = project;
    Ok(())
}

fn merge(output: &mut SemanticStore, mut store: SemanticStore) {
    output.definitions.append(&mut store.definitions);
    output.references.append(&mut store.references);
    let target = output
        .semantic
        .csharp
        .get_or_insert_with(|| Box::new(Data::default()));
    if let Some(mut data) = store.semantic.csharp {
        target.declarations.append(&mut data.declarations);
        target.uses.append(&mut data.uses);
        target.bindings.append(&mut data.bindings);
        target.imports.append(&mut data.imports);
        target.errors.append(&mut data.errors);
    }
}
