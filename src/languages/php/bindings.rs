//! Capture local binding evidence without assuming general PHP type inference.

use tree_sitter::Node;

use crate::graph::BindingType;
use crate::languages::CapturedTree;

use super::syntax::*;

pub(super) fn capture(nodes: &[Node<'_>], source: &[u8], captured: &mut CapturedTree) {
    let mutations = mutations(nodes, source);
    captured.semantic.bindings.extend(
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

fn binding(node: Node<'_>, mutations: &[Mutation<'_>], source: &[u8]) -> Option<BindingType> {
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
                    || matches!(
                        node.kind(),
                        "conditional_expression" | "match_expression" | "try_statement"
                    )
            });
    Some(BindingType {
        name: raw_name.to_owned(),
        name_start_byte: name.start_byte(),
        name_end_byte: name.end_byte(),
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
            binding.immutable &= stable(name, closure, mutations, None);
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

struct Mutation<'a> {
    owners: Vec<usize>,
    names: Option<Vec<&'a str>>,
    write: Option<usize>,
}

fn stable(
    name: &str,
    scope: Node<'_>,
    mutations: &[Mutation<'_>],
    own_write: Option<usize>,
) -> bool {
    mutations.iter().all(|mutation| {
        !mutation.owners.contains(&scope.id())
            || mutation
                .names
                .as_ref()
                .is_some_and(|names| !names.contains(&name))
            || mutation.write.is_some() && mutation.write == own_write
    })
}

fn mutations<'a>(all_nodes: &[Node<'_>], source: &'a [u8]) -> Vec<Mutation<'a>> {
    let variables = |node| {
        nodes(node)
            .into_iter()
            .filter(|node| node.kind() == "variable_name")
            .map(|node| text(node, source))
            .collect::<Vec<_>>()
    };
    all_nodes
        .iter()
        .copied()
        .filter_map(|node| {
            let (names, write) = match node.kind() {
                "by_ref"
                | "reference_assignment_expression"
                | "global_declaration"
                | "argument"
                | "unset_statement" => (Some(variables(node)), None),
                "dynamic_variable_name"
                | "eval_intrinsic"
                | "include_expression"
                | "include_once_expression"
                | "require_expression"
                | "require_once_expression" => (None, None),
                "function_call_expression"
                    if node
                        .child_by_field_name("function")
                        .is_some_and(|function| {
                            text(function, source)
                                .trim_start_matches('\\')
                                .eq_ignore_ascii_case("extract")
                                || text(function, source).eq_ignore_ascii_case("eval")
                        }) =>
                {
                    (None, None)
                }
                "assignment_expression"
                | "augmented_assignment_expression"
                | "update_expression"
                | "catch_clause"
                | "static_variable_declaration" => {
                    let field = match node.kind() {
                        "assignment_expression" | "augmented_assignment_expression" => "left",
                        "update_expression" => "argument",
                        _ => "name",
                    };
                    (
                        Some(
                            node.child_by_field_name(field)
                                .into_iter()
                                .flat_map(write_targets)
                                .map(|node| text(node, source))
                                .collect(),
                        ),
                        (field == "left").then(|| node.id()),
                    )
                }
                "foreach_statement" => (
                    Some(
                        children(node)
                            .skip(1)
                            .filter(|child| {
                                node.child_by_field_name("body")
                                    .is_none_or(|body| body.id() != child.id())
                            })
                            .flat_map(variables)
                            .collect(),
                    ),
                    None,
                ),
                _ => return None,
            };
            let owners = if matches!(node.kind(), "by_ref" | "reference_assignment_expression") {
                ancestors(node)
                    .filter(|node| is_callable(*node))
                    .map(|node| node.id())
                    .chain(all_nodes.first().map(|node| node.id()))
                    .collect()
            } else {
                vec![scope(node).id()]
            };
            Some(Mutation {
                owners,
                names,
                write,
            })
        })
        .collect()
}

fn write_targets(node: Node<'_>) -> Vec<Node<'_>> {
    match node.kind() {
        "variable_name" => vec![node],
        "list_literal" | "by_ref" => children(node).flat_map(write_targets).collect(),
        "subscript_expression" => node
            .named_child(0)
            .into_iter()
            .flat_map(write_targets)
            .collect(),
        _ => Vec::new(),
    }
}
