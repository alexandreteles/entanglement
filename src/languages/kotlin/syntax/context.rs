use super::*;

/// Read context clauses and their declarations from the Kotlin syntax tree.
pub(in crate::languages::kotlin) fn context_parameters<'tree>(
    nodes: &[Node<'tree>],
    source: &[u8],
) -> Vec<ContextParameters<'tree>> {
    nodes
        .iter()
        .copied()
        .filter(|node| node.kind() == "context_receiver")
        .map(|context| {
            let declaration = context.parent().and_then(|parent| match parent.kind() {
                "modifiers" => parent.parent(),
                "anonymous_function" => Some(parent),
                _ => None,
            });
            let children = context.children(&mut context.walk()).collect::<Vec<_>>();
            let parameters = children
                .split(|child| matches!(child.kind(), "context" | "(" | ")" | ","))
                .filter_map(|entry| context_parameter(entry, source))
                .collect();
            ContextParameters {
                declaration,
                range: context.byte_range(),
                parameters,
            }
        })
        .collect()
}

/// Read a named parameter or the type of a legacy receiver.
fn context_parameter(entry: &[Node<'_>], source: &[u8]) -> Option<ContextParameter> {
    let named_parameter = entry
        .iter()
        .copied()
        .find(|part| part.kind() == "parameter");
    let name = named_parameter.and_then(|parameter| {
        super::super::named_children(parameter)
            .into_iter()
            .find(|node| node.kind() == "simple_identifier" && !node.is_missing())
    });
    let type_nodes = if let Some(parameter) = named_parameter {
        parameter
            .children(&mut parameter.walk())
            .skip_while(|child| child.kind() != ":")
            .skip(1)
            .collect()
    } else {
        entry.to_vec()
    };
    let type_name = type_nodes
        .into_iter()
        .flat_map(super::super::all_nodes)
        .filter(|node| {
            node.child_count() == 0
                && !node.is_missing()
                && !matches!(node.kind(), "line_comment" | "multiline_comment")
        })
        .map(|node| String::from_utf8_lossy(&source[node.byte_range()]))
        .collect::<String>();
    (!type_name.is_empty()).then(|| ContextParameter {
        name: name.map(|node| {
            String::from_utf8_lossy(&source[node.byte_range()])
                .trim_matches('`')
                .to_owned()
        }),
        name_range: name.map(|node| node.byte_range()),
        type_name,
    })
}
