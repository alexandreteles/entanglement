use super::*;

pub(super) struct Mutation<'a> {
    owners: Vec<usize>,
    names: Option<Vec<&'a str>>,
    write: Option<usize>,
}

pub(super) fn stable(
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

pub(super) fn mutations<'a>(all_nodes: &[Node<'_>], source: &'a [u8]) -> Vec<Mutation<'a>> {
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
