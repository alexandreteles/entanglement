//! Capture local binding evidence without assuming general PHP type inference.

use tree_sitter::Node;

use crate::languages::CapturedTree;
use crate::model::{BindingEvidence, LocalBinding};

use super::syntax::*;

mod mutations;
use mutations::{Mutation, mutations, stable};

pub(super) fn capture(nodes: &[Node<'_>], source: &[u8], captured: &mut CapturedTree) {
    let mutations = mutations(nodes, source);
    captured.facts.locals.extend(
        nodes
            .iter()
            .copied()
            .filter(|node| {
                matches!(
                    node.kind(),
                    "simple_parameter"
                        | "variadic_parameter"
                        | "property_promotion_parameter"
                        | "assignment_expression"
                        | "reference_assignment_expression"
                )
            })
            .filter_map(|node| binding(node, &mutations, source)),
    );
    for closure in nodes
        .iter()
        .copied()
        .filter(|node| matches!(node.kind(), "anonymous_function" | "arrow_function"))
    {
        capture_bindings(closure, nodes, &mutations, source, captured);
    }
}

fn binding(node: Node<'_>, mutations: &[Mutation<'_>], source: &[u8]) -> Option<LocalBinding> {
    let assignment = matches!(
        node.kind(),
        "assignment_expression" | "reference_assignment_expression"
    );
    let name = node.child_by_field_name(if assignment { "left" } else { "name" })?;
    if assignment && name.kind() != "variable_name" {
        return None;
    }
    let name = if name.kind() == "by_ref" {
        name.named_child(0).unwrap_or(name)
    } else {
        name
    };
    let scope = scope(node);
    let raw_name = text(name, source);
    let (type_name, alias_target, callable_reference) = if assignment {
        initializer(unwrap(node.child_by_field_name("right")?), source)
    } else {
        (
            node.child_by_field_name("type")
                .map(|kind| text(kind, source).to_owned()),
            None,
            false,
        )
    };
    let conditional = assignment
        && ancestors(node)
            .take_while(|node| !is_callable(*node))
            .any(|node| {
                is_branch(node)
                    || matches!(node.kind(), "conditional_expression" | "match_expression")
            });
    Some(LocalBinding {
        evidence: BindingEvidence::Semantic,
        context_id: 0,
        name: raw_name.to_owned(),
        start_byte: name.start_byte(),
        end_byte: name.end_byte(),
        scope_start: if assignment {
            node.end_byte()
        } else {
            scope.start_byte()
        },
        scope_end: scope.end_byte(),
        type_name,
        alias_target,
        callable_reference,
        immutable: !conditional
            && node.kind() != "reference_assignment_expression"
            && (assignment || node.child_by_field_name("reference_modifier").is_none())
            && stable(raw_name, scope, mutations, assignment.then(|| node.id())),
    })
}

fn initializer(node: Node<'_>, source: &[u8]) -> (Option<String>, Option<String>, bool) {
    match node.kind() {
        "anonymous_function" | "arrow_function" => {
            (None, Some(format!("@closure:{}", node.start_byte())), true)
        }
        "variable_name" => (None, Some(text(node, source).to_owned()), false),
        "object_creation_expression" => {
            let kind = children(node)
                .find(|child| is_name(*child))
                .map(|kind| text(kind, source).to_owned());
            let alias = kind.as_ref().map(|_| "@new".to_owned());
            (kind, alias, false)
        }
        "function_call_expression" if first_class_callable(node) => (
            None,
            node.child_by_field_name("function")
                .filter(|function| is_name(*function))
                .map(|function| text(function, source).to_owned()),
            true,
        ),
        "scoped_call_expression" | "member_call_expression" | "nullsafe_member_call_expression"
            if first_class_callable(node) =>
        {
            let receiver = node
                .child_by_field_name("scope")
                .or_else(|| node.child_by_field_name("object"));
            let method = node
                .child_by_field_name("name")
                .filter(|name| is_name(*name));
            let alias = receiver.zip(method).map(|(receiver, method)| {
                format!(
                    "@method:{}::{}",
                    super::references::receiver_text(receiver, source),
                    text(method, source)
                )
            });
            (None, alias, true)
        }
        _ => (
            None,
            literal(node, source).map(|name| match name.rsplit_once("::") {
                Some((receiver, method)) => {
                    format!("@method:\\{}::{method}", receiver.trim_start_matches('\\'))
                }
                None => format!("\\{}", name.trim_start_matches('\\')),
            }),
            false,
        ),
    }
}

fn capture_bindings(
    closure: Node<'_>,
    all_nodes: &[Node<'_>],
    mutations: &[Mutation<'_>],
    source: &[u8],
    captured: &mut CapturedTree,
) {
    let variables = if closure.kind() == "arrow_function" {
        nodes(closure)
            .into_iter()
            .filter(|node| node.kind() == "variable_name" && scope(*node).id() == closure.id())
            .collect::<Vec<_>>()
    } else {
        children(closure)
            .filter(|child| child.kind() == "anonymous_function_use_clause")
            .flat_map(nodes)
            .filter(|node| node.kind() == "variable_name")
            .collect()
    };
    let outer_owner = scope(closure);
    for variable in variables {
        let name = text(variable, source);
        let local = captured.facts.locals.iter().any(|binding| {
            binding.name == name
                && closure.start_byte() <= binding.start_byte
                && binding.start_byte < closure.end_byte()
        });
        if local {
            continue;
        }
        let binding = captured
            .facts
            .locals
            .iter()
            .filter(|binding| {
                binding.name == name
                    && binding.scope_start <= closure.start_byte()
                    && closure.end_byte() <= binding.scope_end
                    && owner_at(binding.start_byte, all_nodes) == Some(outer_owner.id())
            })
            .max_by_key(|binding| binding.scope_start)
            .cloned();
        if let Some(mut binding) = binding {
            binding.start_byte = if closure.kind() == "arrow_function" {
                closure.start_byte()
            } else {
                variable.start_byte()
            };
            binding.end_byte = variable.end_byte();
            binding.scope_start = closure.start_byte();
            binding.scope_end = closure.end_byte();
            binding.immutable &= stable(name, closure, mutations, None);
            captured.facts.locals.push(binding);
        }
    }
}

fn owner_at(position: usize, nodes: &[Node<'_>]) -> Option<usize> {
    nodes
        .iter()
        .filter(|node| {
            is_callable(**node) && node.start_byte() <= position && position < node.end_byte()
        })
        .min_by_key(|node| node.end_byte() - node.start_byte())
        .map(|node| node.id())
        .or_else(|| nodes.first().map(|node| node.id()))
}
