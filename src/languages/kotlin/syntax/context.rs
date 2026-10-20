use super::*;

/// Read context parameters from declaration prefixes in the syntax tree.
pub(in crate::languages::kotlin) fn context_parameters(
    nodes: &[Node<'_>],
    source: &[u8],
) -> Vec<ContextParameters> {
    nodes
        .iter()
        .copied()
        .filter(|node| node.kind() == "context_parameters")
        .map(|context| ContextParameters {
            range: context.byte_range(),
            parameters: super::super::named_children(context)
                .into_iter()
                .filter(|node| node.kind() == "context_parameter")
                .filter_map(|parameter| {
                    let children = super::super::named_children(parameter);
                    let name = children
                        .iter()
                        .find(|node| node.kind() == "simple_identifier");
                    let types = children
                        .iter()
                        .filter(|node| {
                            !matches!(
                                node.kind(),
                                "simple_identifier" | "line_comment" | "multiline_comment"
                            )
                        })
                        .collect::<Vec<_>>();
                    let type_name = types
                        .into_iter()
                        .flat_map(|ty| super::super::all_nodes(*ty))
                        .filter(|node| {
                            node.child_count() == 0
                                && !matches!(node.kind(), "line_comment" | "multiline_comment")
                        })
                        .map(|node| String::from_utf8_lossy(&source[node.byte_range()]))
                        .collect::<String>();
                    if type_name.is_empty() {
                        return None;
                    }
                    Some(ContextParameter {
                        name: name.map(|node| {
                            String::from_utf8_lossy(&source[node.byte_range()])
                                .trim_matches('`')
                                .to_owned()
                        }),
                        name_range: name.map(Node::byte_range),
                        type_name,
                    })
                })
                .collect(),
        })
        .collect()
}
