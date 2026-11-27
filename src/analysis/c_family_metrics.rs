use super::{ParsedFile, Worker, facts};
use crate::languages::c_family::{
    physical,
    project::{FileView, Project, Replacement, projection::Projection},
};
use crate::metrics::selection::Metric;
use crate::model::ModulePath;
use std::collections::BTreeMap;

fn replacements<'a>(views: impl Iterator<Item = &'a FileView>) -> Vec<Replacement> {
    let views: Vec<_> = views.collect();
    let mut result = BTreeMap::new();
    for item in views
        .iter()
        .flat_map(|view| view.uses.values())
        .filter(|item| item.text.is_some())
    {
        if !result.contains_key(&item.range.start)
            && views.iter().all(|view| {
                view.position(item.range.start).is_none()
                    || view
                        .uses
                        .get(&item.range.start)
                        .is_some_and(|other| other.text == item.text)
            })
        {
            result.insert(item.range.start, item.clone());
        }
    }
    result.into_values().collect()
}

/// Rebuild physical scopes from the written branches and map grammar recovery
/// back to the input bytes. Use original tokens for size and Halstead metrics.
pub(super) fn recover(
    file: &mut ParsedFile,
    project: &Project,
    worker: &mut Worker,
) -> crate::Result<()> {
    let raw = file.facts.source.clone();
    let has_includer = project.has_includer(&file.facts.target);
    let uses = replacements(
        project
            .units
            .iter()
            .filter(|unit| !unit.fallback || !has_includer)
            .filter_map(|unit| unit.files.get(&file.facts.target)),
    );
    let projection = Projection::new(&raw, uses.iter());
    if !projection.changed() && !file.recovered {
        return Ok(());
    }
    let cpp = file.language == "source.cpp";
    let choice = worker
        .registry
        .select_injection(if cpp { "cpp" } else { "c" })?
        .ok_or("Missing C-family grammar")?;
    let tree = if projection.changed() {
        worker.parse_tree(&choice, &projection.bytes, &[], None)?
    } else {
        file.tree.clone()
    };
    let mut captured = worker
        .registry
        .capture(&choice.id, &tree, &projection.bytes, false)?;
    if worker.selection.includes(Metric::Structure) {
        crate::structure::extract::capture(
            &tree,
            &projection.bytes,
            &choice.id,
            &mut captured.facts,
        );
    }
    projection.semantic(&mut captured.facts);
    for function in &mut captured.functions {
        projection.range(&mut function.range.start, &mut function.range.end);
    }
    for event in &mut captured.events {
        projection.range(&mut event.start_byte, &mut event.end_byte);
    }
    let range = 0..raw.len();
    physical::capture(
        &raw,
        std::iter::once(range.clone()),
        cpp,
        worker.selection.needs_halstead(),
        &mut captured,
    );
    let summary = facts::summary(
        &choice,
        captured,
        facts::SummaryContext {
            semantic_context: 0,
            metric_id: choice.id.clone(),
            excluded: vec![],
            inherited_module: &ModulePath::default(),
            owner_range: &range,
            inherited_scope: None,
        },
    );
    let mut updated = facts::build_facts(
        &file.facts.path,
        raw,
        &choice,
        vec![summary],
        &[],
        worker.selection,
    );
    updated.target = file.facts.target.clone();
    updated.aliases = file.facts.aliases.clone();
    file.facts = updated;
    file.recovered = projection.changed();
    Ok(())
}
