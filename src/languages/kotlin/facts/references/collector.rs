use super::super::*;

use super::emission::reference;
use super::targets::*;

struct ReferenceCandidate<'tree> {
    priority: u8,
    path: Vec<String>,
    range: Range<usize>,
    kind: ReferenceKind,
    call: Option<Node<'tree>>,
}

struct ReferenceCollector<'index, 'tree> {
    scope_index: &'index crate::languages::kotlin::scopes::ScopeIndex<'tree>,
    excluded: &'index [Range<usize>],
    claimed: Vec<Range<usize>>,
    references: Vec<Reference>,
}

impl<'index, 'tree> ReferenceCollector<'index, 'tree> {
    fn emit(&mut self, candidate: ReferenceCandidate<'tree>) {
        if is_excluded(&candidate.range, self.excluded)
            || self
                .claimed
                .iter()
                .any(|range| contains(range, &candidate.range))
        {
            return;
        }
        self.claimed.push(candidate.range.clone());
        self.references.push(reference(
            candidate.path,
            candidate.range,
            candidate.kind,
            candidate.call,
            self.scope_index,
        ));
    }
}

pub(in crate::languages::kotlin::facts) fn collect_references(
    scope_index: &crate::languages::kotlin::scopes::ScopeIndex<'_>,
    nodes: &[Node<'_>],
    excluded: &[Range<usize>],
) -> Vec<Reference> {
    let source = scope_index.source();
    let mut candidates = Vec::with_capacity(nodes.len());
    for node in nodes.iter().copied() {
        match node.kind() {
            "call_expression" | "constructor_invocation" => {
                if let Some(candidate) = call_candidate(node, source) {
                    candidates.push(candidate);
                }
            }
            "user_type" if !is_declaration_type(node) => {
                push_target_candidate(&mut candidates, node, source, 1, ReferenceKind::Type);
            }
            "callable_reference" => {
                push_target_candidate(
                    &mut candidates,
                    node,
                    source,
                    2,
                    ReferenceKind::CallableReference,
                );
            }
            "navigation_expression" => {
                push_target_candidate(&mut candidates, node, source, 3, ReferenceKind::Qualified);
            }
            "directly_assignable_expression" | "assignable_expression" => {
                push_target_candidate(&mut candidates, node, source, 3, ReferenceKind::Qualified);
            }
            "simple_identifier" | "type_identifier"
                if !is_label(node)
                    && !is_named_argument(node, source)
                    && !is_declaration_name(node)
                    && !is_navigation_member(node) =>
            {
                candidates.push(ReferenceCandidate {
                    priority: 4,
                    path: vec![text(node, source).trim_matches('`').to_owned()],
                    range: node.byte_range(),
                    kind: ReferenceKind::Value,
                    call: None,
                });
            }
            _ => {}
        }
    }
    candidates.sort_by_key(|candidate| {
        (
            candidate.priority,
            candidate.range.start,
            candidate.range.end,
        )
    });

    let mut collector = ReferenceCollector {
        scope_index,
        excluded,
        claimed: Vec::with_capacity(candidates.len()),
        references: Vec::with_capacity(candidates.len()),
    };
    for candidate in candidates {
        collector.emit(candidate);
    }
    collector.references.sort_by_key(|reference| {
        (
            reference.start_byte,
            reference.end_byte,
            reference.kind as u8,
        )
    });
    collector.references.dedup_by(|left, right| {
        left.start_byte == right.start_byte && left.end_byte == right.end_byte
    });
    collector.references
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

fn push_target_candidate<'tree>(
    candidates: &mut Vec<ReferenceCandidate<'tree>>,
    node: Node<'tree>,
    source: &[u8],
    priority: u8,
    kind: ReferenceKind,
) {
    if let Some((path, range)) = reference_target(node, source) {
        candidates.push(ReferenceCandidate {
            priority,
            path,
            range,
            kind,
            call: None,
        });
    }
}
