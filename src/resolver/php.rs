//! Resolve PHP namespaces and members over the shared semantic graph.

use std::collections::{HashMap, HashSet};

use crate::graph::{
    CallDispatch, DeclarationFacts, FileFactsView, PhpDeclaration, PhpFacts, PhpReference,
    PhpReferenceKind, PhpSymbolKind, ProgramGraph, ProgramSymbolId, SemanticVisibility,
};
use crate::languages::ResolutionFamily;
use crate::model::{Reference, ReferenceAnalysis, Resolution, SymbolId};
use crate::snapshot::Snapshot;

mod execution;
mod members;
mod names;
mod project;

use members::Member;
use project::Projects;

struct Candidate<'a> {
    id: ProgramSymbolId,
    symbol: &'a SymbolId,
    php: &'a PhpDeclaration,
    declaration: Option<&'a DeclarationFacts>,
    context: usize,
    name: String,
    owner: Option<String>,
}

struct Index<'a> {
    graph: &'a ProgramGraph<'a>,
    projects: Projects,
    candidates: Vec<Candidate<'a>>,
    names: HashMap<(PhpSymbolKind, String), Vec<usize>>,
    members: HashMap<(String, PhpSymbolKind, String), Vec<usize>>,
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
        let mut index = Self {
            graph,
            projects: Projects::new(graph.files(), snapshot),
            candidates: Vec::new(),
            names: HashMap::new(),
            members: HashMap::new(),
        };
        for file in 0..graph.file_count() {
            for symbol in graph.symbols_for_file(file) {
                let context = symbol.scope.context_id;
                let Some(facts) = index.facts(file, context) else {
                    continue;
                };
                let Some(php) = facts
                    .declarations
                    .iter()
                    .find(|item| item.start_byte == symbol.semantic_id.start_byte)
                else {
                    continue;
                };
                let declaration = graph.declaration(
                    file,
                    context,
                    php.start_byte,
                    Some(ResolutionFamily::PhpNamespaces),
                );
                let owner = declaration.and_then(|item| item.owner.first()).cloned();
                let full_name = php
                    .namespace
                    .iter()
                    .map(String::as_str)
                    .chain(std::iter::once(symbol.name.as_str()))
                    .collect::<Vec<_>>()
                    .join("\\");
                let ordinal = index.candidates.len();
                if let Some(owner) = &owner {
                    index
                        .members
                        .entry((
                            owner.to_ascii_lowercase(),
                            php.kind,
                            names::key(php.kind, &symbol.name),
                        ))
                        .or_default()
                        .push(ordinal);
                } else {
                    index
                        .names
                        .entry((php.kind, names::key(php.kind, &full_name)))
                        .or_default()
                        .push(ordinal);
                }
                index.candidates.push(Candidate {
                    id: symbol.id,
                    symbol: &symbol.semantic_id,
                    php,
                    declaration,
                    context,
                    name: full_name,
                    owner,
                });
            }
        }
        index
    }

    fn facts(&self, file: usize, context: usize) -> Option<&'a PhpFacts> {
        self.graph
            .semantic(file, context, Some(ResolutionFamily::PhpNamespaces))?
            .php
            .as_ref()
    }

    fn resolve(&self, file: usize, reference: &Reference) -> ReferenceAnalysis {
        let Some(php) = self.facts(file, reference.context_id).and_then(|facts| {
            facts
                .references
                .iter()
                .find(|item| item.start_byte == reference.start_byte)
        }) else {
            return analysis(reference, Resolution::Unresolved, None);
        };
        let members = match php.kind {
            PhpReferenceKind::Method
            | PhpReferenceKind::StaticMethod
            | PhpReferenceKind::Property
            | PhpReferenceKind::StaticProperty
            | PhpReferenceKind::ClassConstant => {
                self.member_reference(file, reference, php, &mut HashSet::new())
            }
            PhpReferenceKind::Variable => {
                return self.callable_reference(file, reference, php, &mut HashSet::new());
            }
            _ => self
                .lookup(file, reference.context_id, php)
                .into_iter()
                .map(Member::new)
                .collect(),
        };
        self.finish(file, reference, php, members)
    }

    fn finish(
        &self,
        file: usize,
        reference: &Reference,
        php: &PhpReference,
        mut members: Vec<Member>,
    ) -> ReferenceAnalysis {
        let direct = members
            .iter()
            .all(|member| self.dispatch(php, member) == CallDispatch::Direct);
        members.sort_by_key(|member| member.candidate);
        members.dedup_by_key(|member| member.candidate);
        if members
            .iter()
            .any(|member| self.candidates[member.candidate].php.conditional)
        {
            return analysis(reference, Resolution::Unresolved, None);
        }
        match members.as_slice() {
            [] => {
                let external = self
                    .expanded_names(file, reference.context_id, php)
                    .iter()
                    .any(|name| self.projects.external(file, name));
                analysis(
                    reference,
                    if external {
                        Resolution::External
                    } else {
                        Resolution::Unresolved
                    },
                    None,
                )
            }
            [member] => {
                let candidate = &self.candidates[member.candidate];
                let executable = php.callable || candidate.php.kind == PhpSymbolKind::Property;
                let dispatch = executable.then_some(if direct {
                    CallDispatch::Direct
                } else {
                    CallDispatch::Virtual
                });
                let mut result = analysis(
                    reference,
                    Resolution::Exact(candidate.symbol.clone()),
                    dispatch,
                );
                self.execution_targets(file, reference, php, member, &mut result);
                result
            }
            _ => analysis(
                reference,
                Resolution::Ambiguous(
                    members
                        .iter()
                        .map(|member| self.candidates[member.candidate].symbol.clone())
                        .collect(),
                ),
                None,
            ),
        }
    }

    fn visible(&self, file: usize, context: usize, candidate: &Candidate<'_>) -> bool {
        let target = candidate.id.file_index;
        let contexts_agree = file != target || context == candidate.context;
        contexts_agree
            && ((self.projects.reaches(file, target)
                && self.projects.autoloads(
                    file,
                    target,
                    self.graph.file(target).target,
                    &candidate.name,
                    candidate.php.kind == PhpSymbolKind::Class,
                ))
                || self.included(file, context, target, &mut HashSet::new()))
    }

    fn included(
        &self,
        file: usize,
        context: usize,
        target: usize,
        visited: &mut HashSet<usize>,
    ) -> bool {
        if file == target {
            return true;
        }
        if !visited.insert(file) {
            return false;
        }
        self.facts(file, context)
            .into_iter()
            .flat_map(|facts| &facts.includes)
            .filter(|include| !include.conditional)
            .filter_map(|include| include.path.as_deref())
            .filter_map(|path| {
                let source = self.graph.file(file).target;
                let path = if let Some(suffix) = path.strip_prefix("@dir:") {
                    source.parent()?.join(suffix.trim_start_matches('/'))
                } else if let Some(suffix) = path.strip_prefix("@file:") {
                    std::path::PathBuf::from(format!("{}{suffix}", source.display()))
                } else {
                    source.parent()?.join(path)
                };
                let path = super::files::indexing::normalize(&path);
                self.graph.aliases().get(&path)
            })
            .flatten()
            .copied()
            .any(|included| included == target || self.included(included, context, target, visited))
    }
}

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
