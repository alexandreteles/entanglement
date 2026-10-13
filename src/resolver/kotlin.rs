//! Conservative Kotlin declaration resolution over the shared program graph.
//!
//! This module reads only static Gradle literals and source layout. It does
//! not evaluate Gradle or infer dynamic build variants.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::PathBuf;

mod arguments;
mod build;
mod constructor_resolution;
mod context;
mod execution;
mod helpers;
mod index;
mod inference;
mod lookup_imports;
mod lookup_local;
mod lookup_qualified;
mod member_extensions;
mod member_inheritance;
mod member_types;
mod members;
mod overloads;
mod receiver_lambdas;
mod receiver_scopes;
mod receiver_types;
mod symbols;
mod visibility;

use arguments::{
    arguments_match, function_type_parameters, function_type_receiver, generic_type_substitutions,
    lambda_parameter_for_argument, normalize_type, substitute_type_name,
};
use context::{
    SourceContext, conventional_source_edge, project_dependencies, read_text, source_context,
    source_set_reaches, transitive_closure,
};
use helpers::*;
use symbols::*;

use crate::graph::{
    CallConvention, CallDispatch, DeclarationFacts, ProgramGraph, ProgramSymbolId, ReceiverScope,
    SemanticVisibility,
};
use crate::languages::ResolutionFamily;
use crate::model::{
    DefinitionKind, ExecutionTarget, FileFacts, Import, Reference, ReferenceAccess,
    ReferenceAnalysis, ReferenceKind, Resolution, SourceRange, SymbolId,
};
use crate::snapshot::Snapshot;

struct Candidate {
    /// Stable position in `Index::candidates`, used as the resolver-local id.
    candidate_index: usize,
    graph_id: ProgramSymbolId,
    symbol: SymbolId,
    file: usize,
    context_id: usize,
    name: String,
    package: Vec<String>,
    owner: Vec<String>,
    kind: DefinitionKind,
    declaration: Option<DeclarationFacts>,
    source: SourceContext,
    public: bool,
    start: usize,
    end: usize,
    scope_start: usize,
    scope_end: usize,
}

struct Index<'a> {
    graph: &'a ProgramGraph<'a>,
    snapshot: &'a Snapshot,
    sources: Vec<SourceContext>,
    candidates: Vec<Candidate>,
    by_name: HashMap<String, Vec<usize>>,
    dependencies: HashMap<PathBuf, BTreeMap<String, BTreeSet<build::ProjectDependency>>>,
    source_edges: HashMap<PathBuf, BTreeMap<String, BTreeSet<String>>>,
}

pub(super) fn resolve(
    facts: &[FileFacts],
    snapshot: &Snapshot,
    graph: &ProgramGraph<'_>,
) -> Vec<Option<Vec<ReferenceAnalysis>>> {
    let index = Index::new(facts, snapshot, graph);
    facts
        .iter()
        .enumerate()
        .map(|(file, fact)| {
            let has_kotlin_reference = fact.references.iter().any(|reference| {
                graph
                    .semantic(
                        file,
                        reference.context_id,
                        Some(ResolutionFamily::KotlinModules),
                    )
                    .is_some()
            });
            if !has_kotlin_reference {
                return None;
            }
            Some(
                fact.references
                    .iter()
                    .map(|reference| {
                        if graph
                            .semantic(
                                file,
                                reference.context_id,
                                Some(ResolutionFamily::KotlinModules),
                            )
                            .is_none()
                        {
                            return analysis(reference, Resolution::Unresolved, None);
                        }
                        index.resolve_reference(file, reference)
                    })
                    .collect(),
            )
        })
        .collect()
}

impl<'a> Index<'a> {
    fn resolve_reference(&self, file: usize, reference: &Reference) -> ReferenceAnalysis {
        let Some(semantic) = self.graph.semantic(
            file,
            reference.context_id,
            Some(ResolutionFamily::KotlinModules),
        ) else {
            return analysis(reference, Resolution::Unresolved, None);
        };
        let package = semantic
            .context
            .as_ref()
            .map_or(&[][..], |context| context.package.as_slice());
        let path = &reference.path;
        let Some(name) = path.last() else {
            return analysis(reference, Resolution::Unresolved, None);
        };

        if reference.kind == ReferenceKind::Method {
            let is_super = self.is_super_reference(file, reference);
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

        let candidates = if path.len() == 1 {
            self.simple_candidates(file, reference, name, package)
        } else {
            self.qualified_candidates(file, reference, path, package)
        };
        let candidates = self.filter_callable_overloads(file, reference, candidates);
        let candidates = self.select_expect_actual(file, candidates);
        self.finish_resolution(file, reference, candidates)
    }
}
