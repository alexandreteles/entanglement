//! Conservative Kotlin declaration resolution over the shared program graph.
//!
//! This module reads only static Gradle literals and source layout. It does
//! not evaluate Gradle or infer dynamic build variants.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

mod arguments;
mod build;
mod calls;
mod constructors;
mod context;
mod execution;
mod lookup;
mod members;
mod owners;
mod receivers;
mod symbols;
mod types;
mod visibility;

use super::{analysis, only};
use arguments::{
    CallArgumentEvidence, CallArguments, function_type_parameters, function_type_receiver,
    normalize_type, substitute_type_name,
};
pub(super) use context::ProjectCache;
use context::{SourceContext, conventional_source_edge, source_set_reaches};
use symbols::*;

use crate::graph::{
    CallArgumentFact, CallConvention, CallDispatch, CallFacts, CallableSignature,
    DeclarationExecutionFacts, DeclarationFacts, DeclarationKind, FileFactsView, ProgramGraph,
    ProgramSymbolId, ReceiverScope, SemanticVisibility,
};
use crate::languages::ResolutionFamily;
use crate::model::{
    DefinitionKind, ExecutionTarget, Import, Reference, ReferenceAccess, ReferenceAnalysis,
    ReferenceKind, Resolution, SourceRange, SymbolId,
};
use crate::snapshot::Snapshot;

struct Candidate<'a> {
    /// Stable position in `Index::candidates`, used as the resolver-local id.
    candidate_index: usize,
    graph_id: ProgramSymbolId,
    symbol: &'a SymbolId,
    file: usize,
    definition: &'a crate::model::Definition,
    package: &'a [String],
    owner: &'a [String],
    declaration: Option<&'a DeclarationFacts>,
}

impl std::ops::Deref for Candidate<'_> {
    type Target = crate::model::Definition;

    fn deref(&self) -> &Self::Target {
        self.definition
    }
}

impl Candidate<'_> {
    fn signature(&self) -> Option<&CallableSignature> {
        self.declaration?.signature.as_ref()
    }
}

struct Index<'a> {
    graph: &'a ProgramGraph<'a>,
    snapshot: &'a Snapshot,
    sources: Vec<SourceContext>,
    candidates: Vec<Candidate<'a>>,
    by_name: HashMap<String, Vec<usize>>,
    visible_modules: HashMap<String, HashMap<Option<String>, BTreeSet<String>>>,
    source_edges: HashMap<PathBuf, BTreeMap<String, BTreeSet<String>>>,
    argument_evidence: RefCell<HashMap<(usize, usize, usize), Rc<CallArgumentEvidence>>>,
}

pub(super) fn resolve(
    facts: &[FileFactsView<'_>],
    snapshot: &Snapshot,
    graph: &ProgramGraph<'_>,
    cache: &mut ProjectCache,
) -> Vec<Option<Vec<ReferenceAnalysis>>> {
    let contexts = facts
        .iter()
        .map(|fact| {
            fact.semantic()
                .filter(|semantic| {
                    semantic.semantic.resolution_family == Some(ResolutionFamily::KotlinModules)
                })
                .map(|semantic| semantic.semantic.context_id)
                .collect::<HashSet<_>>()
        })
        .collect::<Vec<_>>();
    let active = facts.iter().zip(&contexts).any(|(fact, contexts)| {
        fact.references()
            .any(|reference| contexts.contains(&reference.context_id))
    });
    if !active {
        return vec![None; facts.len()];
    }
    let index = Index::new(facts, snapshot, graph, cache, &contexts);
    facts
        .iter()
        .zip(contexts)
        .enumerate()
        .map(|(file, (fact, contexts))| {
            fact.references()
                .any(|reference| contexts.contains(&reference.context_id))
                .then(|| {
                    fact.references()
                        .map(|reference| {
                            if contexts.contains(&reference.context_id) {
                                index.resolve_reference(file, reference)
                            } else {
                                analysis(reference, Resolution::Unresolved, None)
                            }
                        })
                        .collect()
                })
        })
        .collect()
}

impl<'a> Index<'a> {
    fn resolve_reference(&self, file: usize, reference: &Reference) -> ReferenceAnalysis {
        let path = &reference.path;
        let Some(name) = path.last() else {
            return analysis(reference, Resolution::Unresolved, None);
        };

        if reference.kind == ReferenceKind::Method {
            let is_super = reference.path.first().is_some_and(|part| part == "super");
            let is_type_qualifier = self.is_static_type_qualifier(file, reference);
            let receiver_type = self.method_receiver_type(file, reference, is_super);
            let Some(receiver_type) = receiver_type else {
                return analysis(reference, Resolution::Unresolved, None);
            };
            let mut candidates = self.members_of_type(file, reference, name, &receiver_type);
            if candidates.is_empty() && !is_type_qualifier {
                candidates = self.extensions_for_type(file, reference, name, &receiver_type);
            }
            candidates = self.filter_callable_overloads(file, reference, candidates);
            let candidates = self.accessible(file, reference, candidates);
            let mut analysis = self.finish_resolution(file, reference, candidates);
            if is_super && matches!(analysis.resolution, Resolution::Exact(_)) {
                analysis.dispatch = Some(CallDispatch::Direct);
            }
            return analysis;
        }

        let candidates = self.reference_candidates(file, reference);
        self.finish_resolution(file, reference, candidates)
    }
}

pub(super) fn structural_members(
    facts: &[FileFactsView<'_>],
    graph: &ProgramGraph<'_>,
    snapshot: &Snapshot,
) -> Vec<super::StructuralMemberSelection> {
    let contexts = facts
        .iter()
        .map(|fact| {
            fact.semantic()
                .filter(|store| {
                    store.semantic.resolution_family == Some(ResolutionFamily::KotlinModules)
                })
                .map(|store| store.semantic.context_id)
                .collect::<HashSet<_>>()
        })
        .collect::<Vec<_>>();
    if contexts.iter().all(HashSet::is_empty) {
        return Vec::new();
    }
    let index = Index::new(
        facts,
        snapshot,
        graph,
        &mut ProjectCache::default(),
        &contexts,
    );
    let names = index
        .candidates
        .iter()
        .filter(|candidate| !candidate.owner.is_empty())
        .map(|candidate| candidate.name.clone())
        .collect::<BTreeSet<_>>();
    index
        .candidates
        .iter()
        .filter(|candidate| is_type_declaration(candidate))
        .flat_map(|candidate| {
            let reference = Reference {
                path: vec![candidate.name.clone()],
                module: candidate.module.clone(),
                kind: ReferenceKind::Type,
                start_byte: candidate.start_byte,
                end_byte: candidate.end_byte,
                scope_start: candidate.scope_start,
                scope_end: candidate.scope_end,
                call_owner: None,
                execution_owner: None,
                access: None,
                context_id: candidate.context_id,
            };
            names
                .iter()
                .filter_map(|name| {
                    let members = index.members_of_type_seen(
                        candidate.file,
                        &reference,
                        name,
                        &candidate_type_name(candidate),
                        false,
                        &mut BTreeSet::new(),
                    );
                    (!members.is_empty()).then(|| super::StructuralMemberSelection {
                        obligations: Vec::new(),
                        owner: candidate.symbol.clone(),
                        name: name.clone(),
                        visibility: members
                            .iter()
                            .map(|member| {
                                (
                                    index.candidates[*member].symbol.clone(),
                                    format!(
                                        "{:?}",
                                        candidate_visibility(&index.candidates[*member])
                                    )
                                    .to_lowercase(),
                                )
                            })
                            .collect(),
                        origins: members
                            .into_iter()
                            .map(|member| index.candidates[member].symbol.clone())
                            .collect(),
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect()
}
