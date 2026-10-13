use super::super::*;

pub(super) fn reference(
    path: Vec<String>,
    source_range: Range<usize>,
    kind: ReferenceKind,
    call: Option<Node<'_>>,
    scope_index: &crate::languages::kotlin::scopes::ScopeIndex<'_>,
) -> Reference {
    let root = scope_index.root();
    let source = scope_index.source();
    let reference_node = root
        .descendant_for_byte_range(source_range.start, source_range.end)
        .unwrap_or(root);
    let is_default_argument = is_default_argument_reference(reference_node, source);
    let owns_body_call = call.is_some() && !is_default_argument;
    let call_owner_node = call
        .filter(|_| owns_body_call)
        .and_then(|call| ancestors(call).find(|ancestor| is_executable_scope(ancestor.kind())));
    let call_owner_range = call
        .filter(|_| owns_body_call)
        .and_then(|call| scope_index.metric_scope_range_for(call));
    let execution_owner = (!is_default_argument)
        .then(|| scope_index.metric_scope_range_for(reference_node))
        .flatten()
        .map(|owner| owner.start);
    let scope = call_owner_node
        .and_then(executable_body)
        .or_else(|| call_owner_range.clone())
        .or_else(|| {
            let scope = call.into_iter().flat_map(ancestors).find(|ancestor| {
                matches!(
                    ancestor.kind(),
                    "statements" | "class_body" | "enum_class_body" | "lambda_literal"
                )
            })?;
            Some(range(scope))
        })
        .unwrap_or_else(|| range(root));
    Reference {
        path,
        module: ModulePath::default(),
        kind,
        start_byte: source_range.start,
        end_byte: source_range.end,
        scope_start: scope.start,
        scope_end: scope.end,
        call_owner: call_owner_range.map(|owner| owner.start),
        execution_owner,
        access: if matches!(kind, ReferenceKind::Value | ReferenceKind::Qualified) {
            reference_access(reference_node, source)
        } else {
            None
        },
        context_id: 0,
    }
}

pub(super) fn reference_access(node: Node<'_>, source: &[u8]) -> Option<ReferenceAccess> {
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
    if let Some(operator) = ancestors(node)
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
        && matches!(operator.as_str(), "++" | "--")
    {
        return Some(ReferenceAccess::ReadWrite);
    }
    Some(ReferenceAccess::Read)
}

pub(super) fn lvalue_terminal_matches(lvalue: Node<'_>, reference: Node<'_>) -> bool {
    if all_nodes(lvalue)
        .into_iter()
        .any(|node| node.kind() == "indexing_expression" && node.end_byte() == lvalue.end_byte())
    {
        return false;
    }
    all_nodes(lvalue)
        .into_iter()
        .filter(|node| node.kind() == "simple_identifier")
        .max_by_key(|node| node.start_byte())
        .is_some_and(|node| node.byte_range() == reference.byte_range())
}

pub(super) fn contains_node(outer: Node<'_>, inner: Node<'_>) -> bool {
    outer.start_byte() <= inner.start_byte() && inner.end_byte() <= outer.end_byte()
}

pub(super) fn is_default_argument_reference(node: Node<'_>, source: &[u8]) -> bool {
    for ancestor in ancestors(node) {
        if is_executable_scope(ancestor.kind()) {
            return false;
        }
        if matches!(
            ancestor.kind(),
            "parameter" | "function_value_parameter" | "class_parameter"
        ) && source
            .get(ancestor.byte_range())
            .is_some_and(|parameter| parameter.contains(&b'='))
        {
            return true;
        }
    }
    false
}
