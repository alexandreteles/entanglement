//! Kotlin-specific syntax facts for the shared resolver and call graph.

use std::collections::HashSet;
use std::ops::Range;

use tree_sitter::Node;

use super::{all_nodes, named_children};

use crate::graph::{
    BindingType, CallArgumentFact, CallFacts, CallableParameter, CallableSignature,
    CallableTypeParameter, DeclarationExecutionFacts, DeclarationFacts, DeclarationKind,
    DeclarationModifiers, ReceiverScope, SemanticContext, SemanticVisibility,
};
use crate::model::{
    Definition, DefinitionKind, Import, LocalBinding, ModulePath, Reference, ReferenceAccess,
    ReferenceKind, SourceRange,
};

mod bindings;
mod calls;
mod declarations;
mod execution;
mod recovered;
mod references;
mod signatures;

use bindings::*;
use calls::*;
use declarations::*;
use execution::*;
use recovered::*;
use references::*;
use signatures::*;

pub(super) use crate::languages::CapturedSemantics as Capture;

struct DefinitionContext<'tree, 'source> {
    root: Node<'tree>,
    nodes: &'source [Node<'tree>],
    source: &'source [u8],
    package: &'source [String],
    contexts: &'source [super::syntax::ContextParameters],
    recovered_types: &'source [super::syntax::RecoveredType],
    scope_index: &'source super::scopes::ScopeIndex<'tree>,
}

struct DefinitionSinks<'capture> {
    result: &'capture mut Capture,
    excluded: &'capture mut Vec<Range<usize>>,
    declared_names: &'capture mut HashSet<(usize, usize)>,
}

pub(super) fn capture(scope_index: &super::scopes::ScopeIndex<'_>) -> Capture {
    let root = scope_index.root();
    let source = scope_index.source();
    let contexts = scope_index.contexts();
    let nodes = scope_index.nodes();
    let recovered_types = scope_index.recovered_types();
    let package = package_name(nodes, source);
    let mut result = Capture::default();
    result.semantic.context = Some(SemanticContext {
        package: package.clone(),
    });

    let imports = collect_imports(root, nodes, source, &package);
    let mut excluded = imports
        .iter()
        .map(|import| import.start_byte..import.end_byte)
        .collect::<Vec<_>>();
    let mut declared_names = HashSet::new();
    let definition_context = DefinitionContext {
        root,
        nodes,
        source,
        package: &package,
        contexts,
        recovered_types,
        scope_index,
    };

    for node in nodes
        .iter()
        .copied()
        .filter(|node| is_definition_node(*node))
    {
        collect_definition(
            node,
            &definition_context,
            &mut DefinitionSinks {
                result: &mut result,
                excluded: &mut excluded,
                declared_names: &mut declared_names,
            },
        );
    }

    collect_recovered_type_definitions(&definition_context, &mut result);

    result.imports = imports;
    collect_locals(source, nodes, &declared_names, &mut result);
    collect_context_facts(nodes, contexts, &mut result);
    result.semantic.calls = collect_calls(nodes, source, &result.semantic.bindings);
    result.references = collect_references(scope_index, nodes, &excluded);
    let conventions = super::conventions::capture(
        &result.semantic.bindings,
        &result.semantic.receivers,
        scope_index,
    );
    let convention_ranges = conventions
        .references
        .iter()
        .map(|reference| (reference.start_byte, reference.end_byte))
        .collect::<HashSet<_>>();
    result.references.retain(|reference| {
        !convention_ranges.contains(&(reference.start_byte, reference.end_byte))
    });
    result.references.extend(conventions.references);
    merge_convention_calls(&mut result.semantic.calls, conventions.calls);
    result
}

fn merge_convention_calls(calls: &mut Vec<CallFacts>, conventions: Vec<CallFacts>) {
    for convention in conventions {
        let Some(existing) = calls.iter_mut().find(|call| {
            call.start_byte == convention.start_byte && call.end_byte == convention.end_byte
        }) else {
            calls.push(convention);
            continue;
        };
        existing.convention = convention.convention.or(existing.convention);
        existing.target = convention.target.or(existing.target);
        existing.receiver_type = convention.receiver_type.or(existing.receiver_type.take());
        for argument in convention.arguments {
            if let Some(existing_argument) = existing.arguments.iter_mut().find(|existing| {
                existing.start_byte == argument.start_byte && existing.end_byte == argument.end_byte
            }) {
                existing_argument.type_name =
                    argument.type_name.or(existing_argument.type_name.take());
                existing_argument.is_lambda |= argument.is_lambda;
                if existing_argument.lambda_parameter_types.is_empty() {
                    existing_argument.lambda_parameter_types = argument.lambda_parameter_types;
                }
                existing_argument.lambda_receiver_type = argument
                    .lambda_receiver_type
                    .or(existing_argument.lambda_receiver_type.take());
            } else {
                existing.arguments.push(argument);
            }
        }
    }
    calls.sort_by_key(|call| (call.start_byte, call.end_byte));
}

fn text(node: Node<'_>, source: &[u8]) -> String {
    source
        .get(node.byte_range())
        .map(String::from_utf8_lossy)
        .map(|value| value.trim().to_owned())
        .unwrap_or_default()
}

fn ancestors(node: Node<'_>) -> impl Iterator<Item = Node<'_>> {
    super::ancestors(node).skip(1)
}

fn range(node: Node<'_>) -> Range<usize> {
    node.start_byte()..node.end_byte()
}

#[cfg(test)]
#[path = "../../../tests/unit/kotlin_facts.rs"]
mod tests;
