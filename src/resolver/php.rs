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
mod structural;

use super::analysis;
use members::Member;
use project::Projects;

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
                return self.callable_reference(file, reference, php);
            }
            _ => self
                .lookup(
                    Site::at(file, reference),
                    &php.name,
                    names::reference_kind(php.kind),
                    php.class.as_deref(),
                )
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
        let resolution = match members.as_slice() {
            [] => match self
                .expanded_names(
                    Site::at(file, reference),
                    &php.name,
                    names::reference_kind(php.kind),
                )
                .iter()
                .any(|name| self.projects.external(file, name))
            {
                true => Resolution::External,
                false => Resolution::Unresolved,
            },
            [member] => Resolution::Exact(self.candidates[member.candidate].symbol.clone()),
            _ => Resolution::Ambiguous(
                members
                    .iter()
                    .map(|member| self.candidates[member.candidate].symbol.clone())
                    .collect(),
            ),
        };
        let mut result = analysis(reference, resolution, None);
        if let [member] = members.as_slice() {
            let executable = php.callable
                || self.candidates[member.candidate].php.kind == PhpSymbolKind::Property;
            result.dispatch = executable.then_some(match direct {
                true => CallDispatch::Direct,
                false => CallDispatch::Virtual,
            });
            self.execution_targets(file, reference, php, member, &mut result);
        }
        result
    }

    fn visible(&self, file: usize, context: usize, candidate: &Candidate<'_>) -> bool {
        let target = candidate.site.file;
        let contexts_agree = file != target || context == candidate.site.context;
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

pub(super) fn structural_members(
    graph: &ProgramGraph<'_>,
    snapshot: &Snapshot,
) -> Vec<super::StructuralMemberSelection> {
    let index = Index::new(graph, snapshot);
    let names = index
        .candidates
        .iter()
        .filter(|candidate| candidate.owner.is_some())
        .map(|candidate| candidate.symbol.name.clone())
        .chain(
            graph
                .files()
                .iter()
                .flat_map(|fact| fact.semantic())
                .filter_map(|store| store.semantic.php.as_ref())
                .flat_map(|php| &php.traits)
                .flat_map(|usage| &usage.adaptations)
                .filter_map(|adaptation| adaptation.alias.clone()),
        )
        .collect::<std::collections::BTreeSet<_>>();
    index
        .candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| candidate.php.kind == PhpSymbolKind::Class)
        .flat_map(|(owner, candidate)| {
            names
                .iter()
                .flat_map(|name| {
                    let index = &index;
                    [
                        PhpSymbolKind::Method,
                        PhpSymbolKind::Property,
                        PhpSymbolKind::Constant,
                    ]
                    .into_iter()
                    .filter_map(move |kind| {
                        let candidates = index.structural_member_candidates(owner, kind, name);
                        let members = candidates.members;
                        (!members.is_empty()).then(|| super::StructuralMemberSelection {
                            owner: candidate.symbol.clone(),
                            name: name.clone(),
                            obligations: candidates
                                .version_dependent
                                .then(|| format!("php_version:{name}"))
                                .into_iter()
                                .collect(),
                            visibility: members
                                .iter()
                                .map(|member| {
                                    (
                                        index.candidates[member.candidate].symbol.clone(),
                                        format!(
                                            "{:?}",
                                            member.visibility.unwrap_or(
                                                index.candidates[member.candidate]
                                                    .declaration
                                                    .visibility
                                            )
                                        )
                                        .to_lowercase(),
                                    )
                                })
                                .collect(),
                            origins: members
                                .into_iter()
                                .map(|member| index.candidates[member.candidate].symbol.clone())
                                .collect(),
                        })
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Resolve trait uses and parent declarations with the existing PHP index.
pub(super) fn structural_type_relationships(
    graph: &ProgramGraph<'_>,
    snapshot: &Snapshot,
) -> Vec<super::StructuralTypeRelationship> {
    use crate::structure::model::RelationshipKind;
    let index = Index::new(graph, snapshot);
    index
        .candidates
        .iter()
        .enumerate()
        .filter(|(_, candidate)| candidate.php.kind == PhpSymbolKind::Class)
        .flat_map(|(owner, candidate)| {
            let traits = index.trait_uses(owner).flat_map(|usage| {
                usage
                    .traits
                    .iter()
                    .map(move |name| (name, usage.start_byte, RelationshipKind::Compose))
            });
            let parents = candidate
                .declaration
                .supertypes
                .iter()
                .map(|name| (name, candidate.symbol.start_byte, RelationshipKind::Inherit));
            traits
                .chain(parents)
                .map(|(name, start, kind)| {
                    let candidates = index.lookup(candidate.site, name, PhpSymbolKind::Class, None);
                    let kind = if kind == RelationshipKind::Inherit
                        && !candidate.php.is_interface
                        && candidates.len() == 1
                        && index.candidates[candidates[0]].php.is_interface
                    {
                        RelationshipKind::Implement
                    } else {
                        kind
                    };
                    let targets = candidates
                        .into_iter()
                        .map(|target| index.candidates[target].symbol.clone())
                        .collect::<Vec<_>>();
                    let target = match targets.as_slice() {
                        [] => Resolution::Unresolved,
                        [target] => Resolution::Exact(target.clone()),
                        _ => Resolution::Ambiguous(targets),
                    };
                    super::StructuralTypeRelationship {
                        owner: candidate.symbol.clone(),
                        target,
                        name: name.clone(),
                        kind,
                        start,
                    }
                })
                .collect::<Vec<_>>()
        })
        .collect()
}
