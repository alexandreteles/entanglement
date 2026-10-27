//! Resolve PHP namespaces and members over the shared semantic graph.

use std::collections::{HashMap, HashSet};

use crate::graph::{
    CallDispatch, DeclarationFacts, FileFactsView, PhpDeclaration, PhpFacts, PhpReference,
    PhpReferenceKind, PhpSymbolKind, ProgramGraph, SemanticVisibility,
};
use crate::languages::ResolutionFamily;
use crate::model::{Reference, ReferenceAnalysis, Resolution, SymbolId};
use crate::snapshot::Snapshot;

mod execution;
mod members;
mod names;
mod project;
mod reporting;
mod resolution;
mod structural;
mod visibility;

use super::analysis;
use members::Member;
use project::Projects;
pub(super) use reporting::{structural_members, structural_type_relationships};

#[derive(Clone, Copy)]
struct Site {
    file: usize,
    context: usize,
    position: usize,
}

impl Site {
    fn at(file: usize, reference: &Reference) -> Self {
        Self {
            file,
            context: reference.context_id,
            position: reference.start_byte,
        }
    }
}

struct Candidate<'a> {
    site: Site,
    symbol: &'a SymbolId,
    php: &'a PhpDeclaration,
    declaration: &'a DeclarationFacts,
    name: String,
    owner: Option<&'a str>,
}

struct Index<'a> {
    graph: &'a ProgramGraph<'a>,
    projects: Projects,
    candidates: Vec<Candidate<'a>>,
    symbols: HashMap<(Option<String>, PhpSymbolKind, String), Vec<usize>>,
}

pub(super) fn resolve(
    facts: &[FileFactsView<'_>],
    snapshot: &Snapshot,
    graph: &ProgramGraph<'_>,
) -> Vec<Option<Vec<ReferenceAnalysis>>> {
    if !facts
        .iter()
        .any(|fact| fact.resolution_family == ResolutionFamily::PhpNamespaces)
    {
        return vec![None; facts.len()];
    }
    let index = Index::new(graph, snapshot);
    facts
        .iter()
        .enumerate()
        .map(|(file, fact)| {
            (fact.resolution_family == ResolutionFamily::PhpNamespaces).then(|| {
                fact.references()
                    .map(|reference| index.resolve(file, reference))
                    .collect()
            })
        })
        .collect()
}

impl<'a> Index<'a> {
    fn new(graph: &'a ProgramGraph<'a>, snapshot: &Snapshot) -> Self {
        let candidates = (0..graph.file_count())
            .flat_map(|file| graph.symbols_for_file(file))
            .filter_map(|symbol| {
                let site = Site {
                    file: symbol.id.file_index,
                    context: symbol.scope.context_id,
                    position: symbol.semantic_id.start_byte,
                };
                let semantic = graph
                    .semantic(
                        site.file,
                        site.context,
                        Some(ResolutionFamily::PhpNamespaces),
                    )
                    .next()?;
                let facts = semantic.semantic.php.as_ref()?;
                let declaration = symbol.definition.declaration.as_deref()?;
                let php = declaration.php.as_ref()?;
                let namespace = facts.namespace_at(site.position)?;
                let name = namespace.qualify(&symbol.name);
                Some(Candidate {
                    site,
                    symbol: &symbol.semantic_id,
                    php,
                    declaration,
                    name,
                    owner: declaration.owner.first().map(String::as_str),
                })
            })
            .collect::<Vec<_>>();
        let mut index = Self {
            graph,
            projects: Projects::new(graph.files(), snapshot),
            candidates,
            symbols: HashMap::new(),
        };
        for (ordinal, candidate) in index.candidates.iter().enumerate() {
            let owner = candidate.owner.map(str::to_ascii_lowercase);
            let name = match candidate.owner {
                Some(_) => &candidate.symbol.name,
                None => &candidate.name,
            };
            index
                .symbols
                .entry((
                    owner,
                    candidate.php.kind,
                    names::key(candidate.php.kind, name),
                ))
                .or_default()
                .push(ordinal);
        }
        index
    }

    fn facts(&self, file: usize, context: usize) -> Option<&'a PhpFacts> {
        self.graph
            .semantic(file, context, Some(ResolutionFamily::PhpNamespaces))
            .next()?
            .semantic
            .php
            .as_ref()
    }
}
