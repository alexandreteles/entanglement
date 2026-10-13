//! Kotlin-specific syntax facts for the shared resolver and call graph.

use std::collections::HashSet;
use std::ops::Range;

use tree_sitter::Node;

use crate::graph::{
    BindingType, CallArgumentFact, CallFacts, CallableParameter, CallableSignature,
    CallableTypeParameter, DeclarationExecutionFacts, DeclarationFacts, DeclarationKind,
    DeclarationModifiers, ReceiverScope, SemanticContext, SemanticFacts, SemanticVisibility,
};
use crate::model::{
    Definition, DefinitionKind, Import, LocalBinding, ModulePath, Reference, ReferenceAccess,
    ReferenceKind, SourceRange,
};

mod bindings;
mod calls;
mod declarations;
mod execution;
mod modifiers;
mod ownership;
mod recovered;
mod references;
mod signatures;

use bindings::*;
use calls::*;
use declarations::*;
use execution::*;
use modifiers::*;
use ownership::*;
use recovered::*;
use references::*;
use signatures::*;

#[derive(Default)]
pub(super) struct Capture {
    pub definitions: Vec<Definition>,
    pub imports: Vec<Import>,
    pub references: Vec<Reference>,
    pub locals: Vec<LocalBinding>,
    pub semantic: SemanticFacts,
}

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
    declarations: &'capture mut Vec<DeclarationFacts>,
    excluded: &'capture mut Vec<Range<usize>>,
    declared_names: &'capture mut HashSet<(usize, usize)>,
}

pub(super) fn capture(scope_index: &super::scopes::ScopeIndex<'_>) -> Capture {
    let root = scope_index.root();
    let source = scope_index.source();
    let contexts = scope_index.contexts();
    let nodes = all_nodes(root);
    let recovered_types = scope_index.recovered_types();
    let package = package_name(&nodes, source);
    let mut result = Capture::default();
    result.semantic.context = Some(SemanticContext {
        package: package.clone(),
    });

    let imports = collect_imports(root, &nodes, source, &package);
    let mut excluded = imports
        .iter()
        .map(|import| import.start_byte..import.end_byte)
        .collect::<Vec<_>>();
    let mut declarations = Vec::new();
    let mut declared_names = HashSet::new();
    let mut dsl_markers = HashSet::new();
    let definition_context = DefinitionContext {
        root,
        nodes: &nodes,
        source,
        package: &package,
        contexts,
        recovered_types,
        scope_index,
    };

    for node in nodes.iter().copied() {
        if node.kind() == "class_declaration"
            && has_annotation(node, source, "DslMarker")
            && let Some(name) = declaration_name(node, source)
        {
            dsl_markers.insert(name);
        }
    }

    for node in nodes.iter().copied() {
        if is_definition_node(node) {
            collect_definition(
                node,
                &definition_context,
                &mut DefinitionSinks {
                    result: &mut result,
                    declarations: &mut declarations,
                    excluded: &mut excluded,
                    declared_names: &mut declared_names,
                },
            );
        }
    }

    collect_recovered_type_definitions(&definition_context, &mut result, &mut declarations);

    result.imports = imports;
    result.semantic.declarations = declarations;
    collect_locals(root, source, &nodes, &declared_names, &mut result);
    collect_receivers(root, source, &nodes, &dsl_markers, &mut result.semantic);
    collect_context_facts(root, source, &nodes, contexts, &mut result);
    result.semantic.calls = collect_calls(&nodes, source);
    enrich_call_argument_types(
        &mut result.semantic.calls,
        &result.semantic.bindings,
        &nodes,
        source,
    );
    result.references = collect_references(scope_index, &nodes, &excluded);
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
        existing.receiver_type = convention
            .receiver_type
            .or_else(|| existing.receiver_type.clone());
        for argument in convention.arguments {
            if let Some(existing_argument) = existing.arguments.iter_mut().find(|existing| {
                existing.start_byte == argument.start_byte && existing.end_byte == argument.end_byte
            }) {
                existing_argument.type_name = argument
                    .type_name
                    .or_else(|| existing_argument.type_name.clone());
                existing_argument.is_lambda |= argument.is_lambda;
                if existing_argument.lambda_parameter_types.is_empty() {
                    existing_argument.lambda_parameter_types = argument.lambda_parameter_types;
                }
                existing_argument.lambda_receiver_type = argument
                    .lambda_receiver_type
                    .or_else(|| existing_argument.lambda_receiver_type.clone());
            } else {
                existing.arguments.push(argument);
            }
        }
    }
    calls.sort_by_key(|call| (call.start_byte, call.end_byte));
}

fn named_children<'tree>(node: Node<'tree>) -> Vec<Node<'tree>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

fn all_nodes<'tree>(root: Node<'tree>) -> Vec<Node<'tree>> {
    let mut result = Vec::new();
    let mut pending = vec![root];
    while let Some(node) = pending.pop() {
        result.push(node);
        let mut cursor = node.walk();
        pending.extend(node.children(&mut cursor));
    }
    result.sort_by_key(|node| (node.start_byte(), node.end_byte()));
    result
}

fn text(node: Node<'_>, source: &[u8]) -> String {
    source
        .get(node.byte_range())
        .map(String::from_utf8_lossy)
        .map(|value| value.trim().to_owned())
        .unwrap_or_default()
}

fn ancestors<'tree>(mut node: Node<'tree>) -> impl Iterator<Item = Node<'tree>> {
    std::iter::from_fn(move || {
        let current = node;
        node = current.parent()?;
        Some(node)
    })
}

fn range(node: Node<'_>) -> Range<usize> {
    node.start_byte()..node.end_byte()
}

struct BindingEvidence<'tree> {
    name: String,
    name_node: Node<'tree>,
    scope: Range<usize>,
    type_name: Option<String>,
    alias_target: Option<String>,
    callable_reference: bool,
    immutable: bool,
}

#[cfg(test)]
mod tests {
    use super::{
        all_nodes, collect_calls, collect_references, integer_literal_type, real_literal_type,
    };
    use crate::model::{ReferenceAccess, ReferenceKind};

    #[test]
    fn numeric_literal_types_respect_suffixes_and_ranges() {
        assert_eq!(integer_literal_type("42"), Some("Int"));
        assert_eq!(integer_literal_type("2_147_483_648"), Some("Long"));
        assert_eq!(integer_literal_type("42L"), Some("Long"));
        assert_eq!(integer_literal_type("42u"), Some("UInt"));
        assert_eq!(integer_literal_type("4_294_967_296u"), Some("ULong"));
        assert_eq!(integer_literal_type("0xFFFF_FFFFuL"), Some("ULong"));
        assert_eq!(integer_literal_type("9_223_372_036_854_775_808L"), None);
        assert_eq!(integer_literal_type("18_446_744_073_709_551_616u"), None);
        assert_eq!(real_literal_type("1.25"), "Double");
        assert_eq!(real_literal_type("1.25F"), "Float");
    }

    #[test]
    fn call_argument_types_come_from_numeric_literal_cst_wrappers() {
        let source = "fun sample() { consume(1L); consume(1u); consume(0xFF); consume(0b1); consume(1f); consume(1.0F); consume(1.0) }";
        let mut parser = tree_sitter::Parser::new();
        let language: tree_sitter::Language = tree_sitter_kotlin::LANGUAGE.into();
        parser.set_language(&language).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let calls = collect_calls(&all_nodes(tree.root_node()), source.as_bytes());
        let argument_types = calls
            .into_iter()
            .flat_map(|call| {
                call.arguments
                    .into_iter()
                    .map(|argument| argument.type_name)
            })
            .collect::<Vec<_>>();

        assert_eq!(
            argument_types,
            [
                Some("Long".to_owned()),
                Some("UInt".to_owned()),
                Some("Int".to_owned()),
                Some("Int".to_owned()),
                Some("Float".to_owned()),
                Some("Float".to_owned()),
                Some("Double".to_owned()),
            ]
        );
    }

    #[test]
    fn qualified_member_reads_writes_and_postfix_updates_are_referenced() {
        let source = "class Box(var value: Int) { fun update(input: Int, other: Box) { val before = other.value; other.value = input; other.value++ } }";
        let mut parser = tree_sitter::Parser::new();
        let language: tree_sitter::Language = tree_sitter_kotlin::LANGUAGE.into();
        parser.set_language(&language).unwrap();
        let tree = parser.parse(source, None).unwrap();
        let root = tree.root_node();
        let nodes = all_nodes(root);
        let scope_index = super::super::scopes::ScopeIndex::new(root, source.as_bytes(), &[]);
        let references = collect_references(&scope_index, &nodes, &[]);
        let access_for = |member_start: usize| {
            references
                .iter()
                .find(|reference| {
                    reference.start_byte == member_start
                        && reference.kind == ReferenceKind::Qualified
                        && reference.path == ["other", "value"]
                })
                .and_then(|reference| reference.access)
        };
        let member_starts = source
            .match_indices(".value")
            .map(|(dot, _)| dot + 1)
            .collect::<Vec<_>>();

        assert_eq!(member_starts.len(), 3);
        assert_eq!(access_for(member_starts[0]), Some(ReferenceAccess::Read));
        assert_eq!(access_for(member_starts[1]), Some(ReferenceAccess::Write));
        assert_eq!(
            access_for(member_starts[2]),
            Some(ReferenceAccess::ReadWrite)
        );
    }
}
