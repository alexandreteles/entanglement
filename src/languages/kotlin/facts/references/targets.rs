use super::super::*;

pub(in crate::languages::kotlin::facts) fn reference_target(
    node: Node<'_>,
    source: &[u8],
) -> Option<(Vec<String>, Range<usize>)> {
    match node.kind() {
        "simple_identifier" | "type_identifier" | "user_type" => {
            Some((simple_path(node, source)?, range(node)))
        }
        "navigation_expression" | "directly_assignable_expression" | "assignable_expression" => {
            navigation_target(node, source)
        }
        "callable_reference" => {
            let names = all_nodes(node)
                .into_iter()
                .filter(|child| matches!(child.kind(), "simple_identifier" | "type_identifier"))
                .map(|child| text(child, source).trim_matches('`').to_owned())
                .collect::<Vec<_>>();
            (!names.is_empty()).then_some((names, range(node)))
        }
        _ => None,
    }
}

pub(in crate::languages::kotlin::facts) fn simple_path(
    node: Node<'_>,
    source: &[u8],
) -> Option<Vec<String>> {
    match node.kind() {
        "simple_identifier" | "type_identifier" => {
            Some(vec![text(node, source).trim_matches('`').to_owned()])
        }
        "user_type" => Some(
            named_children(node)
                .into_iter()
                .filter(|child| matches!(child.kind(), "type_identifier" | "simple_identifier"))
                .map(|child| text(child, source).trim_matches('`').to_owned())
                .collect(),
        ),
        "super_expression" => Some(vec!["super".to_owned()]),
        "this_expression" => {
            let expression = text(node, source);
            let label = expression
                .split_once('@')
                .map(|(_, label)| label.trim().trim_matches('`'));
            Some(label.map_or_else(
                || vec!["this".to_owned()],
                |label| vec![format!("this@{label}")],
            ))
        }
        "navigation_expression" | "directly_assignable_expression" | "assignable_expression" => {
            navigation_path(node, source)
        }
        _ => None,
    }
}

fn navigation_target(node: Node<'_>, source: &[u8]) -> Option<(Vec<String>, Range<usize>)> {
    let suffix = named_children(node)
        .into_iter()
        .rfind(|child| child.kind() == "navigation_suffix")?;
    let member = named_children(suffix)
        .into_iter()
        .find(|child| matches!(child.kind(), "simple_identifier" | "type_identifier"))?;
    let path = navigation_path(node, source)
        .unwrap_or_else(|| vec![text(member, source).trim_matches('`').to_owned()]);
    Some((path, range(member)))
}

fn navigation_path(node: Node<'_>, source: &[u8]) -> Option<Vec<String>> {
    let mut children = named_children(node).into_iter();
    let left = children.next()?;
    let mut path = simple_path(left, source)?;
    for suffix in children.filter(|child| child.kind() == "navigation_suffix") {
        let member = named_children(suffix)
            .into_iter()
            .find(|child| matches!(child.kind(), "simple_identifier" | "type_identifier"))?;
        path.push(text(member, source).trim_matches('`').to_owned());
    }
    Some(path)
}

pub(super) fn is_declaration_type(node: Node<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    matches!(
        parent.kind(),
        "class_declaration" | "object_declaration" | "type_alias"
    ) && declaration_name_node(parent).is_some_and(|name| name.byte_range() == node.byte_range())
}

pub(super) fn is_declaration_name(node: Node<'_>) -> bool {
    node.parent().is_some_and(|parent| {
        matches!(
            parent.kind(),
            "function_declaration"
                | "variable_declaration"
                | "class_parameter"
                | "type_alias"
                | "class_declaration"
                | "object_declaration"
                | "companion_object"
        ) && declaration_name_node(parent)
            .is_some_and(|name| name.byte_range() == node.byte_range())
    })
}

pub(super) fn is_named_argument(node: Node<'_>, source: &[u8]) -> bool {
    node.parent().is_some_and(|parent| {
        parent.kind() == "value_argument" && is_argument_label(node, parent, source)
    })
}

pub(in crate::languages::kotlin::facts) fn is_argument_label(
    node: Node<'_>,
    parent: Node<'_>,
    source: &[u8],
) -> bool {
    if node.kind() != "simple_identifier" || parent.kind() != "value_argument" {
        return false;
    }
    let Some(next) = named_children(parent)
        .into_iter()
        .find(|child| child.start_byte() > node.end_byte())
    else {
        return false;
    };
    source
        .get(node.end_byte()..next.start_byte())
        .is_some_and(|between| between.contains(&b'='))
}

pub(super) fn is_navigation_member(node: Node<'_>) -> bool {
    node.parent()
        .is_some_and(|parent| parent.kind() == "navigation_suffix")
}

pub(super) fn is_label(node: Node<'_>) -> bool {
    node.parent()
        .is_some_and(|parent| parent.kind() == "label" || parent.kind() == "label_reference")
}

pub(super) fn is_excluded(range: &Range<usize>, excluded: &[Range<usize>]) -> bool {
    excluded.iter().any(|item| contains(item, range))
}

pub(super) fn contains(outer: &Range<usize>, inner: &Range<usize>) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}
