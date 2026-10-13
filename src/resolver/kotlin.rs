//! Conservative Kotlin declaration resolution over the shared program graph.
//!
//! This module reads only static Gradle literals and source layout. It does
//! not evaluate Gradle or infer dynamic build variants.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Component, Path, PathBuf};

mod build;

use crate::graph::{
    CallConvention, CallDispatch, CallableSignature, DeclarationFacts, ProgramGraph,
    ProgramSymbolId, ReceiverScope, SemanticVisibility,
};
use crate::languages::ResolutionFamily;
use crate::model::{
    DefinitionKind, ExecutionTarget, FileFacts, Reference, ReferenceAccess, ReferenceAnalysis,
    ReferenceKind, Resolution, SourceRange, SymbolId,
};
use crate::snapshot::Snapshot;

#[derive(Clone, Debug, Default)]
struct SourceContext {
    path: PathBuf,
    project_root: PathBuf,
    module_root: PathBuf,
    /// Project-unique identity used to prevent unrelated roots colliding.
    module: String,
    /// Gradle's module coordinate relative to project_root (for example `:lib`).
    gradle_module: String,
    source_set: Option<String>,
    isolated_source: bool,
}

#[derive(Clone)]
struct Candidate {
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
    fn new(facts: &'a [FileFacts], snapshot: &'a Snapshot, graph: &'a ProgramGraph<'a>) -> Self {
        let sources = facts
            .iter()
            .map(|fact| source_context(&fact.target, snapshot))
            .collect::<Vec<_>>();
        let mut dependencies = HashMap::new();
        let mut source_edges = HashMap::new();
        for source in &sources {
            dependencies
                .entry(source.project_root.clone())
                .or_insert_with(|| project_dependencies(&source.project_root, snapshot));
            source_edges
                .entry(source.module_root.clone())
                .or_insert_with(|| {
                    let mut edges = BTreeMap::new();
                    for name in ["build.gradle.kts", "build.gradle"] {
                        if let Some(text) = read_text(snapshot, &source.module_root.join(name)) {
                            build::parse_source_edges(&text, &mut edges);
                        }
                    }
                    edges
                });
        }

        let mut candidates = Vec::new();
        let mut by_name = HashMap::<String, Vec<usize>>::new();
        for (file, source) in sources.iter().enumerate().take(graph.file_count()) {
            for symbol in graph.symbols_for_file(file) {
                if graph
                    .semantic(
                        file,
                        symbol.scope.context_id,
                        Some(ResolutionFamily::KotlinModules),
                    )
                    .is_none()
                {
                    continue;
                }
                let Some(definition) = graph.definition(symbol.id) else {
                    continue;
                };
                let semantic = graph.semantic(
                    file,
                    symbol.scope.context_id,
                    Some(ResolutionFamily::KotlinModules),
                );
                let package = semantic
                    .and_then(|semantic| semantic.context.as_ref())
                    .map_or_else(
                        || definition.module.0.clone(),
                        |context| context.package.clone(),
                    );
                let declaration = graph
                    .declaration(
                        file,
                        symbol.scope.context_id,
                        definition.start_byte,
                        Some(ResolutionFamily::KotlinModules),
                    )
                    .cloned();
                let index = candidates.len();
                candidates.push(Candidate {
                    candidate_index: index,
                    graph_id: symbol.id,
                    symbol: symbol.semantic_id.clone(),
                    file,
                    context_id: symbol.scope.context_id,
                    name: symbol.name.clone(),
                    package,
                    owner: declaration
                        .as_ref()
                        .map_or_else(Vec::new, |item| item.owner.clone()),
                    kind: symbol.kind.clone(),
                    declaration,
                    source: source.clone(),
                    public: symbol.is_public,
                    start: definition.start_byte,
                    end: definition.end_byte,
                    scope_start: definition.scope_start,
                    scope_end: definition.scope_end,
                });
                by_name.entry(symbol.name.clone()).or_default().push(index);
            }
        }
        Self {
            graph,
            snapshot,
            sources,
            candidates,
            by_name,
            dependencies,
            source_edges,
        }
    }

    fn callee_reference_for_call<'b>(
        &'b self,
        file: usize,
        context_id: usize,
        call: &crate::graph::CallFacts,
    ) -> Option<&'b Reference> {
        let target = call.target.as_ref()?;
        self.graph.file(file).references.iter().find(|reference| {
            reference.context_id == context_id
                && reference.start_byte == target.start_byte
                && reference.end_byte == target.end_byte
                && matches!(reference.kind, ReferenceKind::Call | ReferenceKind::Method)
        })
    }

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

    fn finish_resolution(
        &self,
        file: usize,
        reference: &Reference,
        candidates: Vec<usize>,
    ) -> ReferenceAnalysis {
        let candidates = unique_candidates(candidates);
        let name = reference
            .path
            .last()
            .map(String::as_str)
            .unwrap_or_default();
        match candidates.as_slice() {
            [] if is_kotlin_builtin(name, reference) => {
                analysis(reference, Resolution::External, None)
            }
            [] => analysis(reference, Resolution::Unresolved, None),
            [candidate] => {
                let candidate = &self.candidates[*candidate];
                let accessor_access = matches!(
                    reference.kind,
                    ReferenceKind::Value | ReferenceKind::Qualified
                ) && matches!(
                    reference.access,
                    Some(
                        ReferenceAccess::Read | ReferenceAccess::Write | ReferenceAccess::ReadWrite
                    )
                ) && candidate.declaration.as_ref().is_some_and(
                    |declaration| declaration.kind == crate::graph::DeclarationKind::Property,
                );
                let mut dispatch =
                    if matches!(reference.kind, ReferenceKind::Call | ReferenceKind::Method)
                        || accessor_access
                    {
                        Some(call_dispatch(candidate, reference))
                    } else {
                        None
                    };
                let mut result = analysis(
                    reference,
                    Resolution::Exact(candidate.symbol.clone()),
                    dispatch,
                );
                let ranges = self.execution_ranges_for_reference(file, reference, candidate);
                if !ranges.is_empty()
                    && let Some(caller_start_byte) =
                        reference.execution_owner.or(reference.call_owner)
                    && dispatch == Some(CallDispatch::Direct)
                {
                    dispatch = Some(CallDispatch::Direct);
                    result.dispatch = dispatch;
                    result
                        .execution_targets
                        .extend(ranges.into_iter().map(|range| ExecutionTarget {
                            caller_start_byte,
                            file_index: candidate.file,
                            context_id: candidate.context_id,
                            range,
                        }));
                }
                result
            }
            _ => analysis(
                reference,
                Resolution::Ambiguous(
                    candidates
                        .into_iter()
                        .map(|candidate| self.candidates[candidate].symbol.clone())
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect(),
                ),
                None,
            ),
        }
    }

    fn execution_ranges_for_reference(
        &self,
        file: usize,
        reference: &Reference,
        candidate: &Candidate,
    ) -> Vec<SourceRange> {
        let Some(declaration) = candidate.declaration.as_ref() else {
            return Vec::new();
        };
        let Some(execution) = declaration.execution.as_ref() else {
            return Vec::new();
        };
        if reference.kind == ReferenceKind::Call {
            if declaration.kind == crate::graph::DeclarationKind::Property {
                if let Some(range) = execution.callable_value {
                    return vec![range];
                }
                return execution.getter.into_iter().collect();
            }
            if is_type_declaration(candidate) {
                if let Some(range) = self.selected_constructor_range(file, reference, candidate) {
                    return vec![range];
                }
                let Some(execution) = declaration.execution.as_ref() else {
                    return Vec::new();
                };
                let has_primary_signature =
                    declaration.signature.as_ref().is_some_and(|signature| {
                        self.graph
                            .call_facts(
                                file,
                                reference.context_id,
                                Some(ResolutionFamily::KotlinModules),
                            )
                            .iter()
                            .find(|call| {
                                call.start_byte <= reference.start_byte
                                    && reference.end_byte <= call.end_byte
                            })
                            .is_some_and(|call| {
                                let arguments = self.infer_argument_return_types(
                                    file,
                                    reference,
                                    &call.arguments,
                                );
                                arguments_match(signature, &arguments)
                            })
                    });
                if execution.constructors.is_empty() && has_primary_signature {
                    return execution.initializers.clone();
                }
                return Vec::new();
            }
            return Vec::new();
        }
        if !matches!(
            reference.kind,
            ReferenceKind::Value | ReferenceKind::Qualified
        ) || declaration.kind != crate::graph::DeclarationKind::Property
        {
            return Vec::new();
        }
        match reference.access {
            Some(ReferenceAccess::Write) => execution.setter.into_iter().collect(),
            Some(ReferenceAccess::ReadWrite) => execution
                .getter
                .into_iter()
                .chain(execution.setter)
                .collect(),
            Some(ReferenceAccess::Read) => execution.getter.into_iter().collect(),
            None => Vec::new(),
        }
    }

    fn selected_constructor_range(
        &self,
        file: usize,
        reference: &Reference,
        class: &Candidate,
    ) -> Option<SourceRange> {
        let constructor_class = self.typealias_target(class).unwrap_or(class);
        let execution = constructor_class.declaration.as_ref()?.execution.as_ref()?;
        let call = self
            .graph
            .call_facts(
                file,
                reference.context_id,
                Some(ResolutionFamily::KotlinModules),
            )
            .iter()
            .filter(|call| {
                call.start_byte <= reference.start_byte && reference.end_byte <= call.end_byte
            })
            .min_by_key(|call| call.end_byte - call.start_byte)?;
        let arguments = self.infer_argument_return_types(file, reference, &call.arguments);
        let mut matched = Vec::new();
        if constructor_class
            .declaration
            .as_ref()?
            .signature
            .as_ref()
            .is_some_and(|signature| arguments_match(signature, &arguments))
        {
            let mut owner = constructor_class.owner.clone();
            owner.push(constructor_class.name.clone());
            if let Some(range) = execution.constructors.iter().copied().find(|range| {
                !self.candidates.iter().any(|candidate| {
                    candidate.file == constructor_class.file
                        && candidate.context_id == constructor_class.context_id
                        && candidate.owner == owner
                        && candidate.start == range.start_byte
                        && candidate.end == range.end_byte
                        && candidate.declaration.as_ref().is_some_and(|item| {
                            item.kind == crate::graph::DeclarationKind::Constructor
                        })
                })
            }) {
                matched.push(range);
            }
        }
        let mut owner = constructor_class.owner.clone();
        owner.push(constructor_class.name.clone());
        for constructor in self.candidates.iter().filter(|candidate| {
            candidate.file == constructor_class.file
                && candidate.context_id == constructor_class.context_id
                && candidate.owner == owner
                && candidate
                    .declaration
                    .as_ref()
                    .is_some_and(|item| item.kind == crate::graph::DeclarationKind::Constructor)
        }) {
            let Some(signature) = constructor
                .declaration
                .as_ref()
                .and_then(|item| item.signature.as_ref())
            else {
                continue;
            };
            if arguments_match(signature, &arguments)
                && let Some(range) = execution.constructors.iter().copied().find(|range| {
                    range.start_byte == constructor.start && range.end_byte == constructor.end
                })
            {
                matched.push(range);
            }
        }
        matched.sort_by_key(|range| (range.start_byte, range.end_byte));
        matched.dedup();
        if matched.len() == 1 {
            matched.pop()
        } else {
            None
        }
    }

    fn typealias_target<'b>(&'b self, candidate: &'b Candidate) -> Option<&'b Candidate> {
        let declaration = candidate.declaration.as_ref()?;
        if declaration.kind != crate::graph::DeclarationKind::TypeAlias {
            return None;
        }
        let alias = declaration.alias_target.as_deref()?;
        let mut targets = self.resolve_declared_type(
            candidate.file,
            candidate.context_id,
            alias,
            &candidate.package,
        );
        if targets.len() != 1 {
            return None;
        }
        targets.pop()
    }

    fn type_constructor_matches(
        &self,
        candidate: &Candidate,
        arguments: &[crate::graph::CallArgumentFact],
    ) -> bool {
        let target = self.typealias_target(candidate).unwrap_or(candidate);
        if target
            .declaration
            .as_ref()
            .and_then(|item| item.signature.as_ref())
            .is_some_and(|signature| arguments_match(signature, arguments))
        {
            return true;
        }
        let mut owner = target.owner.clone();
        owner.push(target.name.clone());
        self.candidates.iter().any(|constructor| {
            constructor.file == target.file
                && constructor.context_id == target.context_id
                && constructor.owner == owner
                && constructor.declaration.as_ref().is_some_and(|declaration| {
                    declaration.kind == crate::graph::DeclarationKind::Constructor
                        && declaration
                            .signature
                            .as_ref()
                            .is_some_and(|signature| arguments_match(signature, arguments))
                })
        })
    }

    fn simple_candidates(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
        package: &[String],
    ) -> Vec<usize> {
        let position = reference.start_byte;
        let local_definitions = self
            .named(name)
            .filter(|candidate| candidate.file == file)
            .filter(|candidate| candidate.context_id == reference.context_id)
            .filter(|candidate| candidate.start < position)
            .filter(|candidate| candidate.scope_start <= position && position < candidate.scope_end)
            .filter(|candidate| {
                candidate.kind == DefinitionKind::Function
                    || candidate.kind == DefinitionKind::Method
            })
            .filter(|candidate| {
                self.graph
                    .definition(candidate.graph_id)
                    .is_some_and(|definition| self.graph.is_local_definition(file, definition))
            })
            .collect::<Vec<_>>();
        if !local_definitions.is_empty() {
            return self.accessible(
                file,
                reference,
                local_definitions
                    .iter()
                    .map(|candidate| self.candidate_index(candidate))
                    .collect(),
            );
        }
        if reference.kind == ReferenceKind::Call {
            let callable_values = self
                .named(name)
                .filter(|candidate| candidate.file == file)
                .filter(|candidate| candidate.context_id == reference.context_id)
                .filter(|candidate| candidate.start < position)
                .filter(|candidate| {
                    candidate.scope_start <= position && position < candidate.scope_end
                })
                .filter(|candidate| {
                    candidate.declaration.as_ref().is_some_and(|declaration| {
                        declaration.kind == crate::graph::DeclarationKind::Property
                            && declaration
                                .execution
                                .as_ref()
                                .is_some_and(|execution| execution.callable_value.is_some())
                    })
                })
                .filter(|candidate| {
                    self.graph
                        .definition(candidate.graph_id)
                        .is_some_and(|definition| self.graph.is_local_definition(file, definition))
                })
                .map(|candidate| self.candidate_index(candidate))
                .collect::<Vec<_>>();
            if !callable_values.is_empty() {
                return self.accessible(file, reference, callable_values);
            }
        }
        let callable_alias = if reference.kind == ReferenceKind::Call {
            self.graph
                .binding_callable_alias(file, name, reference.context_id, position)
                .map(|target| self.callable_alias_candidates(file, reference, &target, package))
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        if !callable_alias.is_empty() {
            return self.accessible(file, reference, callable_alias);
        }
        let aliased = self
            .graph
            .binding_alias(file, name, reference.context_id, position)
            .map(|target| {
                let mut segments = target.split('.').map(str::to_owned).collect::<Vec<_>>();
                let target_name = segments.pop().unwrap_or_default();
                let imported = self.imported_candidates(file, reference, &target_name, false);
                if !imported.is_empty() {
                    return imported;
                }
                let target_package = if segments.is_empty() {
                    package.to_vec()
                } else {
                    segments
                };
                self.named(&target_name)
                    .filter(|candidate| {
                        candidate.package == target_package && candidate.owner.is_empty()
                    })
                    .filter(|candidate| self.visible_in_source_set(file, candidate))
                    .map(|candidate| self.candidate_index(candidate))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if !aliased.is_empty() {
            return self.accessible(file, reference, aliased);
        }
        if self
            .graph
            .has_local_binding(file, name, reference.context_id, position)
        {
            return Vec::new();
        }

        if let Some(receiver) = self.implicit_receiver_candidates(file, reference, name)
            && !receiver.is_empty()
        {
            return receiver;
        }

        let mut imported = self
            .imported_candidates(file, reference, name, false)
            .into_iter()
            .filter(|index| !is_extension_candidate(&self.candidates[*index]))
            .collect::<Vec<_>>();
        if !imported.is_empty() {
            return self.accessible(file, reference, std::mem::take(&mut imported));
        }

        // Calls inside a lambda passed to a callable whose receiver type is
        // unknown cannot borrow same-package declarations as implicit DSL
        // members. A statically proven receiver has already been considered
        // above and ordinary explicit imports remain usable.
        if self.inside_unresolved_lambda_argument(file, reference) {
            return Vec::new();
        }

        let in_package = self
            .named(name)
            .filter(|candidate| candidate.package == package && candidate.owner.is_empty())
            .filter(|candidate| !self.is_local_candidate(candidate))
            .filter(|candidate| !is_extension_candidate(candidate))
            .filter(|candidate| self.visible_in_source_set(file, candidate))
            .collect::<Vec<_>>();
        if !in_package.is_empty() {
            return self.accessible(
                file,
                reference,
                in_package
                    .iter()
                    .map(|candidate| self.candidate_index(candidate))
                    .collect(),
            );
        }
        let wildcard = self
            .imported_candidates(file, reference, name, true)
            .into_iter()
            .filter(|index| {
                !is_extension_candidate(&self.candidates[*index])
                    && !self.is_local_candidate(&self.candidates[*index])
            })
            .collect();
        self.accessible(file, reference, wildcard)
    }

    fn qualified_candidates(
        &self,
        file: usize,
        reference: &Reference,
        path: &[String],
        package: &[String],
    ) -> Vec<usize> {
        let name = path.last().expect("nonempty Kotlin path");
        if path.len() == 2 {
            let receiver_name = &path[0];
            if receiver_name == "this"
                || receiver_name.starts_with("this@")
                || receiver_name == "super"
            {
                let scopes = self.active_receivers(file, reference);
                let scope = if receiver_name.starts_with("this@") {
                    let label = receiver_name.trim_start_matches("this@");
                    scopes
                        .iter()
                        .find(|scope| scope.label.as_deref() == Some(label))
                        .cloned()
                } else {
                    scopes.first().cloned()
                };
                if let Some(scope) = scope {
                    let mut members =
                        self.members_of_type(file, reference, name, &scope.receiver_type);
                    if members.is_empty() {
                        members =
                            self.extensions_for_type(file, reference, name, &scope.receiver_type);
                    }
                    return self.accessible(file, reference, members);
                }
                if let Some(label) = receiver_name.strip_prefix("this@") {
                    let package = self
                        .graph
                        .semantic(
                            file,
                            reference.context_id,
                            Some(ResolutionFamily::KotlinModules),
                        )
                        .and_then(|semantic| semantic.context.as_ref())
                        .map_or(&[][..], |context| context.package.as_slice());
                    if let Some(class) = self.named(label).find(|candidate| {
                        candidate.package == package && is_type_declaration(candidate)
                    }) {
                        return self.accessible(
                            file,
                            reference,
                            self.members_of_type(file, reference, name, &class.name),
                        );
                    }
                }
                return Vec::new();
            }
            let bindings = self
                .graph
                .bindings_named(
                    file,
                    receiver_name,
                    reference.context_id,
                    reference.start_byte,
                )
                .collect::<Vec<_>>();
            if !bindings.is_empty() {
                let typed = self
                    .graph
                    .binding_type(
                        file,
                        receiver_name,
                        reference.context_id,
                        reference.start_byte,
                    )
                    .or_else(|| {
                        self.inferred_lambda_parameter_type(file, reference, receiver_name)
                    });
                return typed.map_or_else(Vec::new, |ty| {
                    let mut members = self.members_of_type(file, reference, name, &ty);
                    if members.is_empty() {
                        members = self.extensions_for_type(file, reference, name, &ty);
                    }
                    self.accessible(file, reference, members)
                });
            }
            if let Some(typed) = self.inferred_lambda_parameter_type(file, reference, receiver_name)
            {
                let mut members = self.members_of_type(file, reference, name, &typed);
                if members.is_empty() {
                    members = self.extensions_for_type(file, reference, name, &typed);
                }
                return self.accessible(file, reference, members);
            }
            if let Some(imported_type) = self.imported_type(file, reference, receiver_name) {
                return self.accessible(
                    file,
                    reference,
                    self.members_of_type(file, reference, name, &imported_type),
                );
            }
            let owner_type = self
                .named(receiver_name)
                .filter(|candidate| candidate.package == package && is_type_declaration(candidate))
                .filter(|candidate| candidate.owner.is_empty())
                .map(candidate_type_name)
                .collect::<BTreeSet<_>>();
            let owner_type = (owner_type.len() == 1)
                .then(|| owner_type.into_iter().next())
                .flatten();
            if let Some(owner_type) = owner_type {
                let mut members = self.members_of_type(file, reference, name, &owner_type);
                if members.is_empty() && !self.is_static_type_qualifier(file, reference) {
                    members = self.extensions_for_type(file, reference, name, &owner_type);
                }
                return self.accessible(file, reference, members);
            }
            for import in self.imports(file, reference.context_id) {
                let local_name = import
                    .alias
                    .as_deref()
                    .or_else(|| import.path.last().map(String::as_str));
                if local_name == Some(path[0].as_str()) && import.namespace {
                    let package_path = import.path.clone();
                    let imported = self
                        .named(name)
                        .filter(|candidate| {
                            candidate.package == package_path && candidate.owner.is_empty()
                        })
                        .map(|candidate| self.candidate_index(candidate))
                        .collect::<Vec<_>>();
                    return self.accessible(file, reference, imported);
                }
            }
            // A package path is considered only after lexical receiver
            // shadowing and imported type/namespace lookup.
            let package_candidates = self
                .named(name)
                .filter(|candidate| candidate.package == [path[0].clone()])
                .filter(|candidate| candidate.owner.is_empty())
                .filter(|candidate| self.visible_in_source_set(file, candidate))
                .collect::<Vec<_>>();
            if !package_candidates.is_empty() {
                return self.accessible(
                    file,
                    reference,
                    package_candidates
                        .iter()
                        .map(|candidate| self.candidate_index(candidate))
                        .collect(),
                );
            }
            // A syntactically qualified call with an unknown value receiver
            // stays unresolved; do not resolve the last segment as a package
            // or same-package top-level function.
            if !path[0].is_empty() {
                return Vec::new();
            }
        }

        // Fully-qualified package names require every path segment before the
        // declaration name to be the package path.
        if path.len() > 2 {
            let prefix = &path[..path.len() - 1];
            let candidates = self
                .named(name)
                .filter(|candidate| candidate.package == prefix && candidate.owner.is_empty())
                .filter(|candidate| self.visible_in_source_set(file, candidate))
                .collect::<Vec<_>>();
            if !candidates.is_empty() {
                return self.accessible(
                    file,
                    reference,
                    candidates
                        .iter()
                        .map(|candidate| self.candidate_index(candidate))
                        .collect(),
                );
            }
        }

        // Imported object/class aliases can qualify static-looking members.
        if let Some(ty) = self.imported_type(file, reference, &path[0]) {
            return self.accessible(
                file,
                reference,
                self.members_of_type(file, reference, name, &ty),
            );
        }

        // A known import of a package namespace can qualify package members.
        for import in self.imports(file, reference.context_id) {
            let local_name = import
                .alias
                .as_deref()
                .or_else(|| import.path.last().map(String::as_str));
            if local_name == Some(path[0].as_str()) && import.namespace {
                let mut qualified = import.path.clone();
                qualified.extend(path.iter().skip(1).cloned());
                for split in (1..qualified.len()).rev() {
                    let candidates = self
                        .named(name)
                        .filter(|candidate| candidate.package == qualified[..split])
                        .filter(|candidate| candidate.owner.is_empty())
                        .collect::<Vec<_>>();
                    if !candidates.is_empty() {
                        return self.accessible(
                            file,
                            reference,
                            candidates
                                .iter()
                                .map(|candidate| self.candidate_index(candidate))
                                .collect(),
                        );
                    }
                }
            }
        }
        Vec::new()
    }

    fn imported_candidates(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
        wildcard_only: bool,
    ) -> Vec<usize> {
        let mut result = Vec::new();
        for import in self.imports(file, reference.context_id) {
            if import.namespace {
                if !wildcard_only {
                    continue;
                }
                result.extend(
                    self.named(name)
                        .filter(|candidate| candidate.package == import.path)
                        .filter(|candidate| candidate.owner.is_empty())
                        .filter(|candidate| !self.is_local_candidate(candidate))
                        .filter(|candidate| self.visible_in_source_set(file, candidate))
                        .map(|candidate| self.candidate_index(candidate)),
                );
                continue;
            }
            if wildcard_only {
                continue;
            }
            let local_name = import
                .alias
                .as_deref()
                .or_else(|| import.path.last().map(String::as_str));
            if local_name != Some(name) {
                continue;
            }
            let imported = import.path.last().map(String::as_str).unwrap_or(name);
            result.extend(
                self.named(imported)
                    .filter(|candidate| {
                        candidate_type_segments(candidate) == import.path
                            && !self.is_local_candidate(candidate)
                    })
                    .filter(|candidate| self.visible_in_source_set(file, candidate))
                    .map(|candidate| self.candidate_index(candidate)),
            );
        }
        result
    }

    fn callable_alias_candidates(
        &self,
        file: usize,
        reference: &Reference,
        target: &str,
        package: &[String],
    ) -> Vec<usize> {
        let segments = target.split('.').map(str::to_owned).collect::<Vec<_>>();
        let Some(name) = segments.last() else {
            return Vec::new();
        };
        let target_package = if segments.len() == 1 {
            package.to_vec()
        } else {
            segments[..segments.len() - 1].to_vec()
        };
        self.named(name)
            .filter(|candidate| candidate.package == target_package && candidate.owner.is_empty())
            .filter(|candidate| {
                candidate.declaration.as_ref().is_some_and(|declaration| {
                    declaration.kind == crate::graph::DeclarationKind::Function
                        && !declaration.modifiers.is_open
                        && !declaration.modifiers.is_abstract
                        && !declaration.modifiers.is_expect
                })
            })
            .map(|candidate| self.candidate_index(candidate))
            .filter(|index| !self.accessible(file, reference, vec![*index]).is_empty())
            .collect()
    }

    fn members_of_type(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
        type_name: &str,
    ) -> Vec<usize> {
        self.members_of_type_seen(
            file,
            reference,
            name,
            type_name,
            self.is_static_type_qualifier(file, reference),
            &mut BTreeSet::new(),
        )
    }

    fn is_static_type_qualifier(&self, file: usize, reference: &Reference) -> bool {
        if reference.path.len() < 2 {
            return false;
        }
        let receiver = reference.path[0].as_str();
        if receiver == "this" || receiver.starts_with("this@") || receiver == "super" {
            return false;
        }
        if self
            .graph
            .binding_type(file, receiver, reference.context_id, reference.start_byte)
            .is_some()
            || self.graph.has_local_binding(
                file,
                receiver,
                reference.context_id,
                reference.start_byte,
            )
        {
            return false;
        }
        if self.imported_type(file, reference, receiver).is_some() {
            return true;
        }
        let package = self
            .graph
            .semantic(
                file,
                reference.context_id,
                Some(ResolutionFamily::KotlinModules),
            )
            .and_then(|semantic| semantic.context.as_ref())
            .map_or(&[][..], |semantic| semantic.package.as_slice());
        let candidates = self
            .named(receiver)
            .filter(|candidate| {
                candidate.package == package
                    && candidate.owner.is_empty()
                    && is_type_declaration(candidate)
            })
            .collect::<Vec<_>>();
        candidates.len() == 1
    }

    fn members_of_type_seen(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
        type_name: &str,
        static_qualifier: bool,
        seen: &mut BTreeSet<SymbolId>,
    ) -> Vec<usize> {
        let type_name = normalize_type(type_name);
        let final_type = type_name.rsplit('.').next().unwrap_or(&type_name);
        let semantic = self.graph.semantic(
            file,
            reference.context_id,
            Some(ResolutionFamily::KotlinModules),
        );
        let package = semantic
            .and_then(|semantic| semantic.context.as_ref())
            .map_or(&[][..], |ctx| ctx.package.as_slice());
        let imported_types = self.imported_candidates(file, reference, final_type, false);
        let mut classes = self
            .candidates
            .iter()
            .filter(|candidate| candidate.name == final_type)
            .filter(|candidate| is_type_declaration(candidate))
            .filter(|candidate| self.visible_in_source_set(file, candidate))
            .filter(|candidate| {
                if type_name.contains('.') {
                    candidate_type_name(candidate) == type_name
                } else if !imported_types.is_empty() {
                    imported_types.contains(&self.candidate_index(candidate))
                } else {
                    candidate.package == package && candidate.owner.is_empty()
                }
            })
            .collect::<Vec<_>>();
        if classes.len() == 1
            && let Some(alias) = classes[0]
                .declaration
                .as_ref()
                .and_then(|declaration| declaration.alias_target.as_deref())
        {
            let alias_type = alias.rsplit('.').next().unwrap_or(alias);
            if alias_type != final_type {
                let aliased =
                    self.members_of_type_seen(file, reference, name, alias, static_qualifier, seen);
                if !aliased.is_empty() {
                    return aliased;
                }
            }
        }
        classes.dedup_by_key(|candidate| candidate.symbol.clone());
        let owner_paths = classes
            .iter()
            .map(|candidate| {
                let mut owner = candidate.owner.clone();
                owner.push(candidate.name.clone());
                (candidate.package.clone(), owner)
            })
            .collect::<Vec<_>>();
        let mut member_candidates = if !static_qualifier {
            self.named(name)
                .filter(|candidate| !candidate.owner.is_empty())
                .filter(|candidate| {
                    owner_paths.iter().any(|(package, owner)| {
                        *package == candidate.package && *owner == candidate.owner
                    })
                })
                .filter(|candidate| self.visible_in_source_set(file, candidate))
                .map(|candidate| self.candidate_index(candidate))
                .collect::<Vec<_>>()
        } else {
            Vec::new()
        };
        // Object declarations are values and can be qualified directly; class
        // names only expose their companion's static-looking members.
        if static_qualifier {
            member_candidates.extend(
                classes
                    .iter()
                    .filter(|class| {
                        class.declaration.as_ref().is_some_and(|declaration| {
                            declaration.kind == crate::graph::DeclarationKind::Object
                        })
                    })
                    .flat_map(|class| {
                        let mut owner = class.owner.clone();
                        owner.push(class.name.clone());
                        self.named(name)
                            .filter(move |candidate| {
                                candidate.package == class.package && candidate.owner == owner
                            })
                            .map(|candidate| self.candidate_index(candidate))
                    }),
            );
        }
        // A class's companion object is addressable as `Class.member`.
        for companion in self.candidates.iter().filter(|candidate| {
            static_qualifier
                && candidate
                    .declaration
                    .as_ref()
                    .is_some_and(|item| item.kind == crate::graph::DeclarationKind::CompanionObject)
                && owner_paths.iter().any(|(package, owner)| {
                    *package == candidate.package && *owner == candidate.owner
                })
        }) {
            let mut companion_owner = companion.owner.clone();
            companion_owner.push(companion.name.clone());
            member_candidates.extend(
                self.named(name)
                    .filter(|candidate| {
                        candidate.package == companion.package && candidate.owner == companion_owner
                    })
                    .map(|candidate| self.candidate_index(candidate)),
            );
        }
        if static_qualifier {
            return unique_candidates(member_candidates);
        }
        let mut inherited_candidates = Vec::new();
        for class in &classes {
            if !seen.insert(class.symbol.clone()) {
                continue;
            }
            let Some(declaration) = class.declaration.as_ref() else {
                continue;
            };
            for supertype in &declaration.supertypes {
                let super_candidates = self.resolve_declared_type(
                    class.file,
                    class.context_id,
                    supertype,
                    &class.package,
                );
                for parent in super_candidates {
                    let qualified = candidate_type_name(parent);
                    inherited_candidates.extend(
                        self.members_of_type_seen(file, reference, name, &qualified, false, seen),
                    );
                }
            }
        }
        let overriding = member_candidates
            .iter()
            .copied()
            .filter(|index| {
                self.candidates[*index]
                    .declaration
                    .as_ref()
                    .is_some_and(|declaration| declaration.modifiers.is_override)
            })
            .collect::<Vec<_>>();
        inherited_candidates.retain(|inherited| {
            !overriding.iter().any(|override_index| {
                same_callable_signature(
                    self.candidates[*override_index].declaration.as_ref(),
                    self.candidates[*inherited].declaration.as_ref(),
                )
            })
        });
        member_candidates.extend(inherited_candidates);
        unique_candidates(member_candidates)
    }

    fn resolve_declared_type(
        &self,
        file: usize,
        context_id: usize,
        type_name: &str,
        package: &[String],
    ) -> Vec<&Candidate> {
        let name = type_name.rsplit('.').next().unwrap_or(type_name);
        if let Some((qualified_package, _)) = type_name.rsplit_once('.') {
            let qualified = qualified_package
                .split('.')
                .map(str::to_owned)
                .chain(std::iter::once(name.to_owned()))
                .collect::<Vec<_>>();
            return self
                .named(name)
                .filter(|candidate| {
                    candidate_type_segments(candidate) == qualified
                        && is_type_declaration(candidate)
                })
                .collect();
        }
        let imported = self
            .graph
            .imports(file)
            .iter()
            .filter(|import| import.context_id == context_id && !import.namespace)
            .filter(|import| {
                import
                    .alias
                    .as_deref()
                    .or_else(|| import.path.last().map(String::as_str))
                    == Some(name)
            })
            .flat_map(|import| {
                let imported_name = import.path.last().map(String::as_str).unwrap_or(name);
                self.named(imported_name).filter(move |candidate| {
                    candidate_type_segments(candidate) == import.path
                        && is_type_declaration(candidate)
                })
            })
            .collect::<Vec<_>>();
        if !imported.is_empty() {
            return imported;
        }
        self.named(name)
            .filter(|candidate| {
                candidate.package == package
                    && candidate.owner.is_empty()
                    && is_type_declaration(candidate)
            })
            .collect()
    }

    fn extensions_for_type(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
        type_name: &str,
    ) -> Vec<usize> {
        let semantic = self.graph.semantic(
            file,
            reference.context_id,
            Some(ResolutionFamily::KotlinModules),
        );
        let current_package = semantic
            .and_then(|item| item.context.as_ref())
            .map_or(&[][..], |item| item.package.as_slice());
        let expected = self.canonical_type_names(
            file,
            reference.context_id,
            current_package,
            type_name,
            &mut BTreeSet::new(),
        );
        let imports = self.imports(file, reference.context_id).collect::<Vec<_>>();
        self.named(name)
            .filter(|candidate| candidate.owner.is_empty())
            .filter(|candidate| is_extension_candidate(candidate))
            .filter(|candidate| self.visible_in_source_set(file, candidate))
            .filter(|candidate| {
                candidate.package == current_package
                    || imports.iter().any(|import| {
                        if import.namespace {
                            import.path == candidate.package
                        } else {
                            import
                                .path
                                .last()
                                .is_some_and(|imported_name| imported_name == name)
                                && import.path[..import.path.len().saturating_sub(1)]
                                    == candidate.package
                        }
                    })
            })
            .filter(|candidate| {
                let Some(receiver) = candidate
                    .declaration
                    .as_ref()
                    .and_then(|item| item.signature.as_ref())
                    .and_then(|signature| signature.receiver_type.as_deref())
                else {
                    return false;
                };
                let actual = self.canonical_type_names(
                    candidate.file,
                    candidate.context_id,
                    &candidate.package,
                    receiver,
                    &mut BTreeSet::new(),
                );
                !expected.is_empty()
                    && !actual.is_empty()
                    && expected.iter().any(|item| actual.contains(item))
            })
            .map(|candidate| self.candidate_index(candidate))
            .collect()
    }

    fn canonical_type_names(
        &self,
        file: usize,
        context_id: usize,
        package: &[String],
        type_name: &str,
        seen: &mut BTreeSet<String>,
    ) -> BTreeSet<String> {
        let normalized = normalize_type(type_name);
        if !seen.insert(normalized.clone()) {
            return BTreeSet::new();
        }
        let declarations = self.resolve_declared_type(file, context_id, &normalized, package);
        if declarations.len() > 1 {
            return BTreeSet::new();
        }
        if let Some(candidate) = declarations.first() {
            if let Some(alias) = candidate
                .declaration
                .as_ref()
                .and_then(|declaration| declaration.alias_target.as_deref())
            {
                return self.canonical_type_names(
                    candidate.file,
                    candidate.context_id,
                    &candidate.package,
                    alias,
                    seen,
                );
            }
            return [candidate_type_name(candidate)].into_iter().collect();
        }
        let builtins = [
            "Any", "Nothing", "Unit", "Boolean", "Byte", "Short", "Int", "Long", "Float", "Double",
            "Char", "String",
        ];
        if builtins.contains(&normalized.as_str()) || normalized.contains('.') {
            [normalized].into_iter().collect()
        } else {
            BTreeSet::new()
        }
    }

    fn implicit_receiver_candidates(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
    ) -> Option<Vec<usize>> {
        self.implicit_receiver_candidates_from_scopes(
            file,
            reference,
            name,
            self.active_receivers(file, reference),
        )
    }

    fn implicit_receiver_candidates_from_scopes(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
        scopes: Vec<ReceiverScope>,
    ) -> Option<Vec<usize>> {
        let mut seen_markers = HashSet::new();
        for scope in scopes {
            if scope
                .dsl_markers
                .iter()
                .any(|marker| seen_markers.contains(marker))
            {
                continue;
            }
            seen_markers.extend(scope.dsl_markers.iter().cloned());
            let members = self.members_of_type(file, reference, name, &scope.receiver_type);
            if !members.is_empty() {
                return Some(self.accessible(file, reference, members));
            }
            let extensions = self.extensions_for_type(file, reference, name, &scope.receiver_type);
            if !extensions.is_empty() {
                return Some(self.accessible(file, reference, extensions));
            }
        }
        None
    }

    fn active_receivers(&self, file: usize, reference: &Reference) -> Vec<ReceiverScope> {
        let Some(semantic) = self.graph.semantic(
            file,
            reference.context_id,
            Some(ResolutionFamily::KotlinModules),
        ) else {
            return Vec::new();
        };
        let mut scopes = semantic
            .receivers
            .iter()
            .filter(|scope| {
                scope.implicit
                    && scope.start_byte <= reference.start_byte
                    && reference.start_byte < scope.end_byte
            })
            .cloned()
            .collect::<Vec<_>>();
        scopes.extend(self.inferred_lambda_receivers(file, reference));
        scopes.sort_by_key(|scope| scope.end_byte.saturating_sub(scope.start_byte));
        scopes
    }

    fn inferred_lambda_receivers(&self, file: usize, reference: &Reference) -> Vec<ReceiverScope> {
        let calls = self.graph.call_facts(
            file,
            reference.context_id,
            Some(ResolutionFamily::KotlinModules),
        );
        let mut active_arguments = calls
            .iter()
            .filter_map(|call| {
                call.arguments
                    .iter()
                    .find(|argument| {
                        argument.is_lambda
                            && argument.start_byte <= reference.start_byte
                            && reference.start_byte < argument.end_byte
                    })
                    .map(|argument| (call, argument))
            })
            .collect::<Vec<_>>();
        active_arguments.sort_by_key(|(_, argument)| {
            (argument.start_byte, std::cmp::Reverse(argument.end_byte))
        });
        active_arguments.dedup_by_key(|(_, argument)| (argument.start_byte, argument.end_byte));

        let mut inferred = Vec::<ReceiverScope>::new();
        for (call, argument) in active_arguments {
            let Some(receiver_type) = argument.lambda_receiver_type.clone().or_else(|| {
                self.inferred_receiver_for_lambda(file, reference, call, argument, &inferred)
            }) else {
                continue;
            };
            let outer = self.callee_reference_for_call(file, reference.context_id, call);
            let label = argument
                .lambda_label
                .clone()
                .or_else(|| outer.and_then(|call| call.path.last().cloned()));
            inferred.push(ReceiverScope {
                start_byte: argument.start_byte,
                end_byte: argument.end_byte,
                dsl_markers: self.dsl_markers_for_type(file, reference, &receiver_type),
                receiver_type,
                implicit: true,
                label,
            });
        }
        inferred
    }

    fn inferred_receiver_for_lambda(
        &self,
        file: usize,
        reference: &Reference,
        call: &crate::graph::CallFacts,
        argument: &crate::graph::CallArgumentFact,
        inherited_receivers: &[ReceiverScope],
    ) -> Option<String> {
        let outer = self.callee_reference_for_call(file, reference.context_id, call)?;
        let mut lexical_receivers = self
            .graph
            .semantic(
                file,
                reference.context_id,
                Some(ResolutionFamily::KotlinModules),
            )
            .into_iter()
            .flat_map(|semantic| semantic.receivers.iter())
            .filter(|scope| {
                scope.implicit
                    && scope.start_byte <= reference.start_byte
                    && reference.start_byte < scope.end_byte
            })
            .cloned()
            .collect::<Vec<_>>();
        lexical_receivers.extend_from_slice(inherited_receivers);
        lexical_receivers.sort_by_key(|scope| scope.end_byte.saturating_sub(scope.start_byte));
        let candidates = if outer.path.len() == 1 {
            self.implicit_receiver_candidates_from_scopes(
                file,
                outer,
                outer.path.last()?,
                lexical_receivers,
            )
            .unwrap_or_else(|| self.callable_candidates(file, outer))
        } else {
            self.callable_candidates(file, outer)
        };
        let candidates = self.filter_callable_overloads(file, outer, candidates);
        let mut receivers = candidates
            .into_iter()
            .filter_map(|index| {
                let signature = self.candidates[index]
                    .declaration
                    .as_ref()?
                    .signature
                    .as_ref()?;
                let parameter =
                    lambda_parameter_for_argument(signature, &call.arguments, argument)?;
                let type_name = parameter.type_name.as_deref()?;
                let inferred_arguments =
                    self.infer_argument_return_types(file, outer, &call.arguments);
                let substitutions = generic_type_substitutions(signature, &inferred_arguments);
                let type_name = substitute_type_name(type_name, &substitutions);
                self.receiver_from_function_type(
                    self.candidates[index].file,
                    self.candidates[index].context_id,
                    &type_name,
                    &self.candidates[index].package,
                    &mut BTreeSet::new(),
                )
            })
            .collect::<BTreeSet<_>>();
        (receivers.len() == 1)
            .then(|| receivers.pop_first())
            .flatten()
    }

    fn callable_candidates(&self, file: usize, reference: &Reference) -> Vec<usize> {
        let semantic = self.graph.semantic(
            file,
            reference.context_id,
            Some(ResolutionFamily::KotlinModules),
        );
        let package = semantic
            .and_then(|semantic| semantic.context.as_ref())
            .map_or(&[][..], |ctx| ctx.package.as_slice());
        let Some(name) = reference.path.last() else {
            return Vec::new();
        };
        let candidates = if reference.path.len() == 1 {
            let explicit = self.imported_candidates(file, reference, name, false);
            if !explicit.is_empty() {
                explicit
            } else {
                let package_candidates = self
                    .named(name)
                    .filter(|candidate| candidate.package == package && candidate.owner.is_empty())
                    .filter(|candidate| self.visible_in_source_set(file, candidate))
                    .map(|candidate| self.candidate_index(candidate))
                    .collect::<Vec<_>>();
                if package_candidates.is_empty() {
                    self.imported_candidates(file, reference, name, true)
                } else {
                    package_candidates
                }
            }
        } else {
            self.qualified_candidates(file, reference, &reference.path, package)
        };
        self.filter_callable_overloads(file, reference, candidates)
    }

    fn inside_unresolved_lambda_argument(&self, file: usize, reference: &Reference) -> bool {
        let active = self
            .graph
            .call_facts(
                file,
                reference.context_id,
                Some(ResolutionFamily::KotlinModules),
            )
            .iter()
            .filter_map(|call| {
                call.arguments
                    .iter()
                    .find(|argument| {
                        argument.is_lambda
                            && argument.start_byte <= reference.start_byte
                            && reference.start_byte < argument.end_byte
                    })
                    .map(|argument| (call, argument))
            })
            .min_by_key(|(_, argument)| argument.end_byte - argument.start_byte);
        let Some((call, argument)) = active else {
            return false;
        };
        if argument.lambda_receiver_type.is_some()
            || !self.inferred_lambda_receivers(file, reference).is_empty()
        {
            return false;
        }
        let Some(outer) = self.callee_reference_for_call(file, reference.context_id, call) else {
            return true;
        };
        let candidates = self.callable_candidates(file, outer);
        if candidates.is_empty() {
            return true;
        }
        let mut found_signature = false;
        let all_plain_callbacks = candidates.into_iter().all(|index| {
            let Some(signature) = self.candidates[index]
                .declaration
                .as_ref()
                .and_then(|declaration| declaration.signature.as_ref())
            else {
                return false;
            };
            let Some(parameter) =
                lambda_parameter_for_argument(signature, &call.arguments, argument)
            else {
                return false;
            };
            let Some(type_name) = parameter.type_name.as_deref() else {
                return false;
            };
            if !type_name.contains("->") {
                return false;
            }
            found_signature = true;
            function_type_receiver(type_name).is_none()
        });
        !found_signature || !all_plain_callbacks
    }

    fn dsl_markers_for_type(
        &self,
        file: usize,
        reference: &Reference,
        type_name: &str,
    ) -> Vec<String> {
        let type_name = normalize_type(type_name);
        let final_name = type_name.rsplit('.').next().unwrap_or(&type_name);
        let types = self.resolve_declared_type(
            file,
            reference.context_id,
            &type_name,
            self.graph
                .semantic(
                    file,
                    reference.context_id,
                    Some(ResolutionFamily::KotlinModules),
                )
                .and_then(|semantic| semantic.context.as_ref())
                .map_or(&[][..], |context| context.package.as_slice()),
        );
        let mut markers = Vec::new();
        for ty in types
            .into_iter()
            .filter(|candidate| candidate.name == final_name)
        {
            let Some(declaration) = ty.declaration.as_ref() else {
                continue;
            };
            for annotation in &declaration.dsl_markers {
                let marker_name = annotation.rsplit('.').next().unwrap_or(annotation);
                let is_dsl_marker = self.named(marker_name).any(|candidate| {
                    candidate.declaration.as_ref().is_some_and(|declaration| {
                        declaration
                            .dsl_markers
                            .iter()
                            .any(|marker| marker.rsplit('.').next() == Some("DslMarker"))
                    })
                });
                if is_dsl_marker {
                    markers.push(annotation.clone());
                }
            }
        }
        markers
    }

    fn receiver_from_function_type(
        &self,
        file: usize,
        context_id: usize,
        function_type: &str,
        package: &[String],
        seen: &mut BTreeSet<String>,
    ) -> Option<String> {
        let normalized = normalize_type(function_type);
        if !seen.insert(normalized.clone()) {
            return None;
        }
        if let Some(receiver) = function_type_receiver(&normalized) {
            let canonical = self.canonical_type_names(
                file,
                context_id,
                package,
                receiver,
                &mut BTreeSet::new(),
            );
            return (canonical.len() == 1)
                .then(|| canonical.into_iter().next())
                .flatten();
        }
        for candidate in self.resolve_declared_type(file, context_id, &normalized, package) {
            let Some(alias) = candidate
                .declaration
                .as_ref()
                .and_then(|declaration| declaration.alias_target.as_deref())
            else {
                continue;
            };
            if let Some(receiver) = self.receiver_from_function_type(
                candidate.file,
                candidate.context_id,
                alias,
                &candidate.package,
                seen,
            ) {
                return Some(receiver);
            }
        }
        None
    }

    fn is_super_reference(&self, file: usize, reference: &Reference) -> bool {
        if reference.path.first().is_some_and(|part| part == "super") {
            return true;
        }
        let path = &self.graph.file(file).target;
        let Some(source) = self.snapshot.read(path).ok().flatten() else {
            return false;
        };
        let Some(prefix) = source.get(..reference.start_byte) else {
            return false;
        };
        let prefix = String::from_utf8_lossy(prefix);
        prefix.trim_end().ends_with("super.")
    }

    fn method_receiver_type(
        &self,
        file: usize,
        reference: &Reference,
        is_super: bool,
    ) -> Option<String> {
        let containing_call = self
            .graph
            .call_facts(
                file,
                reference.context_id,
                Some(ResolutionFamily::KotlinModules),
            )
            .iter()
            .filter(|call| {
                call.start_byte <= reference.start_byte && reference.end_byte <= call.end_byte
            })
            .min_by_key(|call| (call.end_byte - call.start_byte, call.convention.is_none()));
        if containing_call
            .is_some_and(|call| call.convention.is_some() && call.receiver_type.is_none())
        {
            return None;
        }
        if let Some(ty) = containing_call.and_then(|call| call.receiver_type.clone()) {
            return Some(ty);
        }
        if is_super {
            let class = self
                .candidates
                .iter()
                .filter(|candidate| candidate.file == file)
                .filter(|candidate| {
                    candidate
                        .declaration
                        .as_ref()
                        .is_some_and(|item| item.kind == crate::graph::DeclarationKind::Class)
                })
                .filter(|candidate| {
                    candidate.start <= reference.start_byte && reference.start_byte < candidate.end
                })
                .min_by_key(|candidate| candidate.end - candidate.start)?;
            let declaration = class.declaration.as_ref()?;
            let parents = declaration
                .supertypes
                .iter()
                .flat_map(|supertype| {
                    self.resolve_declared_type(
                        class.file,
                        class.context_id,
                        supertype,
                        &class.package,
                    )
                })
                .collect::<Vec<_>>();
            if parents.len() == 1 {
                return Some(
                    parents[0]
                        .package
                        .iter()
                        .chain(std::iter::once(&parents[0].name))
                        .cloned()
                        .collect::<Vec<_>>()
                        .join("."),
                );
            }
        }
        if reference
            .path
            .first()
            .is_some_and(|name| name == "this" || name.starts_with("this@"))
        {
            if let Some(label) = reference
                .path
                .first()
                .and_then(|name| name.strip_prefix("this@"))
            {
                let scope = self
                    .active_receivers(file, reference)
                    .into_iter()
                    .find(|scope| scope.label.as_deref() == Some(label));
                if let Some(scope) = scope {
                    return Some(scope.receiver_type);
                }
                if let Some(candidate) = self.candidates.iter().find(|candidate| {
                    candidate.file == file
                        && candidate.package
                            == self
                                .graph
                                .semantic(
                                    file,
                                    reference.context_id,
                                    Some(ResolutionFamily::KotlinModules),
                                )
                                .and_then(|semantic| semantic.context.as_ref())
                                .map_or(&[][..], |context| context.package.as_slice())
                        && candidate.name == label
                        && is_type_declaration(candidate)
                }) {
                    return Some(candidate_type_name(candidate));
                }
                return None;
            }
            return self
                .candidates
                .iter()
                .filter(|candidate| candidate.file == file)
                .filter(|candidate| {
                    candidate.declaration.as_ref().is_some_and(|item| {
                        matches!(
                            item.kind,
                            crate::graph::DeclarationKind::Class
                                | crate::graph::DeclarationKind::Object
                                | crate::graph::DeclarationKind::CompanionObject
                        )
                    })
                })
                .filter(|candidate| {
                    candidate.start <= reference.start_byte && reference.start_byte < candidate.end
                })
                .min_by_key(|candidate| candidate.end - candidate.start)
                .map(candidate_type_name);
        }
        let receiver = reference.path.first()?;
        if let Some(type_name) =
            self.graph
                .binding_type(file, receiver, reference.context_id, reference.start_byte)
        {
            return Some(type_name);
        }
        if receiver == "it"
            && let Some(type_name) = self.inferred_lambda_parameter_type(file, reference, receiver)
        {
            return Some(type_name);
        }
        if self
            .graph
            .has_local_binding(file, receiver, reference.context_id, reference.start_byte)
        {
            if let Some(type_name) = self.inferred_lambda_parameter_type(file, reference, receiver)
            {
                return Some(type_name);
            }
            return None;
        }
        self.imported_type(file, reference, receiver).or_else(|| {
            let package = self
                .graph
                .semantic(
                    file,
                    reference.context_id,
                    Some(ResolutionFamily::KotlinModules),
                )
                .and_then(|semantic| semantic.context.as_ref())
                .map_or(&[][..], |context| context.package.as_slice());
            self.named(receiver)
                .filter(|candidate| candidate.package == package && is_type_declaration(candidate))
                .filter(|candidate| candidate.owner.is_empty())
                .map(candidate_type_name)
                .next()
        })
    }

    fn inferred_lambda_parameter_type(
        &self,
        file: usize,
        reference: &Reference,
        name: &str,
    ) -> Option<String> {
        let (call, argument) = self
            .graph
            .call_facts(
                file,
                reference.context_id,
                Some(ResolutionFamily::KotlinModules),
            )
            .iter()
            .filter_map(|call| {
                call.arguments
                    .iter()
                    .find(|argument| {
                        argument.is_lambda
                            && argument.start_byte <= reference.start_byte
                            && reference.start_byte < argument.end_byte
                    })
                    .map(|argument| (call, argument))
            })
            .min_by_key(|(_, argument)| argument.end_byte - argument.start_byte)?;
        let outer = self.callee_reference_for_call(file, reference.context_id, call)?;
        let callback_candidates = self.callable_candidates(file, outer);
        let mut inferred = BTreeSet::new();
        for candidate_index in callback_candidates {
            let candidate = &self.candidates[candidate_index];
            let signature = candidate
                .declaration
                .as_ref()
                .and_then(|declaration| declaration.signature.as_ref())?;
            let parameter = lambda_parameter_for_argument(signature, &call.arguments, argument)?;
            let type_name = parameter.type_name.as_deref()?;
            let inferred_arguments = self.infer_argument_return_types(file, outer, &call.arguments);
            let substitutions = generic_type_substitutions(signature, &inferred_arguments);
            let type_name = substitute_type_name(type_name, &substitutions);
            let parameters = function_type_parameters(&type_name)?;
            let parameter_index = if !argument.lambda_has_explicit_parameters {
                if name != "it" || parameters.len() != 1 {
                    return None;
                }
                0
            } else {
                argument
                    .lambda_parameter_names
                    .iter()
                    .position(|parameter_name| parameter_name == name)?
            };
            let parameter_type = parameters.get(parameter_index)?;
            let canonical = self.canonical_type_names(
                candidate.file,
                candidate.context_id,
                &candidate.package,
                parameter_type,
                &mut BTreeSet::new(),
            );
            if canonical.len() != 1 {
                return None;
            }
            inferred.insert(canonical.into_iter().next()?);
        }
        if inferred.len() != 1 {
            return None;
        }
        inferred.into_iter().next()
    }

    fn filter_callable_overloads(
        &self,
        file: usize,
        reference: &Reference,
        mut candidates: Vec<usize>,
    ) -> Vec<usize> {
        candidates.sort_unstable();
        candidates.dedup();
        if !matches!(reference.kind, ReferenceKind::Call | ReferenceKind::Method) {
            return candidates;
        }
        let Some(call) = self
            .graph
            .call_facts(
                file,
                reference.context_id,
                Some(ResolutionFamily::KotlinModules),
            )
            .iter()
            .filter(|call| {
                call.start_byte <= reference.start_byte && reference.end_byte <= call.end_byte
            })
            .min_by_key(|call| (call.end_byte - call.start_byte, call.convention.is_none()))
        else {
            return candidates;
        };
        let arguments = self.infer_argument_return_types(file, reference, &call.arguments);
        candidates
            .iter()
            .copied()
            .filter(|index| {
                let candidate = &self.candidates[*index];
                match call.convention {
                    Some(CallConvention::Operator)
                        if !candidate
                            .declaration
                            .as_ref()
                            .is_some_and(|declaration| declaration.modifiers.is_operator) =>
                    {
                        return false;
                    }
                    Some(CallConvention::Infix)
                        if !candidate
                            .declaration
                            .as_ref()
                            .is_some_and(|declaration| declaration.modifiers.is_infix) =>
                    {
                        return false;
                    }
                    _ => {}
                }
                if reference.kind == ReferenceKind::Call && is_type_declaration(candidate) {
                    return self.type_constructor_matches(candidate, &arguments);
                }
                let Some(signature) = self.candidates[*index]
                    .declaration
                    .as_ref()
                    .and_then(|item| item.signature.as_ref())
                else {
                    return true;
                };
                arguments_match(signature, &arguments)
            })
            .collect::<Vec<_>>()
    }

    fn infer_argument_return_types(
        &self,
        file: usize,
        reference: &Reference,
        arguments: &[crate::graph::CallArgumentFact],
    ) -> Vec<crate::graph::CallArgumentFact> {
        let mut inferred = arguments.to_vec();
        for argument in inferred
            .iter_mut()
            .filter(|argument| !argument.is_lambda && argument.type_name.is_none())
        {
            if let Some(source) = self
                .snapshot
                .read(&self.graph.file(file).target)
                .ok()
                .flatten()
                && let Some(expression) = source.get(argument.start_byte..argument.end_byte)
            {
                let expression = String::from_utf8_lossy(expression);
                let expression = expression.trim().trim_matches('`');
                if !expression.is_empty()
                    && expression
                        .chars()
                        .all(|character| character.is_alphanumeric() || character == '_')
                    && let Some(type_name) = self.graph.binding_type(
                        file,
                        expression,
                        reference.context_id,
                        argument.start_byte,
                    )
                {
                    argument.type_name = Some(
                        self.canonicalize_type_in_context(file, reference.context_id, &type_name)
                            .unwrap_or(type_name),
                    );
                    continue;
                }
            }
            let Some(call) = self
                .graph
                .call_facts(
                    file,
                    reference.context_id,
                    Some(ResolutionFamily::KotlinModules),
                )
                .iter()
                .filter(|call| {
                    call.start_byte == argument.start_byte
                        && call.end_byte == argument.end_byte
                        && call.target.is_some()
                })
                .min_by_key(|call| call.convention.is_none())
            else {
                continue;
            };
            let Some(target) = call.target.as_ref() else {
                continue;
            };
            let Some(inner) = self
                .graph
                .file(file)
                .references
                .iter()
                .filter(|inner| inner.context_id == reference.context_id)
                .filter(|inner| {
                    inner.start_byte == target.start_byte && inner.end_byte == target.end_byte
                })
                .find(|inner| matches!(inner.kind, ReferenceKind::Call | ReferenceKind::Method))
            else {
                continue;
            };
            let Some(name) = inner.path.last() else {
                continue;
            };
            let semantic = self.graph.semantic(
                file,
                inner.context_id,
                Some(ResolutionFamily::KotlinModules),
            );
            let package = semantic
                .and_then(|semantic| semantic.context.as_ref())
                .map_or(&[][..], |context| context.package.as_slice());
            let candidates = if inner.path.len() == 1 {
                self.simple_candidates(file, inner, name, package)
            } else {
                self.qualified_candidates(file, inner, &inner.path, package)
            };
            let candidates = self.filter_callable_overloads(file, inner, candidates);
            let candidates = self.select_expect_actual(file, candidates);
            let candidates = self.accessible(file, inner, candidates);
            if let [candidate] = candidates.as_slice() {
                let candidate = &self.candidates[*candidate];
                if is_type_declaration(candidate) {
                    argument.type_name = Some(candidate_type_name(candidate));
                } else if let Some(signature) = candidate
                    .declaration
                    .as_ref()
                    .and_then(|declaration| declaration.signature.as_ref())
                {
                    let substitutions = generic_type_substitutions(signature, &call.arguments);
                    argument.type_name = signature
                        .return_type
                        .as_deref()
                        .map(|return_type| substitute_type_name(return_type, &substitutions));
                    if let Some(type_name) = argument.type_name.as_deref() {
                        argument.type_name = Some(
                            self.canonicalize_type_in_context(
                                candidate.file,
                                candidate.context_id,
                                type_name,
                            )
                            .unwrap_or_else(|| type_name.to_owned()),
                        );
                    }
                }
            }
        }
        inferred
    }

    fn canonicalize_type_in_context(
        &self,
        file: usize,
        context_id: usize,
        type_name: &str,
    ) -> Option<String> {
        let semantic = self
            .graph
            .semantic(file, context_id, Some(ResolutionFamily::KotlinModules));
        let package = semantic
            .and_then(|semantic| semantic.context.as_ref())
            .map_or(&[][..], |semantic| semantic.package.as_slice());
        let canonical =
            self.canonical_type_names(file, context_id, package, type_name, &mut BTreeSet::new());
        (canonical.len() == 1)
            .then(|| canonical.into_iter().next())
            .flatten()
    }

    fn select_expect_actual(&self, file: usize, candidates: Vec<usize>) -> Vec<usize> {
        if candidates.len() < 2 {
            return candidates;
        }
        let caller = &self.sources[file];
        let has_platform = caller
            .source_set
            .as_deref()
            .is_some_and(|set| !set.starts_with("common"));
        if !has_platform {
            return candidates
                .into_iter()
                .filter(|candidate| {
                    let declaration = self.candidates[*candidate].declaration.as_ref();
                    declaration.is_none_or(|declaration| !declaration.modifiers.is_actual)
                })
                .collect();
        }
        let actual = candidates
            .iter()
            .copied()
            .filter(|index| {
                self.candidates[*index]
                    .declaration
                    .as_ref()
                    .is_some_and(|item| item.modifiers.is_actual)
            })
            .collect::<Vec<_>>();
        if actual.is_empty() {
            return candidates;
        }
        candidates
            .into_iter()
            .filter(|index| {
                let Some(declaration) = self.candidates[*index].declaration.as_ref() else {
                    return true;
                };
                if !declaration.modifiers.is_expect {
                    return true;
                }
                !actual.iter().any(|actual_index| {
                    same_logical_declaration(
                        &self.candidates[*index],
                        &self.candidates[*actual_index],
                    )
                })
            })
            .collect()
    }

    fn accessible(&self, file: usize, reference: &Reference, candidates: Vec<usize>) -> Vec<usize> {
        let caller = &self.sources[file];
        candidates
            .into_iter()
            .filter(|index| {
                let candidate = &self.candidates[*index];
                if candidate.file == file && candidate.context_id != reference.context_id {
                    return false;
                }
                let visibility = candidate.declaration.as_ref().map_or_else(
                    || {
                        if candidate.public {
                            SemanticVisibility::Public
                        } else {
                            SemanticVisibility::Private
                        }
                    },
                    |item| item.visibility,
                );
                match visibility {
                    SemanticVisibility::Private => {
                        if candidate.file == file {
                            candidate.owner.is_empty()
                                || reference.start_byte >= candidate.scope_start
                                    && reference.start_byte < candidate.scope_end
                        } else {
                            false
                        }
                    }
                    SemanticVisibility::Protected => {
                        candidate.file == file
                            && candidate.owner.last().is_some_and(|owner| {
                                self.candidates.iter().any(|possible_owner| {
                                    possible_owner.file == file
                                        && possible_owner.name == *owner
                                        && possible_owner.scope_start <= reference.start_byte
                                        && reference.start_byte < possible_owner.scope_end
                                })
                            })
                    }
                    SemanticVisibility::Internal => {
                        caller.project_root == candidate.source.project_root
                            && caller.gradle_module == candidate.source.gradle_module
                            && self.visible_in_source_set(file, candidate)
                    }
                    SemanticVisibility::Public | SemanticVisibility::Unknown => {
                        self.module_visible(caller, &candidate.source)
                            && self.visible_in_source_set(file, candidate)
                    }
                }
            })
            .collect()
    }

    fn is_local_candidate(&self, candidate: &Candidate) -> bool {
        self.graph
            .definition(candidate.graph_id)
            .is_some_and(|definition| self.graph.is_local_definition(candidate.file, definition))
    }

    fn module_visible(&self, caller: &SourceContext, target: &SourceContext) -> bool {
        if caller.isolated_source || target.isolated_source {
            return caller.isolated_source && target.isolated_source && caller.path == target.path;
        }
        if caller.module == target.module {
            return true;
        }
        if caller.project_root != target.project_root {
            return false;
        }
        let Some(deps) = self.dependencies.get(&caller.project_root) else {
            return false;
        };
        let reachable =
            transitive_closure(&caller.gradle_module, deps, caller.source_set.as_deref());
        reachable.contains(&target.gradle_module)
    }

    fn visible_in_source_set(&self, file: usize, candidate: &Candidate) -> bool {
        let caller = &self.sources[file];
        if caller.module != candidate.source.module {
            let Some(target) = candidate.source.source_set.as_deref() else {
                return true;
            };
            // A module's test source sets are not on another module's compile
            // classpath. Common production code is visible to all platforms;
            // platform production sets require a matching caller set.
            if target.eq_ignore_ascii_case("test") || target.to_ascii_lowercase().ends_with("test")
            {
                return false;
            }
            if target.eq_ignore_ascii_case("main") || target == "commonMain" {
                return true;
            }
            return caller.source_set.as_deref() == Some(target);
        }
        match (&caller.source_set, &candidate.source.source_set) {
            (None, _) | (_, None) => true,
            (Some(caller), Some(target)) if caller == target => true,
            (Some(caller), Some(target)) => {
                let edges = self
                    .source_edges
                    .get(&self.sources[file].module_root)
                    .cloned()
                    .unwrap_or_default();
                source_set_reaches(caller, target, &edges)
                    || conventional_source_edge(caller, target)
            }
        }
    }

    fn imported_type(&self, file: usize, reference: &Reference, name: &str) -> Option<String> {
        let candidates = self
            .imported_candidates(file, reference, name, false)
            .into_iter()
            .filter(|index| is_type_declaration(&self.candidates[*index]))
            .map(|index| &self.candidates[index])
            .collect::<Vec<_>>();
        let [candidate] = candidates.as_slice() else {
            return None;
        };
        Some(candidate_type_name(candidate))
    }

    fn imports(
        &self,
        file: usize,
        context_id: usize,
    ) -> impl Iterator<Item = &'a crate::model::Import> + '_ {
        self.graph
            .imports(file)
            .iter()
            .filter(move |item| item.context_id == context_id)
    }

    fn named(&self, name: &str) -> impl Iterator<Item = &Candidate> {
        self.by_name
            .get(name)
            .into_iter()
            .flatten()
            .map(|index| &self.candidates[*index])
    }

    fn candidate_index(&self, candidate: &Candidate) -> usize {
        candidate.candidate_index
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

fn unique_candidates(mut candidates: Vec<usize>) -> Vec<usize> {
    candidates.sort_unstable();
    candidates.dedup();
    candidates
}

fn call_dispatch(candidate: &Candidate, reference: &Reference) -> CallDispatch {
    let Some(declaration) = candidate.declaration.as_ref() else {
        return CallDispatch::Unknown;
    };
    if reference.path.iter().any(|segment| segment == "super") {
        return CallDispatch::Direct;
    }
    if is_type_declaration(candidate) {
        // A selected constructor is statically dispatched even when instances
        // of its class can later participate in virtual method dispatch.
        return CallDispatch::Direct;
    }
    if declaration.is_interface_member {
        return CallDispatch::Virtual;
    }
    if declaration.modifiers.is_expect
        || declaration.modifiers.is_abstract
        || declaration.modifiers.is_open
    {
        return CallDispatch::Virtual;
    }
    if declaration.modifiers.is_final || !declaration.modifiers.is_override {
        CallDispatch::Direct
    } else {
        CallDispatch::Virtual
    }
}

fn generic_type_substitutions(
    signature: &CallableSignature,
    arguments: &[crate::graph::CallArgumentFact],
) -> BTreeMap<String, String> {
    if signature.type_parameters.is_empty() {
        return BTreeMap::new();
    }
    let type_parameters = signature
        .type_parameters
        .iter()
        .map(|parameter| parameter.name.as_str())
        .collect::<HashSet<_>>();
    let mut substitutions = BTreeMap::<String, String>::new();
    let mut conflicting = HashSet::<String>::new();
    let mut bound = HashSet::<usize>::new();
    let mut positional = 0usize;
    for argument in arguments {
        let Some(actual) = argument.type_name.as_deref() else {
            let Some(index) = parameter_index(signature, argument, &bound, &mut positional) else {
                continue;
            };
            if !signature.parameters[index].is_vararg {
                bound.insert(index);
            }
            continue;
        };
        let Some(index) = parameter_index(signature, argument, &bound, &mut positional) else {
            continue;
        };
        let Some(expected) = signature.parameters[index].type_name.as_deref() else {
            continue;
        };
        let expected = expected.trim_end_matches('?');
        if !type_parameters.contains(expected) {
            continue;
        }
        let name = expected.to_owned();
        let actual = normalize_type(actual);
        if substitutions
            .get(&name)
            .is_some_and(|previous| normalize_type(previous) != actual)
        {
            conflicting.insert(name.clone());
            substitutions.remove(&name);
        } else if !conflicting.contains(&name) {
            substitutions.insert(name, actual);
        }
        if !signature.parameters[index].is_vararg {
            bound.insert(index);
        }
    }
    substitutions
}

fn substitute_type_name(type_name: &str, substitutions: &BTreeMap<String, String>) -> String {
    let mut result = String::with_capacity(type_name.len());
    let mut identifier = String::new();
    let flush_identifier = |identifier: &mut String, result: &mut String| {
        if identifier.is_empty() {
            return;
        }
        if let Some(replacement) = substitutions.get(identifier) {
            result.push_str(replacement);
        } else {
            result.push_str(identifier);
        }
        identifier.clear();
    };
    for character in type_name.chars() {
        if character.is_ascii_alphanumeric() || character == '_' {
            identifier.push(character);
        } else {
            flush_identifier(&mut identifier, &mut result);
            result.push(character);
        }
    }
    flush_identifier(&mut identifier, &mut result);
    result
}

fn arguments_match(
    signature: &CallableSignature,
    arguments: &[crate::graph::CallArgumentFact],
) -> bool {
    let substitutions = generic_type_substitutions(signature, arguments);
    for parameter in &signature.type_parameters {
        let Some(actual) = substitutions.get(&parameter.name) else {
            continue;
        };
        if parameter
            .bounds
            .iter()
            .any(|bound| builtin_bound_result(actual, bound) == Some(false))
        {
            return false;
        }
    }
    let mut bound = HashSet::<usize>::new();
    let mut positional = 0usize;
    for argument in arguments {
        let Some(parameter_index) = parameter_index(signature, argument, &bound, &mut positional)
        else {
            return false;
        };
        let Some(parameter) = signature.parameters.get(parameter_index) else {
            return false;
        };
        if !parameter.is_vararg && !bound.insert(parameter_index) {
            return false;
        }
        if let (Some(actual), Some(expected)) = (
            argument.type_name.as_deref(),
            parameter.type_name.as_deref(),
        ) {
            let expected = substitute_type_name(expected, &substitutions);
            if !is_type_compatible(&expected, actual) {
                return false;
            }
        }
        if argument.is_lambda
            && let Some(expected) = parameter.type_name.as_deref()
        {
            let expected = substitute_type_name(expected, &substitutions);
            if !lambda_parameters_match(&expected, &argument.lambda_parameter_types) {
                return false;
            }
        }
    }
    signature
        .parameters
        .iter()
        .enumerate()
        .all(|(index, parameter)| {
            bound.contains(&index) || parameter.has_default || parameter.is_vararg
        })
}

fn is_type_compatible(expected: &str, actual: &str) -> bool {
    if types_match(expected, actual) {
        return true;
    }
    let expected = normalize_type(expected);
    let actual = normalize_type(actual);
    match expected.as_str() {
        "Any" | "Object" => true,
        "Number" => {
            matches!(
                actual.as_str(),
                "Byte" | "Short" | "Int" | "Long" | "Float" | "Double"
            ) || !is_builtin_type(&actual)
        }
        "CharSequence" => actual == "String" || !is_builtin_type(&actual),
        _ if is_builtin_type(&expected) && is_builtin_type(&actual) => false,
        // User-defined source types may be related through inheritance or an
        // alias. If that relation is not proven here, keep the candidate
        // uncertain instead of discarding it and selecting a broader overload.
        _ => true,
    }
}

fn is_builtin_type(type_name: &str) -> bool {
    matches!(
        type_name,
        "Any"
            | "Object"
            | "Nothing"
            | "Unit"
            | "Boolean"
            | "Byte"
            | "Short"
            | "Int"
            | "Long"
            | "UByte"
            | "UShort"
            | "UInt"
            | "ULong"
            | "Float"
            | "Double"
            | "Char"
            | "String"
            | "Number"
            | "CharSequence"
    )
}

fn builtin_bound_result(actual: &str, bound: &str) -> Option<bool> {
    let actual = normalize_type(actual);
    let bound = normalize_type(bound);
    if types_match(&actual, &bound) || bound == "Any" || bound == "Object" {
        return Some(true);
    }
    match bound.as_str() {
        "Number" => {
            if matches!(
                actual.as_str(),
                "Byte" | "Short" | "Int" | "Long" | "Float" | "Double"
            ) {
                Some(true)
            } else if is_builtin_type(&actual) {
                Some(false)
            } else {
                None
            }
        }
        "CharSequence" => {
            if actual == "String" {
                Some(true)
            } else if is_builtin_type(&actual) {
                Some(false)
            } else {
                None
            }
        }
        "String" | "Int" | "Long" | "Double" | "Float" | "Short" | "Byte" | "UByte" | "UShort"
        | "UInt" | "ULong" | "Boolean" | "Char" => Some(false),
        _ => None,
    }
}

fn lambda_parameter_for_argument<'a>(
    signature: &'a CallableSignature,
    arguments: &[crate::graph::CallArgumentFact],
    wanted: &crate::graph::CallArgumentFact,
) -> Option<&'a crate::graph::CallableParameter> {
    let mut bound = HashSet::<usize>::new();
    let mut positional = 0usize;
    for argument in arguments {
        let parameter_index = parameter_index(signature, argument, &bound, &mut positional)?;
        let parameter = signature.parameters.get(parameter_index)?;
        if !parameter.is_vararg {
            bound.insert(parameter_index);
        }
        if argument.start_byte == wanted.start_byte && argument.end_byte == wanted.end_byte {
            return Some(parameter);
        }
    }
    None
}

fn parameter_index(
    signature: &CallableSignature,
    argument: &crate::graph::CallArgumentFact,
    bound: &HashSet<usize>,
    positional: &mut usize,
) -> Option<usize> {
    if let Some(name) = argument.name.as_deref() {
        return signature
            .parameters
            .iter()
            .position(|parameter| parameter.name.as_deref() == Some(name));
    }
    if argument.is_trailing_lambda
        && let Some((index, _)) =
            signature
                .parameters
                .iter()
                .enumerate()
                .rev()
                .find(|(index, parameter)| {
                    !bound.contains(index)
                        && parameter
                            .type_name
                            .as_deref()
                            .is_some_and(|ty| ty.contains("->"))
                })
    {
        return Some(index);
    }
    while *positional < signature.parameters.len() && bound.contains(positional) {
        *positional += 1;
    }
    if *positional < signature.parameters.len() {
        let index = *positional;
        if !signature.parameters[index].is_vararg {
            *positional += 1;
        }
        Some(index)
    } else if signature
        .parameters
        .last()
        .is_some_and(|parameter| parameter.is_vararg)
    {
        Some(signature.parameters.len() - 1)
    } else {
        None
    }
}

fn function_type_receiver(function_type: &str) -> Option<&str> {
    let arrow = function_type.find("->")?;
    let signature = &function_type[..arrow];
    let open = signature.find(".(")?;
    let receiver = signature[..open].trim_end_matches('.');
    (!receiver.is_empty()).then_some(receiver)
}

fn function_type_parameters(function_type: &str) -> Option<Vec<String>> {
    let arrow = function_type.find("->")?;
    let signature = function_type[..arrow].trim();
    if signature.contains(".(") {
        return None;
    }
    let open = signature.find('(')?;
    let close = signature.rfind(')')?;
    if close < open {
        return None;
    }
    let parameters = signature[open + 1..close].trim();
    if parameters.is_empty() {
        return Some(Vec::new());
    }
    Some(
        parameters
            .split(',')
            .map(|parameter| {
                parameter
                    .rsplit_once(':')
                    .map_or(parameter, |(_, ty)| ty)
                    .trim()
                    .to_owned()
            })
            .collect(),
    )
}

fn lambda_parameters_match(expected: &str, actual: &[Option<String>]) -> bool {
    if actual.iter().all(Option::is_none) {
        return true;
    }
    let Some((parameters, _return_type)) = expected.split_once("->") else {
        return true;
    };
    let parameter_text = parameters
        .rsplit_once('.')
        .map_or(parameters, |(_, function_parameters)| function_parameters);
    let parameter_text = parameter_text
        .trim()
        .trim_start_matches('(')
        .trim_end_matches(')');
    let expected_types = if parameter_text.is_empty() {
        Vec::new()
    } else {
        parameter_text
            .split(',')
            .map(|part| {
                part.rsplit_once(':')
                    .map_or(part, |(_, ty)| ty)
                    .trim()
                    .to_owned()
            })
            .collect::<Vec<_>>()
    };
    expected_types.len() == actual.len()
        && expected_types.iter().zip(actual).all(|(expected, actual)| {
            actual
                .as_deref()
                .is_none_or(|actual| types_match(expected, actual))
        })
}

fn same_logical_declaration(left: &Candidate, right: &Candidate) -> bool {
    if left.name != right.name || left.package != right.package || left.owner != right.owner {
        return false;
    }
    let left_decl = left.declaration.as_ref();
    let right_decl = right.declaration.as_ref();
    match (left_decl, right_decl) {
        (Some(left), Some(right)) => {
            let both_type = matches!(
                left.kind,
                crate::graph::DeclarationKind::Class | crate::graph::DeclarationKind::TypeAlias
            ) && matches!(
                right.kind,
                crate::graph::DeclarationKind::Class | crate::graph::DeclarationKind::TypeAlias
            );
            both_type
                || match (&left.signature, &right.signature) {
                    (Some(left), Some(right)) => {
                        left.receiver_type == right.receiver_type
                            && left.parameters.len() == right.parameters.len()
                            && left
                                .parameters
                                .iter()
                                .zip(&right.parameters)
                                .all(|(left, right)| left.type_name == right.type_name)
                    }
                    (None, None) => left.kind == right.kind,
                    _ => false,
                }
        }
        _ => false,
    }
}

fn normalize_type(value: &str) -> String {
    value
        .trim()
        .trim_end_matches('?')
        .split('<')
        .next()
        .unwrap_or(value)
        .replace("kotlin.", "")
}

fn candidate_type_segments(candidate: &Candidate) -> Vec<String> {
    candidate
        .package
        .iter()
        .chain(&candidate.owner)
        .chain(std::iter::once(&candidate.name))
        .cloned()
        .collect()
}

fn candidate_type_name(candidate: &Candidate) -> String {
    candidate_type_segments(candidate).join(".")
}

fn types_match(left: &str, right: &str) -> bool {
    let left = normalize_type(left);
    let right = normalize_type(right);
    left == right
}

fn is_extension_candidate(candidate: &Candidate) -> bool {
    candidate
        .declaration
        .as_ref()
        .and_then(|declaration| declaration.signature.as_ref())
        .is_some_and(|signature| signature.receiver_type.is_some())
}

fn same_callable_signature(
    left: Option<&crate::graph::DeclarationFacts>,
    right: Option<&crate::graph::DeclarationFacts>,
) -> bool {
    let (Some(left), Some(right)) = (left, right) else {
        return false;
    };
    if left.kind != right.kind {
        return false;
    }
    let (Some(left), Some(right)) = (left.signature.as_ref(), right.signature.as_ref()) else {
        return false;
    };
    left.receiver_type.as_deref().map(normalize_type)
        == right.receiver_type.as_deref().map(normalize_type)
        && left.parameters.len() == right.parameters.len()
        && left
            .parameters
            .iter()
            .zip(&right.parameters)
            .all(|(left, right)| {
                left.is_vararg == right.is_vararg
                    && left.type_name.as_deref().map(normalize_type)
                        == right.type_name.as_deref().map(normalize_type)
            })
}

fn is_type_declaration(candidate: &Candidate) -> bool {
    candidate.declaration.as_ref().is_some_and(|declaration| {
        matches!(
            declaration.kind,
            crate::graph::DeclarationKind::Class
                | crate::graph::DeclarationKind::Object
                | crate::graph::DeclarationKind::CompanionObject
                | crate::graph::DeclarationKind::TypeAlias
        )
    }) || matches!(
        candidate.kind,
        DefinitionKind::Struct | DefinitionKind::TypeAlias
    )
}

fn is_kotlin_builtin(name: &str, reference: &Reference) -> bool {
    let builtins = [
        "Any",
        "Nothing",
        "Unit",
        "Boolean",
        "Byte",
        "Short",
        "Int",
        "Long",
        "Float",
        "Double",
        "Char",
        "String",
        "Array",
        "List",
        "MutableList",
        "Set",
        "Map",
        "Pair",
        "Triple",
        "println",
        "print",
        "TODO",
        "require",
        "check",
        "error",
        "listOf",
        "mutableListOf",
    ];
    reference.path.len() == 1 && builtins.contains(&name)
}

fn source_context(path: &Path, snapshot: &Snapshot) -> SourceContext {
    let absolute = path.to_path_buf();
    let settings_root = path
        .ancestors()
        .find(|directory| {
            ["settings.gradle.kts", "settings.gradle"]
                .iter()
                .any(|name| {
                    snapshot
                        .read(&directory.join(name))
                        .ok()
                        .flatten()
                        .is_some()
                })
        })
        .map(Path::to_path_buf);
    let source_parts = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => part.to_str().map(str::to_owned),
            _ => None,
        })
        .collect::<Vec<_>>();
    let source_index = source_parts.iter().rposition(|part| part == "src");
    let source_set = source_index
        .filter(|index| *index + 2 < source_parts.len())
        .filter(|index| matches!(source_parts[*index + 2].as_str(), "kotlin" | "java"))
        .map(|index| source_parts[index + 1].clone());
    let inferred_module_root = source_index
        .and_then(|index| {
            path.ancestors()
                .nth(path.components().count().saturating_sub(index + 1))
        })
        .map(Path::to_path_buf);
    let build_root = path
        .ancestors()
        .find(|directory| {
            ["build.gradle.kts", "build.gradle"].iter().any(|name| {
                snapshot
                    .read(&directory.join(name))
                    .ok()
                    .flatten()
                    .is_some()
            })
        })
        .map(Path::to_path_buf);
    let settings_root = settings_root.or_else(|| build_root.clone());
    let project_root = settings_root
        .clone()
        .or_else(|| inferred_module_root.clone())
        .unwrap_or_else(|| absolute.parent().unwrap_or(&absolute).to_path_buf());
    let module_root = build_root
        .or(inferred_module_root)
        .unwrap_or_else(|| project_root.clone());
    let gradle_module = module_name(&project_root, &module_root, snapshot);
    let module = format!("{}{}", project_root.display(), gradle_module);
    SourceContext {
        path: absolute.clone(),
        project_root,
        module_root,
        module,
        gradle_module,
        source_set,
        isolated_source: path.extension().and_then(|extension| extension.to_str()) != Some("kt"),
    }
}

fn module_name(project_root: &Path, module_root: &Path, snapshot: &Snapshot) -> String {
    let settings = ["settings.gradle.kts", "settings.gradle"]
        .iter()
        .find_map(|name| read_text(snapshot, &project_root.join(name)))
        .unwrap_or_default();
    let configured = build::parse_project_directories(&settings);
    for module in build::parse_includes(&settings) {
        let configured_path = configured
            .get(&module)
            .cloned()
            .unwrap_or_else(|| PathBuf::from(module.trim_start_matches(':').replace(':', "/")));
        if normalize_path(&project_root.join(configured_path)) == normalize_path(module_root) {
            return module;
        }
    }
    if normalize_path(project_root) == normalize_path(module_root) {
        return ":".to_owned();
    }
    module_root
        .strip_prefix(project_root)
        .ok()
        .map(|relative| {
            relative
                .components()
                .filter_map(|part| part.as_os_str().to_str())
                .collect::<Vec<_>>()
        })
        .filter(|parts| !parts.is_empty())
        .map(|parts| format!(":{}", parts.join(":")))
        .unwrap_or_else(|| ":".to_owned())
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

fn read_text(snapshot: &Snapshot, path: &Path) -> Option<String> {
    snapshot
        .read(path)
        .ok()
        .flatten()
        .and_then(|bytes| String::from_utf8(bytes.to_vec()).ok())
}

fn project_dependencies(
    root: &Path,
    snapshot: &Snapshot,
) -> BTreeMap<String, BTreeSet<build::ProjectDependency>> {
    let settings = ["settings.gradle.kts", "settings.gradle"]
        .iter()
        .find_map(|name| read_text(snapshot, &root.join(name)))
        .unwrap_or_default();
    let modules = build::parse_includes(&settings);
    let project_directories = build::parse_project_directories(&settings);
    let mut dependencies = BTreeMap::<String, BTreeSet<build::ProjectDependency>>::new();
    let mut module_roots = vec![(":".to_owned(), root.to_path_buf())];
    module_roots.extend(modules.into_iter().map(|module| {
        let path = project_directories
            .get(&module)
            .map(|relative| root.join(relative))
            .unwrap_or_else(|| root.join(module.trim_start_matches(':').replace(':', "/")));
        (module, path)
    }));
    for (module, path) in module_roots {
        let mut deps = BTreeSet::new();
        for name in ["build.gradle.kts", "build.gradle"] {
            if let Some(source) = read_text(snapshot, &path.join(name)) {
                deps.extend(build::parse_project_dependency_details(&source));
            }
        }
        dependencies.insert(module, deps);
    }
    dependencies
}

fn source_set_reaches(
    caller: &str,
    target: &str,
    edges: &BTreeMap<String, BTreeSet<String>>,
) -> bool {
    let mut pending = vec![caller.to_owned()];
    let mut seen = HashSet::new();
    while let Some(current) = pending.pop() {
        if current == target {
            return true;
        }
        if !seen.insert(current.clone()) {
            continue;
        }
        pending.extend(edges.get(&current).into_iter().flatten().cloned());
    }
    false
}

fn conventional_source_edge(caller: &str, target: &str) -> bool {
    if caller == "commonTest" && target == "commonMain" {
        return true;
    }
    if caller.ends_with("Test") {
        let main = caller.trim_end_matches("Test").to_owned() + "Main";
        return target == main || target == "commonTest" || target == "commonMain";
    }
    caller.ends_with("Main") && target == "commonMain"
}

fn transitive_closure(
    root: &str,
    graph: &BTreeMap<String, BTreeSet<build::ProjectDependency>>,
    source_set: Option<&str>,
) -> BTreeSet<String> {
    let mut result = BTreeSet::new();
    let mut pending = graph
        .get(root)
        .into_iter()
        .flatten()
        .filter(|dependency| {
            dependency_visible_in_source_set(&dependency.scope, source_set)
                && dependency_compile_visible(&dependency.exposure)
        })
        .map(|dependency| dependency.path.clone())
        .collect::<Vec<_>>();
    while let Some(item) = pending.pop() {
        if result.insert(item.clone()) {
            pending.extend(
                graph
                    .get(&item)
                    .into_iter()
                    .flatten()
                    .filter(|dependency| {
                        dependency_visible_in_source_set(&dependency.scope, source_set)
                            && dependency.exposure == build::DependencyExposure::Api
                    })
                    .map(|dependency| dependency.path.clone()),
            );
        }
    }
    result
}

fn dependency_compile_visible(exposure: &build::DependencyExposure) -> bool {
    matches!(
        exposure,
        build::DependencyExposure::Api
            | build::DependencyExposure::Implementation
            | build::DependencyExposure::CompileOnly
    )
}

fn dependency_visible_in_source_set(
    scope: &build::DependencyScope,
    source_set: Option<&str>,
) -> bool {
    match scope {
        build::DependencyScope::Production => true,
        build::DependencyScope::Test(test_source_set) => {
            source_set.is_some_and(|set| set == test_source_set)
        }
        build::DependencyScope::Variant(variant) => source_set.is_some_and(|set| set == variant),
        build::DependencyScope::Other => false,
    }
}
