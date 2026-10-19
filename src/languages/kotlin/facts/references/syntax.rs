use super::*;

pub(crate) fn reference_access(node: Node<'_>, source: &[u8]) -> Option<ReferenceAccess> {
    let assignment = ancestors(node).find(|ancestor| ancestor.kind() == "assignment");
    if let Some(assignment) = assignment {
        let children = named_children(assignment);
        let left = children.first().copied()?;
        if contains_node(left, node) && lvalue_terminal_matches(left, node) {
            let operator = (0..assignment.child_count())
                .filter_map(|index| assignment.child(index))
                .map(|child| text(child, source))
                .find(|text| {
                    matches!(
                        text.as_str(),
                        "=" | "+=" | "-=" | "*=" | "/=" | "%=" | "&&=" | "||="
                    )
                });
            return match operator.as_deref() {
                Some("=") => Some(ReferenceAccess::Write),
                Some("+=") | Some("-=") | Some("*=") | Some("/=") | Some("%=") | Some("&&=")
                | Some("||=") => Some(ReferenceAccess::Read),
                _ => None,
            };
        }
    }
    if ancestors(node)
        .find(|ancestor| matches!(ancestor.kind(), "prefix_expression" | "postfix_expression"))
        .and_then(|expression| {
            let operand = named_children(expression)
                .into_iter()
                .find(|child| !matches!(child.kind(), "annotation" | "label"))?;
            if !lvalue_terminal_matches(operand, node) {
                return None;
            }
            (0..expression.child_count())
                .filter_map(|index| expression.child(index))
                .map(|child| text(child, source))
                .find(|text| matches!(text.as_str(), "++" | "--"))
        })
        .is_some()
    {
        return Some(ReferenceAccess::ReadWrite);
    }
    Some(ReferenceAccess::Read)
}

pub(crate) fn lvalue_terminal_matches(lvalue: Node<'_>, reference: Node<'_>) -> bool {
    let nodes = all_nodes(lvalue);
    if nodes
        .iter()
        .any(|node| node.kind() == "indexing_expression" && node.end_byte() == lvalue.end_byte())
    {
        return false;
    }
    nodes
        .into_iter()
        .filter(|node| node.kind() == "simple_identifier")
        .max_by_key(|node| node.start_byte())
        .is_some_and(|node| node.byte_range() == reference.byte_range())
}

pub(crate) fn contains_node(outer: Node<'_>, inner: Node<'_>) -> bool {
    outer.start_byte() <= inner.start_byte() && inner.end_byte() <= outer.end_byte()
}

pub(crate) fn reference_target(
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

pub(crate) fn simple_path(node: Node<'_>, source: &[u8]) -> Option<Vec<String>> {
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

pub(crate) fn is_declaration_type(node: Node<'_>) -> bool {
    let Some(parent) = node.parent() else {
        return false;
    };
    matches!(
        parent.kind(),
        "class_declaration" | "object_declaration" | "type_alias"
    ) && declaration_name_node(parent).is_some_and(|name| name.byte_range() == node.byte_range())
}

pub(crate) fn is_declaration_name(node: Node<'_>) -> bool {
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

pub(crate) fn is_named_argument(node: Node<'_>, source: &[u8]) -> bool {
    node.parent().is_some_and(|parent| {
        parent.kind() == "value_argument" && is_argument_label(node, parent, source)
    })
}

pub(crate) fn is_argument_label(node: Node<'_>, parent: Node<'_>, source: &[u8]) -> bool {
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

pub(crate) fn is_navigation_member(node: Node<'_>) -> bool {
    node.parent()
        .is_some_and(|parent| parent.kind() == "navigation_suffix")
}

pub(crate) fn is_label(node: Node<'_>) -> bool {
    node.parent()
        .is_some_and(|parent| parent.kind() == "label" || parent.kind() == "label_reference")
}

pub(crate) fn is_excluded(range: &Range<usize>, excluded: &[Range<usize>]) -> bool {
    excluded.iter().any(|item| contains(item, range))
}

pub(crate) fn contains(outer: &Range<usize>, inner: &Range<usize>) -> bool {
    outer.start <= inner.start && inner.end <= outer.end
}
