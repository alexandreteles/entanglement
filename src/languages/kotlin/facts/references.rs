use super::*;

mod syntax;
pub(super) use syntax::*;

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
    let is_default_argument =
        super::super::scopes::is_default_argument(reference_node, source, is_executable_scope);
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
