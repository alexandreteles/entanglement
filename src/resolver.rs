//! Resolve source references through shared graphs and language strategies.
//!
//! Language adapters provide syntax evidence, and strategies combine it with
//! static project relationships. Uncertain targets stay ambiguous or unresolved.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use rayon::prelude::*;

use crate::graph::{CallDispatch, FileFactsView, ProgramGraph};
use crate::model::{FileFacts, Reference, ReferenceAnalysis, Resolution, SymbolId};

mod files;
mod indexing;
mod kotlin;
mod module_graph;
mod packages;
mod php;
mod resolution;
mod strategies;

/// Keep a shared-file fallback separate from every logical crate root.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum CrateKey {
    Logical(PathBuf),
    Detached(PathBuf),
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct ModuleKey {
    crate_root: CrateKey,
    path: Vec<String>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum Target {
    Symbol(SymbolId),
    Module(ModuleKey),
    External,
    Ambiguous(Vec<SymbolId>),
    Unresolved,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct FileContext {
    crate_root: CrateKey,
    module: Vec<String>,
}

#[derive(Clone, Debug)]
struct Symbol {
    id: SymbolId,
    file: usize,
    scope_start: usize,
    scope_end: usize,
    is_local: bool,
    is_public: bool,
}

#[derive(Clone, Debug)]
struct ImportEntry {
    file: usize,
    module: ModuleKey,
    name: String,
    path: Vec<String>,
    scope_start: usize,
    scope_end: usize,
    is_local: bool,
    is_public: bool,
}

struct Index {
    contexts: Vec<Vec<FileContext>>,
    symbols: BTreeMap<ModuleKey, BTreeMap<String, Vec<Symbol>>>,
    modules: BTreeSet<ModuleKey>,
    imports: Vec<ImportEntry>,
    import_targets: Vec<Vec<Target>>,
}

/// Resolve references in all facts and store their results in each analysis.
///
/// Call this after all files have been parsed. The function sorts symbols and
/// ambiguity results to keep output stable. Only statically proven execution
/// targets become call-graph edges.
pub(crate) fn resolve_with_snapshot(
    facts: &mut [FileFacts],
    selection: crate::metrics::selection::Selection,
    snapshot: &crate::snapshot::Snapshot,
) {
    let graph = ProgramGraph::new(facts);
    let resolutions = strategies::resolve(facts, snapshot, &graph);
    let calls = selection
        .includes(crate::metrics::selection::Metric::Cogc)
        .then(|| graph.confirmed_relations(&resolutions));
    drop(graph);
    facts
        .par_iter_mut()
        .zip(resolutions)
        .for_each(|(fact, resolved)| fact.analysis.resolution = resolved);
    if let Some(calls) = calls {
        crate::metrics::recursion::annotate(facts, &calls);
    }
}

fn resolve_references(
    facts: &[FileFactsView<'_>],
    snapshot: &crate::snapshot::Snapshot,
    graph: &ProgramGraph<'_>,
    kotlin_cache: &mut kotlin::ProjectCache,
) -> Vec<Vec<ReferenceAnalysis>> {
    let mut index = Index::new(graph);
    index.resolve_imports();
    let file_resolutions = files::resolve(facts, graph);
    let kotlin_resolutions = kotlin::resolve(facts, snapshot, graph, kotlin_cache);
    let package_resolutions = packages::resolve(facts, snapshot, graph);
    let php_resolutions = php::resolve(facts, snapshot, graph);
    facts
        .iter()
        .enumerate()
        .map(|(file_index, fact)| {
            let resolved = file_resolutions[file_index]
                .as_ref()
                .or(kotlin_resolutions[file_index].as_ref())
                .or(php_resolutions[file_index].as_ref())
                .or(package_resolutions[file_index].as_ref());
            resolve_fact(&index, file_index, fact, resolved)
        })
        .collect::<Vec<_>>()
}

fn resolve_fact(
    index: &Index,
    file_index: usize,
    fact: &FileFactsView<'_>,
    resolved: Option<&Vec<ReferenceAnalysis>>,
) -> Vec<ReferenceAnalysis> {
    if let Some(resolved) = resolved {
        return resolved.clone();
    }
    if fact.resolution_family != crate::languages::ResolutionFamily::RustCrates {
        return Vec::new();
    }
    let context = &index.contexts[file_index];
    fact.references()
        .map(|reference| {
            analysis(
                reference,
                index.to_resolution(
                    context
                        .iter()
                        .map(|context| {
                            match index.resolve_reference(file_index, context, reference, fact) {
                                Resolution::Exact(symbol) => Target::Symbol(symbol),
                                Resolution::Ambiguous(symbols) => Target::Ambiguous(symbols),
                                Resolution::External => Target::External,
                                Resolution::Unresolved => Target::Unresolved,
                            }
                        })
                        .collect(),
                ),
                None,
            )
        })
        .collect()
}

/// Build a reference result with no execution targets.
fn analysis(
    reference: &Reference,
    resolution: Resolution,
    dispatch: Option<CallDispatch>,
) -> ReferenceAnalysis {
    ReferenceAnalysis {
        path: reference.path.clone(),
        start_byte: reference.start_byte,
        end_byte: reference.end_byte,
        resolution,
        dispatch,
        execution_targets: Vec::new(),
    }
}

fn joined_path(base: &[String], relative: &[String]) -> Vec<String> {
    base.iter().chain(relative).cloned().collect()
}

/// Return a value only when the input contains exactly one item.
fn only<T>(items: impl IntoIterator<Item = T>) -> Option<T> {
    let mut items = items.into_iter();
    let first = items.next()?;
    items.next().is_none().then_some(first)
}

/// Resolve captured literal resources with the shared file and alias index.
pub(crate) fn structural_resources(
    facts: &[FileFacts],
) -> Vec<(
    usize,
    usize,
    crate::structure::resources::ResourceFact,
    Resolution,
)> {
    let graph = ProgramGraph::new(facts);
    let views = facts.iter().map(FileFactsView::new).collect::<Vec<_>>();
    let resources = facts
        .iter()
        .enumerate()
        .flat_map(|(file, fact)| {
            fact.contexts.iter().flat_map(move |store| {
                store
                    .semantic
                    .structure_resources
                    .iter()
                    .cloned()
                    .map(move |resource| (file, store.semantic.context_id, resource))
            })
        })
        .collect();
    files::structural_resources(&views, &graph, resources)
}

/// A language resolver's selected origins for one effective member name.
pub(crate) struct StructuralMemberSelection {
    pub owner: SymbolId,
    pub name: String,
    pub origins: Vec<SymbolId>,
    pub visibility: BTreeMap<SymbolId, String>,
    pub obligations: Vec<String>,
}

/// Read selected member origins through the existing language strategies.
pub(crate) fn structural_members(
    facts: &[FileFacts],
    snapshot: &crate::snapshot::Snapshot,
) -> Vec<StructuralMemberSelection> {
    let graph = ProgramGraph::new(facts);
    let views = facts.iter().map(FileFactsView::new).collect::<Vec<_>>();
    let mut selections = php::structural_members(&graph, snapshot);
    selections.extend(kotlin::structural_members(&views, &graph, snapshot));
    selections
}

/// Read import and re-export module targets through the existing file strategy.
pub(crate) fn structural_modules(
    facts: &[FileFacts],
) -> Vec<(usize, usize, usize, String, Resolution)> {
    let graph = ProgramGraph::new(facts);
    let views = facts.iter().map(FileFactsView::new).collect::<Vec<_>>();
    files::structural_modules(&views, &graph)
}

/// Read canonical Go package identities without selecting build-tag variants.
pub(crate) fn structural_go_contexts(facts: &[FileFacts]) -> BTreeMap<(usize, usize), String> {
    packages::structural_contexts(&ProgramGraph::new(facts))
}

/// A language-resolved type relation that has no generic reference capture.
pub(crate) struct StructuralTypeRelationship {
    pub owner: SymbolId,
    pub target: Resolution,
    pub name: String,
    pub kind: crate::structure::model::RelationshipKind,
    pub start: usize,
}

/// Read canonical language composition and inheritance relationships.
pub(crate) fn structural_type_relationships(
    facts: &[FileFacts],
    snapshot: &crate::snapshot::Snapshot,
) -> Vec<StructuralTypeRelationship> {
    php::structural_type_relationships(&ProgramGraph::new(facts), snapshot)
}
