use super::*;

struct ReferenceCandidate<'tree> {
    priority: u8,
    path: Vec<String>,
    range: Range<usize>,
    kind: ReferenceKind,
    call: Option<Node<'tree>>,
}

pub(in crate::languages::kotlin::facts) fn collect_references(
    scope_index: &crate::languages::kotlin::scopes::ScopeIndex<'_>,
    nodes: &[Node<'_>],
    excluded: &[Range<usize>],
) -> Vec<Reference> {
    let source = scope_index.source();
    let mut candidates = nodes
        .iter()
        .copied()
        .filter_map(|node| match node.kind() {
            "call_expression" | "constructor_invocation" => call_candidate(node, source),
            "user_type" if !is_declaration_type(node) => {
                target_candidate(node, source, 1, ReferenceKind::Type)
            }
            "callable_reference" => {
                target_candidate(node, source, 2, ReferenceKind::CallableReference)
            }
            "navigation_expression"
            | "directly_assignable_expression"
            | "assignable_expression" => {
                target_candidate(node, source, 3, ReferenceKind::Qualified)
            }
            "simple_identifier" | "type_identifier"
                if !is_label(node)
                    && !is_named_argument(node, source)
                    && !is_declaration_name(node)
                    && !is_navigation_member(node) =>
            {
                Some(ReferenceCandidate {
                    priority: 4,
                    path: vec![text(node, source).trim_matches('`').to_owned()],
                    range: node.byte_range(),
                    kind: ReferenceKind::Value,
                    call: None,
                })
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|candidate| {
        (
            candidate.priority,
            candidate.range.start,
            candidate.range.end,
        )
    });

    let mut claimed = Vec::with_capacity(candidates.len());
    let mut references = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        if is_excluded(&candidate.range, excluded)
            || claimed
                .iter()
                .any(|range| contains(range, &candidate.range))
        {
            continue;
        }
        claimed.push(candidate.range.clone());
        references.push(reference(
            candidate.path,
            candidate.range,
            candidate.kind,
            candidate.call,
            scope_index,
        ));
    }
    references.sort_by_key(|reference| {
        (
            reference.start_byte,
            reference.end_byte,
            reference.kind as u8,
        )
    });
    references.dedup_by(|left, right| {
        left.start_byte == right.start_byte && left.end_byte == right.end_byte
    });
    references
}

fn call_candidate<'tree>(call: Node<'tree>, source: &[u8]) -> Option<ReferenceCandidate<'tree>> {
    let target = named_children(call)
        .into_iter()
        .find(|child| !matches!(child.kind(), "call_suffix" | "value_arguments"))?;
    let (path, range) = reference_target(target, source)?;
    let kind = if call.kind() == "call_expression" && target.kind() == "navigation_expression" {
        ReferenceKind::Method
    } else {
        ReferenceKind::Call
    };
    Some(ReferenceCandidate {
        priority: 0,
        path,
        range,
        kind,
        call: Some(call),
    })
}

fn target_candidate<'tree>(
    node: Node<'tree>,
    source: &[u8],
    priority: u8,
    kind: ReferenceKind,
) -> Option<ReferenceCandidate<'tree>> {
    let (path, range) = reference_target(node, source)?;
    Some(ReferenceCandidate {
        priority,
        path,
        range,
        kind,
        call: None,
    })
}

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
        .map(executable_body)
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

pub(super) fn lvalue_terminal_matches(lvalue: Node<'_>, reference: Node<'_>) -> bool {
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
