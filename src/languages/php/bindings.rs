//! Capture local binding evidence without assuming general PHP type inference.

use tree_sitter::Node;

use crate::graph::BindingType;
use crate::languages::CapturedTree;

use super::syntax::*;

pub(super) fn capture(nodes: &[Node<'_>], source: &[u8], captured: &mut CapturedTree) {
    for node in nodes.iter().copied() {
        match node.kind() {
            "simple_parameter" | "variadic_parameter" | "property_promotion_parameter" => {
                parameter(node, nodes, source, captured)
            }
            "assignment_expression" | "reference_assignment_expression" => {
                assignment(node, nodes, source, captured)
            }
            _ => {}
        }
    }
    for closure in nodes
        .iter()
        .copied()
        .filter(|node| matches!(node.kind(), "anonymous_function" | "arrow_function"))
    {
        capture_bindings(closure, nodes, source, captured);
    }
}

fn parameter(node: Node<'_>, nodes: &[Node<'_>], source: &[u8], captured: &mut CapturedTree) {
    let Some(name) = node.child_by_field_name("name") else {
        return;
    };
    let name = if name.kind() == "by_ref" {
        name.named_child(0).unwrap_or(name)
    } else {
        name
    };
    let scope = scope(node);
    let raw_name = text(name, source).to_owned();
    let immutable = node.child_by_field_name("reference_modifier").is_none()
        && stable(&raw_name, scope, nodes, source, None);
    captured.semantic.bindings.push(BindingType {
        name: raw_name,
        name_start_byte: name.start_byte(),
        name_end_byte: name.end_byte(),
        scope_start: scope.start_byte(),
        scope_end: scope.end_byte(),
        type_name: node
            .child_by_field_name("type")
            .map(|kind| text(kind, source).to_owned()),
        alias_target: None,
        callable_reference: false,
        immutable,
    });
}

fn assignment(node: Node<'_>, nodes: &[Node<'_>], source: &[u8], captured: &mut CapturedTree) {
    let Some(left) = node
        .child_by_field_name("left")
        .filter(|left| left.kind() == "variable_name")
    else {
        return;
    };
    let Some(right) = node.child_by_field_name("right").map(unwrap) else {
        return;
    };
    let scope = scope(node);
    let name = text(left, source).to_owned();
    let immutable = node.kind() != "reference_assignment_expression"
        && !ancestors(node)
            .take_while(|node| !is_callable(*node))
            .any(|node| {
                matches!(
                    node.kind(),
                    "if_statement"
                        | "else_if_clause"
                        | "else_clause"
                        | "conditional_expression"
                        | "switch_statement"
                        | "match_expression"
                        | "for_statement"
                        | "foreach_statement"
                        | "while_statement"
                        | "do_statement"
                        | "try_statement"
                        | "catch_clause"
                )
            })
        && stable(&name, scope, nodes, source, Some(node.id()));
    let (type_name, alias_target, callable_reference) = initializer(right, source);
    captured.semantic.bindings.push(BindingType {
        name,
        name_start_byte: left.start_byte(),
        name_end_byte: left.end_byte(),
        scope_start: node.end_byte(),
        scope_end: scope.end_byte(),
        type_name,
        alias_target,
        callable_reference,
        immutable,
    });
}

fn initializer(node: Node<'_>, source: &[u8]) -> (Option<String>, Option<String>, bool) {
    match node.kind() {
        "anonymous_function" | "arrow_function" => {
            (None, Some(format!("@closure:{}", node.start_byte())), true)
        }
        "variable_name" => (None, Some(text(node, source).to_owned()), false),
        "object_creation_expression" => {
            let kind = children(node)
                .into_iter()
                .find(|child| is_name(*child))
                .map(|kind| text(kind, source).to_owned());
            (kind.clone(), kind.map(|kind| format!("@new:{kind}")), false)
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
            .into_iter()
            .filter(|child| child.kind() == "anonymous_function_use_clause")
            .flat_map(nodes)
            .filter(|node| node.kind() == "variable_name")
            .collect()
    };
    for variable in variables {
        let name = text(variable, source);
        let local = captured.semantic.bindings.iter().any(|binding| {
            binding.name == name
                && closure.start_byte() <= binding.name_start_byte
                && binding.name_start_byte < closure.end_byte()
        });
        if local {
            continue;
        }
        let outer_owner = scope(closure);
        let binding = captured
            .semantic
            .bindings
            .iter()
            .filter(|binding| {
                binding.name == name
                    && binding.scope_start <= closure.start_byte()
                    && closure.end_byte() <= binding.scope_end
                    && owner_at(binding.name_start_byte, all_nodes) == Some(outer_owner.id())
            })
            .max_by_key(|binding| binding.scope_start)
            .cloned();
        if let Some(mut binding) = binding {
            binding.name_start_byte = if closure.kind() == "arrow_function" {
                closure.start_byte()
            } else {
                variable.start_byte()
            };
            binding.name_end_byte = variable.end_byte();
            binding.scope_start = closure.start_byte();
            binding.scope_end = closure.end_byte();
            binding.immutable &= stable(name, closure, all_nodes, source, None);
            captured.semantic.bindings.push(binding);
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

fn scope(node: Node<'_>) -> Node<'_> {
    ancestors(node)
        .find(|node| is_callable(*node))
        .unwrap_or_else(|| ancestors(node).last().unwrap_or(node))
}

fn stable(
    name: &str,
    scope: Node<'_>,
    nodes: &[Node<'_>],
    source: &[u8],
    own_write: Option<usize>,
) -> bool {
    !nodes
        .iter()
        .copied()
        .filter(|node| {
            scope.start_byte() <= node.start_byte() && node.end_byte() <= scope.end_byte()
        })
        .any(|node| invalidates(node, name, scope, source, own_write))
}

fn invalidates(
    node: Node<'_>,
    name: &str,
    owner: Node<'_>,
    source: &[u8],
    own_write: Option<usize>,
) -> bool {
    if matches!(node.kind(), "by_ref" | "reference_assignment_expression")
        && nodes(node).iter().any(|node| text(*node, source) == name)
    {
        return true;
    }
    if scope(node).id() != owner.id() {
        return false;
    }
    match node.kind() {
        "dynamic_variable_name"
        | "eval_intrinsic"
        | "include_expression"
        | "include_once_expression"
        | "require_expression"
        | "require_once_expression" => true,
        "global_declaration" => nodes(node)
            .iter()
            .any(|node| node.kind() == "variable_name" && text(*node, source) == name),
        "function_call_expression" => {
            node.child_by_field_name("function")
                .is_some_and(|function| {
                    text(function, source)
                        .trim_start_matches('\\')
                        .eq_ignore_ascii_case("extract")
                        || text(function, source).eq_ignore_ascii_case("eval")
                })
        }
        "assignment_expression"
        | "augmented_assignment_expression"
        | "reference_assignment_expression" => {
            own_write != Some(node.id())
                && node
                    .child_by_field_name("left")
                    .is_some_and(|left| writes(left, name, source))
        }
        "update_expression" => node
            .child_by_field_name("argument")
            .is_some_and(|argument| writes(argument, name, source)),
        "catch_clause" => node
            .child_by_field_name("name")
            .is_some_and(|target| writes(target, name, source)),
        "foreach_statement" => children(node)
            .into_iter()
            .skip(1)
            .filter(|child| {
                node.child_by_field_name("body")
                    .is_none_or(|body| body.id() != child.id())
            })
            .any(|target| {
                nodes(target)
                    .iter()
                    .any(|node| node.kind() == "variable_name" && text(*node, source) == name)
            }),
        "static_variable_declaration" => children(node)
            .into_iter()
            .find(|child| child.kind() == "variable_name")
            .is_some_and(|target| writes(target, name, source)),
        "argument" | "unset_statement" => nodes(node)
            .iter()
            .any(|node| node.kind() == "variable_name" && text(*node, source) == name),
        _ => false,
    }
}

fn writes(node: Node<'_>, name: &str, source: &[u8]) -> bool {
    match node.kind() {
        "variable_name" => text(node, source) == name,
        "list_literal" | "by_ref" => children(node)
            .into_iter()
            .any(|target| writes(target, name, source)),
        "subscript_expression" => node
            .named_child(0)
            .is_some_and(|target| writes(target, name, source)),
        _ => false,
    }
}
