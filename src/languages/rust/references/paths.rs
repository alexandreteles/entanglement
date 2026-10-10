use super::super::NodeGraph;

pub(in crate::languages::rust) fn child_field(
    node_id: usize,
    field: &str,
    graph: &NodeGraph<'_>,
) -> Option<usize> {
    graph
        .nodes
        .get(&node_id)?
        .child_by_field_name(field)
        .map(|node| node.id())
}

pub(super) fn path_segments(node_id: usize, graph: &NodeGraph<'_>) -> Vec<String> {
    let Some(node) = graph.nodes.get(&node_id).copied() else {
        return Vec::new();
    };
    if let Some(segments) = field_path_segments(node_id, graph) {
        return segments;
    }
    if is_path_node(node_id, graph) {
        return graph.text(node_id).into_iter().collect();
    }
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|child| is_path_node(child.id(), graph))
        .flat_map(|child| path_segments(child.id(), graph))
        .collect()
}

fn field_path_segments(node_id: usize, graph: &NodeGraph<'_>) -> Option<Vec<String>> {
    let (Some(path), Some(name)) = (
        child_field(node_id, "path", graph),
        child_field(node_id, "name", graph),
    ) else {
        return None;
    };
    let mut segments = path_segments(path, graph);
    if let Some(name) = graph.text(name) {
        segments.push(name);
    }
    Some(segments)
}

pub(super) fn is_use_node(id: usize, graph: &NodeGraph<'_>) -> bool {
    graph.nodes.get(&id).is_some_and(|node| {
        matches!(
            node.kind(),
            "scoped_use_list"
                | "use_as_clause"
                | "use_list"
                | "use_wildcard"
                | "scoped_identifier"
                | "identifier"
                | "crate"
                | "self"
                | "super"
        )
    })
}

pub(super) fn is_path_node(id: usize, graph: &NodeGraph<'_>) -> bool {
    graph.nodes.get(&id).is_some_and(|node| {
        matches!(
            node.kind(),
            "scoped_identifier"
                | "scoped_type_identifier"
                | "identifier"
                | "type_identifier"
                | "crate"
                | "self"
                | "super"
        )
    })
}
